//! 能力、项目、快照、消息与草稿接口（文档 5.1～5.4）。

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use base64::Engine;
use rusqlite::Connection;
use serde::Deserialize;
use serde_json::{json, Value};

use super::{ok, page_limit, parse_body, parse_cursor, reply, CurrentUser, Idempotency};
use crate::catalog::empty_draft;
use crate::db::{self, ProjectRow};
use crate::error::{ApiError, ApiResult};
use crate::models::*;
use crate::ops;
use crate::state::AppState;
use crate::util::{make_id, now_iso, preview, short};

/// 项目列表游标的编码方式（URL 安全、无填充）。
const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::URL_SAFE_NO_PAD;

/// `GET /capabilities`：可用参数、默认值与上传限制。
pub async fn capabilities(State(state): State<AppState>, _user: CurrentUser) -> Response {
    ok(state.catalog.capabilities(
        &state.config.public_base(),
        state.config.event_retention_seconds,
        crate::providers::Providers::is_demo(&state.config),
    ))
}

// ---------- 快照 ----------

/// 组装项目快照（文档 5.2）：项目、草稿、活动任务、最新成片、最近 50 条消息与事件游标。
///
/// 在同一次加锁读取中完成，保证各部分来自同一个一致状态；
/// 前端随后用 `event_cursor` 订阅事件，不会漏掉快照之后的变化。
pub fn build_snapshot(state: &AppState, conn: &Connection, row: &ProjectRow) -> ApiResult<Value> {
    let mut draft = row.draft.clone();
    state.media.hydrate_draft(conn, &mut draft)?;
    let active_job = match &row.project.active_job_id {
        Some(id) => db::load_job(conn, id)?.map(|j| j.job),
        None => None,
    };
    let latest_video = match &row.project.latest_video_version_id {
        Some(id) => match db::load_version(conn, id)? {
            Some(version) => Some(state.media.version_api(conn, version, true)?),
            None => None,
        },
        None => None,
    };
    let (items, next_cursor) = db::list_messages(conn, &row.project.id, None, 50)?;
    Ok(json!({
        "project": row.project,
        "draft": draft,
        "active_job": active_job,
        "latest_video": latest_video,
        "messages_page": { "items": items, "next_cursor": next_cursor },
        "event_cursor": row.event_seq.to_string(),
    }))
}

/// `GET /projects/{id}`：工作台快照。
pub async fn snapshot(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(project_id): Path<String>,
) -> ApiResult<Response> {
    let data = state.db.read(|conn| {
        let row = db::project_for(conn, &project_id, &user)?;
        build_snapshot(&state, conn, &row)
    })?;
    Ok(ok(data))
}

// ---------- 项目 ----------

/// 分页查询参数。
#[derive(Deserialize)]
pub struct ListQuery {
    /// 上一页返回的 `next_cursor`。
    cursor: Option<String>,
    /// 每页数量。
    limit: Option<i64>,
}

/// `GET /projects`：左侧历史项目列表。
pub async fn list(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Query(query): Query<ListQuery>,
) -> ApiResult<Response> {
    let limit = page_limit(query.limit, 20);
    let after = match query.cursor.as_deref().filter(|c| !c.is_empty()) {
        Some(cursor) => {
            let text = B64
                .decode(cursor)
                .ok()
                .and_then(|b| String::from_utf8(b).ok())
                .ok_or_else(|| ApiError::bad_request("分页游标无效。"))?;
            let (updated, id) = text
                .split_once('|')
                .ok_or_else(|| ApiError::bad_request("分页游标无效。"))?;
            Some((updated.to_string(), id.to_string()))
        }
        None => None,
    };
    let rows = state
        .db
        .read(|conn| db::list_projects(conn, &user, after, limit + 1))?;
    let more = rows.len() as i64 > limit;
    let items: Vec<Project> = rows
        .into_iter()
        .take(limit as usize)
        .map(|r| r.project)
        .collect();
    let next_cursor = if more {
        items
            .last()
            .map(|p| B64.encode(format!("{}|{}", p.updated_at, p.id)))
    } else {
        None
    };
    Ok(ok(json!({ "items": items, "next_cursor": next_cursor })))
}

