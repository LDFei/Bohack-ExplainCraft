//! SQLite 持久化。所有状态变更与对应事件在同一事务提交，提交后再通知 SSE 订阅者。

use std::ops::Deref;
use std::path::Path;
use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension, Row, Transaction};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use tokio::sync::broadcast;

use crate::error::{ApiError, ApiResult};
use crate::models::*;
use crate::util::now_iso;

/// 数据库表结构。启动时执行，`IF NOT EXISTS` 保证可以重复执行。
///
/// - `projects`：项目、当前草稿（JSON）与内部产物（JSON），`event_seq` 为项目内事件序号；
/// - `messages` / `assets` 用自增 `seq` 保证分页顺序稳定；
/// - `jobs.attempt` 是 fencing token，每次被 worker 认领时递增；
/// - `events` 按 `(project_id, seq)` 存储 SSE 事件，供断线补发；
/// - `idempotency` 记录已受理请求的响应，用于幂等重放。
const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS projects (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    title TEXT NOT NULL,
    status TEXT NOT NULL,
    draft_revision INTEGER NOT NULL,
    active_job_id TEXT,
    latest_video_version_id TEXT,
    draft_dirty INTEGER NOT NULL,
    draft TEXT NOT NULL,
    artifacts TEXT NOT NULL DEFAULT '{}',
    event_seq INTEGER NOT NULL DEFAULT 0,
    pruned_through INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS projects_by_user ON projects(user_id, updated_at DESC, id DESC);

CREATE TABLE IF NOT EXISTS messages (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    id TEXT NOT NULL UNIQUE,
    project_id TEXT NOT NULL,
    role TEXT NOT NULL,
    text TEXT NOT NULL,
    status TEXT NOT NULL,
    asset_ids TEXT NOT NULL,
    job_id TEXT,
    blocks TEXT NOT NULL,
    last_delta_seq INTEGER NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS messages_by_project ON messages(project_id, seq);

CREATE TABLE IF NOT EXISTS jobs (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    parent_job_id TEXT,
    kind TEXT NOT NULL,
    status TEXT NOT NULL,
    current_stage TEXT,
    progress_percent INTEGER,
    stages TEXT NOT NULL,
    question TEXT,
    result TEXT,
    error TEXT,
    input TEXT NOT NULL,
    attempt INTEGER NOT NULL DEFAULT 0,
    checkpoint_revision INTEGER NOT NULL,
    assistant_message_id TEXT,
    created_at TEXT NOT NULL,
    started_at TEXT,
    finished_at TEXT
);
CREATE INDEX IF NOT EXISTS jobs_by_status ON jobs(status, created_at);
CREATE INDEX IF NOT EXISTS jobs_by_project ON jobs(project_id, created_at);

CREATE TABLE IF NOT EXISTS assets (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    id TEXT NOT NULL UNIQUE,
    project_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    source TEXT NOT NULL,
    status TEXT NOT NULL,
    file_name TEXT NOT NULL,
    mime_type TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    duration_ms INTEGER,
    width INTEGER,
    height INTEGER,
    error TEXT,
    storage_path TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS assets_by_project ON assets(project_id, seq);

CREATE TABLE IF NOT EXISTS versions (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    version_number INTEGER NOT NULL,
    source_draft_revision INTEGER NOT NULL,
    source_job_id TEXT NOT NULL,
    created_at TEXT NOT NULL,
    video_asset_id TEXT NOT NULL,
    thumbnail_asset_id TEXT,
    duration_ms INTEGER NOT NULL,
    width INTEGER NOT NULL,
    height INTEGER NOT NULL,
    snapshot TEXT NOT NULL,
    exports TEXT NOT NULL,
    UNIQUE (project_id, version_number)
);

CREATE TABLE IF NOT EXISTS events (
    project_id TEXT NOT NULL,
    seq INTEGER NOT NULL,
    event TEXT NOT NULL,
    data TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY (project_id, seq)
);

CREATE TABLE IF NOT EXISTS idempotency (
    user_id TEXT NOT NULL,
    method TEXT NOT NULL,
    path TEXT NOT NULL,
    key TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    status_code INTEGER NOT NULL,
    response TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY (user_id, method, path, key)
);
"#;

/// 数据库句柄：单个 SQLite 连接加互斥锁，并附带事件通知通道。
///
/// 所有读写在锁内同步完成（单次操作为毫秒级），锁内不跨越 `await`。
pub struct Db {
    /// SQLite 连接。
    conn: Mutex<Connection>,
    /// 事务提交后广播受影响的项目 ID，唤醒对应的 SSE 连接。
    notify: broadcast::Sender<String>,
}

/// 写事务。`emit` 写入的事件随事务一起提交，提交后通知订阅者。
pub struct Tx<'a> {
    /// 底层事务。
    tx: Transaction<'a>,
    /// 本事务内写过事件的项目，提交后逐个通知。
    touched: Vec<String>,
}

impl<'a> Deref for Tx<'a> {
    type Target = Transaction<'a>;
    fn deref(&self) -> &Self::Target {
        &self.tx
    }
}

impl Tx<'_> {
    /// 写入一条项目事件，返回事件序号。
    pub fn emit(&mut self, project_id: &str, event: &str, data: Value) -> ApiResult<i64> {
        let seq: i64 = self.tx.query_row(
            "UPDATE projects SET event_seq = event_seq + 1 WHERE id = ?1 RETURNING event_seq",
            params![project_id],
            |row| row.get(0),
        )?;
        self.tx.execute(
            "INSERT INTO events (project_id, seq, event, data, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![project_id, seq, event, data.to_string(), now_iso()],
        )?;
        if !self.touched.iter().any(|p| p == project_id) {
            self.touched.push(project_id.to_string());
        }
        Ok(seq)
    }
}

impl Db {
    /// 打开（必要时创建）数据库文件并初始化表结构。
    ///
    /// 开启 WAL 以减少读写互相阻塞，设置 5 秒忙等待。
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "busy_timeout", 5000)?;
        conn.execute_batch(SCHEMA)?;
        let (notify, _) = broadcast::channel(1024);
        Ok(Self {
            conn: Mutex::new(conn),
            notify,
        })
    }

    /// 获取连接锁。某个线程 panic 导致锁中毒时仍继续使用：SQLite 事务保证数据一致。
    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// 在锁内执行只读操作。
    pub fn read<T>(&self, f: impl FnOnce(&Connection) -> ApiResult<T>) -> ApiResult<T> {
        let conn = self.lock();
        f(&conn)
    }

    /// 在一个事务内执行写操作。
    ///
    /// 闭包返回 `Err` 时事务回滚；成功提交后才通知 SSE 订阅者，
    /// 保证前端收到事件时一定能读到对应的新状态。
    pub fn write<T>(&self, f: impl FnOnce(&mut Tx) -> ApiResult<T>) -> ApiResult<T> {
        let touched;
        let value;
        {
            let mut conn = self.lock();
            let tx = conn.transaction()?;
            let mut wrapped = Tx {
                tx,
                touched: vec![],
            };
            // `?` 提前返回时 `wrapped` 被丢弃，事务自动回滚。
            value = f(&mut wrapped)?;
            touched = wrapped.touched;
            wrapped.tx.commit()?;
        }
        for project in touched {
            let _ = self.notify.send(project);
        }
        Ok(value)
    }

    /// 订阅“项目有新事件”的通知，消息内容为项目 ID。
    pub fn subscribe(&self) -> broadcast::Receiver<String> {
        self.notify.subscribe()
    }
}

