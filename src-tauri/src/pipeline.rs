//! 制作流程编排：选题 → 文案 → 分镜 → 画面 / 音频（并行）→ 剪辑 → 调色字幕 → 输出。
//!
//! - 每个阶段完成后把产物写回草稿（draft_revision + 1）并从 `invalidated_stages` 中移除，
//!   作为检查点；失败重试、对话修改都从仍然失效的阶段继续，已有产物直接复用。
//! - 每次数据库写入都校验 fencing token（attempt）与任务状态，旧 worker、取消后迟到的
//!   结果都无法覆盖新状态。
//! - 供应商调用与取消令牌竞争，用户停止后尽快放弃等待。

use std::collections::HashMap;
use std::future::Future;

use serde_json::json;
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

use crate::db::{self, AssetRow, JobRow, ProjectRow, Tx, VersionRow};
use crate::error::ApiError;
use crate::models::*;
use crate::ops;
use crate::providers::*;
use crate::state::AppState;
use crate::util::{elapsed, iso, make_id, now, now_iso, short};

/// 一次任务执行尝试。由 worker 在认领任务后创建，执行结束即丢弃。
///
/// 同一任务的每次执行尝试有不同的 `attempt`；所有写入都校验它，
/// 因此被取代的旧执行者即使还在运行，也无法再修改任务。
pub struct Runner {
    /// 全局状态。
    state: AppState,
    /// 正在执行的任务。
    job_id: String,
    /// 本次执行尝试的 fencing token。
    attempt: i64,
    /// 用户请求停止时触发。
    cancel: CancellationToken,
    /// 本次执行开始的时间，用于日志中的总耗时。
    started: std::time::Instant,
}

/// 流程提前结束的原因。
enum Stop {
    /// 用户取消。
    Cancelled,
    /// 执行尝试已失效（被取消或被新的尝试取代）。
    Fenced,
    /// 进入等待用户回答，释放执行线程。
    Paused,
    /// 失败，携带给用户看的原因。
    Failed(JobError),
}

impl From<ApiError> for Stop {
    fn from(error: ApiError) -> Self {
        if error.is_fenced() {
            Stop::Fenced
        } else {
            Stop::Failed(JobError {
                code: "INTERNAL_ERROR".into(),
                message: "制作过程中出现异常，已保留已完成的部分，可以重试。".into(),
                retryable: true,
                failed_stage: None,
            })
        }
    }
}

/// 流程中每一步的结果类型。
type Step<T> = Result<T, Stop>;

/// 从用户明确的“60秒 / 60s”文本提取目标时长，避免默认值覆盖需求。
fn requested_duration_seconds(text: &str) -> Option<u32> {
    for (start, _) in text.char_indices().filter(|(_, ch)| ch.is_ascii_digit()) {
        let tail = &text[start..];
        let digits = tail
            .chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>();
        let suffix = tail[digits.len()..].trim_start();
        if suffix.starts_with('秒') || suffix.starts_with('s') || suffix.starts_with('S') {
            if let Ok(seconds) = digits.parse::<u32>() {
                if (15..=300).contains(&seconds) {
                    return Some(seconds);
                }
            }
        }
    }
    None
}

/// 检测明确超过 300 秒的需求，不能用时长上限静默截断。
fn duration_exceeds_limit(text: &str) -> bool {
    text.char_indices().any(|(start, _)| {
        let tail = &text[start..];
        let digits = tail
            .chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>();
        let suffix = tail[digits.len()..].trim_start();
        (suffix.starts_with('秒') || suffix.starts_with('s') || suffix.starts_with('S'))
            && digits.parse::<u32>().is_ok_and(|seconds| seconds > 300)
    })
}

#[cfg(test)]
mod duration_tests {
    use super::{duration_exceeds_limit, requested_duration_seconds};

    #[test]
    fn honors_explicit_duration_and_rejects_overlong_requests() {
        assert_eq!(requested_duration_seconds("请做60秒的视频"), Some(60));
        assert_eq!(requested_duration_seconds("请做300秒的视频"), Some(300));
        assert_eq!(requested_duration_seconds("请做301秒的视频"), None);
        assert!(duration_exceeds_limit("请做301秒的视频"));
        assert!(!duration_exceeds_limit("请做300秒的视频"));
    }
}

/// 把供应商错误转为任务失败，并标记失败阶段。
fn provider_error(stage: StageKey, error: ProviderError) -> Stop {
    Stop::Failed(JobError {
        code: error.code.into(),
        message: error.message,
        retryable: error.retryable,
        failed_stage: Some(stage),
    })
}

/// 配音指纹：旁白、音色、语速、语言、时长都相同时可以复用已生成的配音。
///
/// 只取 SHA-256 的前 12 字节，足以区分同一镜头的不同版本。
fn narration_key(shot: &Shot, audio: &AudioSettings, language: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(format!(
        "{}|{}|{}|{}|{}",
        shot.narration, audio.voice_id, audio.speech_rate, language, shot.duration_ms
    ));
    hex::encode(&hasher.finalize()[..12])
}

/// 由当前草稿和任务输入组装供应商使用的制作上下文。
fn context(draft: &Draft, input: &JobInput) -> ProductionContext {
    ProductionContext {
        brief: draft.brief.clone(),
        settings: draft.settings.clone(),
        user_text: input.text.clone(),
        answers: input.answers.clone(),
    }
}

impl Runner {
    /// 创建执行者。
    pub fn new(state: AppState, job_id: String, attempt: i64, cancel: CancellationToken) -> Self {
        Self {
            state,
            job_id,
            attempt,
            cancel,
            started: std::time::Instant::now(),
        }
    }

    /// 日志中的任务标识（短 ID）。
    fn tag(&self) -> &str {
        short(&self.job_id)
    }

    /// 打印阶段日志：`[序号/8] 阶段名 状态 说明`。
    fn log_stage(&self, key: StageKey, status: &str, note: &str) {
        let index = StageKey::ALL.iter().position(|k| *k == key).unwrap_or(0) + 1;
        let label = key.label();
        let job = self.tag();
        match status {
            "running" => tracing::info!(target: "job", "{job} ▶ [{index}/8] {label} 开始"),
            "skipped" => {
                tracing::info!(target: "job", "{job} ↷ [{index}/8] {label} 跳过（沿用已有结果）")
            }
            _ if note.is_empty() => {
                tracing::info!(target: "job", "{job} ✓ [{index}/8] {label} 完成")
            }
            _ => tracing::info!(target: "job", "{job} ✓ [{index}/8] {label} 完成：{note}"),
        }
    }

