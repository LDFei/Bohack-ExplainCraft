//! 持久化人机确认流程回归：使用真实路由、SQLite、编码器，模型仅用确定性替身。
use crate::{
    db,
    models::*,
    providers::*,
    state::{AppState, WorkerHandle},
    util::{make_id, now_iso},
};
use async_trait::async_trait;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tower::ServiceExt;

/// 记录模型调用次数，用于证明等待确认不会发出下一阶段请求。
struct ScriptMock(Arc<AtomicUsize>);
#[async_trait]
impl ScriptModel for ScriptMock {
    async fn plan(&self, r: PlanRequest) -> ProviderResult<PlanOutcome> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(PlanOutcome::Ready(Plan {
            brief: Brief {
                subject: "科普".into(),
                audience: "学生".into(),
                purpose: "说明".into(),
                style: "极简".into(),
            },
            project_title: "技能回归".into(),
            summary: "讲清楚一个概念".into(),
            settings: r.ctx.settings,
        }))
    }
    async fn write_script(&self, request: ScriptRequest) -> ProviderResult<Script> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(Script {
            title: "测试".into(),
            text: if request.ctx.user_text.contains("换成抛硬币") {
                "抛硬币的每次结果相互独立。"
            } else {
                "这是一个概念。"
            }
            .into(),
        })
    }
    async fn storyboard(&self, _: StoryboardRequest) -> ProviderResult<Vec<ShotPlan>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(vec![ShotPlan {
            duration_ms: 1000,
            narration: "这是一个概念。".into(),
            visual_prompt: "蓝色图形".into(),
            shot_size: "中景".into(),
            camera_motion: "固定".into(),
        }])
    }
    async fn revise(&self, r: ReviseRequest) -> ProviderResult<Revision> {
        Ok(Revision {
            draft: r.draft,
            invalidated: vec![],
            summary: "已修改".into(),
        })
    }
    async fn edit_plan(&self, _: EditRequest) -> ProviderResult<EditPlan> {
        panic!("未确认可选剪辑之前不能调用剪辑模型")
    }
    async fn chat(&self, _: ChatRequest) -> ProviderResult<String> {
        Ok("讨论".into())
    }
}
/// 仅产生真实 PNG；编码流程仍运行真实 FFmpeg。
struct VisualMock;
#[async_trait]
impl VisualModel for VisualMock {
    async fn generate_shot(&self, _: VisualRequest) -> ProviderResult<MediaOutput> {
        let mut bytes = std::io::Cursor::new(vec![]);
        image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            80,
            80,
            image::Rgb([30, 120, 230]),
        ))
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
        Ok(MediaOutput {
            data: MediaData::Bytes(bytes.into_inner()),
            file_name: "shot.png".into(),
            mime_type: "image/png".into(),
            kind: "image".into(),
            source: "generated".into(),
            width: Some(80),
            height: Some(80),
            duration_ms: None,
        })
    }
}
/// 用纯 BGM 时不允许调配音模型。
struct PendingVoice;
#[async_trait]
impl VoiceModel for PendingVoice {
    async fn synthesize(&self, _: VoiceRequest) -> ProviderResult<MediaOutput> {
        std::future::pending().await
    }
}
/// 统计默认输出，证明基础成片走直接编码器。
struct CountRenderer {
    inner: direct::DirectRenderer,
    calls: Arc<AtomicUsize>,
}
#[async_trait]
impl Renderer for CountRenderer {
    async fn render(&self, r: RenderRequest) -> ProviderResult<MediaOutput> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.inner.render(r).await
    }
    async fn export(&self, r: ExportRequest) -> ProviderResult<MediaOutput> {
        self.inner.export(r).await
    }
    async fn export_bundle(&self, _: ProjectBundleRequest) -> ProviderResult<MediaOutput> {
        panic!("不能静默导出剪辑工程")
    }
}
/// 隔离测试状态，不运行后台自动调度器，逐次恢复同一个真实任务。
fn fixture() -> (
    AppState,
    std::path::PathBuf,
    Arc<AtomicUsize>,
    Arc<AtomicUsize>,
) {
    let root = std::env::temp_dir().join(make_id("guided-test"));
    let mut cfg = crate::Config::from_env();
    cfg.database_path = root.join("test.sqlite3");
    cfg.storage_dir = root.join("storage");
    cfg.demo_media_dir =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../shared/public/media");
    cfg.settings_file = root.join(".env");
    cfg.provider_llm4 = "qwen".into();
    cfg.qwen_tts.api_key = "test-only".into();
    cfg.qwen_tts_voice = "Cherry".into();
    cfg.voice_timeout_seconds = Some(1);
    let model_calls = Arc::new(AtomicUsize::new(0));
    let render_calls = Arc::new(AtomicUsize::new(0));
    let providers = Providers {
        script: Arc::new(ScriptMock(model_calls.clone())),
        visual: Arc::new(VisualMock),
        music: Arc::new(procedural::ProceduralMusic),
        voice: Arc::new(PendingVoice),
        renderer: Arc::new(CountRenderer {
            inner: direct::DirectRenderer::new(&cfg),
            calls: render_calls.clone(),
        }),
    };
    let state = AppState {
        db: Arc::new(db::Db::open(&cfg.database_path).unwrap()),
        media: Arc::new(crate::media::Media::new(&cfg).unwrap()),
        config: Arc::new(cfg.clone()),
        model_config: Arc::new(arc_swap::ArcSwap::from_pointee(cfg)),
        providers: Arc::new(arc_swap::ArcSwap::from_pointee(providers)),
        catalog: Arc::new(crate::catalog::Catalog::default()),
        worker: WorkerHandle::default(),
    };
    (state, root, model_calls, render_calls)
}
/// 真实业务路由请求，检查幂等键、状态和 JSON 错误格式。
async fn request(state: &AppState, path: &str, body: Value) -> (u16, Value) {
    let req = axum::http::Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json")
        .header("idempotency-key", make_id("key"))
        .body(axum::body::Body::from(body.to_string()))
        .unwrap();
    let response = crate::api::router(state.clone())
        .oneshot(req)
        .await
        .unwrap();
    let code = response.status().as_u16();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (code, serde_json::from_slice(&bytes).unwrap())
}
/// 在事务内认领任务，模拟 worker 的 fencing 令牌后执行一次。
async fn run(state: &AppState, id: &str) {
    let attempt = state
        .db
        .write(|c| {
            let mut job = db::load_job(c, id)?.unwrap();
            assert_eq!(job.job.status, JobStatus::Queued);
            job.job.status = JobStatus::Running;
            job.attempt += 1;
            job.job.started_at.get_or_insert_with(now_iso);
            db::save_job(c, &job)?;
            Ok(job.attempt)
        })
        .unwrap();
    crate::pipeline::Runner::new(
        state.clone(),
        id.into(),
        attempt,
        tokio_util::sync::CancellationToken::new(),
    )
    .run()
    .await;
}
/// 读取任务快照供每个门控断言使用。
fn job(state: &AppState, id: &str) -> db::JobRow {
    state
        .db
        .read(|c| Ok(db::load_job(c, id)?.unwrap()))
        .unwrap()
}
/// 使用服务器当前问题 ID 回答，不能把旧答案提交到下一步。
async fn answer(state: &AppState, id: &str, value: &str) {
    let q = job(state, id).job.question.unwrap();
    let (status, data) = request(
        state,
        &format!("/api/v1/jobs/{id}/answers"),
        json!({"question_id":q.id,"value":value}),
    )
    .await;
    assert_eq!(status, 202, "{data}");
    run(state, id).await;
}
/// 创建真实项目和生成任务，主题本身不能跳过任何用户确认。
async fn start(state: &AppState) -> String {
    let (status, data) = request(state, "/api/v1/projects", json!({})).await;
    assert_eq!(status, 201, "{data}");
    let project = data["data"]["project"]["id"].as_str().unwrap();
    let(status,data)=request(state,&format!("/api/v1/projects/{project}/messages"),json!({"intent":"generate","text":"说明一个概念","asset_ids":[],"expected_draft_revision":1})).await;
    assert_eq!(status, 202, "{data}");
    let id = data["data"]["job_id"].as_str().unwrap().to_string();
    run(state, &id).await;
    id
}
/// 模型服务超时应产生可识别错误，后续才能执行用户已同意的回退策略。
#[tokio::test]
async fn voice_deadline_is_bounded() {
    let r = voice_with_deadline(
        &PendingVoice,
        VoiceRequest {
            text: "测试".into(),
            voice_id: "Cherry".into(),
            speech_rate: 1.0,
            language: "zh-CN".into(),
            target_duration_ms: 1000,
        },
        std::time::Duration::from_millis(10),
    )
    .await;
    assert_eq!(r.unwrap_err().code, "VOICE_TIMEOUT");
}
/// 覆盖逐步确认、非法选择、数据库重开、默认不剪辑、成片先交付与最终结束。
#[tokio::test]
async fn guided_choices_survive_restart_and_publish_before_optional_edit() {
    let (mut state, root, models, renders) = fixture();
    let id = start(&state).await;
    assert_eq!(models.load(Ordering::SeqCst), 0);
    let original = job(&state, &id).job.question.unwrap();
    assert_eq!(original.skill_id.as_deref(), Some("style"));
    assert_eq!(original.options.len(), 4);
    let (status, _) = request(
        &state,
        &format!("/api/v1/jobs/{id}/answers"),
        json!({"question_id":original.id,"value":"invented"}),
    )
    .await;
    assert_eq!(status, 422);
    state.db = Arc::new(db::Db::open(&state.config.database_path).unwrap());
    assert_eq!(job(&state, &id).job.question.unwrap().id, original.id);
    for (step, value) in [
        ("style", "minimal"),
        ("duration", "15"),
        ("visual", "svg"),
        ("voice", "bgm"),
        ("music", "auto"),
        ("plan", "continue"),
        ("script", "continue"),
        ("storyboard", "continue"),
        ("production", "continue"),
    ] {
        let j = job(&state, &id);
        assert_eq!(j.job.status, JobStatus::WaitingInput, "{j:?}");
        assert_eq!(
            j.job.question.as_ref().unwrap().skill_id.as_deref(),
            Some(step),
            "{j:?}"
        );
        answer(&state, &id, value).await;
    }
    let j = job(&state, &id);
    assert!(j.input.awaiting_post_edit);
    assert_eq!(
        j.job.question.as_ref().unwrap().skill_id.as_deref(),
        Some("post_edit")
    );
    assert_eq!(models.load(Ordering::SeqCst), 3);
    assert_eq!(renders.load(Ordering::SeqCst), 1);
    let messages = state.db.read(|c| {
        Ok(c.query_row("SELECT COUNT(*) FROM messages WHERE project_id=?1 AND role='user' AND text='深色几何 · 青绿极简'", [&j.job.project_id], |r| r.get::<_, i64>(0))?)
    }).unwrap();
    assert_eq!(messages, 1, "选择应显示用户看见的中文标签而非内部值");
    assert!(j
        .job
        .logs
        .iter()
        .any(|l| l.message.contains("基础视频编码")));
    let count: i64 = state
        .db
        .read(|c| Ok(c.query_row("SELECT COUNT(*) FROM versions", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(count, 1);
    answer(&state, &id, "skip").await;
    assert_eq!(
        job(&state, &id).job.question.unwrap().skill_id.as_deref(),
        Some("delivery")
    );
    answer(&state, &id, "continue").await;
    assert_eq!(job(&state, &id).job.status, JobStatus::Succeeded);
    assert_eq!(renders.load(Ordering::SeqCst), 1);
    let _ = std::fs::remove_dir_all(root);
}

/// 重试保留原规则版本，规则升级不得通过重试静默沿用旧选择。
#[tokio::test]
async fn retry_preserves_skill_version_and_rejects_silent_migration() {
    let (state, root, models, _) = fixture();
    let id = start(&state).await;
    let project = job(&state, &id).job.project_id;
    state
        .db
        .write(|t| {
            let mut old = db::load_job(t, &id)?.unwrap();
            old.input.skill_version = Some("old-release".into());
            old.job.status = JobStatus::Cancelled;
            old.job.question = None;
            db::save_job(t, &old)?;
            let mut p = db::load_project(t, &project)?.unwrap();
            crate::ops::release_project(t, &mut p)?;
            db::save_project(t, &mut p)
        })
        .unwrap();
    let (status, response) = request(
        &state,
        &format!("/api/v1/jobs/{id}/retry"),
        json!({"expected_draft_revision":1}),
    )
    .await;
    assert_eq!(status, 202, "{response}");
    let retry = response["data"]["job_id"].as_str().unwrap();
    assert_eq!(
        job(&state, retry).input.skill_version.as_deref(),
        Some("old-release")
    );
    run(&state, retry).await;
    assert_eq!(
        job(&state, retry)
            .job
            .question
            .as_ref()
            .unwrap()
            .skill_id
            .as_deref(),
        Some("rules_update")
    );
    answer(&state, retry, "continue").await;
    assert_eq!(
        job(&state, retry).input.skill_version.as_deref(),
        Some(crate::skills::version().as_str())
    );
    assert_eq!(models.load(Ordering::SeqCst), 0);
    let _ = std::fs::remove_dir_all(root);
}
/// 缺模型必须停在下载许可，不接受“已经就绪”的虚假声明，也不自动下载。
#[tokio::test]
async fn missing_breeze_model_requires_explicit_download_or_alternative() {
    let (state, root, models, _) = fixture();
    let id = start(&state).await;
    for value in ["minimal", "15", "svg", "breeze"] {
        answer(&state, &id, value).await;
    }
    let q = job(&state, &id).job.question.unwrap();
    assert!(q.options.iter().any(|o| o.value == "download"));
    assert_eq!(models.load(Ordering::SeqCst), 0);
    answer(&state, &id, "ready").await;
    assert_ne!(job(&state, &id).job.question.as_ref().unwrap().id, q.id);
    let (status, _) = request(
        &state,
        &format!("/api/v1/jobs/{id}/answers"),
        json!({"question_id":q.id,"value":"bgm"}),
    )
    .await;
    assert_eq!(status, 409, "重新检测后的问题不能接受旧卡片答案");
    assert!(job(&state, &id)
        .job
        .question
        .unwrap()
        .prompt
        .contains("尚未找到"));
    answer(&state, &id, "bgm").await;
    assert_eq!(
        job(&state, &id).job.question.unwrap().skill_id.as_deref(),
        Some("music")
    );
    assert!(!root.join("models").exists());
    let _ = std::fs::remove_dir_all(root);
}

/// 确认到基础成片，便于分别验证超时回退与可选剪辑，不跳过任何实际门控。
async fn approve_to_preview(state: &AppState, id: &str, voice: &str, music: &str) {
    for value in [
        "minimal", "15", "svg", voice, music, "continue", "continue", "continue", "continue",
    ] {
        answer(state, id, value).await;
    }
    assert!(job(state, id).input.awaiting_post_edit);
}
/// API 真实请求超时后，持久化关闭所有旁白并用本地 BGM 完成视频，不要求重试。
#[tokio::test]
async fn api_timeout_falls_back_to_bgm_and_finishes_video() {
    let (state, root, _, renders) = fixture();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            axum::Router::new().route(
                "/audio/speech",
                axum::routing::post(|| async {
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    "late"
                }),
            ),
        )
        .await
        .unwrap()
    });
    let mut cfg = (**state.model_config.load()).clone();
    cfg.qwen_tts_base = format!("http://{address}");
    state.model_config.store(Arc::new(cfg));
    let id = start(&state).await;
    approve_to_preview(&state, &id, "api", "none").await;
    let j = job(&state, &id);
    assert!(j.input.voice_fallback);
    assert!(j
        .job
        .logs
        .iter()
        .any(|l| l.level == "warn" && l.message.contains("配音超时")));
    let draft = state
        .db
        .read(|c| Ok(db::load_project(c, &j.job.project_id)?.unwrap().draft))
        .unwrap();
    assert!(!draft.audio.narration_enabled);
    assert!(draft.audio.resolved_music_asset_id.is_some());
    answer(&state, &id, "skip").await;
    answer(&state, &id, "continue").await;
    assert_eq!(job(&state, &id).job.status, JobStatus::Succeeded);
    assert_eq!(renders.load(Ordering::SeqCst), 1);
    server.abort();
    let _ = std::fs::remove_dir_all(root);
}
/// 显式同意后才生成第二个剪辑版本；交付确认恢复不能再次执行剪辑。
#[tokio::test]
async fn editor_runs_only_after_opt_in_and_delivers_exactly_one_new_version() {
    let (state, root, _, renders) = fixture();
    let id = start(&state).await;
    approve_to_preview(&state, &id, "bgm", "auto").await;
    answer(&state, &id, "quercus").await;
    let j = job(&state, &id);
    assert!(j.input.post_edit_applied, "{j:?}");
    assert_eq!(
        j.job.question.unwrap().skill_id.as_deref(),
        Some("delivery")
    );
    assert_eq!(renders.load(Ordering::SeqCst), 1);
    let count = || {
        state
            .db
            .read(|c| Ok(c.query_row("SELECT COUNT(*) FROM versions", [], |r| r.get::<_, i64>(0))?))
            .unwrap()
    };
    assert_eq!(count(), 2);
    answer(&state, &id, "continue").await;
    assert_eq!(job(&state, &id).job.status, JobStatus::Succeeded);
    assert_eq!(count(), 2);
    let _ = std::fs::remove_dir_all(root);
}