// ---------- JSON 列工具 ----------

/// 把值序列化为 JSON 文本，用于写入 JSON 列。
pub fn to_json<T: Serialize>(value: &T) -> ApiResult<String> {
    Ok(serde_json::to_string(value)?)
}

/// 从 JSON 列反序列化；失败时转换为 rusqlite 的类型转换错误。
fn from_json<T: DeserializeOwned>(text: &str) -> rusqlite::Result<T> {
    serde_json::from_str(text).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })
}

/// 可空 JSON 列的反序列化。
fn opt_json<T: DeserializeOwned>(text: Option<String>) -> rusqlite::Result<Option<T>> {
    text.map(|t| from_json(&t)).transpose()
}

/// 可空值序列化为可空 JSON 列。
fn opt_to_json<T: Serialize>(value: &Option<T>) -> ApiResult<Option<String>> {
    value.as_ref().map(to_json).transpose()
}

// ---------- Project ----------

/// 项目表的一行：对外的 [`Project`] 加上内部字段。
#[derive(Clone, Debug)]
pub struct ProjectRow {
    /// 对外的项目对象。
    pub project: Project,
    /// 所属用户，用于访问隔离。
    pub user_id: String,
    /// 当前草稿。
    pub draft: Draft,
    /// 内部制作产物（检查点）。
    pub artifacts: Artifacts,
    /// 最新事件序号，即快照里的 `event_cursor`。
    pub event_seq: i64,
    /// 已清理到的事件序号；早于它的游标返回 410。
    pub pruned_through: i64,
}

