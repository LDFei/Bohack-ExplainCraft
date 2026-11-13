//! 接口层与工作线程共用的状态变更：创建任务、更新消息、结束任务。
//! 这些函数都在调用方的写事务内执行，并写入对应事件。

use rusqlite::Connection;
use serde_json::{json, Value};

use crate::db::{self, JobRow, ProjectRow, Tx};
use crate::error::{ApiError, ApiResult};
use crate::models::*;
use crate::util::{make_id, now_iso};

/// 任务被停止后 assistant 消息显示的文字。
pub const CANCELLED_TEXT: &str = "本次制作已停止。你可以调整想法后重新开始，之前的作品会为你保留。";

/// 构造一条新消息（尚未写库）。用户消息直接是 `completed`，assistant 消息从 `pending` 开始。
pub fn new_message(project_id: &str, role: &str, text: &str, job_id: Option<&str>) -> Message {
    Message {
        id: make_id("msg"),
        project_id: project_id.into(),
        role: role.into(),
        text: text.into(),
        status: if role == "user" {
            "completed".into()
        } else {
            "pending".into()
        },
        asset_ids: vec![],
        job_id: job_id.map(Into::into),
        blocks: vec![],
        last_delta_seq: 0,
        created_at: now_iso(),
    }
}

/// 按任务类型生成初始阶段列表：生成 / 修改走全部八个阶段，聊天没有阶段，导出只有 render。
pub fn stages_for(kind: JobKind) -> Vec<Stage> {
    let keys: Vec<StageKey> = match kind {
        JobKind::Chat => vec![],
        JobKind::Export => vec![StageKey::Render],
        _ => StageKey::ALL.to_vec(),
    };
    keys.into_iter()
        .map(|key| Stage {
            key,
            status: "pending".into(),
            progress_percent: Some(0),
            message: key.label().into(),
        })
        .collect()
}

/// 进度估算：已完成 / 跳过的阶段占比。
pub fn recompute_progress(job: &mut Job) {
    if job.stages.is_empty() {
        return;
    }
    let done = job
        .stages
        .iter()
        .filter(|s| matches!(s.status.as_str(), "succeeded" | "skipped"))
        .count();
    let running = job.stages.iter().filter(|s| s.status == "running").count();
    // 运行中的阶段按 30% 计，让进度条在长阶段中也有变化；成功前最多显示 99%。
    let percent = (done * 100 + running * 30) / job.stages.len();
    job.progress_percent = Some(percent.min(99) as u8);
}

/// 写入 `job.updated` 事件，携带任务摘要和一句给用户看的进度说明。
pub fn emit_job_updated(t: &mut Tx, job: &Job, message: &str) -> ApiResult<()> {
    t.emit(
        &job.project_id,
        "job.updated",
        json!({
            "project_id": job.project_id,
            "job_id": job.id,
            "status": job.status,
            "current_stage": job.current_stage,
            "progress_percent": job.progress_percent,
            "message": message,
        }),
    )?;
    Ok(())
}

/// 项目摘要状态（文档 3.1）。
pub fn summary_status(
    conn: &Connection,
    project: &Project,
    active: Option<JobStatus>,
) -> ApiResult<String> {
    Ok(match active {
        Some(JobStatus::WaitingInput) => "waiting_input".into(),
        Some(_) => "processing".into(),
        None => match db::last_production_status(conn, &project.id)? {
            Some(JobStatus::Failed) => "failed".into(),
            _ if project.latest_video_version_id.is_some() => "ready".into(),
            _ => "draft".into(),
        },
    })
}

/// 释放项目的活动任务并重算状态。
pub fn release_project(conn: &Connection, project: &mut ProjectRow) -> ApiResult<()> {
    project.project.active_job_id = None;
    project.project.status = summary_status(conn, &project.project, None)?;
    Ok(())
}

/// 校验项目没有活动任务，否则返回 `409 PROJECT_BUSY`。
pub fn assert_idle(project: &ProjectRow) -> ApiResult<()> {
    match &project.project.active_job_id {
        Some(id) => Err(ApiError::project_busy(Some(id))),
        None => Ok(()),
    }
}

/// 校验草稿版本与请求一致，否则返回 `409 REVISION_CONFLICT` 并附带当前版本。
pub fn assert_revision(project: &ProjectRow, expected: i64) -> ApiResult<()> {
    if project.project.draft_revision != expected {
        return Err(ApiError::conflict(
            "REVISION_CONFLICT",
            "草稿已更新，请刷新后再提交；你的输入会保留。",
        )
        .details(json!({ "draft_revision": project.project.draft_revision })));
    }
    Ok(())
}