/// 真实音乐文件入库，供测试上传选择和供应商 HTTP 输出复用。
async fn local_music(state: &AppState, project: &str, id: &str) -> db::AssetRow {
    let draft = crate::catalog::empty_draft();
    let media = procedural::ProceduralMusic
        .compose(MusicRequest {
            ctx: ProductionContext {
                brief: None,
                settings: draft.settings,
                user_text: "节奏测试".into(),
                answers: vec![],
                speech_rate: 1.,
            },
            mode: "auto".into(),
            track_id: None,
            duration_ms: 6000,
        })
        .await
        .unwrap();
    let (path, size) = state.media.store_output(project, id, &media).await.unwrap();
    let asset=serde_json::from_value(json!({"id":id,"project_id":project,"kind":"audio","source":"upload","status":"ready","file_name":"我的节奏.wav","mime_type":"audio/wav","size_bytes":size,"duration_ms":6000,"width":null,"height":null,"thumbnail_url":null,"error":null})).unwrap();
    let row = db::AssetRow {
        asset,
        storage_path: path.to_string_lossy().into(),
    };
    state.db.write(|t| db::insert_asset(t, &row)).unwrap();
    row
}

/// 审阅允许修改与返回；空意见、旧卡片均不能推进，也不会丢失原偏好。
#[tokio::test]
async fn review_edit_and_back_preserve_preferences_and_reject_stale_card() {
    let (state, root, models, _) = fixture();
    let id = start(&state).await;
    for value in ["minimal", "15", "svg", "bgm", "auto", "continue"] {
        answer(&state, &id, value).await;
    }
    let q = job(&state, &id).job.question.unwrap();
    assert_eq!(q.skill_id.as_deref(), Some("script"));
    let (status, _) = request(
        &state,
        &format!("/api/v1/jobs/{id}/answers"),
        json!({"question_id":q.id,"value":"edit","instruction":""}),
    )
    .await;
    assert_eq!(status, 422);
    let (status, _) = request(
        &state,
        &format!("/api/v1/jobs/{id}/answers"),
        json!({"question_id":q.id,"value":"edit","instruction":"换成抛硬币的例子"}),
    )
    .await;
    assert_eq!(status, 202);
    run(&state, &id).await;
    let j = job(&state, &id);
    assert_ne!(j.job.question.as_ref().unwrap().id, q.id);
    assert_eq!(crate::skills::choice(&j.input, "music"), Some("auto"));
    let draft = state
        .db
        .read(|c| Ok(db::load_project(c, &j.job.project_id)?.unwrap().draft))
        .unwrap();
    assert!(draft.script.unwrap().text.contains("抛硬币"));
    let (status, _) = request(
        &state,
        &format!("/api/v1/jobs/{id}/answers"),
        json!({"question_id":q.id,"value":"continue"}),
    )
    .await;
    assert_eq!(status, 409);
    answer(&state, &id, "continue").await;
    let before = models.load(Ordering::SeqCst);
    answer(&state, &id, "back").await;
    assert_eq!(models.load(Ordering::SeqCst), before);
    assert_eq!(
        job(&state, &id).job.question.unwrap().skill_id.as_deref(),
        Some("script")
    );
    let _ = std::fs::remove_dir_all(root);
}