/// 创建项目的请求体。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateBody {
    /// 标题，省略时为“新的创作”。
    title: Option<String>,
}

/// 校验并去除标题首尾空白：1～100 字。
fn valid_title(title: &str) -> ApiResult<String> {
    let title = title.trim();
    if title.is_empty() || title.chars().count() > 100 {
        return Err(ApiError::validation("标题需要在 1～100 字之间。"));
    }
    Ok(title.to_string())
}

/// `POST /projects`：创建空项目（幂等）。创建本身不调用模型。
pub async fn create(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<Response> {
    let idem = Idempotency::from_headers(&headers, &user, "POST", "/projects".into(), &body)?;
    let input: CreateBody = parse_body(&body)?;
    let title = match input.title {
        Some(title) => valid_title(&title)?,
        None => "新的创作".into(),
    };
    state.db.write(|t| {
        if let Some(response) = idem.replay(t)? {
            return Ok(response);
        }
        let now = now_iso();
        let mut row = ProjectRow {
            project: Project {
                id: make_id("prj"),
                title,
                status: "draft".into(),
                draft_revision: 1,
                active_job_id: None,
                latest_video_version_id: None,
                draft_dirty: true,
                created_at: now.clone(),
                updated_at: now,
            },
            user_id: user.clone(),
            draft: empty_draft(),
            artifacts: Artifacts::default(),
            event_seq: 0,
            pruned_through: 0,
        };
        db::insert_project(t, &row)?;
        db::save_project(t, &mut row)?;
        let data = json!({ "project": row.project });
        idem.store(t, StatusCode::CREATED, &data)?;
        tracing::info!(target: "api", "新建项目 {}「{}」", short(&row.project.id), row.project.title);
        Ok(reply(StatusCode::CREATED, data))
    })
}

/// 修改标题的请求体。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RenameBody {
    /// 新标题。
    title: String,
}

/// `PATCH /projects/{id}`：修改标题。不增加草稿版本，制作期间也允许。
pub async fn rename(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(project_id): Path<String>,
    body: Bytes,
) -> ApiResult<Response> {
    let input: RenameBody = parse_body(&body)?;
    let title = valid_title(&input.title)?;
    let project = state.db.write(|t| {
        let mut row = db::project_for(t, &project_id, &user)?;
        row.project.title = title;
        db::save_project(t, &mut row)?;
        Ok(row.project)
    })?;
    tracing::info!(target: "api", "项目 {} 改名为「{}」", short(&project.id), project.title);
    Ok(ok(json!({ "project": project })))
}

/// `DELETE /projects/{id}`：删除项目。有活动任务时返回 409；文件在后台清理。
pub async fn remove(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(project_id): Path<String>,
) -> ApiResult<StatusCode> {
    let files = state.db.write(|t| {
        let row = db::project_for(t, &project_id, &user)?;
        // 制作中的项目不能删除，避免任务写入已删除的项目。
        ops::assert_idle(&row)?;
        db::delete_project(t, &project_id)
    })?;
    tracing::info!(target: "api", "删除项目 {}，后台清理 {} 个文件", short(&project_id), files.len());
    // 文件在后台清理；已签发的临时链接因记录删除立即失效。
    let dir = state.media.storage_dir.join(&project_id);
    tokio::spawn(async move {
        for file in files {
            let _ = tokio::fs::remove_file(file).await;
        }
        let _ = tokio::fs::remove_dir_all(dir).await;
    });
    Ok(StatusCode::NO_CONTENT)
}

// ---------- 消息 ----------

/// `GET /projects/{id}/messages`：向更早方向分页加载消息。
pub async fn messages(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(project_id): Path<String>,
    Query(query): Query<ListQuery>,
) -> ApiResult<Response> {
    let before = parse_cursor(query.cursor.as_deref())?;
    let limit = page_limit(query.limit, 50);
    let (items, next_cursor) = state.db.read(|conn| {
        db::project_for(conn, &project_id, &user)?;
        db::list_messages(conn, &project_id, before, limit)
    })?;
    Ok(ok(json!({ "items": items, "next_cursor": next_cursor })))
}