/// 把查询行映射为 [`ProjectRow`]。
fn project_row(row: &Row) -> rusqlite::Result<ProjectRow> {
    Ok(ProjectRow {
        project: Project {
            id: row.get("id")?,
            title: row.get("title")?,
            status: row.get("status")?,
            draft_revision: row.get("draft_revision")?,
            active_job_id: row.get("active_job_id")?,
            latest_video_version_id: row.get("latest_video_version_id")?,
            draft_dirty: row.get::<_, i64>("draft_dirty")? != 0,
            created_at: row.get("created_at")?,
            updated_at: row.get("updated_at")?,
        },
        user_id: row.get("user_id")?,
        draft: from_json(&row.get::<_, String>("draft")?)?,
        artifacts: from_json(&row.get::<_, String>("artifacts")?)?,
        event_seq: row.get("event_seq")?,
        pruned_through: row.get("pruned_through")?,
    })
}

/// 按 ID 读取项目（不校验归属，供内部使用）。
pub fn load_project(conn: &Connection, id: &str) -> ApiResult<Option<ProjectRow>> {
    Ok(conn
        .query_row(
            "SELECT * FROM projects WHERE id = ?1",
            params![id],
            project_row,
        )
        .optional()?)
}

/// 读取当前用户的项目；不存在或属于他人统一返回 404。
pub fn project_for(conn: &Connection, id: &str, user_id: &str) -> ApiResult<ProjectRow> {
    load_project(conn, id)?
        .filter(|row| row.user_id == user_id)
        .ok_or_else(|| ApiError::not_found("这个项目不存在或已被删除，可以新建一份创作。"))
}

/// 插入新项目。
pub fn insert_project(conn: &Connection, row: &ProjectRow) -> ApiResult<()> {
    let p = &row.project;
    conn.execute(
        "INSERT INTO projects (id, user_id, title, status, draft_revision, active_job_id,
            latest_video_version_id, draft_dirty, draft, artifacts, event_seq, pruned_through,
            created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 0, 0, ?11, ?12)",
        params![
            p.id,
            row.user_id,
            p.title,
            p.status,
            p.draft_revision,
            p.active_job_id,
            p.latest_video_version_id,
            p.draft_dirty as i64,
            to_json(&row.draft)?,
            to_json(&row.artifacts)?,
            p.created_at,
            p.updated_at
        ],
    )?;
    Ok(())
}

/// 保存项目可变字段并刷新 updated_at（不改 event_seq，事件序号只由 emit 维护）。
pub fn save_project(conn: &Connection, row: &mut ProjectRow) -> ApiResult<()> {
    row.project.updated_at = now_iso();
    let p = &row.project;
    conn.execute(
        "UPDATE projects SET title = ?2, status = ?3, draft_revision = ?4, active_job_id = ?5,
            latest_video_version_id = ?6, draft_dirty = ?7, draft = ?8, artifacts = ?9,
            updated_at = ?10
         WHERE id = ?1",
        params![
            p.id,
            p.title,
            p.status,
            p.draft_revision,
            p.active_job_id,
            p.latest_video_version_id,
            p.draft_dirty as i64,
            to_json(&row.draft)?,
            to_json(&row.artifacts)?,
            p.updated_at
        ],
    )?;
    Ok(())
}