    /// 执行任务直到结束、暂停或失败，并把结果写回数据库。
    ///
    /// 不返回错误：所有结局都转为任务状态（succeeded / failed / cancelled / waiting_input）。
    pub async fn run(self) {
        let loaded = self.state.db.read(|conn| db::load_job(conn, &self.job_id));
        let Ok(Some(job)) = loaded else { return };
        tracing::info!(
            target: "job",
            "{} 开始执行 {}（项目 {}，第 {} 次执行）",
            self.tag(),
            match job.job.kind {
                JobKind::Generate => "生成视频",
                JobKind::Revise => "修改视频",
                JobKind::Chat => "仅讨论",
                JobKind::Export => "导出",
            },
            short(&job.job.project_id),
            self.attempt
        );
        let result = match job.job.kind {
            JobKind::Generate | JobKind::Revise => self.produce().await,
            JobKind::Chat => self.chat().await,
            JobKind::Export => self.export().await,
        };
        match result {
            Ok(()) | Err(Stop::Paused) => {}
            Err(Stop::Cancelled) | Err(Stop::Fenced) => self.settle_cancel(),
            Err(Stop::Failed(error)) => self.fail(error),
        }
    }

    // ---------- 事务与调用工具 ----------

    /// 带 fencing 校验的写事务：只有当前执行尝试且任务仍在 running 时才能写入。
    fn guarded<T>(
        &self,
        f: impl FnOnce(&mut Tx, &mut JobRow, &mut ProjectRow) -> Result<T, ApiError>,
    ) -> Step<T> {
        self.state
            .db
            .write(|t| {
                let mut job = db::load_job(t, &self.job_id)?.ok_or_else(ApiError::fenced)?;
                // fencing：执行尝试已被取代，或任务已进入 cancelling / 终态，本次写入作废。
                if job.attempt != self.attempt || job.job.status != JobStatus::Running {
                    return Err(ApiError::fenced());
                }
                let mut project =
                    db::load_project(t, &job.job.project_id)?.ok_or_else(ApiError::fenced)?;
                let value = f(t, &mut job, &mut project)?;
                db::save_job(t, &job)?;
                db::save_project(t, &mut project)?;
                Ok(value)
            })
            .map_err(Stop::from)
    }

    /// 调用供应商，同时监听取消令牌。
    ///
    /// 用户停止后立即返回 [`Stop::Cancelled`]，未完成的供应商 future 被丢弃；
    /// 即使外部调用随后才返回，结果也不会再写入。
    async fn call<T>(
        &self,
        stage: StageKey,
        label: &str,
        future: impl Future<Output = ProviderResult<T>>,
    ) -> Step<T> {
        let started = std::time::Instant::now();
        let job = self.tag();
        tokio::select! {
            _ = self.cancel.cancelled() => {
                tracing::info!(target: "job", "{job}   · {label} 已中止（用户停止）");
                Err(Stop::Cancelled)
            }
            result = future => match result {
                Ok(value) => {
                    tracing::info!(target: "job", "{job}   · {label} 完成，用时 {}", elapsed(started));
                    Ok(value)
                }
                Err(error) => {
                    tracing::warn!(
                        target: "job",
                        "{job}   · {label} 失败（{}，用时 {}）：{}",
                        error.code,
                        elapsed(started),
                        error.message
                    );
                    Err(provider_error(stage, error))
                }
            },
        }
    }

    /// 批量设置阶段状态，并写 `job.updated` 事件更新前端进度。
    fn set_stage(&self, keys: &[StageKey], status: &str, message: Option<&str>) -> Step<()> {
        let keys = keys.to_vec();
        let status = status.to_string();
        let message = message.map(str::to_string);
        let (keys_in, status_in) = (keys.clone(), status.clone());
        self.guarded(move |t, job, _| {
            let (keys, status) = (keys_in, status_in);
            for stage in &mut job.job.stages {
                if keys.contains(&stage.key) {
                    stage.status = status.clone();
                    stage.progress_percent = Some(match status.as_str() {
                        "succeeded" | "skipped" => 100,
                        _ => 0,
                    });
                    stage.message = match (&message, status.as_str()) {
                        (Some(text), _) => text.clone(),
                        (None, "skipped") => format!("{}（沿用已有结果）", stage.key.label()),
                        _ => stage.key.label().into(),
                    };
                }
            }
            if status == "running" {
                job.job.current_stage = keys.first().copied();
            }
            ops::recompute_progress(&mut job.job);
            let text = message
                .clone()
                .unwrap_or_else(|| keys[0].label().to_string());
            ops::emit_job_updated(t, &job.job, &text)
        })?;
        for key in &keys {
            self.log_stage(*key, &status, "");
        }
        Ok(())
    }

    /// 阶段产物写回草稿：草稿版本 + 1，并更新任务检查点。
    fn save_draft(
        t: &mut Tx,
        job: &mut JobRow,
        project: &mut ProjectRow,
        draft: &Draft,
    ) -> Result<(), ApiError> {
        project.draft = draft.clone();
        project.project.draft_revision += 1;
        project.project.draft_dirty = true;
        job.checkpoint_revision = project.project.draft_revision;
        t.emit(
            &project.project.id,
            "draft.updated",
            json!({ "project_id": project.project.id, "draft_revision": project.project.draft_revision }),
        )?;
        Ok(())
    }

    /// 把某个阶段标记为成功并重算进度（只改内存中的任务，由调用方保存）。
    fn complete_stage(job: &mut JobRow, key: StageKey) {
        for stage in &mut job.job.stages {
            if stage.key == key {
                stage.status = "succeeded".into();
                stage.progress_percent = Some(100);
                stage.message = key.label().into();
            }
        }
        ops::recompute_progress(&mut job.job);
    }

    /// 向 assistant 消息追加一句话（`message.delta`）。
    fn say(&self, delta: &str) -> Step<()> {
        let delta = delta.to_string();
        self.guarded(move |t, job, _| match &job.assistant_message_id {
            Some(id) => ops::append_text(t, id, &delta),
            None => Ok(()),
        })
    }

    /// 把供应商产出保存到项目存储，返回待写库的素材记录。
    ///
    /// 宽高未知的图片 / 视频用 ffprobe 补全；音频直接使用供应商给出的时长。
    async fn store_asset(&self, project_id: &str, output: &MediaOutput) -> Step<AssetRow> {
        let asset_id = make_id("ast");
        let (path, size) = self
            .state
            .media
            .store_output(project_id, &asset_id, output)
            .await?;
        // 未知宽高的画面、未知时长的音频（云端 / 本地模型的输出）都用 ffprobe 解析。
        let needs_probe =
            (output.width.is_none() && output.kind != "audio") || output.duration_ms.is_none();
        let (width, height, duration) = if needs_probe {
            let probe = self.state.media.probe(&path).await;
            (
                probe.width,
                probe.height,
                probe.duration_ms.or(output.duration_ms),
            )
        } else {
            (output.width, output.height, output.duration_ms)
        };
        Ok(AssetRow {
            asset: Asset {
                id: asset_id,
                project_id: project_id.into(),
                kind: output.kind.clone(),
                source: output.source.clone(),
                status: "ready".into(),
                file_name: output.file_name.clone(),
                mime_type: output.mime_type.clone(),
                size_bytes: size,
                duration_ms: duration,
                width,
                height,
                thumbnail_url: None,
                error: None,
            },
            storage_path: path.to_string_lossy().into_owned(),
        })
    }

