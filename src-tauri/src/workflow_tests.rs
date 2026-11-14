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
    assert_eq!(original.options.len(), crate::skills::manifest()["steps"][0]["options"].as_array().unwrap().len());
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
        Ok(c.query_row("SELECT COUNT(*) FROM messages WHERE project_id=?1 AND role='user' AND text='极简信息图'", [&j.job.project_id], |r| r.get::<_, i64>(0))?)
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

/// 本地音乐实际经过子问题；新上传、旧列表、刷新和返回使用真实 HTTP 与 SQLite。
#[tokio::test]
async fn music_substep_accepts_late_upload_and_has_recovery_exits() {
    for existing in [false, true] {
        let (mut state, root, _, _) = fixture();
        let id = start(&state).await;
        let owner = job(&state, &id).job.project_id;
        if existing { local_music(&state, &owner, "track-before").await; }
        for value in ["minimal", "15", "svg", "bgm", "upload"] { answer(&state, &id, value).await; }
        let q = job(&state, &id).job.question.unwrap();
        assert!(q.id.starts_with("skill:music_asset:"));
        assert!(q.options.iter().any(|o| o.value == "back"));
        state.db = Arc::new(db::Db::open(&state.config.database_path).unwrap());
        assert_eq!(job(&state, &id).job.question.unwrap().id, q.id);
        if !existing {
            answer(&state, &id, "ready").await;
            answer(&state, &id, "back").await;
            assert_eq!(job(&state, &id).job.question.unwrap().step, Some(5));
            answer(&state, &id, "upload").await;
        }
        let old = job(&state, &id).job.question.unwrap();
        let asset = if existing { "track-before" } else {
            local_music(&state, &owner, "track-after").await;
            "track-after"
        };
        answer(&state, &id, asset).await;
        assert_eq!(job(&state, &id).job.question.unwrap().step, Some(6));
        let (status, _) = request(&state, &format!("/api/v1/jobs/{id}/answers"), json!({"question_id":old.id,"value":asset})).await;
        assert_eq!(status,409);
        for value in ["continue","continue","continue","continue","skip","continue"] { answer(&state,&id,value).await; }
        assert_eq!(job(&state,&id).job.status,JobStatus::Succeeded);
        let _ = std::fs::remove_dir_all(root);
    }
}

/// 配乐缺配置有改选出口；基础步骤可返回，取消后的卡片不能回答。
#[tokio::test]
async fn setup_backtracking_missing_music_api_and_cancel_are_reachable() {
    let (state, root, _, _) = fixture();
    let id=start(&state).await;
    for value in ["minimal","back","minimal","15","back","15","svg","back","svg","bgm","ai"] {answer(&state,&id,value).await;}
    assert!(job(&state,&id).job.question.unwrap().id.contains("music_setup"));
    answer(&state,&id,"back").await;
    assert_eq!(job(&state,&id).job.question.unwrap().step,Some(5));
    answer(&state,&id,"auto").await;
    let q=job(&state,&id).job.question.unwrap();
    let (status,_) = request(&state,&format!("/api/v1/jobs/{id}/cancel"),json!({})).await;
    assert_eq!(status,202);
    let (status,_) = request(&state,&format!("/api/v1/jobs/{id}/answers"),json!({"question_id":q.id,"value":"continue"})).await;
    assert_eq!(status,409);
    let _=std::fs::remove_dir_all(root);
}

