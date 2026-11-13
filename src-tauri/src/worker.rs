//! 后台任务调度：任务状态全部在 SQLite 中，进程重启后可以恢复。
//!
//! - 认领 queued 任务时 attempt + 1，作为该次执行的 fencing token；
//! - 启动时把上次中断的 running 任务放回队列，从检查点继续；
//! - 收尾没有执行者的 cancelling 任务；处理追问超时；定期清理过期事件。

use std::time::Duration;

use tokio_util::sync::CancellationToken;

use crate::db;
use crate::error::ApiResult;
use crate::models::*;
use crate::ops;
use crate::pipeline::Runner;
use crate::state::AppState;
use crate::util::{iso, now, now_iso, parse_iso, short};

/// 启动后台调度循环。
///
/// 循环由两种信号驱动：接口层的主动唤醒（新任务、取消）与 500ms 定时轮询（兜底）。
pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        match recover(&state) {
            Ok(0) => tracing::info!(target: "worker", "调度器已启动，没有需要恢复的任务"),
            Ok(count) => {
                tracing::info!(target: "worker", "调度器已启动，恢复了 {count} 个上次中断的任务")
            }
            Err(error) => tracing::error!(target: "worker", "恢复中断任务失败：{error:?}"),
        }
        let mut tick = tokio::time::interval(Duration::from_millis(500));
        // 首次循环就执行一次事件清理。
        let mut last_prune = std::time::Instant::now() - Duration::from_secs(3600);
        loop {
            tokio::select! {
                _ = state.worker.wake.notified() => {}
                _ = tick.tick() => {}
            }
            if let Err(error) = schedule(&state) {
                tracing::error!(target: "worker", "调度出错：{error:?}");
            }
            if last_prune.elapsed() > Duration::from_secs(600) {
                last_prune = std::time::Instant::now();
                let cutoff =
                    iso(now() - chrono::Duration::seconds(state.config.event_retention_seconds));
                match state.db.write(|t| db::prune_events(t, &cutoff)) {
                    Ok(count) if count > 0 => {
                        tracing::info!(target: "worker", "已清理 {count} 条过期事件")
                    }
                    Err(error) => tracing::error!(target: "worker", "清理过期事件失败：{error:?}"),
                    _ => {}
                }
            }
        }
    });
}

/// 进程启动：中断的 running 任务回到队列（attempt 在认领时递增，旧尝试自动失效）。
fn recover(state: &AppState) -> ApiResult<usize> {
    state.db.write(|t| {
        // 上次退出时仍在 running 的任务没有执行者了：放回队列，从检查点继续。
        // attempt 在重新认领时递增，旧执行者（若仍残留）的写入会被 fencing 拒绝。
        let interrupted = db::jobs_with_status(t, JobStatus::Running)?;
        let count = interrupted.len();
        for mut job in interrupted {
            tracing::info!(
                target: "worker",
                "{} 上次运行被中断，放回队列，将从检查点继续",
                short(&job.job.id)
            );
            job.job.status = JobStatus::Queued;
            for stage in &mut job.job.stages {
                if stage.status == "running" {
                    stage.status = "pending".into();
                }
            }
            db::save_job(t, &job)?;
            ops::emit_job_updated(t, &job.job, "服务已恢复，继续制作")?;
        }
        Ok(count)
    })
}