    // ---------- generate / revise ----------

    /// generate / revise 的完整制作流程。
    ///
    /// 每个阶段先判断是否需要执行（草稿中仍失效或产物缺失），不需要则标记为 skipped；
    /// 需要则调用供应商，并把产物与阶段状态在同一事务中提交。
    async fn produce(&self) -> Step<()> {
        let (job, project) = self
            .state
            .db
            .read(|conn| {
                let job = db::load_job(conn, &self.job_id)?.ok_or_else(ApiError::fenced)?;
                let project =
                    db::load_project(conn, &job.job.project_id)?.ok_or_else(ApiError::fenced)?;
                Ok((job, project))
            })
            .map_err(Stop::from)?;
        let project_id = project.project.id.clone();
        let kind = job.job.kind;
        let mut input = job.input.clone();
        let mut draft = project.draft.clone();
        let mut artifacts = project.artifacts.clone();
        let providers = self.state.providers.clone();
        if duration_exceeds_limit(&input.text) {
            return Err(Stop::Failed(JobError {
                code: "DURATION_TOO_LONG".into(),
                message: "单个作品最长支持 300 秒，请缩短目标时长后重试。".into(),
                retryable: false,
                failed_stage: Some(StageKey::Planning),
            }));
        }
        if let Some(seconds) = requested_duration_seconds(&input.text) {
            draft.settings.target_duration_seconds = seconds;
        }

        // 开场白（恢复执行时不重复）。
        let opening = match &job.assistant_message_id {
            Some(id) => self
                .state
                .db
                .read(|conn| db::load_message(conn, id))
                .map_err(Stop::from)?
                .map(|m| m.text)
                .unwrap_or_default(),
            None => String::new(),
        };
        if opening.is_empty() {
            self.say(if kind == JobKind::Revise {
                "收到，正在根据你的修改更新作品。"
            } else {
                "好呀，我会为你制作一支有故事感的短片。先整理创作方向，再把灵感拆成一个个镜头。"
            })?;
        }

        // 1. 选题 / 理解修改意见（llm1）
        // revise：先让 llm1 理解修改意见并改写草稿，失效阶段由修改内容决定；
        // generate：草稿还没有创作方案时执行选题。
        if kind == JobKind::Revise && !input.revision_applied {
            self.set_stage(&[StageKey::Planning], "running", Some("理解你的修改意见"))?;
            let revision = self
                .call(
                    StageKey::Planning,
                    "llm1 理解修改意见",
                    providers.load().script.revise(ReviseRequest {
                        draft: draft.clone(),
                        instruction: input.text.clone(),
                    }),
                )
                .await?;
            draft = revision.draft;
            draft.invalidated_stages =
                merge_stages(&draft.invalidated_stages, &revision.invalidated);
            input.revision_applied = true;
            let summary = revision.summary;
            let saved_draft = draft.clone();
            let saved_input = input.clone();
            self.guarded(move |t, job, project| {
                job.input = saved_input;
                Self::complete_stage(job, StageKey::Planning);
                if let Some(id) = job.assistant_message_id.clone() {
                    let block = json!({ "type": "plan", "summary": summary, "draft_revision": project.project.draft_revision + 1 });
                    ops::update_blocks(t, &id, |blocks| blocks.insert(0, block))?;
                }
                Self::save_draft(t, job, project, &saved_draft)?;
                ops::emit_job_updated(t, &job.job, "已理解修改意见")
            })?;
            self.log_stage(StageKey::Planning, "succeeded", "已理解修改意见");
        } else if draft.brief.is_none() || draft.invalidated_stages.contains(&StageKey::Planning) {
            self.set_stage(&[StageKey::Planning], "running", None)?;
            let attachments = self.attachments(&input.asset_ids)?;
            let outcome = self
                .call(
                    StageKey::Planning,
                    "llm1 选题",
                    providers.load().script.plan(PlanRequest {
                        ctx: context(&draft, &input),
                        attachments,
                    }),
                )
                .await?;
            match outcome {
                PlanOutcome::NeedInput(question) => return self.ask(question),
                PlanOutcome::Ready(plan) => {
                    draft.brief = Some(plan.brief);
                    draft.settings = plan.settings;
                    draft
                        .invalidated_stages
                        .retain(|s| *s != StageKey::Planning);
                    artifacts.plan_summary = Some(plan.summary.clone());
                    let (saved_draft, saved_artifacts, title) =
                        (draft.clone(), artifacts.clone(), plan.project_title);
                    let summary = plan.summary;
                    self.guarded(move |t, job, project| {
                        if project.project.title == "新的创作" {
                            project.project.title = title;
                        }
                        project.artifacts = saved_artifacts;
                        Self::complete_stage(job, StageKey::Planning);
                        if let Some(id) = job.assistant_message_id.clone() {
                            let block = json!({ "type": "plan", "summary": summary, "draft_revision": project.project.draft_revision + 1 });
                            ops::update_blocks(t, &id, |blocks| {
                                blocks.retain(|b| b["type"] != "plan" && b["type"] != "question");
                                blocks.insert(0, block);
                            })?;
                        }
                        Self::save_draft(t, job, project, &saved_draft)?;
                        ops::emit_job_updated(t, &job.job, "创作方向已确定")
                    })?;
                    self.log_stage(StageKey::Planning, "succeeded", "创作方向已确定");
                }
            }
        } else {
            self.set_stage(&[StageKey::Planning], "skipped", None)?;
        }

        let ctx = context(&draft, &input);

        // 2. 文案（llm1）
        if draft.script.is_none() || draft.invalidated_stages.contains(&StageKey::Script) {
            self.set_stage(&[StageKey::Script], "running", None)?;
            let brief = draft.brief.clone().unwrap_or_else(default_brief);
            let script = self
                .call(
                    StageKey::Script,
                    "llm1 写文案",
                    providers.load().script.write_script(ScriptRequest {
                        ctx: ctx.clone(),
                        brief,
                    }),
                )
                .await?;
            draft.script = Some(script);
            draft.shots.clear();
            draft.invalidated_stages =
                merge_stages(&draft.invalidated_stages, &StageKey::Storyboard.and_after());
            draft.invalidated_stages.retain(|s| *s != StageKey::Script);
            self.commit_stage(StageKey::Script, &draft, "文案已完成")?;
        } else {
            self.set_stage(&[StageKey::Script], "skipped", None)?;
        }

        // 3. 分镜（llm1）
        if draft.shots.is_empty() || draft.invalidated_stages.contains(&StageKey::Storyboard) {
            self.set_stage(&[StageKey::Storyboard], "running", None)?;
            let brief = draft.brief.clone().unwrap_or_else(default_brief);
            let script = draft.script.clone().expect("script prepared");
            let plans = self
                .call(
                    StageKey::Storyboard,
                    "llm1 画分镜",
                    providers.load().script.storyboard(StoryboardRequest {
                        ctx: ctx.clone(),
                        brief,
                        script,
                    }),
                )
                .await?;
            draft.shots = plans
                .into_iter()
                .map(|p| Shot {
                    id: make_id("shot"),
                    duration_ms: p.duration_ms,
                    narration: p.narration,
                    visual_prompt: p.visual_prompt,
                    shot_size: p.shot_size,
                    camera_motion: p.camera_motion,
                    asset_ids: vec![],
                    thumbnail_url: None,
                })
                .collect();
            draft
                .invalidated_stages
                .retain(|s| *s != StageKey::Storyboard);
            draft.invalidated_stages = merge_stages(
                &draft.invalidated_stages,
                &[StageKey::Visuals, StageKey::Audio],
            );
            self.commit_stage(StageKey::Storyboard, &draft, "分镜已完成")?;
            self.say("脚本与分镜已准备好，接下来让画面和声音相遇。")?;
        } else {
            self.set_stage(&[StageKey::Storyboard], "skipped", None)?;
        }

        // 用户本次附带的素材：图片 / 视频依次放进镜头，音频作为背景音乐。
        if !input.attachments_applied && !input.asset_ids.is_empty() {
            self.apply_attachments(&mut draft, &mut input)?;
        }

        // 4. SVG 画面（llm1）与 5. 音频（llm3 + llm4）并行
        let ctx = context(&draft, &input);
        // 有失效标记，或有镜头还没有画面，才需要准备画面。
        let need_visuals = draft.invalidated_stages.contains(&StageKey::Visuals)
            || draft.shots.iter().any(|s| s.asset_ids.is_empty());
        // 有失效标记、任一镜头的配音指纹变化、或需要音乐但还没解析出来，才需要准备音频。
        // 关闭配音时不再检查台词指纹，避免每次都判定音频失效。
        let narration_changed = draft.audio.narration_enabled
            && draft.shots.iter().any(|s| {
                artifacts.narration.get(&s.id).map(|n| n.key.as_str())
                    != Some(narration_key(s, &draft.audio, &draft.settings.language).as_str())
            });
        let need_audio = draft.invalidated_stages.contains(&StageKey::Audio)
            || narration_changed
            || (draft.audio.music_mode != "none" && draft.audio.resolved_music_asset_id.is_none());
        let mut running = vec![];
        if need_visuals {
            running.push(StageKey::Visuals);
        } else {
            self.set_stage(&[StageKey::Visuals], "skipped", None)?;
        }
        if need_audio {
            running.push(StageKey::Audio);
        } else {
            self.set_stage(&[StageKey::Audio], "skipped", None)?;
        }
        if !running.is_empty() {
            self.set_stage(&running, "running", None)?;
        }
        let visuals = async {
            if !need_visuals {
                return Ok(vec![]);
            }
            let r = self.prepare_visuals(&project_id, &ctx, &draft).await;
            if r.is_ok() {
                self.set_stage(&[StageKey::Visuals], "succeeded", Some("画面已就绪"))?;
            }
            r
        };
        let audio = async {
            if !need_audio {
                return Ok((vec![], None, false));
            }
            let r = self
                .prepare_audio(&project_id, &ctx, &draft, &artifacts)
                .await;
            if r.is_ok() {
                self.set_stage(&[StageKey::Audio], "succeeded", Some("配音与音乐已就绪"))?;
            }
            r
        };
        // 画面与音频互不依赖，并行执行；任一失败时整体失败，已完成的那一路产物不提交，重试时重新生成。
        let (visuals, audio) = tokio::join!(visuals, audio);
        let visuals = visuals?;
        let (narrations, music, music_changed) = match audio {
            Ok(audio) => audio,
            Err(stop) => {
                // 音频失败时先保存已经生成好的画面：云端文生图有成本，重试只补缺的部分。
                if !visuals.is_empty() {
                    for (shot_id, asset) in &visuals {
                        if let Some(shot) = draft.shots.iter_mut().find(|s| &s.id == shot_id) {
                            shot.asset_ids = vec![asset.asset.id.clone()];
                        }
                    }
                    draft.invalidated_stages.retain(|s| *s != StageKey::Visuals);
                    let saved_draft = draft.clone();
                    let assets: Vec<AssetRow> =
                        visuals.into_iter().map(|(_, asset)| asset).collect();
                    self.guarded(move |t, job, project| {
                        for asset in &assets {
                            db::insert_asset(t, asset)?;
                        }
                        Self::save_draft(t, job, project, &saved_draft)?;
                        ops::emit_job_updated(t, &job.job, "画面已保存")
                    })?;
                }
                return Err(stop);
            }
        };
        if need_visuals || need_audio {
            let mut new_assets: Vec<AssetRow> = vec![];
            for (shot_id, asset) in visuals {
                if let Some(shot) = draft.shots.iter_mut().find(|s| s.id == shot_id) {
                    shot.asset_ids = vec![asset.asset.id.clone()];
                }
                new_assets.push(asset);
            }
            for (shot_id, key, asset) in narrations {
                let duration = asset.asset.duration_ms.unwrap_or_default();
                artifacts.narration.insert(
                    shot_id,
                    NarrationArtifact {
                        asset_id: asset.asset.id.clone(),
                        key,
                        duration_ms: duration,
                    },
                );
                new_assets.push(asset);
            }
            if music_changed {
                draft.audio.resolved_music_asset_id = match music {
                    Some(MusicChoice::New(asset)) => {
                        let id = asset.asset.id.clone();
                        new_assets.push(*asset);
                        Some(id)
                    }
                    Some(MusicChoice::Existing(id)) => Some(id),
                    None => None,
                };
            }
            draft
                .invalidated_stages
                .retain(|s| *s != StageKey::Visuals && *s != StageKey::Audio);
            let (saved_draft, saved_artifacts) = (draft.clone(), artifacts.clone());
            self.guarded(move |t, job, project| {
                for asset in &new_assets {
                    db::insert_asset(t, asset)?;
                }
                project.artifacts = saved_artifacts;
                Self::save_draft(t, job, project, &saved_draft)?;
                ops::emit_job_updated(t, &job.job, "画面与声音已就绪")
            })?;
        }

        // 6. 剪辑方案（llm1）
        self.set_stage(&[StageKey::Edit], "running", None)?;
        // 关闭配音时忽略历史配音产物，避免旧版本的声音混进新成片。
        let narration_map: HashMap<String, (String, i64)> = if draft.audio.narration_enabled {
            artifacts
                .narration
                .iter()
                .map(|(shot, n)| (shot.clone(), (n.asset_id.clone(), n.duration_ms)))
                .collect()
        } else {
            HashMap::new()
        };
        // 背景音乐自身时长：比成片短时剪辑指令需要循环铺满。
        let music_duration_ms = match &draft.audio.resolved_music_asset_id {
            Some(id) => self
                .state
                .db
                .read(|conn| db::load_asset(conn, id))
                .map_err(Stop::from)?
                .and_then(|row| row.asset.duration_ms),
            None => None,
        };
        let mut plan = self
            .call(
                StageKey::Edit,
                "llm1 生成剪辑方案",
                providers.load().script.edit_plan(EditRequest {
                    draft: draft.clone(),
                    narration: narration_map,
                    music_duration_ms,
                }),
            )
            .await?;
        draft.invalidated_stages.retain(|s| *s != StageKey::Edit);
        artifacts.edit_plan = Some(plan.clone());
        self.commit_artifacts(StageKey::Edit, &draft, &artifacts, "剪辑方案已完成")?;

        // 7. 调色、特效与字幕：按用户选择的预设写入剪辑方案。
        self.set_stage(&[StageKey::Finishing], "running", None)?;
        plan.color_preset_id = draft.finishing.color_preset_id.clone();
        plan.effect_preset_id = draft.finishing.effect_preset_id.clone();
        plan.subtitles_enabled = draft.finishing.subtitles_enabled;
        for clip in &mut plan.clips {
            if !plan.subtitles_enabled {
                clip.subtitle = None;
            }
            if plan.effect_preset_id == "fade" {
                clip.transition = "fade".into();
            }
        }
        // 剪辑指令与当前特效预设保持一致：用户在右侧切换“淡入淡出”后无需重新生成剪辑方案。
        let fade = plan.effect_preset_id == "fade";
        crate::providers::edit_ops::set_crossfades(&mut plan.ops, fade, 300);
        draft
            .invalidated_stages
            .retain(|s| *s != StageKey::Finishing);
        artifacts.edit_plan = Some(plan.clone());
        self.commit_artifacts(StageKey::Finishing, &draft, &artifacts, "调色与字幕已完成")?;

        // 8. 渲染输出（剪辑库）
        self.set_stage(&[StageKey::Render], "running", None)?;
        let inputs = self.plan_inputs(&plan)?;
        let ctx = context(&draft, &input);
        let output = self
            .call(
                StageKey::Render,
                "剪辑库 渲染成片",
                providers.load().renderer.render(RenderRequest {
                    ctx,
                    plan: plan.clone(),
                    inputs,
                }),
            )
            .await?;
        let video = self.store_asset(&project_id, &output).await?;
        self.publish(draft, video, plan)
    }