/// 本地音乐在当前步骤选定，后端检查归属并实际分析，不让前端任意引用别的项目文件。
#[tokio::test]
async fn local_music_selection_is_owned_persisted_and_analyzed() {
    let (state, root, _, _) = fixture();
    let id = start(&state).await;
    let project = job(&state, &id).job.project_id;
    let music = local_music(&state, &project, "local-track").await;
    for value in ["minimal", "15", "svg", "bgm"] {
        answer(&state, &id, value).await;
    }
    let q = job(&state, &id).job.question.unwrap();
    let (status, _) = request(
        &state,
        &format!("/api/v1/jobs/{id}/answers"),
        json!({"question_id":q.id,"value":"upload","asset_id":"missing"}),
    )
    .await;
    assert_eq!(status, 404);
    let (status, _) = request(
        &state,
        &format!("/api/v1/jobs/{id}/answers"),
        json!({"question_id":q.id,"value":"upload","asset_id":music.asset.id}),
    )
    .await;
    assert_eq!(status, 202);
    run(&state, &id).await;
    answer(&state, &id, "continue").await;
    answer(&state, &id, "continue").await;
    let p = state
        .db
        .read(|c| Ok(db::load_project(c, &project)?.unwrap()))
        .unwrap();
    assert_eq!(
        p.draft.audio.resolved_music_asset_id.as_deref(),
        Some("local-track")
    );
    assert_eq!(p.artifacts.rhythm.as_ref().unwrap().asset_id, "local-track");
    assert!(!p.artifacts.rhythm.as_ref().unwrap().beats_ms.is_empty());
    let mut shots = p.draft.shots.clone();
    shots[0].narration = "修改的字幕完整保留，不应截断。".into();
    let q = job(&state, &id).job.question.unwrap();
    let(status,_)=request(&state,&format!("/api/v1/jobs/{id}/answers"),json!({"question_id":q.id,"value":"save_shots","instruction":serde_json::to_string(&shots).unwrap()})).await;
    assert_eq!(status, 202);
    run(&state, &id).await;
    assert_eq!(
        job(&state, &id).job.question.unwrap().skill_id.as_deref(),
        Some("storyboard")
    );
    let p = state
        .db
        .read(|c| Ok(db::load_project(c, &project)?.unwrap()))
        .unwrap();
    assert_eq!(p.draft.shots[0].narration, shots[0].narration);
    let _ = std::fs::remove_dir_all(root);
}