/// 按 `updated_at DESC, id DESC` 分页列出用户的项目。
///
/// `after` 是上一页最后一项的 `(updated_at, id)`。采用键集分页，翻页期间新增项目不会导致重复或遗漏。
pub fn list_projects(
    conn: &Connection,
    user_id: &str,
    after: Option<(String, String)>,
    limit: i64,
) -> ApiResult<Vec<ProjectRow>> {
    let mut rows = vec![];
    match after {
        Some((updated_at, id)) => {
            let mut stmt = conn.prepare(
                "SELECT * FROM projects WHERE user_id = ?1
                   AND (updated_at < ?2 OR (updated_at = ?2 AND id < ?3))
                 ORDER BY updated_at DESC, id DESC LIMIT ?4",
            )?;
            for row in stmt.query_map(params![user_id, updated_at, id, limit], project_row)? {
                rows.push(row?);
            }
        }
        None => {
            let mut stmt = conn.prepare(
                "SELECT * FROM projects WHERE user_id = ?1 ORDER BY updated_at DESC, id DESC LIMIT ?2",
            )?;
            for row in stmt.query_map(params![user_id, limit], project_row)? {
                rows.push(row?);
            }
        }
    }
    Ok(rows)
}

// ---------- Message ----------

/// 把查询行映射为 `(seq, Message)`。
fn message_row(row: &Row) -> rusqlite::Result<(i64, Message)> {
    Ok((
        row.get("seq")?,
        Message {
            id: row.get("id")?,
            project_id: row.get("project_id")?,
            role: row.get("role")?,
            text: row.get("text")?,
            status: row.get("status")?,
            asset_ids: from_json(&row.get::<_, String>("asset_ids")?)?,
            job_id: row.get("job_id")?,
            blocks: from_json(&row.get::<_, String>("blocks")?)?,
            last_delta_seq: row.get("last_delta_seq")?,
            created_at: row.get("created_at")?,
        },
    ))
}

/// 插入消息。
pub fn insert_message(conn: &Connection, m: &Message) -> ApiResult<()> {
    conn.execute(
        "INSERT INTO messages (id, project_id, role, text, status, asset_ids, job_id, blocks,
            last_delta_seq, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            m.id,
            m.project_id,
            m.role,
            m.text,
            m.status,
            to_json(&m.asset_ids)?,
            m.job_id,
            to_json(&m.blocks)?,
            m.last_delta_seq,
            m.created_at
        ],
    )?;
    Ok(())
}

/// 更新消息的可变字段（文本、状态、卡片、增量序号）。
pub fn save_message(conn: &Connection, m: &Message) -> ApiResult<()> {
    conn.execute(
        "UPDATE messages SET text = ?2, status = ?3, blocks = ?4, last_delta_seq = ?5 WHERE id = ?1",
        params![m.id, m.text, m.status, to_json(&m.blocks)?, m.last_delta_seq],
    )?;
    Ok(())
}

/// 按 ID 读取消息。
pub fn load_message(conn: &Connection, id: &str) -> ApiResult<Option<Message>> {
    Ok(conn
        .query_row(
            "SELECT * FROM messages WHERE id = ?1",
            params![id],
            message_row,
        )
        .optional()?
        .map(|(_, m)| m))
}

/// 按时间倒序取一页（before 为排他游标），返回时转为正序；第二项为更早一页的游标。
pub fn list_messages(
    conn: &Connection,
    project_id: &str,
    before: Option<i64>,
    limit: i64,
) -> ApiResult<(Vec<Message>, Option<String>)> {
    let mut stmt = conn.prepare(
        "SELECT * FROM messages WHERE project_id = ?1 AND seq < ?2 ORDER BY seq DESC LIMIT ?3",
    )?;
    let mut page: Vec<(i64, Message)> = stmt
        .query_map(
            params![project_id, before.unwrap_or(i64::MAX), limit + 1],
            message_row,
        )?
        .collect::<Result<_, _>>()?;
    let more = page.len() as i64 > limit;
    page.truncate(limit as usize);
    page.reverse();
    let cursor = if more {
        page.first().map(|(seq, _)| seq.to_string())
    } else {
        None
    };
    Ok((page.into_iter().map(|(_, m)| m).collect(), cursor))
}