    /// 提交一个阶段的草稿产物：阶段成功、草稿版本 + 1。
    fn commit_stage(&self, key: StageKey, draft: &Draft, message: &str) -> Step<()> {
        let saved = draft.clone();
        let message = message.to_string();
        let note = message.clone();
        self.guarded(move |t, job, project| {
            Self::complete_stage(job, key);
            Self::save_draft(t, job, project, &saved)?;
            ops::emit_job_updated(t, &job.job, &message)
        })?;
        self.log_stage(key, "succeeded", &note);
        Ok(())
    }

    /// 提交只影响内部产物的阶段（剪辑、调色）：阶段成功，但不增加草稿版本。
    ///
    /// 这两个阶段不改变用户可见的草稿内容，只更新检查点与失效标记。
    fn commit_artifacts(
        &self,
        key: StageKey,
        draft: &Draft,
        artifacts: &Artifacts,
        message: &str,
    ) -> Step<()> {
        let (saved_draft, saved_artifacts) = (draft.clone(), artifacts.clone());
        let message = message.to_string();
        let note = message.clone();
        self.guarded(move |t, job, project| {
            Self::complete_stage(job, key);
            project.artifacts = saved_artifacts;
            project.draft = saved_draft;
            ops::emit_job_updated(t, &job.job, &message)
        })?;
        self.log_stage(key, "succeeded", &note);
        Ok(())
    }