/// 仅供 Playwright 的本机契约桥：真实路由、数据库、worker、上传与编码器，模型为确定性替身。
#[tokio::test]
#[ignore = "local Playwright bridge; no paid providers"]
async fn browser_contract_server() {
    let ready=std::env::var("EXPLAINCRAFT_BROWSER_READY").expect("explicit test ready path required");
    let (mut state,root,_,_)=fixture();
    let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port=listener.local_addr().unwrap().port();
    let mut config=(*state.config).clone();config.public_origin=format!("http://127.0.0.1:{port}");config.auth_mode=crate::AuthMode::Dev;
    state.media=Arc::new(crate::media::Media::new(&config).unwrap());
    state.config=Arc::new(config.clone());state.model_config=Arc::new(arc_swap::ArcSwap::from_pointee(config));
    crate::worker::spawn(state.clone());
    let runtime_state=state.clone();
    let router=crate::api::router(state).route("/__test/runtime",axum::routing::get(move || {let s=runtime_state.clone();async move {axum::Json(crate::preflight::inspect(&s.model_config.load_full()).await)}}));
    std::fs::write(&ready,json!({"port":port,"root":root}).to_string()).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(600),async {axum::serve(listener,router).await}).await.ok();
    // 测试数据留给审查，绝不使用真实应用的数据库路径。
}

/// 遍历发布清单的每个确认选项，真实推进状态机并输出可审阅的分支转换证据。
#[tokio::test]
async fn every_manifest_choice_reaches_an_actionable_state() {
    let manifest=crate::skills::manifest();let steps=manifest["steps"].as_array().unwrap();let mut evidence=vec![];
    let defaults=[("style","minimal"),("duration","15"),("visual","svg"),("voice","bgm"),("music","auto"),("plan","continue"),("script","continue"),("storyboard","continue"),("production","continue"),("post_edit","skip"),("delivery","continue")];
    for step in steps.iter().filter(|s|s["requires_confirmation"]==true){
        let target=step["id"].as_str().unwrap();
        for option in step["options"].as_array().unwrap(){
            let value=option["value"].as_str().unwrap();let(state,root,_,_)=fixture();let id=start(&state).await;
            for(name,value)in defaults {if name==target{break;}answer(&state,&id,value).await;}
            let q=job(&state,&id).job.question.unwrap();assert_eq!(q.skill_id.as_deref(),Some(target));
            let owner=job(&state,&id).job.project_id;
            let instruction=if value=="save_shots" {state.db.read(|c|Ok(serde_json::to_string(&db::load_project(c,&owner)?.unwrap().draft.shots).unwrap())).unwrap()}else{"保留用户自定义视觉要求，用具体对象改进解释。".into()};
            let(status,response)=request(&state,&format!("/api/v1/jobs/{id}/answers"),json!({"question_id":q.id,"value":value,"instruction":instruction})).await;
            assert_eq!(status,202,"{target}/{value}: {response}");run(&state,&id).await;
            let next=job(&state,&id);assert!([JobStatus::WaitingInput,JobStatus::Succeeded].contains(&next.job.status),"{target}/{value}: {:?}",next.job);
            if let Some(q)=&next.job.question {assert!(!q.options.is_empty(),"{target}/{value} dead end");assert_ne!(q.id,job(&state,&id).input.answers.last().map(|a|a.question_id.clone()).unwrap_or_default());}
            evidence.push(json!({"step":target,"choice":value,"status":next.job.status,"next_step":next.job.question.as_ref().and_then(|q|q.skill_id.clone()),"next_question":next.job.question.as_ref().map(|q|q.id.clone()),"recoverable":true}));
            let _=std::fs::remove_dir_all(root);
        }
    }
    assert!(evidence.len()>=46);
    if let Ok(dir)=std::env::var("EXPLAINCRAFT_ARTIFACT_DIR"){std::fs::create_dir_all(&dir).unwrap();std::fs::write(std::path::Path::new(&dir).join("workflow-branches.json"),serde_json::to_vec_pretty(&evidence).unwrap()).unwrap();}
}