/// 发送消息的请求体（文档 5.3）。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MessageBody {
    /// `generate` / `revise` / `chat`。
    intent: String,
    /// 消息内容，1～5000 字。
    text: String,
    /// 附带的素材，必须属于本项目且已就绪。
    #[serde(default)]
    asset_ids: Vec<String>,
    /// 前端看到的草稿版本，用于冲突检测。
    expected_draft_revision: i64,
    /// revise 时必须是当前最新成片。
    #[serde(default)]
    base_video_version_id: Option<String>,
}

/// 校验素材属于本项目且已就绪。
fn check_assets(
    conn: &Connection,
    project_id: &str,
    ids: &[String],
    kind: Option<&str>,
) -> ApiResult<()> {
    let mut not_ready = vec![];
    for id in ids {
        match db::load_asset(conn, id)? {
            Some(row)
                if row.asset.project_id == project_id
                    && kind.is_none_or(|k| row.asset.kind == k) =>
            {
                if row.asset.status != "ready" {
                    not_ready.push(id.clone());
                }
            }
            _ => {
                return Err(ApiError::validation("所选素材不属于当前项目。")
                    .details(json!({ "fields": [{ "path": "asset_ids", "reason": id }] })))
            }
        }
    }
    if !not_ready.is_empty() {
        return Err(
            ApiError::conflict("ASSET_NOT_READY", "请等待所选素材准备完成。")
                .details(json!({ "asset_ids": not_ready })),
        );
    }
    Ok(())
}