    /// 进入追问：任务改为 `waiting_input`，写入问题卡片和 `input.required` 事件。
    ///
    /// 返回 [`Stop::Paused`] 释放执行线程；用户回答后任务回到队列，从选题阶段继续。
    fn ask(&self, question: QuestionSpec) -> Step<()> {
        let ttl = self.state.config.question_ttl_seconds;
        self.guarded(move |t, job, project| {
            let q = Question {
                id: make_id("question"),
                prompt: question.prompt,
                input_type: question.input_type,
                options: question
                    .options
                    .into_iter()
                    .map(|(value, label)| QuestionOption { value, label })
                    .collect(),
                expires_at: iso(now() + chrono::Duration::seconds(ttl)),
            };
            job.job.status = JobStatus::WaitingInput;
            for stage in &mut job.job.stages {
                if stage.key == StageKey::Planning {
                    stage.message = "等待你补充信息".into();
                }
            }
            project.project.status = "waiting_input".into();
            if let Some(id) = job.assistant_message_id.clone() {
                ops::append_text(t, &id, "内容方向已经准备好，再补充一个小信息就能继续。")?;
                let block =
                    json!({ "type": "question", "job_id": job.job.id, "question_id": q.id });
                ops::update_blocks(t, &id, |blocks| blocks.push(block))?;
            }
            t.emit(
                &project.project.id,
                "input.required",
                json!({ "job_id": job.job.id, "question_id": q.id }),
            )?;
            job.job.question = Some(q);
            ops::emit_job_updated(t, &job.job, "等待你补充信息")
        })?;
        tracing::info!(target: "job", "{} ⏸ 等待用户补充信息，执行线程已释放", self.tag());
        Err(Stop::Paused)
    }

    /// 读取本次附带素材的摘要。
    fn attachments(&self, ids: &[String]) -> Step<Vec<AttachmentInfo>> {
        let ids = ids.to_vec();
        self.state
            .db
            .read(|conn| {
                let mut list = vec![];
                for id in &ids {
                    if let Some(row) = db::load_asset(conn, id)? {
                        list.push(AttachmentInfo {
                            asset_id: row.asset.id,
                            kind: row.asset.kind,
                            file_name: row.asset.file_name,
                            duration_ms: row.asset.duration_ms,
                        });
                    }
                }
                Ok(list)
            })
            .map_err(Stop::from)
    }