/// AI配乐使用真实HTTP协议，供应商收到纯音乐参数；恢复不会再次生成。
#[tokio::test]
async fn ai_music_calls_actual_http_adapter_once_with_instrumental_request() {
    let (state, root, _, _) = fixture();
    let id = start(&state).await;
    let project = job(&state, &id).job.project_id;
    let wave = local_music(&state, &project, "reference-wave").await;
    let hex = hex::encode(std::fs::read(&wave.storage_path).unwrap());
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            axum::Router::new().route(
                "/music_generation",
                axum::routing::post(move |axum::Json(body): axum::Json<Value>| {
                    let hex = hex.clone();
                    let calls = observed.clone();
                    async move {
                        assert_eq!(body["is_instrumental"], true);
                        assert_eq!(body["model"], "music-3.0");
                        assert_eq!(body["audio_setting"]["format"], "mp3");
                        calls.fetch_add(1, Ordering::SeqCst);
                        axum::Json(json!({"data":{"audio":hex}}))
                    }
                }),
            ),
        )
        .await
        .unwrap();
    });
    let mut cfg = (**state.model_config.load()).clone();
    cfg.provider_llm3 = "minimax".into();
    cfg.llm3.api_key = "test-only".into();
    cfg.llm3.model = "music-3.0".into();
    cfg.llm3_base = format!("http://{address}");
    state.model_config.store(Arc::new(cfg.clone()));
    let mut providers = (**state.providers.load()).clone();
    providers.music = Arc::new(openai::MiniMaxApiMusic::new(&cfg));
    state.providers.store(Arc::new(providers));
    for value in ["minimal", "15", "svg", "bgm", "ai", "continue", "continue"] {
        answer(&state, &id, value).await;
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    answer(&state, &id, "back").await;
    answer(&state, &id, "continue").await;
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let p = state
        .db
        .read(|c| Ok(db::load_project(c, &project)?.unwrap()))
        .unwrap();
    let a = state
        .db
        .read(|c| {
            Ok(
                db::load_asset(c, p.draft.audio.resolved_music_asset_id.as_ref().unwrap())?
                    .unwrap(),
            )
        })
        .unwrap();
    assert_eq!(a.asset.source, "music_generated");
    assert!(p.artifacts.rhythm.is_some());
    server.abort();
    let _ = std::fs::remove_dir_all(root);
}