/// `POST /projects/{id}/messages`：唯一的创作入口（幂等）。
///
/// 在一个事务内写入用户消息、assistant 消息、任务和幂等记录，提交后再唤醒 worker。
/// 返回 202 只表示已受理，制作进度通过 SSE 推送。
pub async fn send_message(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(project_id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<Response> {
    let idem = Idempotency::from_headers(
        &headers,
        &user,
        "POST",
        format!("/projects/{project_id}/messages"),
        &body,
    )?;
    let input: MessageBody = parse_body(&body)?;
    let kind = match input.intent.as_str() {
        "generate" => JobKind::Generate,
        "revise" => JobKind::Revise,
        "chat" => JobKind::Chat,
        _ => {
            return Err(ApiError::validation(
                "intent 只能是 generate、revise 或 chat。",
            ))
        }
    };
    let text = input.text.trim().to_string();
    if text.is_empty() || text.chars().count() > 5000 {
        return Err(ApiError::validation("请输入 1～5000 字的内容。"));
    }
    // 演示开关：仅在使用演示供应商时生效，用于体验失败重试与追问。
    // 演示开关：仅在使用演示供应商时生效，用于体验失败重试与追问。
    let demo_scenario = headers
        .get("x-demo-scenario")
        .and_then(|v| v.to_str().ok())
        .filter(|v| matches!(*v, "failure" | "question"))
        .filter(|_| crate::providers::Providers::is_demo(&state.config))
        .map(str::to_string);

    let response = state.db.write(|t| {
        // 网络重发：返回首次受理的结果，不重复创建消息和任务。
        if let Some(response) = idem.replay(t)? {
            return Ok(response);
        }
        let mut project = db::project_for(t, &project_id, &user)?;
        ops::assert_idle(&project)?;
        ops::assert_revision(&project, input.expected_draft_revision)?;
        // revise 必须基于最新成片，防止在旧版本上修改后覆盖新版本。
        if kind == JobKind::Revise
            && (project.project.latest_video_version_id.is_none()
                || input.base_video_version_id != project.project.latest_video_version_id)
        {
            return Err(ApiError::conflict(
                "BASE_VERSION_MISMATCH",
                "请回到最新版本后继续修改。",
            ));
        }
        check_assets(t, &project_id, &input.asset_ids, None)?;

        let mut user_message = ops::new_message(&project_id, "user", &text, None);
        user_message.asset_ids = input.asset_ids.clone();
        // 保存完整输入，重试时原样复用；仅讨论不使用附件。
        let job_input = JobInput {
            text: text.clone(),
            asset_ids: if kind == JobKind::Chat {
                vec![]
            } else {
                input.asset_ids.clone()
            },
            base_video_version_id: input.base_video_version_id.clone(),
            demo_scenario: demo_scenario.clone(),
            ..Default::default()
        };
        db::insert_message(t, &user_message)?;
        t.emit(
            &project_id,
            "message.created",
            json!({ "message_id": user_message.id }),
        )?;
        let job = ops::create_job(t, &mut project, kind, job_input, None)?;
        // 用户消息关联本次任务，便于前端展示进度。
        t.execute(
            "UPDATE messages SET job_id = ?2 WHERE id = ?1",
            rusqlite::params![user_message.id, job.job.id],
        )?;
        let data = json!({
            "user_message_id": user_message.id,
            "assistant_message_id": job.assistant_message_id,
            "job_id": job.job.id,
            "job_status": "queued",
        });
        idem.store(t, StatusCode::ACCEPTED, &data)?;
        tracing::info!(
            target: "api",
            "项目 {} 收到{}请求「{}」{}→ 任务 {} 已排队",
            short(&project_id),
            match kind {
                JobKind::Generate => "生成",
                JobKind::Revise => "修改",
                JobKind::Chat => "讨论",
                JobKind::Export => "导出",
            },
            preview(&text, 30),
            if input.asset_ids.is_empty() {
                String::new()
            } else {
                format!("（附带 {} 个素材）", input.asset_ids.len())
            },
            short(&job.job.id)
        );
        Ok(reply(StatusCode::ACCEPTED, data))
    })?;
    state.worker.wake();
    Ok(response)
}

// ---------- 草稿 ----------

/// 保存草稿的请求体。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PatchBody {
    /// 前端看到的草稿版本。
    expected_draft_revision: i64,
    /// 要整体替换的顶层字段。
    changes: DraftChanges,
}

/// 422 错误，并在 `details.fields` 中指出字段路径与原因。
fn field_error(path: &str, reason: &str) -> ApiError {
    ApiError::validation(reason.to_string())
        .details(json!({ "fields": [{ "path": path, "reason": reason }] }))
}

/// `PATCH /projects/{id}/draft`：保存脚本、分镜与设置（文档 5.4）。
///
/// 逐项校验后按修改内容计算失效阶段：脚本变化 → 分镜及之后；画面变化 → 画面及之后；
/// 音频变化 → 音频及之后；只改音量 → 剪辑及之后；调色特效 → 调色与输出。
/// 保存只更新草稿，需要用户点“更新视频”才会重新生成。
pub async fn patch_draft(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(project_id): Path<String>,
    body: Bytes,
) -> ApiResult<Response> {
    let input: PatchBody = parse_body(&body)?;
    let changes = input.changes;
    let catalog = state.catalog.clone();
    if changes.script.is_some() && changes.shots.is_some() {
        return Err(ApiError::validation("请分别保存完整脚本与分镜旁白。"));
    }
    let data = state.db.write(|t| {
        let mut project = db::project_for(t, &project_id, &user)?;
        ops::assert_idle(&project)?;
        ops::assert_revision(&project, input.expected_draft_revision)?;
        let mut draft = project.draft.clone();
        let mut invalid: Vec<StageKey> = vec![];

        // 设置：时长变化需要重新分镜；比例或风格变化需要重新出画面；语言变化需要重新配音。
        if let Some(settings) = changes.settings {
            if settings.target_duration_seconds < catalog.duration_min
                || settings.target_duration_seconds > catalog.duration_max
            {
                return Err(field_error(
                    "settings.target_duration_seconds",
                    &format!(
                        "目标时长需要在 {}～{} 秒之间。",
                        catalog.duration_min, catalog.duration_max
                    ),
                ));
            }
            if !catalog
                .aspect_ratios
                .contains(&settings.aspect_ratio.as_str())
            {
                return Err(field_error("settings.aspect_ratio", "不支持这个画面比例。"));
            }
            if !catalog.languages.contains(&settings.language.as_str()) {
                return Err(field_error("settings.language", "不支持这个语言。"));
            }
            if !catalog.has_style(&settings.style_id) {
                return Err(field_error("settings.style_id", "不支持这个风格。"));
            }
            let old = &draft.settings;
            if settings.target_duration_seconds != old.target_duration_seconds {
                invalid.extend(StageKey::Storyboard.and_after());
            }
            if settings.aspect_ratio != old.aspect_ratio || settings.style_id != old.style_id {
                invalid.extend(StageKey::Visuals.and_after());
                drop_generated_visuals(t, &mut draft.shots, None)?;
            }
            if settings.language != old.language {
                invalid.extend(StageKey::Audio.and_after());
            }
            draft.settings = settings;
        }

        // 脚本：内容变化后，旧分镜与之后的产物都失效。
        if let Some(script) = changes.script {
            let title = script.title.trim();
            if title.is_empty() || title.chars().count() > 100 {
                return Err(field_error("script.title", "标题需要在 1～100 字之间。"));
            }
            if script.text.trim().is_empty() || script.text.chars().count() > 20000 {
                return Err(field_error("script.text", "脚本需要在 1～20000 字之间。"));
            }
            if draft.script.as_ref() != Some(&script) {
                invalid.extend(StageKey::Storyboard.and_after());
            }
            draft.script = Some(Script {
                title: title.into(),
                text: script.text,
            });
        }

        // 分镜：逐个镜头比较，旁白或时长变化只影响配音，画面描述或素材变化只影响画面。
        if let Some(shots) = changes.shots {
            if shots.is_empty() || shots.len() > 100 {
                return Err(field_error("shots", "分镜需要 1～100 个镜头。"));
            }
            let mut seen = std::collections::HashSet::new();
            let mut next = vec![];
            for (index, mut shot) in shots.into_iter().enumerate() {
                shot.thumbnail_url = None;
                if shot.duration_ms <= 0 {
                    return Err(field_error(
                        &format!("shots[{index}].duration_ms"),
                        "镜头时长必须大于 0。",
                    ));
                }
                check_assets(t, &project_id, &shot.asset_ids, None)?;
                if shot.id.is_empty() {
                    shot.id = make_id("shot");
                    invalid.extend([StageKey::Visuals, StageKey::Audio]);
                } else {
                    if !seen.insert(shot.id.clone()) {
                        return Err(field_error(&format!("shots[{index}].id"), "镜头 ID 重复。"));
                    }
                    let Some(old) = draft.shots.iter().find(|s| s.id == shot.id) else {
                        return Err(field_error(&format!("shots[{index}].id"), "镜头不存在。"));
                    };
                    if old.narration != shot.narration || old.duration_ms != shot.duration_ms {
                        invalid.push(StageKey::Audio);
                    }
                    // 只改了画面描述、没换素材：丢弃模型生成的旧画面（保留用户上传），让画面阶段重新生成。
                    if old.visual_prompt != shot.visual_prompt && old.asset_ids == shot.asset_ids {
                        let mut one = vec![shot.clone()];
                        drop_generated_visuals(t, &mut one, None)?;
                        shot.asset_ids = one.remove(0).asset_ids;
                    }
                    if old.asset_ids != shot.asset_ids || old.visual_prompt != shot.visual_prompt {
                        invalid.push(StageKey::Visuals);
                    }
                }
                next.push(shot);
            }
            if next.len() != draft.shots.len() {
                invalid.push(StageKey::Audio);
            }
            // 分镜旁白是脚本的来源：同步重建完整解说词。
            // 分镜旁白是脚本的来源：同步重建完整解说词，避免两份文本互相覆盖。
            let text = next
                .iter()
                .map(|s| s.narration.trim())
                .filter(|n| !n.is_empty())
                .collect::<Vec<_>>()
                .join("\n");
            let title = draft
                .script
                .as_ref()
                .map(|s| s.title.clone())
                .unwrap_or_else(|| project.project.title.clone());
            draft.script = Some(Script { title, text });
            draft.shots = next;
            invalid.extend(StageKey::Edit.and_after());
        }

        // 音频：换音色、语速或音乐需要重新准备音频；只改音量只需重新剪辑混音。
        if let Some(mut audio) = changes.audio {
            if !catalog.has_voice(&audio.voice_id) {
                return Err(field_error("audio.voice_id", "不支持这个音色。"));
            }
            if !(0.5..=2.0).contains(&audio.speech_rate) {
                return Err(field_error(
                    "audio.speech_rate",
                    "语速需要在 0.5～2.0 之间。",
                ));
            }
            if !(0.0..=1.0).contains(&audio.narration_volume)
                || !(0.0..=1.0).contains(&audio.music_volume)
            {
                return Err(field_error("audio", "音量需要在 0～1 之间。"));
            }
            match audio.music_mode.as_str() {
                "auto" | "none" => {
                    audio.music_id = None;
                    audio.music_asset_id = None;
                }
                "library" => {
                    if !audio
                        .music_id
                        .as_deref()
                        .is_some_and(|id| catalog.has_music(id))
                    {
                        return Err(field_error("audio.music_id", "请选择曲库中的音乐。"));
                    }
                    audio.music_asset_id = None;
                }
                "upload" => {
                    let Some(id) = audio.music_asset_id.clone() else {
                        return Err(field_error("audio.music_asset_id", "请选择已上传的音频。"));
                    };
                    check_assets(t, &project_id, &[id], Some("audio"))?;
                    audio.music_id = None;
                }
                _ => return Err(field_error("audio.music_mode", "音乐模式不正确。")),
            }
            let old = &draft.audio;
            let music_changed = old.music_mode != audio.music_mode
                || old.music_id != audio.music_id
                || old.music_asset_id != audio.music_asset_id;
            audio.resolved_music_asset_id = if music_changed {
                None
            } else {
                old.resolved_music_asset_id.clone()
            };
            if music_changed
                || old.voice_id != audio.voice_id
                || old.speech_rate != audio.speech_rate
            {
                invalid.extend(StageKey::Audio.and_after());
            } else if old.narration_volume != audio.narration_volume
                || old.music_volume != audio.music_volume
            {
                invalid.extend(StageKey::Edit.and_after());
            }
            draft.audio = audio;
        }

        // 调色与特效：只影响调色和输出两个阶段。
        if let Some(finishing) = changes.finishing {
            if !catalog.has_color(&finishing.color_preset_id) {
                return Err(field_error(
                    "finishing.color_preset_id",
                    "不支持这个调色预设。",
                ));
            }
            if !catalog.has_effect(&finishing.effect_preset_id) {
                return Err(field_error(
                    "finishing.effect_preset_id",
                    "不支持这个特效预设。",
                ));
            }
            if finishing != draft.finishing {
                invalid.extend([StageKey::Finishing, StageKey::Render]);
            }
            draft.finishing = finishing;
        }

        // 与尚未处理的失效阶段合并：之前保存过但还没生成的修改依然有效。
        draft.invalidated_stages = merge_stages(&draft.invalidated_stages, &invalid);
        project.draft = draft;
        project.project.draft_revision += 1;
        project.project.draft_dirty = true;
        db::save_project(t, &mut project)?;
        t.emit(
            &project_id,
            "draft.updated",
            json!({ "project_id": project_id, "draft_revision": project.project.draft_revision }),
        )?;
        tracing::info!(
            target: "api",
            "项目 {} 保存草稿 → 版本 {}，待更新阶段 {:?}",
            short(&project_id),
            project.project.draft_revision,
            project.draft.invalidated_stages
        );
        let mut draft = project.draft.clone();
        state.media.hydrate_draft(t, &mut draft)?;
        Ok(json!({
            "draft_revision": project.project.draft_revision,
            "draft": draft,
            "draft_dirty": true,
        }))
    })?;
    Ok(ok(data))
}

/// 去掉镜头里由模型生成 / 素材库挑选的画面（保留用户上传），让画面阶段重新准备。
fn drop_generated_visuals(
    conn: &Connection,
    shots: &mut [Shot],
    only: Option<&str>,
) -> ApiResult<()> {
    for shot in shots
        .iter_mut()
        .filter(|s| only.is_none_or(|id| s.id == id))
    {
        let mut kept = vec![];
        for id in &shot.asset_ids {
            if let Some(row) = db::load_asset(conn, id)? {
                if row.asset.source == "upload" {
                    kept.push(id.clone());
                }
            }
        }
        shot.asset_ids = kept;
    }
    Ok(())
}