    /// 把用户附带的素材放进草稿：图片 / 视频依次作为镜头画面，音频作为背景音乐。
    ///
    /// 完成后在任务输入中记下 `attachments_applied`，重试或恢复时不会重复放入。
    fn apply_attachments(&self, draft: &mut Draft, input: &mut JobInput) -> Step<()> {
        let attachments = self.attachments(&input.asset_ids)?;
        let mut shot_index = 0;
        for attachment in attachments {
            if attachment.kind == "audio" {
                draft.audio.music_mode = "upload".into();
                draft.audio.music_id = None;
                draft.audio.music_asset_id = Some(attachment.asset_id.clone());
                draft.audio.resolved_music_asset_id = None;
                draft.invalidated_stages =
                    merge_stages(&draft.invalidated_stages, &[StageKey::Audio]);
            } else if let Some(shot) = draft.shots.get_mut(shot_index) {
                shot.asset_ids = vec![attachment.asset_id.clone()];
                shot_index += 1;
            }
        }
        input.attachments_applied = true;
        let (saved_draft, saved_input) = (draft.clone(), input.clone());
        self.guarded(move |t, job, project| {
            job.input = saved_input;
            Self::save_draft(t, job, project, &saved_draft)?;
            ops::emit_job_updated(t, &job.job, "已放入你上传的素材")
        })?;
        tracing::info!(target: "job", "{}   · 已把用户上传的素材放入分镜 / 背景音乐", self.tag());
        Ok(())
    }

    /// llm1：为没有画面的镜头生成 SVG 动画（用户上传的素材保留）。
    async fn prepare_visuals(
        &self,
        project_id: &str,
        ctx: &ProductionContext,
        draft: &Draft,
    ) -> Step<Vec<(String, AssetRow)>> {
        let count = draft.shots.len();
        // 各镜头互不依赖，并发生成（云端文生图单张约 20 秒）；任一失败则整体失败，重试时只补缺失的镜头。
        let jobs = draft
            .shots
            .iter()
            .enumerate()
            .filter(|(_, shot)| shot.asset_ids.is_empty())
            .map(|(index, shot)| async move {
                let request = VisualRequest {
                    ctx: ctx.clone(),
                    shot: shot.clone(),
                    shot_index: index,
                    shot_count: count,
                };
                let media = self
                    .call(
                        StageKey::Visuals,
                        &format!("llm1 生成 SVG 画面（镜头 {}/{}）", index + 1, count),
                        self.state.providers.load().visual.generate_shot(request),
                    )
                    .await?;
                Ok::<_, Stop>((shot.id.clone(), self.store_asset(project_id, &media).await?))
            });
        futures::future::join_all(jobs).await.into_iter().collect()
    }

    /// llm4 配音（按镜头，内容未变则复用）+ llm3 背景音乐。
    async fn prepare_audio(
        &self,
        project_id: &str,
        ctx: &ProductionContext,
        draft: &Draft,
        artifacts: &Artifacts,
    ) -> Step<(Vec<(String, String, AssetRow)>, Option<MusicChoice>, bool)> {
        // 上传模式必须有音频素材；缺失时直接失败，避免静默生成一条没有声音的成片。
        if draft.audio.music_mode == "upload" && draft.audio.music_asset_id.is_none() {
            return Err(Stop::Failed(JobError {
                code: "VALIDATION_ERROR".into(),
                message: "背景音乐来源选的是上传，但项目里还没有音频素材，请先上传 BGM。".into(),
                retryable: false,
                failed_stage: Some(StageKey::Audio),
            }));
        }
        // 配音（llm4，逐句）与背景音乐（llm3，异步生成）互不依赖，同时进行：
        // 本地 TTS 每句数十秒、云端音乐需排队，串行会白白多等几分钟。
        let narration = async {
            let mut narrations = vec![];
            // 关闭配音时字幕承担全部讲解，成片只保留背景音乐。
            if !draft.audio.narration_enabled {
                return Ok::<_, Stop>(narrations);
            }
            for shot in &draft.shots {
                let key = narration_key(shot, &draft.audio, &draft.settings.language);
                if artifacts.narration.get(&shot.id).map(|n| &n.key) == Some(&key)
                    || shot.narration.trim().is_empty()
                {
                    continue;
                }
                let request = VoiceRequest {
                    text: shot.narration.clone(),
                    voice_id: draft.audio.voice_id.clone(),
                    speech_rate: draft.audio.speech_rate,
                    language: draft.settings.language.clone(),
                    target_duration_ms: shot.duration_ms,
                };
                let media = self
                    .call(
                        StageKey::Audio,
                        &format!("llm4 配音（{}字）", shot.narration.chars().count()),
                        self.state.providers.load().voice.synthesize(request),
                    )
                    .await?;
                narrations.push((
                    shot.id.clone(),
                    key,
                    self.store_asset(project_id, &media).await?,
                ));
            }
            Ok::<_, Stop>(narrations)
        };
        let music = async {
            let audio = &draft.audio;
            let reuse = audio.resolved_music_asset_id.is_some()
                && !draft.invalidated_stages.contains(&StageKey::Audio);
            if reuse {
                return Ok::<_, Stop>((None, false));
            }
            let total: i64 = draft.shots.iter().map(|s| s.duration_ms).sum();
            let music = match audio.music_mode.as_str() {
                "none" => None,
                "upload" => audio.music_asset_id.clone().map(MusicChoice::Existing),
                mode => {
                    let request = MusicRequest {
                        ctx: ctx.clone(),
                        mode: mode.into(),
                        track_id: audio.music_id.clone(),
                        duration_ms: total,
                    };
                    let media = self
                        .call(
                            StageKey::Audio,
                            "llm3 背景音乐",
                            self.state.providers.load().music.compose(request),
                        )
                        .await?;
                    Some(MusicChoice::New(Box::new(
                        self.store_asset(project_id, &media).await?,
                    )))
                }
            };
            Ok((music, true))
        };
        let (narrations, music) = tokio::join!(narration, music);
        let (music, music_changed) = music?;
        Ok((narrations?, music, music_changed))
    }

    /// 收集剪辑方案中引用的全部素材的本地路径，交给渲染器。
    fn plan_inputs(&self, plan: &EditPlan) -> Step<HashMap<String, std::path::PathBuf>> {
        let mut ids: Vec<String> = plan
            .clips
            .iter()
            .flat_map(|c| [c.visual_asset_id.clone(), c.narration_asset_id.clone()])
            .flatten()
            .collect();
        ids.extend(plan.music_asset_id.clone());
        self.state
            .db
            .read(|conn| {
                let mut map = HashMap::new();
                for id in ids {
                    if let Some(row) = db::load_asset(conn, &id)? {
                        map.insert(id, row.storage_path.into());
                    }
                }
                Ok(map)
            })
            .map_err(Stop::from)
    }