// ---------- Job ----------

/// 任务表的一行：对外的 [`Job`] 加上内部字段。
#[derive(Clone, Debug)]
pub struct JobRow {
    /// 对外的任务对象。
    pub job: Job,
    /// 受理时保存的完整输入。
    pub input: JobInput,
    /// fencing token：每次被 worker 认领时递增。
    pub attempt: i64,
    /// 任务最后一次保存阶段产物后的草稿版本，重试时校验。
    pub checkpoint_revision: i64,
    /// 任务对应的 assistant 消息；导出任务没有消息。
    pub assistant_message_id: Option<String>,
}

/// 把查询行映射为 [`JobRow`]。
fn job_row(row: &Row) -> rusqlite::Result<JobRow> {
    let stage: Option<String> = row.get("current_stage")?;
    Ok(JobRow {
        job: Job {
            id: row.get("id")?,
            project_id: row.get("project_id")?,
            parent_job_id: row.get("parent_job_id")?,
            kind: JobKind::parse(&row.get::<_, String>("kind")?),
            status: JobStatus::parse(&row.get::<_, String>("status")?),
            current_stage: stage.map(|s| from_json(&format!("\"{s}\""))).transpose()?,
            progress_percent: row.get("progress_percent")?,
            stages: from_json(&row.get::<_, String>("stages")?)?,
            question: opt_json(row.get("question")?)?,
            result: opt_json(row.get("result")?)?,
            error: opt_json(row.get("error")?)?,
            created_at: row.get("created_at")?,
            started_at: row.get("started_at")?,
            finished_at: row.get("finished_at")?,
        },
        input: from_json(&row.get::<_, String>("input")?)?,
        attempt: row.get("attempt")?,
        checkpoint_revision: row.get("checkpoint_revision")?,
        assistant_message_id: row.get("assistant_message_id")?,
    })
}

/// 阶段枚举转为数据库字符串（与 JSON 中的 `snake_case` 一致）。
fn stage_text(stage: Option<StageKey>) -> ApiResult<Option<String>> {
    Ok(stage
        .map(|s| serde_json::to_value(s).map(|v| v.as_str().unwrap_or_default().to_string()))
        .transpose()?)
}

/// 插入新任务。
pub fn insert_job(conn: &Connection, r: &JobRow) -> ApiResult<()> {
    let j = &r.job;
    conn.execute(
        "INSERT INTO jobs (id, project_id, parent_job_id, kind, status, current_stage,
            progress_percent, stages, question, result, error, input, attempt, checkpoint_revision,
            assistant_message_id, created_at, started_at, finished_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
        params![
            j.id,
            j.project_id,
            j.parent_job_id,
            j.kind.as_str(),
            j.status.as_str(),
            stage_text(j.current_stage)?,
            j.progress_percent,
            to_json(&j.stages)?,
            opt_to_json(&j.question)?,
            opt_to_json(&j.result)?,
            opt_to_json(&j.error)?,
            to_json(&r.input)?,
            r.attempt,
            r.checkpoint_revision,
            r.assistant_message_id,
            j.created_at,
            j.started_at,
            j.finished_at
        ],
    )?;
    Ok(())
}