/// 只有用户批准预算并显式开启才执行；最多 19 次现有模型调用，保留 usage 与真实视频。
#[tokio::test]
#[ignore = "paid DeepSeek verification requires explicit user approval"]
async fn paid_deepseek_scene_validation() {
    assert_eq!(std::env::var("EXPLAINCRAFT_APPROVED_PAID").as_deref(),Ok("3"));
    crate::load_env_file(Some(&std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".env")));
    let cfg=crate::Config::from_env();assert_eq!(cfg.provider_llm1,"siliconflow");assert!(cfg.llm1.model.contains("DeepSeek-V4.1-Flash"));
    let client=Arc::new(siliconflow::SiliconFlow::new(&cfg).unwrap());let model=siliconflow::GlmScript::new(client.clone(),&cfg);
    let (state,_,_,_)=fixture();let id=start(&state).await;
    let root=std::path::PathBuf::from(std::env::var("EXPLAINCRAFT_ARTIFACT_DIR").unwrap()).join("paid");std::fs::create_dir_all(&root).unwrap();
    let records=|| state.db.read(|c|{let mut stmt=c.prepare("SELECT data FROM generation_model_calls WHERE job_id=?1 ORDER BY rowid")?;let rows=stmt.query_map([&id],|r|r.get::<_,String>(0))?.collect::<Result<Vec<_>,_>>()?;Ok(rows.iter().map(|s|serde_json::from_str::<Value>(s).unwrap()).collect::<Vec<_>>())}).unwrap();
    let mut report=json!({"model":cfg.llm1.model,"budget_cny":3,"max_calls":19,"scenes":[]});
    let gate=||{let r=records();assert!(r.len()<19);let upper:f64=r.iter().map(|v|v["usage"]["input_tokens"].as_f64().unwrap_or(16000.)*2./1e6+v["usage"]["output_tokens"].as_f64().unwrap_or(12000.)*8./1e6).sum();assert!(upper+0.16<3.);};
    gate();let probe=crate::metrics::operation(&state,&id,1,StageKey::Visuals,"批准的Schema探针",client.probe_scene_schema(&cfg.llm1)).await;
    report["json_schema_supported"]=match probe{Ok(v)=>json!(v),Err(e)=>json!({"supported":false,"error":e.message})};
    for(name,raw)in [("probability",include_str!("../tests/fixtures/scenes/probability.json")),("tcp",include_str!("../tests/fixtures/scenes/tcp.json")),("newton",include_str!("../tests/fixtures/scenes/newton.json"))]{
        let base:crate::scene_plan::ScenePlan=serde_json::from_str(raw).unwrap();
        let supplied_data:Value=if name=="probability"{json!({"points":base.entities.iter().find(|e|e.id=="curve").unwrap().points,"note":"固定样本，统计结果不是每次保证"})}else{json!({"claims":base.claims})};
        for ratio in ["16:9","9:16"]{
            let stem=format!("{name}-{}",ratio.replace(':',"x"));let mut draft=crate::catalog::empty_draft();draft.settings.aspect_ratio=ratio.into();draft.settings.style_id="scientific".into();
            let mut ctx=ProductionContext{brief:None,settings:draft.settings,user_text:format!("用户原始要求：用科学纪录片风格展示{}。完整实验物体、内部组成、动态曲线和实时数字。不要矩形文字卡，不要网络素材；图形占据当前画幅。必须遵守这些给定事实和真实数据：{}",base.objective,supplied_data),answers:vec![],speech_rate:1.};
            let narration=std::fs::read_to_string(root.parent().unwrap().join(format!("{name}-narration.txt"))).unwrap();
            let mut shot:Shot=serde_json::from_value(json!({"id":stem,"duration_ms":6000,"narration":narration,"visual_prompt":format!("目标：{}。核心图形概念：{}",base.objective,base.visual_concept)})).unwrap();
            let mut repaired=false;let mut plan=None;
            for attempt in 0..=1 {gate();let result=crate::metrics::operation(&state,&id,1,StageKey::Visuals,&format!("{stem} 导演计划 {}",attempt+1),model.plan_scene(VisualRequest{ctx:ctx.clone(),shot:shot.clone(),shot_index:0,shot_count:1})).await;
                match result{Ok(Some(p))=>{plan=Some(p);break;},Ok(None)=>break,Err(e)=>{ctx.user_text.push_str(&format!("\n仅修复当前导演计划，错误：{}。严格检查 action、字段和边界。",e.message));repaired=true;}}
            }
            let Some(plan)=plan else{report["scenes"].as_array_mut().unwrap().push(json!({"scene":stem,"passed":false,"reason":"invalid director plan after one repair"}));continue;};
            shot.visual_prompt=json!({"scene_plan":plan}).to_string();std::fs::write(root.join(format!("{stem}-plan.json")),serde_json::to_vec_pretty(&plan).unwrap()).unwrap();
            let mut outcome=None;
            for attempt in 0..if repaired{1}else{2}{gate();let result=crate::metrics::operation(&state,&id,1,StageKey::Visuals,&format!("{stem} 画面代码 {}",attempt+1),model.generate_shot(VisualRequest{ctx:ctx.clone(),shot:shot.clone(),shot_index:0,shot_count:1})).await;
                match result{Ok(media)=>{outcome=Some(media);break;},Err(e)=>{ctx.user_text.push_str(&format!("\n仅修复当前镜头代码：{}。保证画布和viewBox一致。",e.message));}}
            }
            let entry=if let Some(media)=outcome{
                let path=root.join(format!("{stem}.mp4"));match media.data{MediaData::Bytes(bytes)=>std::fs::write(&path,bytes).unwrap(),MediaData::Path(p)=>{std::fs::copy(p,&path).unwrap();}}
                let(w,h)=crate::util::dimensions(ratio);let technical=crate::quality::inspect(&path,6000,w,h,false).await;
                let audit=crate::quality::visual_audit(&path,6000,Some(&plan)).await;
                for at in [0.5,2.5,4.8]{std::process::Command::new("ffmpeg").args(["-v","error","-y","-ss",&at.to_string(),"-i"]).arg(&path).args(["-frames:v","1"]).arg(root.join(format!("{stem}-{at:.1}s.png"))).status().unwrap();}
                json!({"scene":stem,"technical_pass":technical.is_ok(),"audit":audit,"video":path,"visual_review_required":true})
            }else{json!({"scene":stem,"passed":false,"reason":"code render failed within repair budget"})};
            report["scenes"].as_array_mut().unwrap().push(entry);report["calls"]=json!(records());std::fs::write(root.join("usage-and-results.json"),serde_json::to_vec_pretty(&report).unwrap()).unwrap();
        }
    }
    let calls=records();report["calls"]=json!(calls);let input:u64=calls.iter().filter_map(|v|v["usage"]["input_tokens"].as_u64()).sum();let output:u64=calls.iter().filter_map(|v|v["usage"]["output_tokens"].as_u64()).sum();report["input_tokens"]=json!(input);report["output_tokens"]=json!(output);report["estimated_cost_cny_upper_rate"]=json!(input as f64*2./1e6+output as f64*8./1e6);std::fs::write(root.join("usage-and-results.json"),serde_json::to_vec_pretty(&report).unwrap()).unwrap();assert!(calls.len()<=19);
    eprintln!("Paid verification: {} calls, {} input + {} output tokens; report {}",calls.len(),input,output,root.display());
}