    /// 发布新成片：创建不可变版本，任务成功，释放项目。
    fn publish(&self, mut draft: Draft, video: AssetRow, edit_plan: EditPlan) -> Step<()> {
        draft.invalidated_stages.clear();
        self.guarded(move |t, job, project| {
            db::insert_asset(t, &video)?;
            Self::complete_stage(job, StageKey::Render);
            Self::save_draft(t, job, project, &draft)?;
            project.project.draft_dirty = false;
            let created = now_iso();
            let version = VersionRow {
                id: make_id("ver"),
                project_id: project.project.id.clone(),
                version_number: db::next_version_number(t, &project.project.id)?,
                source_draft_revision: project.project.draft_revision,
                source_job_id: job.job.id.clone(),
                created_at: created.clone(),
                video_asset_id: video.asset.id.clone(),
                thumbnail_asset_id: draft.shots.iter().flat_map(|s| s.asset_ids.first()).next().cloned(),
                duration_ms: video.asset.duration_ms.unwrap_or_default(),
                width: video.asset.width.unwrap_or_default(),
                height: video.asset.height.unwrap_or_default(),
                snapshot: draft.clone(),
                edit_plan: Some(edit_plan),
                exports: vec![VideoExport {
                    asset_id: video.asset.id.clone(),
                    resolution: "720p".into(),
                    fps: 30,
                    format: "mp4".into(),
                    created_at: created,
                }],
            };
            db::insert_version(t, &version)?;
            project.project.latest_video_version_id = Some(version.id.clone());
            job.job.status = JobStatus::Succeeded;
            job.job.progress_percent = Some(100);
            job.job.current_stage = None;
            job.job.finished_at = Some(now_iso());
            job.job.result = Some(json!({
                "video_version_id": version.id,
                "draft_revision": project.project.draft_revision,
            }));
            for stage in &mut job.job.stages {
                if stage.status != "skipped" {
                    stage.status = "succeeded".into();
                    stage.progress_percent = Some(100);
                }
            }
            if let Some(id) = job.assistant_message_id.clone() {
                let text = if job.job.kind == JobKind::Revise {
                    "新版本准备好了，之前的作品也已经保留。可以切换对比，或继续告诉我想调整的地方。"
                } else {
                    "你的短片准备好了！画面、配音和音乐都安排好了。还想调整哪里，继续告诉我就好。"
                };
                let video_block = json!({ "type": "video", "video_version_id": version.id });
                ops::finish_message(t, &id, Some(text), "completed", |blocks| {
                    blocks.retain(|b| b["type"] != "question");
                    blocks.push(video_block);
                })?;
            }
            // 先发布成片，再释放项目（状态计算需要看到最新版本）。
            project.project.active_job_id = None;
            project.project.status = "ready".into();
            t.emit(
                &project.project.id,
                "video.created",
                json!({ "project_id": project.project.id, "job_id": job.job.id, "video_version_id": version.id }),
            )?;
            ops::emit_job_updated(t, &job.job, "成片已完成")?;
            t.emit(&project.project.id, "job.completed", json!({ "job_id": job.job.id }))?;
            Ok(version.version_number)
        })
        .map(|number| {
            self.log_stage(StageKey::Render, "succeeded", "");
            tracing::info!(
                target: "job",
                "{} ★ 成片 v{number} 已发布，本次执行总用时 {}",
                self.tag(),
                elapsed(self.started)
            );
        })
    }

    // ---------- chat ----------

    /// 仅讨论：调用 llm1 回答，按句推送增量，最后写入完整文本。不修改草稿。
    async fn chat(&self) -> Step<()> {
        let (job, project) = self
            .state
            .db
            .read(|conn| {
                let job = db::load_job(conn, &self.job_id)?.ok_or_else(ApiError::fenced)?;
                let project =
                    db::load_project(conn, &job.job.project_id)?.ok_or_else(ApiError::fenced)?;
                Ok((job, project))
            })
            .map_err(Stop::from)?;
        let reply = self
            .call(
                StageKey::Planning,
                "llm1 仅讨论回答",
                self.state.providers.load().script.chat(ChatRequest {
                    brief: project.draft.brief.clone(),
                    text: job.input.text.clone(),
                }),
            )
            .await?;
        // 按句分段推送，模拟流式输出。
        let mut sent = String::new();
        for sentence in reply.split_inclusive('。') {
            if self.cancel.is_cancelled() {
                return Err(Stop::Cancelled);
            }
            self.say(sentence)?;
            sent.push_str(sentence);
            tokio::time::sleep(std::time::Duration::from_millis(120)).await;
        }
        self.guarded(move |t, job, project| {
            job.job.status = JobStatus::Succeeded;
            job.job.progress_percent = Some(100);
            job.job.finished_at = Some(now_iso());
            if let Some(id) = job.assistant_message_id.clone() {
                job.job.result = Some(json!({ "assistant_message_id": id }));
                ops::finish_message(t, &id, Some(&reply), "completed", |_| {})?;
            }
            ops::release_project(t, project)?;
            ops::emit_job_updated(t, &job.job, "回答完成")?;
            t.emit(
                &project.project.id,
                "job.completed",
                json!({ "job_id": job.job.id }),
            )?;
            Ok(())
        })?;
        tracing::info!(target: "job", "{} ★ 仅讨论回答完成，用时 {}", self.tag(), elapsed(self.started));
        Ok(())
    }

    // ---------- export ----------