/// 保存任务的全部可变字段。调用方负责 fencing 校验。
pub fn save_job(conn: &Connection, r: &JobRow) -> ApiResult<()> {
    let j = &r.job;
    conn.execute(
        "UPDATE jobs SET status = ?2, current_stage = ?3, progress_percent = ?4, stages = ?5,
            question = ?6, result = ?7, error = ?8, input = ?9, attempt = ?10,
            checkpoint_revision = ?11, started_at = ?12, finished_at = ?13
         WHERE id = ?1",
        params![
            j.id,
            j.status.as_str(),
            stage_text(j.current_stage)?,
            j.progress_percent,
            to_json(&j.stages)?,
            opt_to_json(&j.question)?,
            opt_to_json(&j.result)?,
            opt_to_json(&j.error)?,
            to_json(&r.input)?,
            r.attempt,
            r.checkpoint_revision,
            j.started_at,
            j.finished_at
        ],
    )?;
    Ok(())
}

/// 按 ID 读取任务。
pub fn load_job(conn: &Connection, id: &str) -> ApiResult<Option<JobRow>> {
    Ok(conn
        .query_row("SELECT * FROM jobs WHERE id = ?1", params![id], job_row)
        .optional()?)
}

/// 按创建时间顺序列出某个状态的全部任务，供调度器使用。
pub fn jobs_with_status(conn: &Connection, status: JobStatus) -> ApiResult<Vec<JobRow>> {
    let mut stmt = conn.prepare("SELECT * FROM jobs WHERE status = ?1 ORDER BY created_at")?;
    let rows = stmt
        .query_map(params![status.as_str()], job_row)?
        .collect::<Result<_, _>>()?;
    Ok(rows)
}

/// 最近一次 generate / revise 任务的状态，用于计算项目的 failed 摘要状态。
pub fn last_production_status(conn: &Connection, project_id: &str) -> ApiResult<Option<JobStatus>> {
    Ok(conn
        .query_row(
            "SELECT status FROM jobs WHERE project_id = ?1 AND kind IN ('generate', 'revise')
               AND status IN ('succeeded', 'failed') ORDER BY created_at DESC LIMIT 1",
            params![project_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .map(|s| JobStatus::parse(&s)))
}

// ---------- Asset ----------

/// 素材表的一行：对外的 [`Asset`] 加上存储路径。
#[derive(Clone, Debug)]
pub struct AssetRow {
    /// 对外的素材对象（`thumbnail_url` 在响应时填充）。
    pub asset: Asset,
    /// 服务器上的文件路径，绝不返回给前端。
    pub storage_path: String,
}

/// 把查询行映射为 `(seq, AssetRow)`。
fn asset_row(row: &Row) -> rusqlite::Result<(i64, AssetRow)> {
    Ok((
        row.get("seq")?,
        AssetRow {
            asset: Asset {
                id: row.get("id")?,
                project_id: row.get("project_id")?,
                kind: row.get("kind")?,
                source: row.get("source")?,
                status: row.get("status")?,
                file_name: row.get("file_name")?,
                mime_type: row.get("mime_type")?,
                size_bytes: row.get("size_bytes")?,
                duration_ms: row.get("duration_ms")?,
                width: row.get("width")?,
                height: row.get("height")?,
                thumbnail_url: None,
                error: opt_json(row.get("error")?)?,
            },
            storage_path: row.get("storage_path")?,
        },
    ))
}

/// 插入素材记录。
pub fn insert_asset(conn: &Connection, r: &AssetRow) -> ApiResult<()> {
    let a = &r.asset;
    conn.execute(
        "INSERT INTO assets (id, project_id, kind, source, status, file_name, mime_type, size_bytes,
            duration_ms, width, height, error, storage_path, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            a.id,
            a.project_id,
            a.kind,
            a.source,
            a.status,
            a.file_name,
            a.mime_type,
            a.size_bytes,
            a.duration_ms,
            a.width,
            a.height,
            opt_to_json(&a.error)?,
            r.storage_path,
            now_iso()
        ],
    )?;
    Ok(())
}

/// 按 ID 读取素材（不校验归属，供内部使用）。
pub fn load_asset(conn: &Connection, id: &str) -> ApiResult<Option<AssetRow>> {
    Ok(conn
        .query_row("SELECT * FROM assets WHERE id = ?1", params![id], asset_row)
        .optional()?
        .map(|(_, a)| a))
}

/// 按创建时间倒序分页列出项目素材；`uploads_only` 为真时只列用户上传的文件。
pub fn list_assets(
    conn: &Connection,
    project_id: &str,
    before: Option<i64>,
    limit: i64,
    uploads_only: bool,
) -> ApiResult<(Vec<AssetRow>, Option<String>)> {
    let sql = if uploads_only {
        "SELECT * FROM assets WHERE project_id = ?1 AND seq < ?2 AND source = 'upload' ORDER BY seq DESC LIMIT ?3"
    } else {
        "SELECT * FROM assets WHERE project_id = ?1 AND seq < ?2 ORDER BY seq DESC LIMIT ?3"
    };
    let mut stmt = conn.prepare(sql)?;
    let mut page: Vec<(i64, AssetRow)> = stmt
        .query_map(
            params![project_id, before.unwrap_or(i64::MAX), limit + 1],
            asset_row,
        )?
        .collect::<Result<_, _>>()?;
    let more = page.len() as i64 > limit;
    page.truncate(limit as usize);
    let cursor = if more {
        page.last().map(|(seq, _)| seq.to_string())
    } else {
        None
    };
    Ok((page.into_iter().map(|(_, a)| a).collect(), cursor))
}

/// 项目内用户上传的素材数，用于数量上限校验。
pub fn count_uploads(conn: &Connection, project_id: &str) -> ApiResult<i64> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM assets WHERE project_id = ?1 AND source = 'upload'",
        params![project_id],
        |row| row.get(0),
    )?)
}