/// 在真实编码阶段注入一次失败，重试复用已保存的画面、文案、音轨与确认。
struct OnceFailRenderer(AtomicUsize, direct::DirectRenderer);
#[async_trait]
impl Renderer for OnceFailRenderer {
    async fn render(&self,r:RenderRequest)->ProviderResult<MediaOutput>{if self.0.fetch_add(1,Ordering::SeqCst)==0{Err(ProviderError::render_failed("测试：首次编码失败，可重试"))}else{self.1.render(r).await}}
    async fn export(&self,r:ExportRequest)->ProviderResult<MediaOutput>{self.1.export(r).await}
    async fn export_bundle(&self,r:ProjectBundleRequest)->ProviderResult<MediaOutput>{self.1.export_bundle(r).await}
}
#[tokio::test]
async fn failed_render_retry_reuses_completed_assets_and_choices() {
    let(state,root,calls,_)=fixture();let mut providers=(*state.providers.load_full()).clone();providers.renderer=Arc::new(OnceFailRenderer(AtomicUsize::new(0),direct::DirectRenderer::new(&state.config)));state.providers.store(Arc::new(providers));
    let id=start(&state).await;for value in ["minimal","15","svg","bgm","auto","continue","continue","continue","continue"]{answer(&state,&id,value).await;}
    let failed=job(&state,&id);assert_eq!(failed.job.status,JobStatus::Failed);let before=calls.load(Ordering::SeqCst);
    let p=state.db.read(|c|Ok(db::load_project(c,&failed.job.project_id)?.unwrap())).unwrap();let assets=p.draft.shots[0].asset_ids.clone();assert!(!assets.is_empty());
    let(status,data)=request(&state,&format!("/api/v1/jobs/{id}/retry"),json!({"expected_draft_revision":p.project.draft_revision})).await;assert_eq!(status,202,"{data}");let next=data["data"]["job_id"].as_str().unwrap();run(&state,next).await;
    assert_eq!(job(&state,next).job.question.unwrap().skill_id.as_deref(),Some("post_edit"));assert_eq!(calls.load(Ordering::SeqCst),before);
    let p=state.db.read(|c|Ok(db::load_project(c,&failed.job.project_id)?.unwrap())).unwrap();assert_eq!(p.draft.shots[0].asset_ids,assets);assert!(p.draft.creative_brief.is_some());answer(&state,next,"skip").await;answer(&state,next,"continue").await;assert_eq!(job(&state,next).job.status,JobStatus::Succeeded);let _=std::fs::remove_dir_all(root);
}