    /// 导出：严格使用指定成片的不可变快照，不读取之后编辑的草稿（文档 5.8）。
    ///
    /// 成功后把文件追加到该版本的导出列表，不创建新的内容版本。
    async fn export(&self) -> Step<()> {
        let (job, version, source, plan, bundle_assets, title) = self
            .state
            .db
            .read(|conn| {
                let job = db::load_job(conn, &self.job_id)?.ok_or_else(ApiError::fenced)?;
                let spec = job
                    .input
                    .export
                    .clone()
                    .ok_or_else(|| ApiError::internal("export spec missing"))?;
                let version = db::load_version(conn, &spec.version_id)?
                    .ok_or_else(|| ApiError::internal("version missing"))?;
                let source = db::load_asset(conn, &version.video_asset_id)?
                    .ok_or_else(|| ApiError::internal("video missing"))?;
                let project = db::load_project(conn, &job.job.project_id)?
                    .ok_or_else(|| ApiError::internal("project missing"))?;
                let mut bundle_assets = Vec::new();
                let plan = if spec.include_project {
                    // 新版本直接读取不可变时间线。旧版本仅在草稿仍完全一致时
                    // 允许使用当前检查点，避免把后来修改的旁白混入历史工程包。
                    let plan = version
                        .edit_plan
                        .clone()
                        .or_else(|| {
                            let unchanged = match (
                                serde_json::to_value(&project.draft),
                                serde_json::to_value(&version.snapshot),
                            ) {
                                (Ok(current), Ok(original)) => current == original,
                                _ => false,
                            };
                            unchanged
                                .then(|| project.artifacts.edit_plan.clone())
                                .flatten()
                        })
                        .ok_or_else(|| {
                            ApiError::validation(
                                "这个旧版本没有可恢复的工程时间线，请重新生成后再导出工程包。",
                            )
                        })?;
                    let mut ids: Vec<String> = plan
                        .clips
                        .iter()
                        .flat_map(|clip| {
                            [
                                clip.visual_asset_id.clone(),
                                clip.narration_asset_id.clone(),
                            ]
                        })
                        .flatten()
                        .collect();
                    ids.extend(plan.music_asset_id.clone());
                    let mut seen = std::collections::HashSet::new();
                    for id in ids {
                        if !seen.insert(id.clone()) {
                            continue;
                        }
                        let row = db::load_asset(conn, &id)?.ok_or_else(|| {
                            ApiError::internal(format!("bundle asset {id} missing"))
                        })?;
                        bundle_assets.push(BundleAsset {
                            id,
                            file_name: row.asset.file_name,
                            path: row.storage_path.into(),
                        });
                    }
                    // 用户上传的文件即使未放入当前时间线，也一并交给人工剪辑。
                    let (uploads, _) =
                        db::list_assets(conn, &version.project_id, None, 10_000, true)?;
                    for row in uploads {
                        if seen.insert(row.asset.id.clone()) {
                            bundle_assets.push(BundleAsset {
                                id: row.asset.id,
                                file_name: row.asset.file_name,
                                path: row.storage_path.into(),
                            });
                        }
                    }
                    Some(plan)
                } else {
                    None
                };
                Ok((
                    job,
                    version,
                    source,
                    plan,
                    bundle_assets,
                    project.project.title,
                ))
            })
            .map_err(Stop::from)?;
        let spec = job.input.export.clone().expect("checked");
        tracing::info!(target: "job", "{} 导出模式：{}，版本 {}", self.tag(), if spec.include_project { "MP4 + 全部素材 + OVE/OTIO/FCPXML" } else { "仅 MP4" }, short(&version.id));
        self.set_stage(
            &[StageKey::Render],
            "running",
            Some(if spec.include_project {
                "正在打包工程与素材"
            } else {
                "正在导出"
            }),
        )?;
        let output = if spec.include_project {
            self.call(
                StageKey::Render,
                "剪辑库 导出工程包",
                self.state
                    .providers
                    .load()
                    .renderer
                    .export_bundle(ProjectBundleRequest {
                        source: source.storage_path.clone().into(),
                        plan: plan.expect("bundle plan checked"),
                        assets: bundle_assets,
                        title,
                        version_number: version.version_number,
                    }),
            )
            .await?
        } else {
            self.call(
                StageKey::Render,
                "剪辑库 导出成片",
                self.state.providers.load().renderer.export(ExportRequest {
                    source: source.storage_path.clone().into(),
                    duration_ms: source.asset.duration_ms.unwrap_or(version.duration_ms),
                    aspect_ratio: version.snapshot.settings.aspect_ratio.clone(),
                    resolution: spec.resolution.clone(),
                    fps: spec.fps,
                    format: spec.format.clone(),
                }),
            )
            .await?
        };
        let asset = self.store_asset(&version.project_id, &output).await?;
        let version_id = version.id.clone();
        let exported_format = if spec.include_project {
            "zip".into()
        } else {
            spec.format
        };
        self.guarded(move |t, job, project| {
            db::insert_asset(t, &asset)?;
            let mut version = db::load_version(t, &version_id)?
                .ok_or_else(|| ApiError::internal("version missing"))?;
            version.exports.push(VideoExport {
                asset_id: asset.asset.id.clone(),
                resolution: spec.resolution,
                fps: spec.fps,
                format: exported_format,
                created_at: now_iso(),
            });
            db::save_version_exports(t, &version)?;
            Self::complete_stage(job, StageKey::Render);
            job.job.status = JobStatus::Succeeded;
            job.job.progress_percent = Some(100);
            job.job.finished_at = Some(now_iso());
            job.job.result =
                Some(json!({ "asset_id": asset.asset.id, "video_version_id": version.id }));
            ops::release_project(t, project)?;
            ops::emit_job_updated(t, &job.job, "导出完成")?;
            t.emit(
                &project.project.id,
                "job.completed",
                json!({ "job_id": job.job.id }),
            )?;
            Ok(())
        })?;
        tracing::info!(target: "job", "{} ★ 导出完成，用时 {}", self.tag(), elapsed(self.started));
        Ok(())
    }

    // ---------- 终态 ----------

    /// 执行者因取消或 fencing 退出时，把 `cancelling` 的任务收尾为 `cancelled`。
    fn settle_cancel(&self) {
        let result = self
            .state
            .db
            .write(|t| ops::finalize_cancelled(t, &self.job_id));
        match result {
            Ok(true) => {
                tracing::info!(target: "job", "{} ■ 已按用户要求停止，用时 {}", self.tag(), elapsed(self.started))
            }
            // 不是 cancelling：执行尝试被新的尝试取代，旧执行者安静退出即可。
            Ok(false) => {
                tracing::info!(target: "job", "{} 执行尝试已被取代，旧执行者退出", self.tag())
            }
            Err(error) => tracing::error!(target: "job", "{} 收尾取消失败：{error:?}", self.tag()),
        }
    }

    /// 把任务置为失败。
    ///
    /// 只有当前执行尝试仍持有任务时才写入；若此时任务已在 `cancelling`，按取消处理。
    fn fail(&self, error: JobError) {
        let attempt = self.attempt;
        tracing::warn!(
            target: "job",
            "{} ✗ 任务失败 [{}] 阶段 {:?}：{}（{}，用时 {}）",
            self.tag(),
            error.code,
            error.failed_stage,
            error.message,
            if error.retryable { "可重试" } else { "不可重试" },
            elapsed(self.started)
        );
        let result = self.state.db.write(|t| {
            let Some(mut job) = db::load_job(t, &self.job_id)? else {
                return Ok(());
            };
            // 已有新的执行尝试接管，本次失败结果作废。
            if job.attempt != attempt {
                return Ok(());
            }
            match job.job.status {
                JobStatus::Cancelling => {
                    ops::finalize_cancelled(t, &self.job_id)?;
                }
                JobStatus::Running => {
                    let Some(mut project) = db::load_project(t, &job.job.project_id)? else {
                        return Ok(());
                    };
                    ops::mark_failed(t, &mut job, &mut project, error)?;
                }
                _ => {}
            }
            Ok(())
        });
        if let Err(error) = result {
            tracing::error!(target: "job", "{} 记录失败状态时出错：{error:?}", self.tag());
        }
    }
}

/// 音频阶段对背景音乐的处理结果。
enum MusicChoice {
    /// 新生成或从曲库取得的音乐，需要写入素材表。
    New(Box<AssetRow>),
    /// 沿用用户上传的音频素材。
    Existing(String),
}

/// 选题结果缺失时的兜底方案（正常流程不会用到）。
fn default_brief() -> Brief {
    Brief {
        subject: "短视频".into(),
        audience: "大众".into(),
        purpose: "分享".into(),
        style: "温暖自然".into(),
    }
}