// ---------- VideoVersion ----------

/// 成片版本表的一行。
#[derive(Clone, Debug)]
pub struct VersionRow {
    /// 版本 ID。
    pub id: String,
    /// 所属项目。
    pub project_id: String,
    /// 项目内版本号。
    pub version_number: i64,
    /// 生成时的草稿版本。
    pub source_draft_revision: i64,
    /// 生成该版本的任务。
    pub source_job_id: String,
    /// 创建时间。
    pub created_at: String,
    /// 预览成片素材。
    pub video_asset_id: String,
    /// 封面图素材（第一个镜头的画面）。
    pub thumbnail_asset_id: Option<String>,
    /// 时长（毫秒）。
    pub duration_ms: i64,
    /// 宽度。
    pub width: i64,
    /// 高度。
    pub height: i64,
    /// 生成时的完整草稿快照，不可修改。
    pub snapshot: Draft,
    /// 已生成的导出文件。
    pub exports: Vec<VideoExport>,
}

/// 把查询行映射为 [`VersionRow`]。
fn version_row(row: &Row) -> rusqlite::Result<VersionRow> {
    Ok(VersionRow {
        id: row.get("id")?,
        project_id: row.get("project_id")?,
        version_number: row.get("version_number")?,
        source_draft_revision: row.get("source_draft_revision")?,
        source_job_id: row.get("source_job_id")?,
        created_at: row.get("created_at")?,
        video_asset_id: row.get("video_asset_id")?,
        thumbnail_asset_id: row.get("thumbnail_asset_id")?,
        duration_ms: row.get("duration_ms")?,
        width: row.get("width")?,
        height: row.get("height")?,
        snapshot: from_json(&row.get::<_, String>("snapshot")?)?,
        exports: from_json(&row.get::<_, String>("exports")?)?,
    })
}

/// 插入新版本。
pub fn insert_version(conn: &Connection, v: &VersionRow) -> ApiResult<()> {
    conn.execute(
        "INSERT INTO versions (id, project_id, version_number, source_draft_revision, source_job_id,
            created_at, video_asset_id, thumbnail_asset_id, duration_ms, width, height, snapshot, exports)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            v.id,
            v.project_id,
            v.version_number,
            v.source_draft_revision,
            v.source_job_id,
            v.created_at,
            v.video_asset_id,
            v.thumbnail_asset_id,
            v.duration_ms,
            v.width,
            v.height,
            to_json(&v.snapshot)?,
            to_json(&v.exports)?
        ],
    )?;
    Ok(())
}