/// 一次调度：处理追问超时 → 收尾无执行者的取消 → 认领排队任务。
///
/// 认领在事务内把 `queued` 改为 `running` 并递增 `attempt`；
/// 即使有多个调度者同时认领，也只有一个能看到 `queued` 状态。
fn schedule(state: &AppState) -> ApiResult<()> {
    // 追问超时
    // 1. 追问超时：等待超过 expires_at 的任务置为失败，不无限占用项目。
    let waiting = state
        .db
        .read(|conn| db::jobs_with_status(conn, JobStatus::WaitingInput))?;
    for job in waiting {
        let expired = job
            .job
            .question
            .as_ref()
            .and_then(|q| parse_iso(&q.expires_at))
            .is_some_and(|at| at <= now());
        if expired {
            state.db.write(|t| {
                let Some(mut job) = db::load_job(t, &job.job.id)? else {
                    return Ok(());
                };
                if job.job.status != JobStatus::WaitingInput {
                    return Ok(());
                }
                let Some(mut project) = db::load_project(t, &job.job.project_id)? else {
                    return Ok(());
                };
                let error = JobError {
                    code: "INPUT_TIMEOUT".into(),
                    message: "这次提问已过期，可以重新开始创作。".into(),
                    retryable: true,
                    failed_stage: Some(StageKey::Planning),
                };
                tracing::warn!(target: "worker", "{} 追问超时未回答，任务失败", short(&job.job.id));
                ops::mark_failed(t, &mut job, &mut project, error)
            })?;
        }
    }

    // 没有执行者的取消请求（排队中、等待回答中、或执行者已退出）
    // 2. 取消收尾：排队中、等待回答中的任务没有执行者，由调度器直接置为 cancelled；
    //    正在执行的任务由执行者自己收尾。
    let cancelling = state
        .db
        .read(|conn| db::jobs_with_status(conn, JobStatus::Cancelling))?;
    for job in cancelling {
        if !state.worker.is_running(&job.job.id) {
            state
                .db
                .write(|t| ops::finalize_cancelled(t, &job.job.id))?;
            tracing::info!(target: "worker", "{} 已停止（未在执行中，直接收尾）", short(&job.job.id));
        }
    }

    // 认领排队任务
    // 3. 认领排队任务，受并发上限约束。
    let queued = state
        .db
        .read(|conn| db::jobs_with_status(conn, JobStatus::Queued))?;
    for job in queued {
        let running = state
            .worker
            .running
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .len();
        if running >= state.config.max_concurrent_jobs {
            break;
        }
        if state.worker.is_running(&job.job.id) {
            continue;
        }
        let claimed = state.db.write(|t| {
            let Some(mut job) = db::load_job(t, &job.job.id)? else {
                return Ok(None);
            };
            if job.job.status != JobStatus::Queued {
                return Ok(None);
            }
            // fencing token：本次执行尝试的唯一编号。
            job.attempt += 1;
            job.job.status = JobStatus::Running;
            job.job.started_at.get_or_insert_with(now_iso);
            db::save_job(t, &job)?;
            if let Some(mut project) = db::load_project(t, &job.job.project_id)? {
                if project.project.status == "waiting_input" {
                    project.project.status = "processing".into();
                    db::save_project(t, &mut project)?;
                }
            }
            ops::emit_job_updated(t, &job.job, "开始制作")?;
            Ok(Some(job.attempt))
        })?;
        let Some(attempt) = claimed else { continue };
        let token = CancellationToken::new();
        state
            .worker
            .running
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(job.job.id.clone(), token.clone());
        let state = state.clone();
        let job_id = job.job.id.clone();
        tokio::spawn(async move {
            let runner = Runner::new(state.clone(), job_id.clone(), attempt, token);
            // 单独的任务里执行，panic 也不会拖垮调度器。
            // 在独立任务中执行：即使 panic 也只影响这一个任务，不会拖垮调度器。
            let outcome = tokio::spawn(runner.run()).await;
            if outcome.is_err() {
                tracing::error!(target: "worker", "{} 执行时发生 panic，任务置为可重试的失败", short(&job_id));
                let _ = state.db.write(|t| {
                    let Some(mut job) = db::load_job(t, &job_id)? else {
                        return Ok(());
                    };
                    if job.attempt != attempt || job.job.status != JobStatus::Running {
                        return Ok(());
                    }
                    let Some(mut project) = db::load_project(t, &job.job.project_id)? else {
                        return Ok(());
                    };
                    let error = JobError {
                        code: "INTERNAL_ERROR".into(),
                        message: "制作过程中出现异常，已保留已完成的部分，可以重试。".into(),
                        retryable: true,
                        failed_stage: job.job.current_stage,
                    };
                    ops::mark_failed(t, &mut job, &mut project, error)
                });
            }
            state
                .worker
                .running
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .remove(&job_id);
            // 执行完成后立即唤醒调度器，让排队的任务尽快开始。
            state.worker.wake();
        });
    }
    Ok(())
}