/// 已批准 19 次总额度中的最后四次：受控矢量代码修复、严格 Schema 与原生重排，禁止超额重跑。
#[tokio::test]
#[ignore = "remaining approved paid verification budget only"]
async fn paid_bounded_scene_repairs() {
    assert_eq!(std::env::var("EXPLAINCRAFT_APPROVED_PAID").as_deref(),Ok("3"));
    let base=std::path::PathBuf::from(std::env::var("EXPLAINCRAFT_ARTIFACT_DIR").unwrap());
    let prior:Value=serde_json::from_slice(&std::fs::read(base.join("paid/usage-and-results.json")).unwrap()).unwrap();let prior_calls=prior["calls"].as_array().unwrap().len();assert_eq!(prior_calls,15);
    let root=base.join("paid-bounded");std::fs::create_dir_all(&root).unwrap();assert!(!root.join("usage-and-results.json").exists(),"paid repair has already run; inspect ledger before any new call");
    crate::load_env_file(Some(&std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".env")));let cfg=crate::Config::from_env();assert!(cfg.llm1.model.contains("DeepSeek-V4.1-Flash"));let model=siliconflow::GlmScript::new(Arc::new(siliconflow::SiliconFlow::new(&cfg).unwrap()),&cfg);
    let(state,_,_,_)=fixture();let id=start(&state).await;
    let records=||state.db.read(|c|{let mut s=c.prepare("SELECT data FROM generation_model_calls WHERE job_id=?1 ORDER BY rowid")?;let rows=s.query_map([&id],|r|r.get::<_,String>(0))?.collect::<Result<Vec<_>,_>>()?;Ok(rows.iter().map(|s|serde_json::from_str::<Value>(s).unwrap()).collect::<Vec<_>>())}).unwrap();
    let gate=||assert!(prior_calls+records().len()<19);
    let mut report=json!({"model":cfg.llm1.model,"prior_calls":prior_calls,"scenes":[],"note":"概率与物理使用已独立核算的用户测试数据计划修复；TCP测试严格Schema。第二阶段模型提供矢量代码，宿主掌管动态数据和画幅。"});
    for(name,raw)in [("probability",include_str!("../tests/fixtures/scenes/probability.json")),("newton",include_str!("../tests/fixtures/scenes/newton.json")),("tcp",include_str!("../tests/fixtures/scenes/tcp.json"))]{
        let mut plan:crate::scene_plan::ScenePlan=serde_json::from_str(raw).unwrap();
        if name=="probability"{plan.entities.iter_mut().find(|e|e.id=="frequency").unwrap().sample_from="curve".into();}
        let mut settings=crate::catalog::empty_draft().settings;settings.aspect_ratio="16:9".into();settings.style_id="scientific".into();
        let ctx=ProductionContext{brief:None,settings:settings.clone(),user_text:format!("用户要求：科学编辑式暖金图解；禁止文字卡片。必须遵守这些已提供的可验证事实：{}",serde_json::to_string(&plan.claims).unwrap()),answers:vec![],speech_rate:1.};
        let mut shot:Shot=serde_json::from_value(json!({"id":format!("bounded-{name}"),"duration_ms":6000,"narration":std::fs::read_to_string(base.join(format!("{name}-narration.txt"))).unwrap(),"visual_prompt":format!("目标：{}；视觉概念：{}。使用具体终端轮廓、定向消息传递与协议状态变化。",plan.objective,plan.visual_concept)})).unwrap();
        if name=="tcp" {gate();match crate::metrics::operation(&state,&id,1,StageKey::Visuals,"bounded TCP schema plan",model.plan_scene(VisualRequest{ctx:ctx.clone(),shot:shot.clone(),shot_index:0,shot_count:1})).await{Ok(Some(p))=>plan=p,Err(e)=>{report["scenes"].as_array_mut().unwrap().push(json!({"scene":name,"plan_error":e.message,"plan_source":"verified fixture fallback"}));},_=>{}}}
        shot.visual_prompt=json!({"scene_plan":plan}).to_string();gate();let generated=crate::metrics::operation(&state,&id,1,StageKey::Visuals,&format!("bounded {name} vector code"),model.generate_shot(VisualRequest{ctx:ctx.clone(),shot,shot_index:0,shot_count:1})).await;
        if let Err(e)=generated {report["scenes"].as_array_mut().unwrap().push(json!({"scene":name,"code_error":e.message}));}
        let r=records();let effective=r.last().and_then(|r|r.pointer("/visual_source/source")).and_then(Value::as_str).and_then(|s|serde_json::from_str::<Value>(s).ok()).and_then(|v|serde_json::from_value::<crate::scene_plan::ScenePlan>(v["plan"].clone()).ok()).unwrap_or(plan);
        for ratio in ["16:9","9:16"] {let mut p=effective.clone();p.aspect_ratio=ratio.into();let(w,h)=crate::util::dimensions(ratio);let stem=format!("{name}-{}",ratio.replace(':',"x"));let svg=p.svg(6000,w as u32,h as u32).unwrap();std::fs::write(root.join(format!("{stem}.svg")),&svg).unwrap();std::fs::write(root.join(format!("{stem}-plan.json")),serde_json::to_vec_pretty(&p).unwrap()).unwrap();
            let video=crate::svg_animation::render(&svg,6000,w as u32,h as u32,0).await.unwrap();let raw_path=root.join(format!("{stem}-raw.mp4"));match video.data{MediaData::Bytes(b)=>std::fs::write(&raw_path,b).unwrap(),MediaData::Path(src)=>{std::fs::copy(src,&raw_path).unwrap();}}
            let mut context=ctx.clone();context.settings.aspect_ratio=ratio.into();let timeline=EditPlan{aspect_ratio:ratio.into(),output_resolution:"720p".into(),total_ms:6000,clips:vec![Clip{shot_id:stem.clone(),duration_ms:6000,visual_asset_id:Some("v".into()),narration_asset_id:Some("a".into()),subtitle:Some(std::fs::read_to_string(base.join(format!("{name}-narration.txt"))).unwrap()),..Default::default()}],narration_volume:1.,subtitles_enabled:true,..Default::default()};
            let output=direct::DirectRenderer::new(&cfg).render(RenderRequest{ctx:context,plan:timeline,inputs:std::collections::HashMap::from([("v".into(),raw_path),("a".into(),base.join(format!("{name}-voice.wav")))])}).await.unwrap();let path=root.join(format!("{stem}.mp4"));match output.data{MediaData::Bytes(b)=>std::fs::write(&path,b).unwrap(),MediaData::Path(src)=>{std::fs::copy(src,&path).unwrap();}}
            crate::quality::inspect(&path,6000,w,h,true).await.unwrap();for at in [0.5,2.5,5.5]{std::process::Command::new("ffmpeg").args(["-v","error","-y","-ss",&at.to_string(),"-i"]).arg(&path).args(["-frames:v","1"]).arg(root.join(format!("{stem}-{at:.1}s.png"))).status().unwrap();}
            report["scenes"].as_array_mut().unwrap().push(json!({"scene":stem,"technical_pass":true,"audit":crate::quality::visual_audit(&path,6000,Some(&p)).await,"video":path,"visual_review_required":true}));
        }
        report["calls"]=json!(records());std::fs::write(root.join("usage-and-results.json"),serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    }
    let calls=records();let input:u64=calls.iter().filter_map(|c|c["usage"]["input_tokens"].as_u64()).sum();let output:u64=calls.iter().filter_map(|c|c["usage"]["output_tokens"].as_u64()).sum();report["calls"]=json!(calls);report["input_tokens"]=json!(input);report["output_tokens"]=json!(output);report["total_calls_in_round"]=json!(prior_calls+calls.len());report["estimated_additional_cost_cny_upper_rate"]=json!(input as f64*2./1e6+output as f64*8./1e6);std::fs::write(root.join("usage-and-results.json"),serde_json::to_vec_pretty(&report).unwrap()).unwrap();assert!(prior_calls+calls.len()<=19);
    eprintln!("Bounded repairs: {} additional calls, {} input + {} output tokens",calls.len(),input,output);
}