/// 更新版本的导出列表（版本的其他字段不可修改）。
pub fn save_version_exports(conn: &Connection, v: &VersionRow) -> ApiResult<()> {
    conn.execute(
        "UPDATE versions SET exports = ?2 WHERE id = ?1",
        params![v.id, to_json(&v.exports)?],
    )?;
    Ok(())
}

/// 按 ID 读取版本。
pub fn load_version(conn: &Connection, id: &str) -> ApiResult<Option<VersionRow>> {
    Ok(conn
        .query_row(
            "SELECT * FROM versions WHERE id = ?1",
            params![id],
            version_row,
        )
        .optional()?)
}

/// 按版本号倒序分页列出项目的成片版本。
pub fn list_versions(
    conn: &Connection,
    project_id: &str,
    before: Option<i64>,
    limit: i64,
) -> ApiResult<(Vec<VersionRow>, Option<String>)> {
    let mut stmt = conn.prepare(
        "SELECT * FROM versions WHERE project_id = ?1 AND version_number < ?2
         ORDER BY version_number DESC LIMIT ?3",
    )?;
    let mut page: Vec<VersionRow> = stmt
        .query_map(
            params![project_id, before.unwrap_or(i64::MAX), limit + 1],
            version_row,
        )?
        .collect::<Result<_, _>>()?;
    let more = page.len() as i64 > limit;
    page.truncate(limit as usize);
    let cursor = if more {
        page.last().map(|v| v.version_number.to_string())
    } else {
        None
    };
    Ok((page, cursor))
}

/// 项目的下一个版本号。在发布成片的事务内调用，配合唯一约束避免重复。
pub fn next_version_number(conn: &Connection, project_id: &str) -> ApiResult<i64> {
    Ok(conn.query_row(
        "SELECT COALESCE(MAX(version_number), 0) + 1 FROM versions WHERE project_id = ?1",
        params![project_id],
        |row| row.get(0),
    )?)
}

// ---------- Events ----------

/// 一条持久化的 SSE 事件。
pub struct EventRow {
    /// 项目内递增的事件序号，即 SSE 的 `id`。
    pub seq: i64,
    /// 事件名，如 `job.updated`。
    pub event: String,
    /// 单行 JSON 数据。
    pub data: String,
}

/// 读取游标之后的事件（排他游标），最多 `limit` 条。
pub fn events_after(
    conn: &Connection,
    project_id: &str,
    after: i64,
    limit: i64,
) -> ApiResult<Vec<EventRow>> {
    let mut stmt = conn.prepare(
        "SELECT seq, event, data FROM events WHERE project_id = ?1 AND seq > ?2 ORDER BY seq LIMIT ?3",
    )?;
    let rows = stmt
        .query_map(params![project_id, after, limit], |row| {
            Ok(EventRow {
                seq: row.get(0)?,
                event: row.get(1)?,
                data: row.get(2)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok(rows)
}

/// 清理过期事件，并记录每个项目已清理到的序号（早于它的游标返回 410）。
pub fn prune_events(conn: &Connection, older_than: &str) -> ApiResult<usize> {
    conn.execute(
        "UPDATE projects SET pruned_through = MAX(pruned_through, COALESCE(
            (SELECT MAX(seq) FROM events WHERE events.project_id = projects.id AND created_at < ?1),
            pruned_through))",
        params![older_than],
    )?;
    Ok(conn.execute(
        "DELETE FROM events WHERE created_at < ?1",
        params![older_than],
    )?)
}

// ---------- 删除 ----------

/// 删除项目及其全部记录，返回需要清理的文件路径。
pub fn delete_project(conn: &Connection, project_id: &str) -> ApiResult<Vec<String>> {
    let mut stmt = conn.prepare("SELECT storage_path FROM assets WHERE project_id = ?1")?;
    let files: Vec<String> = stmt
        .query_map(params![project_id], |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    for table in ["messages", "jobs", "assets", "versions", "events"] {
        conn.execute(
            &format!("DELETE FROM {table} WHERE project_id = ?1"),
            params![project_id],
        )?;
    }
    conn.execute("DELETE FROM projects WHERE id = ?1", params![project_id])?;
    Ok(files)
}
