//! 任务查询、回答追问、停止、重试，以及成片版本与导出（文档 5.6～5.8）。

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use rusqlite::Connection;
use serde::Deserialize;
use serde_json::json;

use super::{ok, page_limit, parse_body, parse_cursor, reply, CurrentUser, Idempotency};
use crate::db::{self, JobRow};
use crate::error::{ApiError, ApiResult};
use crate::models::*;
use crate::ops;
use crate::state::AppState;
use crate::util::{now, parse_iso, preview, short};

/// 读取任务并校验所属项目属于当前用户；否则统一返回 404。
fn job_for(conn: &Connection, job_id: &str, user: &str) -> ApiResult<JobRow> {
    let job =
        db::load_job(conn, job_id)?.ok_or_else(|| ApiError::not_found("没有找到制作任务。"))?;
    db::project_for(conn, &job.job.project_id, user)
        .map_err(|_| ApiError::not_found("没有找到制作任务。"))?;
    Ok(job)
}

/// `GET /jobs/{id}`：完整任务状态。HTTP 200 只表示查询成功，不代表任务成功。
pub async fn get(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(job_id): Path<String>,
) -> ApiResult<Response> {
    let job = state.db.read(|conn| job_for(conn, &job_id, &user))?;
    Ok(ok(json!({ "job": job.job })))
}

/// 回答追问的请求体。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AnswerBody {
    /// 回答的问题 ID，必须是任务当前的问题。
    question_id: String,
    /// 回答内容。
    value: String,
}