/// 真实原生动画用于审计持久化测试，避免静态 PNG 替身绕过视觉检查。
struct NativeVisual;
#[async_trait]
impl VisualModel for NativeVisual {
    async fn generate_shot(&self,r:VisualRequest)->ProviderResult<MediaOutput>{let(w,h)=crate::util::output_dimensions(&r.ctx.settings.aspect_ratio,&r.ctx.settings.output_resolution);let svg=format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\"><rect width=\"{w}\" height=\"{h}\" fill=\"#0B1120\"/><circle cx=\"110\" cy=\"{}\" r=\"100\" fill=\"#FFD07A\"><animate attributeName=\"cx\" from=\"110\" to=\"{}\" dur=\"{}ms\" fill=\"freeze\"/></circle></svg>",h/2,w-110,r.shot.duration_ms);crate::svg_animation::render(&svg,r.shot.duration_ms,w as u32,h as u32,0).await}
}
#[tokio::test]
async fn regenerated_shot_keeps_new_visual_audit_over_old_stage_snapshot() {
    let(state,root,_,_)=fixture();let mut providers=(*state.providers.load_full()).clone();providers.visual=Arc::new(NativeVisual);state.providers.store(Arc::new(providers));let id=start(&state).await;
    for value in ["minimal","15","svg","bgm","auto","continue","continue","continue","continue","skip"]{answer(&state,&id,value).await;}
    let owner=job(&state,&id).job.project_id;
    let shot_id=state.db.write(|t|{let mut p=db::load_project(t,&owner)?.unwrap();let shot=p.draft.shots[0].id.clone();assert!(p.artifacts.visual_reviews.contains_key(&shot));p.artifacts.visual_reviews.get_mut(&shot).unwrap()["stale_marker"]=json!(true);db::save_project(t,&mut p)?;Ok(shot)}).unwrap();
    answer(&state,&id,"regenerate").await;
    let p=state.db.read(|c|Ok(db::load_project(c,&owner)?.unwrap())).unwrap();let audit=&p.artifacts.visual_reviews[&shot_id];assert!(audit["stale_marker"].is_null());assert_eq!(audit["freeze_scan_verified"],true);assert_eq!(job(&state,&id).job.question.unwrap().skill_id.as_deref(),Some("post_edit"));answer(&state,&id,"skip").await;assert_eq!(job(&state,&id).job.question.unwrap().skill_id.as_deref(),Some("delivery"));let _=std::fs::remove_dir_all(root);
}
