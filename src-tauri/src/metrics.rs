//! 视频生成用量记录：内部保存每次真实模型请求，只在成片完成后汇总总 Token 与端到端耗时。

use std::future::Future;
use std::sync::Arc;
use std::time::Instant;

use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};

use crate::db::{Db, JobRow};
use crate::error::ApiResult;
use crate::models::{JobKind, StageKey};
use crate::providers::{ProviderError, ProviderResult};
use crate::state::AppState;
use crate::util::{make_id, now_iso};

/// 创建内部用量表；同名旧表不是本功能结构时保持原样。
pub fn initialize(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS generation_runs (
            job_id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            config TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS generation_model_calls (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            job_id TEXT NOT NULL,
            attempt INTEGER NOT NULL,
            data TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS generation_calls_by_job
            ON generation_model_calls(job_id);
        CREATE TABLE IF NOT EXISTS stage_spans (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            job_id TEXT NOT NULL,
            attempt INTEGER NOT NULL,
            data TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS generation_spans_by_job ON stage_spans(job_id);
        "#,
    )?;
    // 仅迁移本功能早期使用的 JSON 表；其他版本的同名列式表不能假定有 data 列。
    let mut statement = conn.prepare("PRAGMA table_info(model_calls)")?;
    let columns: Vec<String> = statement
        .query_map([], |row| row.get(1))?
        .collect::<Result<_, _>>()?;
    if ["id", "project_id", "job_id", "attempt", "data"]
        .iter()
        .all(|name| columns.iter().any(|column| column == name))
    {
        conn.execute(
            "INSERT OR IGNORE INTO generation_model_calls
             (id, project_id, job_id, attempt, data)
             SELECT id, project_id, job_id, attempt, data FROM model_calls",
            [],
        )?;
    }
    Ok(())
}

/// 初始化一轮视频生成；自动恢复沿用同一任务和记录。
pub fn begin_run(state: &AppState, job: &JobRow) -> ApiResult<()> {
    if !matches!(job.job.kind, JobKind::Generate | JobKind::Revise) {
        return Ok(());
    }
    let config = state.model_config.load();
    state.db.write(|conn| {
        conn.execute(
            "INSERT OR IGNORE INTO generation_runs (job_id, project_id, config)
             VALUES (?1, ?2, ?3)",
            params![
                job.job.id,
                job.job.project_id,
                json!({
                    "text_provider": config.provider_llm1,
                    "text_model": config.llm1.model,
                    "tts_provider": config.provider_llm4,
                    "app_version": env!("CARGO_PKG_VERSION"),
                })
                .to_string()
            ],
        )?;
        // 进程中断时无法知道上游是否已经扣费，不能把未结束请求当成零用量。
        for table in ["generation_model_calls", "stage_spans"] {
            conn.execute(
                &format!(
                    "UPDATE {table}
                     SET data = json_set(data, '$.status', 'interrupted')
                     WHERE job_id = ?1
                       AND json_extract(data, '$.status') = 'running'
                       AND attempt < ?2"
                ),
                params![job.job.id, job.attempt],
            )?;
        }
        Ok(())
    })
}

/// 异步调用归属；使用 task-local，避免并发镜头或并发项目串账。
#[derive(Clone)]
struct Context {
    /// 数据库。
    db: Arc<Db>,
    /// 项目。
    project_id: String,
    /// 生成任务。
    job_id: String,
    /// 恢复尝试编号。
    attempt: i64,
    /// 当前阶段。
    stage: StageKey,
    /// 当前操作名称。
    label: String,
}

tokio::task_local! {
    /// 当前异步操作的内部记录上下文。
    static CURRENT: Context;
}

/// 写入追加式记录，项目删除后不再插入孤儿明细。
fn persist(ctx: &Context, table: &str, id: &str, data: &Value) -> ApiResult<()> {
    ctx.db.write(|conn| {
        conn.execute(
            &format!(
                "INSERT INTO {table} (id, project_id, job_id, attempt, data)
                 SELECT ?1, ?2, ?3, ?4, ?5
                 WHERE EXISTS (SELECT 1 FROM projects WHERE id = ?2)
                 ON CONFLICT(id) DO UPDATE SET data = excluded.data"
            ),
            params![
                id,
                ctx.project_id,
                ctx.job_id,
                ctx.attempt,
                data.to_string()
            ],
        )?;
        Ok(())
    })
}

/// 操作计时器；被取消或异常退出时保留 interrupted 状态。
struct Span {
    /// 归属信息。
    ctx: Context,
    /// 记录 ID。
    id: String,
    /// 记录内容。
    data: Value,
    /// 单调计时。
    started: Instant,
}

impl Drop for Span {
    fn drop(&mut self) {
        self.data["elapsed_ms"] = json!(self.started.elapsed().as_millis() as u64);
        self.data["finished_at"] = json!(now_iso());
        if self.data["status"] == "running" {
            self.data["status"] = json!("interrupted");
        }
        if let Err(error) = persist(&self.ctx, "stage_spans", &self.id, &self.data) {
            tracing::error!("生成耗时记录写入失败：{error:?}");
        }
    }
}

/// 包住现有生成操作；内部请求另行记录模型用量。
pub async fn operation<T>(
    state: &AppState,
    job_id: &str,
    attempt: i64,
    stage: StageKey,
    label: &str,
    future: impl Future<Output = ProviderResult<T>>,
) -> ProviderResult<T> {
    let project_id = state
        .db
        .read(|conn| {
            Ok(conn
                .query_row(
                    "SELECT project_id FROM generation_runs WHERE job_id = ?1",
                    [job_id],
                    |row| row.get::<_, String>(0),
                )
                .optional()?)
        })
        .map_err(|_| ProviderError::unavailable("无法读取本地生成记录，请检查本机数据库。"))?;
    let Some(project_id) = project_id else {
        return future.await;
    };
    let ctx = Context {
        db: state.db.clone(),
        project_id,
        job_id: job_id.into(),
        attempt,
        stage,
        label: label.into(),
    };
    let mut span = Span {
        ctx: ctx.clone(),
        id: make_id("span"),
        data: json!({
            "stage": stage,
            "label": label,
            "attempt": attempt,
            "started_at": now_iso(),
            "finished_at": null,
            "elapsed_ms": null,
            "status": "running",
        }),
        started: Instant::now(),
    };
    persist(&ctx, "stage_spans", &span.id, &span.data)
        .map_err(|_| ProviderError::unavailable("无法保存本地生成记录，请检查本机数据库。"))?;
    let result = CURRENT.scope(ctx, future).await;
    span.data["status"] = json!(if result.is_ok() {
        "succeeded"
    } else {
        "failed"
    });
    result
}

/// 从供应商返回结构提取总 Token；缓存和推理等细分字段是子集，不重复相加。
fn normalize_usage(raw: &Value) -> Value {
    let integer = |keys: &[&str]| {
        keys.iter()
            .find_map(|key| raw.get(*key).and_then(Value::as_u64))
    };
    let input = integer(&["prompt_tokens", "input_tokens"]);
    let output = integer(&["completion_tokens", "output_tokens"]);
    let total = integer(&["total_tokens"]).or_else(|| Some(input? + output?));
    json!({
        "input_tokens": input,
        "output_tokens": output,
        "total_tokens": total,
        "cached_input_tokens": raw
            .pointer("/prompt_tokens_details/cached_tokens")
            .and_then(Value::as_u64),
        "raw": raw,
    })
}

/// 单个实际模型请求记录；请求发出前创建，收到响应后立即保存 usage。
pub struct ModelCall {
    /// 没有活动生成任务时不写入。
    ctx: Option<Context>,
    /// 记录 ID。
    id: String,
    /// 记录内容。
    data: Value,
    /// 单调计时。
    started: Instant,
    /// 已收尾后 Drop 不再覆盖状态。
    completed: bool,
}

impl ModelCall {
    /// 在真正发送网络请求前开始记录。
    pub fn start(provider: &str, model: &str, kind: &str) -> ProviderResult<Self> {
        let ctx = CURRENT.try_with(Clone::clone).ok();
        let call = Self {
            ctx,
            id: make_id("call"),
            data: json!({
                "provider": provider,
                "model": model,
                "kind": kind,
                "stage": Value::Null,
                "label": Value::Null,
                "started_at": now_iso(),
                "finished_at": null,
                "elapsed_ms": null,
                "status": "running",
                "http_status": null,
                "usage": normalize_usage(&Value::Null),
            }),
            started: Instant::now(),
            completed: false,
        };
        call.save()
            .map_err(|_| ProviderError::unavailable("无法保存本地模型用量，请检查数据库。"))?;
        Ok(call)
    }

    /// 保存当前记录；同一 ID 重写不会产生重复调用。
    fn save(&self) -> ApiResult<()> {
        if let Some(ctx) = &self.ctx {
            let mut data = self.data.clone();
            data["stage"] = json!(ctx.stage);
            data["label"] = json!(ctx.label);
            data["attempt"] = json!(ctx.attempt);
            data["id"] = json!(self.id);
            persist(ctx, "generation_model_calls", &self.id, &data)?;
        }
        Ok(())
    }

    /// 记录 HTTP 状态与供应商请求 ID。
    pub fn response(&mut self, response: &reqwest::Response) {
        self.data["http_status"] = json!(response.status().as_u16());
        self.data["provider_request_id"] = json!(response
            .headers()
            .get("x-request-id")
            .or_else(|| response.headers().get("request-id"))
            .and_then(|value| value.to_str().ok()));
    }

    /// 请求结束后先保存用量；内容解析失败也不丢失已经发生的调用。
    pub fn finish(&mut self, succeeded: bool, usage: &Value) {
        self.data["usage"] = normalize_usage(usage);
        self.data["status"] = json!(if succeeded { "succeeded" } else { "failed" });
        self.close();
    }

    /// 结束计时并持久化。
    fn close(&mut self) {
        self.data["elapsed_ms"] = json!(self.started.elapsed().as_millis() as u64);
        self.data["finished_at"] = json!(now_iso());
        self.completed = true;
        if let Err(error) = self.save() {
            tracing::error!("模型用量记录写入失败：{error:?}");
        }
    }
}

impl Drop for ModelCall {
    fn drop(&mut self) {
        if !self.completed {
            self.data["status"] = json!("interrupted");
            self.close();
        }
    }
}

/// 成片完成后汇总端到端耗时和供应商已报告的总 Token。
pub fn completion_summary(
    conn: &Connection,
    job_id: &str,
    started_at: &str,
    finished_at: &str,
) -> ApiResult<Value> {
    let (total_tokens, missing_calls, calls): (i64, i64, i64) = conn.query_row(
        "SELECT
            COALESCE(SUM(CAST(json_extract(data, '$.usage.total_tokens') AS INTEGER)), 0),
            COALESCE(SUM(CASE
                WHEN json_extract(data, '$.usage.total_tokens') IS NULL THEN 1
                ELSE 0
            END), 0),
            COUNT(*)
         FROM generation_model_calls
         WHERE job_id = ?1",
        [job_id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    let elapsed_ms = chrono::DateTime::parse_from_rfc3339(finished_at)
        .ok()
        .zip(chrono::DateTime::parse_from_rfc3339(started_at).ok())
        .map(|(end, start)| (end - start).num_milliseconds().max(0));
    Ok(json!({
        "elapsed_ms": elapsed_ms,
        "total_tokens": if calls > 0 { Value::from(total_tokens) } else { Value::Null },
        "token_complete": calls > 0 && missing_calls == 0,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 隔离数据库。
    struct Fixture {
        /// 临时目录。
        root: std::path::PathBuf,
        /// 数据库。
        db: Arc<Db>,
    }

    impl Fixture {
        /// 创建项目与一个生成任务，供内部归属测试使用。
        fn new() -> Self {
            let root = std::env::temp_dir().join(make_id("usage-test"));
            let db = Arc::new(Db::open(&root.join("test.sqlite3")).unwrap());
            db.write(|conn| {
                for id in ["project-a", "project-b"] {
                    conn.execute(
                        "INSERT INTO projects
                         (id, user_id, title, status, draft_revision, draft_dirty, draft, created_at, updated_at)
                         VALUES (?1, 'local_user', 'test', 'draft', 1, 0, ?2, ?3, ?3)",
                        params![
                            id,
                            serde_json::to_string(&crate::catalog::empty_draft()).unwrap(),
                            now_iso()
                        ],
                    )?;
                }
                Ok(())
            })
            .unwrap();
            Self { root, db }
        }

        /// 构造相同形状的调用上下文。
        fn context(&self, project: &str) -> Context {
            Context {
                db: self.db.clone(),
                project_id: project.into(),
                job_id: format!("job-{project}"),
                attempt: 1,
                stage: StageKey::Visuals,
                label: "镜头 1".into(),
            }
        }

        /// 读取内部模型调用记录。
        fn calls(&self) -> Vec<Value> {
            self.db
                .read(|conn| {
                    let mut statement =
                        conn.prepare("SELECT data FROM generation_model_calls ORDER BY rowid")?;
                    let rows: Vec<String> = statement
                        .query_map([], |row| row.get(0))?
                        .collect::<Result<_, _>>()?;
                    Ok(rows
                        .iter()
                        .map(|text| serde_json::from_str(text).unwrap())
                        .collect())
                })
                .unwrap()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn legacy_column_table_is_preserved() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE model_calls (
                id TEXT PRIMARY KEY, job_id TEXT NOT NULL, attempt INTEGER NOT NULL,
                total_tokens INTEGER
             );
             INSERT INTO model_calls VALUES ('legacy', 'job-old', 1, 5);",
        )
        .unwrap();
        initialize(&conn).unwrap();
        assert_eq!(
            conn.query_row(
                "SELECT total_tokens FROM model_calls WHERE id = 'legacy'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            5
        );
    }

    #[tokio::test]
    async fn concurrent_projects_keep_usage_separate() {
        let fixture = Fixture::new();
        let first = CURRENT.scope(fixture.context("project-a"), async {
            let mut call = ModelCall::start("text", "model-a", "text").unwrap();
            tokio::task::yield_now().await;
            call.finish(true, &json!({"prompt_tokens": 8, "completion_tokens": 2}));
        });
        let second = CURRENT.scope(fixture.context("project-b"), async {
            let mut call = ModelCall::start("tts", "model-b", "tts").unwrap();
            tokio::task::yield_now().await;
            call.finish(true, &Value::Null);
        });
        tokio::join!(first, second);
        assert_eq!(fixture.calls().len(), 2);
        fixture
            .db
            .read(|conn| {
                assert_eq!(
                    conn.query_row(
                        "SELECT count(*) FROM generation_model_calls WHERE project_id = 'project-a'",
                        [],
                        |row| row.get::<_, i64>(0)
                    )?,
                    1
                );
                Ok(())
            })
            .unwrap();
    }

    #[tokio::test]
    async fn cancellation_and_repeated_save_do_not_duplicate_calls() {
        let fixture = Fixture::new();
        CURRENT
            .scope(fixture.context("project-a"), async {
                let mut call = ModelCall::start("text", "model-a", "text").unwrap();
                call.finish(true, &json!({"input_tokens": 5, "output_tokens": 3}));
                call.save().unwrap();
                let pending = async {
                    let _call = ModelCall::start("text", "model-a", "text").unwrap();
                    std::future::pending::<()>().await;
                };
                assert!(
                    tokio::time::timeout(std::time::Duration::from_millis(10), pending)
                        .await
                        .is_err()
                );
            })
            .await;
        let calls = fixture.calls();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0]["usage"]["total_tokens"], 8);
        assert_eq!(calls[1]["status"], "interrupted");
    }

    #[tokio::test]
    async fn response_usage_is_kept_when_business_json_is_invalid() {
        use axum::{routing::post, Json, Router};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new().route(
                    "/chat/completions",
                    post(|| async {
                        Json(json!({
                            "choices": [{"message": {"content": "not json"}}],
                            "usage": {
                                "prompt_tokens": 12,
                                "completion_tokens": 4,
                                "total_tokens": 16
                            }
                        }))
                    }),
                ),
            )
            .await
            .unwrap();
        });
        let fixture = Fixture::new();
        let mut config = crate::Config::from_env();
        config.provider_llm1 = "custom".into();
        config.llm1_base = format!("http://{address}");
        config.llm1.api_key = "test-only".into();
        let client = Arc::new(crate::providers::siliconflow::SiliconFlow::new(&config).unwrap());
        let script = crate::providers::siliconflow::GlmScript::new(client, &config);
        CURRENT
            .scope(fixture.context("project-a"), async {
                use crate::providers::ScriptModel;
                let draft = crate::catalog::empty_draft();
                let request = crate::providers::ScriptRequest {
                    ctx: crate::providers::ProductionContext {
                        brief: None,
                        settings: draft.settings,
                        user_text: "test".into(),
                        answers: vec![],
                        speech_rate: 1.0,
                    },
                    brief: crate::models::Brief {
                        subject: "test".into(),
                        audience: "test".into(),
                        purpose: "test".into(),
                        style: "test".into(),
                    },
                };
                assert!(script.write_script(request).await.is_err());
            })
            .await;
        assert_eq!(fixture.calls()[0]["usage"]["total_tokens"], 16);
        server.abort();
    }

    #[test]
    fn completion_summary_sums_tokens_and_marks_missing_usage() {
        let conn = Connection::open_in_memory().unwrap();
        initialize(&conn).unwrap();
        conn.execute(
            "INSERT INTO generation_model_calls VALUES
             ('one', 'project', 'job', 1, ?1),
             ('two', 'project', 'job', 1, ?2)",
            params![
                json!({"usage": {"total_tokens": 120}}).to_string(),
                json!({"usage": {"total_tokens": null}}).to_string()
            ],
        )
        .unwrap();
        let summary = completion_summary(
            &conn,
            "job",
            "2026-11-14T00:00:00Z",
            "2026-11-14T00:01:05.500Z",
        )
        .unwrap();
        assert_eq!(summary["total_tokens"], 120);
        assert_eq!(summary["elapsed_ms"], 65_500);
        assert_eq!(summary["token_complete"], false);
    }
}