/// 高清模式从原生SVG开始渲染1080p，真实媒体检查通过后才发布。
#[tokio::test]
async fn native_1080p_vibe_is_checked_before_publication() {
    let (state, root, _, _) = fixture();
    let id = start(&state).await;
    for value in [
        "minimal", "15", "svg", "bgm", "fixed", "continue", "continue", "continue",
    ] {
        answer(&state, &id, value).await;
    }
    answer(&state, &id, "publish").await;
    let j = job(&state, &id);
    assert!(j.input.awaiting_post_edit, "{j:?}");
    let p = state
        .db
        .read(|c| Ok(db::load_project(c, &j.job.project_id)?.unwrap()))
        .unwrap();
    assert_eq!(p.artifacts.quality.as_ref().unwrap()["height"], 1920);
    assert_eq!(p.artifacts.quality.as_ref().unwrap()["width"], 1080);
    assert_eq!(p.artifacts.quality.as_ref().unwrap()["passed"], true);
    if let Ok(dir) = std::env::var("EXPLAINCRAFT_ARTIFACT_DIR") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        let version = state
            .db
            .read(|c| {
                Ok(
                    db::load_version(c, p.project.latest_video_version_id.as_ref().unwrap())?
                        .unwrap(),
                )
            })
            .unwrap();
        let asset = state
            .db
            .read(|c| Ok(db::load_asset(c, &version.video_asset_id)?.unwrap()))
            .unwrap();
        std::fs::copy(asset.storage_path, dir.join("vibe-native-1080p.mp4")).unwrap();
    }
    let _ = std::fs::remove_dir_all(root);
}