/// 创建任务（和 assistant 消息），项目进入 processing。返回 (job, assistant_message_id)。
pub fn create_job(
    t: &mut Tx,
    project: &mut ProjectRow,
    kind: JobKind,
    input: JobInput,
    parent_job_id: Option<String>,
) -> ApiResult<JobRow> {
    let job_id = make_id("job");
    let assistant_id = if kind == JobKind::Export {
        None
    } else {
        let mut assistant = new_message(&project.project.id, "assistant", "", Some(&job_id));
        if kind != JobKind::Chat {
            assistant.blocks = vec![json!({ "type": "progress", "job_id": job_id })];
        }
        db::insert_message(t, &assistant)?;
        Some(assistant.id)
    };
    let row = JobRow {
        job: Job {
            id: job_id.clone(),
            project_id: project.project.id.clone(),
            parent_job_id,
            kind,
            status: JobStatus::Queued,
            current_stage: None,
            progress_percent: Some(0),
            stages: stages_for(kind),
            question: None,
            result: None,
            error: None,
            created_at: now_iso(),
            started_at: None,
            finished_at: None,
        },
        input,
        attempt: 0,
        checkpoint_revision: project.project.draft_revision,
        assistant_message_id: assistant_id.clone(),
    };
    db::insert_job(t, &row)?;
    project.project.active_job_id = Some(job_id);
    project.project.status = "processing".into();
    db::save_project(t, project)?;
    if let Some(id) = &assistant_id {
        t.emit(
            &project.project.id,
            "message.created",
            json!({ "message_id": id }),
        )?;
    }
    emit_job_updated(t, &row.job, "已加入制作队列")?;
    Ok(row)
}

/// 向 assistant 消息追加一段可见文本（message.delta）。
pub fn append_text(t: &mut Tx, message_id: &str, delta: &str) -> ApiResult<()> {
    let Some(mut message) = db::load_message(t, message_id)? else {
        return Ok(());
    };
    message.text.push_str(delta);
    message.last_delta_seq += 1;
    message.status = "streaming".into();
    db::save_message(t, &message)?;
    t.emit(
        &message.project_id,
        "message.delta",
        json!({ "message_id": message.id, "delta_seq": message.last_delta_seq, "text_delta": delta }),
    )?;
    Ok(())
}

/// 修改消息（完整文本、状态、卡片），写 message.completed。
pub fn finish_message(
    t: &mut Tx,
    message_id: &str,
    text: Option<&str>,
    status: &str,
    blocks: impl FnOnce(&mut Vec<Value>),
) -> ApiResult<()> {
    let Some(mut message) = db::load_message(t, message_id)? else {
        return Ok(());
    };
    if let Some(text) = text {
        message.text = text.into();
    }
    message.status = status.into();
    blocks(&mut message.blocks);
    db::save_message(t, &message)?;
    t.emit(
        &message.project_id,
        "message.completed",
        json!({ "message_id": message.id }),
    )?;
    Ok(())
}

/// 只更新消息卡片，不改变状态。调用方在同一事务内还会写 draft/job 事件，前端据此刷新快照。
pub fn update_blocks(
    t: &mut Tx,
    message_id: &str,
    blocks: impl FnOnce(&mut Vec<Value>),
) -> ApiResult<()> {
    let Some(mut message) = db::load_message(t, message_id)? else {
        return Ok(());
    };
    blocks(&mut message.blocks);
    db::save_message(t, &message)?;
    Ok(())
}

/// 把 cancelling 的任务收尾为 cancelled。
pub fn finalize_cancelled(t: &mut Tx, job_id: &str) -> ApiResult<bool> {
    let Some(mut job) = db::load_job(t, job_id)? else {
        return Ok(false);
    };
    if job.job.status != JobStatus::Cancelling {
        return Ok(false);
    }
    let Some(mut project) = db::load_project(t, &job.job.project_id)? else {
        return Ok(false);
    };
    job.job.status = JobStatus::Cancelled;
    job.job.question = None;
    job.job.finished_at = Some(now_iso());
    for stage in &mut job.job.stages {
        if matches!(stage.status.as_str(), "running" | "pending") {
            stage.status = "cancelled".into();
        }
    }
    job.checkpoint_revision = project.project.draft_revision;
    db::save_job(t, &job)?;
    if let Some(message_id) = &job.assistant_message_id {
        finish_message(t, message_id, Some(CANCELLED_TEXT), "cancelled", |_| {})?;
    }
    if project.project.active_job_id.as_deref() == Some(job_id) {
        release_project(t, &mut project)?;
        db::save_project(t, &mut project)?;
    }
    emit_job_updated(t, &job.job, "已停止")?;
    t.emit(
        &job.job.project_id,
        "job.cancelled",
        json!({ "job_id": job_id }),
    )?;
    Ok(true)
}

/// 把任务置为失败（调用方负责 fencing 校验）。
pub fn mark_failed(
    t: &mut Tx,
    job: &mut JobRow,
    project: &mut ProjectRow,
    error: JobError,
) -> ApiResult<()> {
    job.job.status = JobStatus::Failed;
    job.job.question = None;
    job.job.finished_at = Some(now_iso());
    for stage in &mut job.job.stages {
        let failed_here = Some(stage.key) == error.failed_stage;
        if failed_here || stage.status == "running" {
            stage.status = "failed".into();
        }
    }
    job.checkpoint_revision = project.project.draft_revision;
    job.job.error = Some(error.clone());
    db::save_job(t, job)?;
    if let Some(message_id) = job.assistant_message_id.clone() {
        let block = json!({
            "type": "error",
            "job_id": job.job.id,
            "code": error.code,
            "message": error.message,
        });
        finish_message(t, &message_id, Some(&error.message), "failed", |blocks| {
            blocks.push(block)
        })?;
    }
    if project.project.active_job_id.as_deref() == Some(job.job.id.as_str()) {
        release_project(t, project)?;
    }
    db::save_project(t, project)?;
    t.emit(
        &job.job.project_id,
        "job.failed",
        json!({
            "job_id": job.job.id,
            "code": error.code,
            "message": error.message,
            "retryable": error.retryable,
        }),
    )?;
    Ok(())
}