/// `POST /jobs/{id}/answers`：回答追问并继续同一任务（幂等）。
///
/// 原子校验问题仍有效，记录回答后把任务放回队列，不会创建第二次生成。
pub async fn answer(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(job_id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<Response> {
    let idem = Idempotency::from_headers(
        &headers,
        &user,
        "POST",
        format!("/jobs/{job_id}/answers"),
        &body,
    )?;
    let input: AnswerBody = parse_body(&body)?;
    let value = input.value.trim().to_string();
    if value.is_empty() || value.chars().count() > 5000 {
        return Err(ApiError::validation("请填写需要补充的信息。"));
    }
    let response = state.db.write(|t| {
        if let Some(response) = idem.replay(t)? {
            return Ok(response);
        }
        let mut job = job_for(t, &job_id, &user)?;
        // 问题必须仍是当前问题且未过期，防止重复回答或回答已失效的问题。
        let active = job.job.status == JobStatus::WaitingInput
            && job.job.question.as_ref().is_some_and(|q| {
                q.id == input.question_id && parse_iso(&q.expires_at).is_some_and(|at| at > now())
            });
        if !active {
            return Err(ApiError::conflict(
                "QUESTION_NOT_ACTIVE",
                "这个问题已失效，请刷新当前创作。",
            ));
        }
        let mut message = ops::new_message(&job.job.project_id, "user", &value, Some(&job.job.id));
        message.asset_ids = vec![];
        db::insert_message(t, &message)?;
        t.emit(
            &job.job.project_id,
            "message.created",
            json!({ "message_id": message.id }),
        )?;
        job.input.answers.push(Answer {
            question_id: input.question_id.clone(),
            value: value.clone(),
        });
        job.job.question = None;
        job.job.status = JobStatus::Queued;
        if let Some(id) = &job.assistant_message_id {
            ops::update_blocks(t, id, |blocks| blocks.retain(|b| b["type"] != "question"))?;
        }
        db::save_job(t, &job)?;
        if let Some(mut project) = db::load_project(t, &job.job.project_id)? {
            project.project.status = "processing".into();
            db::save_project(t, &mut project)?;
        }
        ops::emit_job_updated(t, &job.job, "已收到补充信息，继续制作")?;
        tracing::info!(target: "api", "{} 收到用户回答「{}」，放回队列继续", short(&job.job.id), preview(&value, 30));
        let data = json!({ "job_id": job.job.id, "job_status": job.job.status });
        idem.store(t, StatusCode::ACCEPTED, &data)?;
        Ok(reply(StatusCode::ACCEPTED, data))
    })?;
    state.worker.wake();
    Ok(response)
}

/// `POST /jobs/{id}/cancel`：请求停止任务。
///
/// 活动任务改为 `cancelling` 并返回 202，由执行者或调度器收尾为 `cancelled`；
/// 已取消返回 200；已成功或失败返回 `409 JOB_ALREADY_FINISHED`。
pub async fn cancel(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(job_id): Path<String>,
) -> ApiResult<Response> {
    let status = state.db.write(|t| {
        let mut job = job_for(t, &job_id, &user)?;
        match job.job.status {
            JobStatus::Cancelled => Ok(JobStatus::Cancelled),
            JobStatus::Cancelling => Ok(JobStatus::Cancelling),
            JobStatus::Succeeded | JobStatus::Failed => Err(ApiError::conflict(
                "JOB_ALREADY_FINISHED",
                "这次创作已经结束。",
            )
            .details(json!({ "job_status": job.job.status }))),
            _ => {
                job.job.status = JobStatus::Cancelling;
                db::save_job(t, &job)?;
                ops::emit_job_updated(t, &job.job, "正在停止")?;
                Ok(JobStatus::Cancelling)
            }
        }
    })?;
    state.worker.cancel(&job_id);
    tracing::info!(target: "api", "{} 用户请求停止（当前 {:?}）", short(&job_id), status);
    let code = if status == JobStatus::Cancelled {
        StatusCode::OK
    } else {
        StatusCode::ACCEPTED
    };
    Ok(reply(
        code,
        json!({ "job_id": job_id, "job_status": status }),
    ))
}

/// 重试的请求体。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RetryBody {
    /// 前端看到的草稿版本。
    expected_draft_revision: i64,
}

/// `POST /jobs/{id}/retry`：基于失败或取消的任务创建新任务（幂等）。
///
/// 复用原任务的输入与检查点，从最早失效的阶段继续。
/// 若草稿在失败后被修改过，返回 `409 RETRY_INPUT_CHANGED`，要求按当前草稿重新生成。
pub async fn retry(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(job_id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<Response> {
    let idem = Idempotency::from_headers(
        &headers,
        &user,
        "POST",
        format!("/jobs/{job_id}/retry"),
        &body,
    )?;
    let input: RetryBody = parse_body(&body)?;
    let response = state.db.write(|t| {
        if let Some(response) = idem.replay(t)? {
            return Ok(response);
        }
        let parent = job_for(t, &job_id, &user)?;
        // 只允许重试已取消的任务，或可重试的失败任务。
        let retryable = parent.job.status == JobStatus::Cancelled
            || (parent.job.status == JobStatus::Failed && parent.job.error.as_ref().is_some_and(|e| e.retryable));
        if !retryable {
            return Err(ApiError::conflict("JOB_NOT_RETRYABLE", "这次创作暂时不能重试，可以调整需求后重新生成。"));
        }
        let mut project = db::project_for(t, &parent.job.project_id, &user)?;
        ops::assert_idle(&project)?;
        ops::assert_revision(&project, input.expected_draft_revision)?;
        // 失败后用户改过草稿：旧任务的输入已过时，要求用户按当前草稿重新生成。
        if project.project.draft_revision != parent.checkpoint_revision {
            return Err(ApiError::conflict("RETRY_INPUT_CHANGED", "草稿已经改变，请根据当前内容重新生成。"));
        }
        // 复用原输入与检查点；演示故障开关不带入重试。
        let mut job_input = parent.input.clone();
        // 演示故障开关不带入重试，否则重试会再次失败。
        job_input.demo_scenario = None;
        let job = ops::create_job(t, &mut project, parent.job.kind, job_input, Some(parent.job.id.clone()))?;
        let data = json!({ "job_id": job.job.id, "parent_job_id": parent.job.id, "job_status": job.job.status });
        idem.store(t, StatusCode::ACCEPTED, &data)?;
        tracing::info!(target: "api", "{} 重试 → 新任务 {}，从检查点继续", short(&parent.job.id), short(&job.job.id));
        Ok(reply(StatusCode::ACCEPTED, data))
    })?;
    state.worker.wake();
    Ok(response)
}

// ---------- 成片版本 ----------

/// 分页查询参数。
#[derive(Deserialize)]
pub struct ListQuery {
    /// 上一页返回的 `next_cursor`。
    cursor: Option<String>,
    /// 每页数量。
    limit: Option<i64>,
}

/// `GET /projects/{id}/versions`：成片版本列表（省略快照与播放地址）。
pub async fn versions(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(project_id): Path<String>,
    Query(query): Query<ListQuery>,
) -> ApiResult<Response> {
    let before = parse_cursor(query.cursor.as_deref())?;
    let limit = page_limit(query.limit, 20);
    let data = state.db.read(|conn| {
        db::project_for(conn, &project_id, &user)?;
        let (rows, next_cursor) = db::list_versions(conn, &project_id, before, limit)?;
        let items = rows
            .into_iter()
            .map(|row| state.media.version_api(conn, row, false))
            .collect::<ApiResult<Vec<_>>>()?;
        Ok(json!({ "items": items, "next_cursor": next_cursor }))
    })?;
    Ok(ok(data))
}

/// `GET /projects/{id}/versions/{vid}`：版本详情，含播放地址与历史草稿快照。
pub async fn version(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path((project_id, version_id)): Path<(String, String)>,
) -> ApiResult<Response> {
    let data = state.db.read(|conn| {
        db::project_for(conn, &project_id, &user)?;
        let row = db::load_version(conn, &version_id)?
            .filter(|v| v.project_id == project_id)
            .ok_or_else(|| ApiError::not_found("这个视频版本不存在。"))?;
        Ok(json!({ "version": state.media.version_api(conn, row, true)? }))
    })?;
    Ok(ok(data))
}

/// 导出请求体。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExportBody {
    /// 容器格式，目前只支持 `mp4`。
    format: String,
    /// 分辨率，取值来自 capabilities。
    resolution: String,
    /// 帧率，目前只支持 30。
    fps: u32,
}

/// `POST /projects/{id}/versions/{vid}/exports`：导出指定成片（幂等）。
///
/// 已有相同规格的文件时直接返回 200 与 `cached=true`；否则创建导出任务并返回 202。
pub async fn export(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path((project_id, version_id)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<Response> {
    let idem = Idempotency::from_headers(
        &headers,
        &user,
        "POST",
        format!("/projects/{project_id}/versions/{version_id}/exports"),
        &body,
    )?;
    let input: ExportBody = parse_body(&body)?;
    if input.format != "mp4"
        || input.fps != 30
        || !state
            .catalog
            .export_resolutions
            .contains(&input.resolution.as_str())
    {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNSUPPORTED_EXPORT",
            format!(
                "目前支持 {} MP4 30fps 导出。",
                state.catalog.export_resolutions.join(" / ")
            ),
        ));
    }
    let response = state.db.write(|t| {
        if let Some(response) = idem.replay(t)? {
            return Ok(response);
        }
        let mut project = db::project_for(t, &project_id, &user)?;
        let version = db::load_version(t, &version_id)?
            .filter(|v| v.project_id == project_id)
            .ok_or_else(|| ApiError::not_found("这个视频版本不存在。"))?;
        // 已有相同规格的文件：直接复用。
        // 已有相同规格的文件：直接复用，无需重新渲染。
        if let Some(existing) = version.exports.iter().find(|e| {
            e.format == input.format && e.resolution == input.resolution && e.fps == input.fps
        }) {
            let data = json!({ "asset_id": existing.asset_id, "cached": true });
            idem.store(t, StatusCode::OK, &data)?;
            tracing::info!(target: "api", "版本 {} 导出 {}：复用已有文件", short(&version.id), input.resolution);
            return Ok(reply(StatusCode::OK, data));
        }
        ops::assert_idle(&project)?;
        let job_input = JobInput {
            text: format!("导出 {} {}", input.resolution, input.format),
            export: Some(ExportSpec {
                version_id: version.id.clone(),
                format: input.format.clone(),
                resolution: input.resolution.clone(),
                fps: input.fps,
            }),
            ..Default::default()
        };
        let job = ops::create_job(t, &mut project, JobKind::Export, job_input, None)?;
        tracing::info!(target: "api", "版本 {} 导出 {} → 任务 {} 已排队", short(&version.id), input.resolution, short(&job.job.id));
        let data = json!({ "job_id": job.job.id, "job_status": job.job.status });
        idem.store(t, StatusCode::ACCEPTED, &data)?;
        Ok(reply(StatusCode::ACCEPTED, data))
    })?;
    state.worker.wake();
    Ok(response)
}
