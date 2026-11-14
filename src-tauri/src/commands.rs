//! Tauri commands：工作台与进程内业务状态之间的唯一调用入口。
//!
//! 普通命令复用现有业务处理器，但不绑定 TCP 端口；上传使用原始 IPC 字节，
//! 媒体通过自定义 URI 协议读取，均不经过网络服务。

use axum::body::Body;
use axum::http::{HeaderMap, Method, Request, Response, StatusCode};
use http_body_util::BodyExt;
use serde::Serialize;
use serde_json::{json, Value};
use tauri::ipc::{InvokeBody, Request as IpcRequest};
use tauri::State;
use tower::ServiceExt;

use crate::api::{self, user_from_headers};
use crate::config::{provider_base, ModelSettings, ProviderSetting};
use crate::db;
use crate::error::ApiError;
use crate::state::AppState;

/// command 返回的统一响应：保留业务状态码和原有 JSON envelope，便于前端复用错误处理。
#[derive(Serialize)]
pub struct CommandReply {
    /// 业务处理状态码。
    status: u16,
    /// JSON 响应正文；空响应为 null。
    body: Value,
}

/// 项目事件轮询结果；发生变化时由前端重新获取一致性快照。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventPoll {
    /// 是否有游标之后的新事件。
    changed: bool,
    /// 本次读取到的新游标。
    cursor: String,
}

/// 在进程内执行一个业务请求，不创建监听器或 HTTP 连接。
async fn route(
    state: AppState,
    method: Method,
    path: String,
    headers: HeaderMap,
    body: Vec<u8>,
) -> Result<Response<Body>, String> {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .body(Body::from(body))
        .map_err(|error| error.to_string())?;
    *request.headers_mut() = headers;
    let router = api::router(state).layer(axum::middleware::from_fn(api::access_log));
    match router.oneshot(request).await {
        Ok(response) => Ok(response),
        Err(error) => match error {},
    }
}

/// 调用业务处理器并收集 JSON 响应。
async fn dispatch(
    state: AppState,
    method: Method,
    path: impl Into<String>,
    headers: HeaderMap,
    body: Value,
) -> Result<CommandReply, String> {
    let bytes = serde_json::to_vec(&body).map_err(|error| error.to_string())?;
    let response = route(state, method, path.into(), headers, bytes).await?;
    let status = response.status().as_u16();
    let bytes = response
        .into_body()
        .collect()
        .await
        .map_err(|error| error.to_string())?
        .to_bytes();
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).map_err(|error| error.to_string())?
    };
    Ok(CommandReply { status, body })
}

/// 读取 command 的 JSON 参数并调用业务处理器。
async fn json_command(
    state: &State<'_, AppState>,
    method: Method,
    path: impl Into<String>,
    request: &IpcRequest<'_>,
    body: Value,
) -> Result<CommandReply, String> {
    dispatch(
        state.inner().clone(),
        method,
        path,
        request.headers().clone(),
        body,
    )
    .await
}

/// 获取后端能力与上传限制。
#[tauri::command]
pub async fn capabilities(
    state: State<'_, AppState>,
    request: IpcRequest<'_>,
) -> Result<CommandReply, String> {
    json_command(
        &state,
        Method::GET,
        "/api/v1/capabilities",
        &request,
        json!({}),
    )
    .await
}

/// 获取最近项目列表。
#[tauri::command]
pub async fn list_projects(
    state: State<'_, AppState>,
    request: IpcRequest<'_>,
) -> Result<CommandReply, String> {
    json_command(
        &state,
        Method::GET,
        "/api/v1/projects?limit=100",
        &request,
        json!({}),
    )
    .await
}

/// 以请求头中的幂等 key 创建空项目。
#[tauri::command]
pub async fn create_project(
    state: State<'_, AppState>,
    request: IpcRequest<'_>,
) -> Result<CommandReply, String> {
    json_command(
        &state,
        Method::POST,
        "/api/v1/projects",
        &request,
        json!({}),
    )
    .await
}

/// 获取项目完整快照。
#[tauri::command]
pub async fn get_project(
    state: State<'_, AppState>,
    request: IpcRequest<'_>,
    project_id: String,
) -> Result<CommandReply, String> {
    json_command(
        &state,
        Method::GET,
        format!("/api/v1/projects/{project_id}"),
        &request,
        json!({}),
    )
    .await
}

/// 修改项目标题。
#[tauri::command]
pub async fn rename_project(
    state: State<'_, AppState>,
    request: IpcRequest<'_>,
    project_id: String,
    title: String,
) -> Result<CommandReply, String> {
    json_command(
        &state,
        Method::PATCH,
        format!("/api/v1/projects/{project_id}"),
        &request,
        json!({ "title": title }),
    )
    .await
}

/// 删除项目及其文件。
#[tauri::command]
pub async fn delete_project(
    state: State<'_, AppState>,
    request: IpcRequest<'_>,
    project_id: String,
) -> Result<CommandReply, String> {
    json_command(
        &state,
        Method::DELETE,
        format!("/api/v1/projects/{project_id}"),
        &request,
        json!({}),
    )
    .await
}

/// 提交生成、修改或仅讨论消息。
#[tauri::command]
pub async fn send_message(
    state: State<'_, AppState>,
    request: IpcRequest<'_>,
    project_id: String,
    input: Value,
) -> Result<CommandReply, String> {
    json_command(
        &state,
        Method::POST,
        format!("/api/v1/projects/{project_id}/messages"),
        &request,
        input,
    )
    .await
}

/// 保存草稿并校验修订号。
#[tauri::command]
pub async fn patch_draft(
    state: State<'_, AppState>,
    request: IpcRequest<'_>,
    project_id: String,
    input: Value,
) -> Result<CommandReply, String> {
    json_command(
        &state,
        Method::PATCH,
        format!("/api/v1/projects/{project_id}/draft"),
        &request,
        input,
    )
    .await
}

/// 获取项目素材列表。
#[tauri::command]
pub async fn list_assets(
    state: State<'_, AppState>,
    request: IpcRequest<'_>,
    project_id: String,
) -> Result<CommandReply, String> {
    json_command(
        &state,
        Method::GET,
        format!("/api/v1/projects/{project_id}/assets?limit=100"),
        &request,
        json!({}),
    )
    .await
}

/// 通过原始 IPC 字节上传 multipart 文件，避免 base64 或数字数组膨胀。
#[tauri::command]
pub async fn upload_asset(
    state: State<'_, AppState>,
    request: IpcRequest<'_>,
) -> Result<CommandReply, String> {
    let project_id = request
        .headers()
        .get("x-project-id")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "上传缺少项目 ID。".to_string())?
        .to_string();
    let InvokeBody::Raw(bytes) = request.body() else {
        return Err("上传必须使用原始字节。".into());
    };
    let response = route(
        state.inner().clone(),
        Method::POST,
        format!("/api/v1/projects/{project_id}/assets"),
        request.headers().clone(),
        bytes.clone(),
    )
    .await?;
    let status = response.status().as_u16();
    let bytes = response
        .into_body()
        .collect()
        .await
        .map_err(|error| error.to_string())?
        .to_bytes();
    let body = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    Ok(CommandReply { status, body })
}

/// 获取单个素材的处理状态。
#[tauri::command]
pub async fn get_asset(
    state: State<'_, AppState>,
    request: IpcRequest<'_>,
    asset_id: String,
) -> Result<CommandReply, String> {
    json_command(
        &state,
        Method::GET,
        format!("/api/v1/assets/{asset_id}"),
        &request,
        json!({}),
    )
    .await
}

/// 获取任务完整状态。
#[tauri::command]
pub async fn get_job(
    state: State<'_, AppState>,
    request: IpcRequest<'_>,
    job_id: String,
) -> Result<CommandReply, String> {
    json_command(
        &state,
        Method::GET,
        format!("/api/v1/jobs/{job_id}"),
        &request,
        json!({}),
    )
    .await
}

/// 请求取消任务。
#[tauri::command]
pub async fn cancel_job(
    state: State<'_, AppState>,
    request: IpcRequest<'_>,
    job_id: String,
) -> Result<CommandReply, String> {
    json_command(
        &state,
        Method::POST,
        format!("/api/v1/jobs/{job_id}/cancel"),
        &request,
        json!({}),
    )
    .await
}

/// 按当前草稿修订号重试任务。
#[tauri::command]
pub async fn retry_job(
    state: State<'_, AppState>,
    request: IpcRequest<'_>,
    job_id: String,
    expected_draft_revision: i64,
) -> Result<CommandReply, String> {
    json_command(
        &state,
        Method::POST,
        format!("/api/v1/jobs/{job_id}/retry"),
        &request,
        json!({ "expected_draft_revision": expected_draft_revision }),
    )
    .await
}

/// 回答任务追问并继续原任务。
#[tauri::command]
pub async fn answer_job(
    state: State<'_, AppState>,
    request: IpcRequest<'_>,
    job_id: String,
    question_id: String,
    value: String,
) -> Result<CommandReply, String> {
    json_command(
        &state,
        Method::POST,
        format!("/api/v1/jobs/{job_id}/answers"),
        &request,
        json!({ "question_id": question_id, "value": value }),
    )
    .await
}

/// 获取项目成片版本列表。
#[tauri::command]
pub async fn list_versions(
    state: State<'_, AppState>,
    request: IpcRequest<'_>,
    project_id: String,
) -> Result<CommandReply, String> {
    json_command(
        &state,
        Method::GET,
        format!("/api/v1/projects/{project_id}/versions?limit=100"),
        &request,
        json!({}),
    )
    .await
}

/// 获取指定成片版本及历史快照。
#[tauri::command]
pub async fn get_version(
    state: State<'_, AppState>,
    request: IpcRequest<'_>,
    project_id: String,
    version_id: String,
) -> Result<CommandReply, String> {
    json_command(
        &state,
        Method::GET,
        format!("/api/v1/projects/{project_id}/versions/{version_id}"),
        &request,
        json!({}),
    )
    .await
}

/// 导出指定版本为 MP4，或包含全部素材与剪辑工程的 ZIP。
#[tauri::command]
pub async fn export_video(
    state: State<'_, AppState>,
    request: IpcRequest<'_>,
    project_id: String,
    version_id: String,
    resolution: String,
    include_project: bool,
) -> Result<CommandReply, String> {
    json_command(
        &state,
        Method::POST,
        format!("/api/v1/projects/{project_id}/versions/{version_id}/exports"),
        &request,
        json!({ "format": "mp4", "resolution": resolution, "fps": 30, "include_project": include_project }),
    )
    .await
}

/// 获取带签名的素材下载地址。
#[tauri::command]
pub async fn download_asset(
    state: State<'_, AppState>,
    request: IpcRequest<'_>,
    asset_id: String,
) -> Result<CommandReply, String> {
    json_command(
        &state,
        Method::GET,
        format!("/api/v1/assets/{asset_id}/download"),
        &request,
        json!({}),
    )
    .await
}

/// 非阻塞读取游标之后的事件，替代 SSE 长连接。
#[tauri::command]
pub async fn project_events(
    state: State<'_, AppState>,
    request: IpcRequest<'_>,
    project_id: String,
    after: String,
) -> Result<EventPoll, String> {
    let user = user_from_headers(&state, request.headers())
        .map_err(|error| error.message)?
        .0;
    let after = after
        .parse::<i64>()
        .ok()
        .filter(|value| *value >= 0)
        .ok_or_else(|| "事件游标无效。".to_string())?;
    state
        .db
        .read(|conn| {
            let project = db::project_for(conn, &project_id, &user)?;
            if after > project.event_seq {
                return Err(ApiError::bad_request("事件游标无效。"));
            }
            if after < project.pruned_through {
                return Err(ApiError::new(
                    StatusCode::GONE,
                    "EVENT_CURSOR_EXPIRED",
                    "正在重新同步项目。",
                ));
            }
            let rows = db::events_after(conn, &project_id, after, 100)?;
            let cursor = rows.last().map_or(after, |row| row.seq);
            Ok(EventPoll {
                changed: !rows.is_empty(),
                cursor: cursor.to_string(),
            })
        })
        .map_err(|error| error.message)
}

/// 单个流程的供应商视图；空密钥只表示“是否存在”，不泄露内容。
fn access_view(provider: String, base: String, model: String, key: String) -> ProviderSetting {
    ProviderSetting {
        provider,
        base_url: base,
        model,
        api_key: None,
        has_api_key: !key.trim().is_empty(),
        ..Default::default()
    }
}

/// SVG 画面配置：未覆盖时把统一模型标为继承，便于页面展示「跟随创作模型」。
fn visual_setting(config: &crate::Config) -> ProviderSetting {
    if config.visual_overrides {
        access_view(
            config.provider_visual.clone(),
            if config.visual_base.is_empty() {
                provider_base(&config.provider_visual).into()
            } else {
                config.visual_base.clone()
            },
            config.visual.model.clone(),
            config.visual.api_key.clone(),
        )
        .overridden(true)
    } else {
        access_view(
            config.provider_llm1.clone(),
            if config.llm1_base.is_empty() {
                provider_base(&config.provider_llm1).into()
            } else {
                config.llm1_base.clone()
            },
            config.llm1.model.clone(),
            config.llm1.api_key.clone(),
        )
        .overridden(false)
    }
}

/// 把当前配置转换为 UI 视图，不返回任何密钥内容。
fn settings_view(config: &crate::Config) -> ModelSettings {
    let voice = match config.provider_llm4.as_str() {
        "siliconflow" => access_view(
            "siliconflow".into(),
            config.siliconflow_tts_base.clone(),
            config.siliconflow_tts.model.clone(),
            config.siliconflow_tts.api_key.clone(),
        )
        .with_voice(config.siliconflow_tts_voice.clone()),
        "indextts" => access_view(
            "indextts".into(),
            config.indextts_base.clone(),
            config.indextts_model.clone(),
            String::new(),
        )
        .with_voice(config.indextts_voice.clone()),
        "breeze" => access_view(
            "breeze".into(),
            config.comfyui_url.clone(),
            config.breeze_model.clone(),
            String::new(),
        )
        .with_voice("voice_warm".into()),
        _ => access_view(
            "qwen".into(),
            config.qwen_tts_base.clone(),
            config.qwen_tts.model.clone(),
            config.qwen_tts.api_key.clone(),
        )
        .with_voice(config.qwen_tts_voice.clone()),
    };
    ModelSettings {
        script: access_view(
            config.provider_llm1.clone(),
            if config.llm1_base.is_empty() {
                provider_base(&config.provider_llm1).into()
            } else {
                config.llm1_base.clone()
            },
            config.llm1.model.clone(),
            config.llm1.api_key.clone(),
        ),
        visual: visual_setting(config),
        music: access_view(
            config.provider_llm3.clone(),
            if config.llm3_base.is_empty() {
                provider_base(&config.provider_llm3).into()
            } else {
                config.llm3_base.clone()
            },
            config.llm3.model.clone(),
            config.llm3.api_key.clone(),
        ),
        voice,
    }
}

/// 读取当前创作、音乐与配音配置。
#[tauri::command]
pub fn get_model_settings(state: State<'_, AppState>) -> ModelSettings {
    settings_view(&state.model_config.load())
}

/// 更新 `.env` 中的单个键，保留其他配置与注释，不把密钥写进日志。
fn write_env_values(path: &std::path::Path, values: &[(&str, String)]) -> Result<(), String> {
    let existing = std::fs::read_to_string(path).unwrap_or_default();
    let mut lines: Vec<String> = existing.lines().map(str::to_string).collect();
    for (key, value) in values {
        let prefix = format!("{key}=");
        let escaped = value.replace('\\', "\\\\").replace('\'', "\\'");
        let line = format!("{key}='{escaped}'");
        if let Some(existing) = lines.iter_mut().find(|line| line.starts_with(&prefix)) {
            *existing = line;
        } else {
            lines.push(line);
        }
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    std::fs::write(path, format!("{}\n", lines.join("\n"))).map_err(|error| error.to_string())
}

/// 校验供应商与 base URL，空地址表示使用内置预设。
fn validate_setting(setting: &ProviderSetting, allowed: &[&str]) -> Result<(), String> {
    if !allowed.contains(&setting.provider.as_str()) {
        return Err(format!("不支持的供应商：{}", setting.provider));
    }
    if setting.model.trim().is_empty() {
        return Err("模型名称不能为空。".into());
    }
    if !setting.base_url.trim().is_empty() {
        let url = reqwest::Url::parse(setting.base_url.trim())
            .map_err(|_| "base URL 格式不正确。".to_string())?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err("base URL 必须使用 HTTP 或 HTTPS。".into());
        }
    }
    Ok(())
}

/// 文案、分镜、剪辑与 SVG 画面可用的 OpenAI 兼容供应商。
const CREATION_PROVIDERS: &[&str] = &[
    "openai",
    "aliyun",
    "deepseek",
    "minimax",
    "siliconflow",
    "glm",
    "custom",
];

/// 保存配置并原子替换供应商；新任务立即使用新配置。
#[tauri::command]
pub fn update_model_settings(
    state: State<'_, AppState>,
    settings: ModelSettings,
) -> Result<ModelSettings, String> {
    validate_setting(
        &settings.script,
        CREATION_PROVIDERS,
    )?;
    if settings.script.provider == "custom" && settings.script.base_url.trim().is_empty() {
        return Err("自定义供应商必须填写 OpenAI Compatible Endpoint。".into());
    }
    // 只有开启独立配置时才校验画面模型；继承统一模型时字段会被忽略。
    if settings.visual.is_overridden() {
        validate_setting(&settings.visual, &CREATION_PROVIDERS)?;
        if settings.visual.provider == "custom" && settings.visual.base_url.trim().is_empty() {
            return Err("自定义画面供应商必须填写 OpenAI Compatible Endpoint。".into());
        }
    }
    validate_setting(
        &settings.music,
        &["procedural", "siliconflow", "minimax", "stable_audio"],
    )?;
    validate_setting(
        &settings.voice,
        &["qwen", "aliyun", "siliconflow", "indextts", "breeze"],
    )?;
    if settings.voice.voice_id.trim().is_empty() {
        return Err("请选择或填写默认声线。".into());
    }
    let current = state.model_config.load();
    let mut next = (**current).clone();
    next.provider_llm1 = settings.script.provider.clone();
    next.llm1_base = settings.script.base_url.trim().into();
    next.llm1.model = settings.script.model.trim().into();
    if let Some(key) = &settings.script.api_key {
        next.llm1.api_key = key.clone();
    }
    // 画面模型可以跟随统一模型，也可以单独选择；未覆盖时清空覆盖字段以保持配置干净。
    next.visual_overrides = settings.visual.is_overridden();
    if next.visual_overrides {
        next.provider_visual = settings.visual.provider.clone();
        next.visual.model = settings.visual.model.trim().into();
        next.visual_base = settings.visual.base_url.trim().into();
        if let Some(key) = &settings.visual.api_key {
            next.visual.api_key = key.clone();
        }
    } else {
        next.visual = next.llm1.clone();
        next.provider_visual = next.provider_llm1.clone();
        next.visual_base.clear();
    }
    next.provider_llm3 = settings.music.provider.clone();
    next.llm3_base = settings.music.base_url.trim().into();
    next.llm3.model = settings.music.model.trim().into();
    if let Some(key) = &settings.music.api_key {
        next.llm3.api_key = key.clone();
    }
    match settings.voice.provider.as_str() {
        "siliconflow" => {
            next.provider_llm4 = "siliconflow".into();
            next.siliconflow_tts_base = settings.voice.base_url.trim().into();
            next.siliconflow_tts.model = settings.voice.model.trim().into();
            next.siliconflow_tts_voice = settings.voice.voice_id.trim().into();
            if let Some(key) = &settings.voice.api_key {
                next.siliconflow_tts.api_key = key.clone();
            }
        }
        "indextts" => {
            next.provider_llm4 = "indextts".into();
            next.indextts_base = settings.voice.base_url.trim().into();
            next.indextts_model = settings.voice.model.trim().into();
            next.indextts_voice = settings.voice.voice_id.trim().into();
        }
        "breeze" => {
            next.provider_llm4 = "breeze".into();
            next.comfyui_url = settings.voice.base_url.trim().into();
            next.breeze_model = settings.voice.model.trim().into();
        }
        _ => {
            next.provider_llm4 = "qwen".into();
            next.qwen_tts_base = settings.voice.base_url.trim().into();
            next.qwen_tts.model = settings.voice.model.trim().into();
            next.qwen_tts_voice = settings.voice.voice_id.trim().into();
            if let Some(key) = &settings.voice.api_key {
                next.qwen_tts.api_key = key.clone();
            }
        }
    }
    if !next.tts_voices.contains(&settings.voice.voice_id) {
        next.tts_voices.push(settings.voice.voice_id.trim().into());
    }
    let providers = crate::providers::Providers::from_config(&next)
        .map_err(|error| format!("配置无法应用：{error}"))?;
    let values = [
        ("PROVIDER_LLM1", next.provider_llm1.clone()),
        ("LLM1_BASE_URL", next.llm1_base.clone()),
        ("LLM1_MODEL", next.llm1.model.clone()),
        ("LLM1_API_KEY", next.llm1.api_key.clone()),
        (
            "VISUAL_MODEL",
            if next.visual_overrides {
                next.visual.model.clone()
            } else {
                String::new()
            },
        ),
        (
            "VISUAL_API_KEY",
            if next.visual_overrides {
                next.visual.api_key.clone()
            } else {
                String::new()
            },
        ),
        ("VISUAL_BASE_URL", next.visual_base.clone()),
        ("PROVIDER_VISUAL", next.provider_visual.clone()),
        ("PROVIDER_LLM3", next.provider_llm3.clone()),
        ("LLM3_BASE_URL", next.llm3_base.clone()),
        ("LLM3_MODEL", next.llm3.model.clone()),
        ("LLM3_API_KEY", next.llm3.api_key.clone()),
        ("PROVIDER_LLM4", next.provider_llm4.clone()),
        ("QWEN_TTS_BASE_URL", next.qwen_tts_base.clone()),
        ("QWEN_TTS_MODEL", next.qwen_tts.model.clone()),
        ("QWEN_TTS_API_KEY", next.qwen_tts.api_key.clone()),
        (
            "SILICONFLOW_TTS_BASE_URL",
            next.siliconflow_tts_base.clone(),
        ),
        ("SILICONFLOW_TTS_MODEL", next.siliconflow_tts.model.clone()),
        (
            "SILICONFLOW_TTS_API_KEY",
            next.siliconflow_tts.api_key.clone(),
        ),
        ("INDEXTTS_BASE_URL", next.indextts_base.clone()),
        ("INDEXTTS_MODEL", next.indextts_model.clone()),
        ("COMFYUI_URL", next.comfyui_url.clone()),
        ("BREEZE_MODEL", next.breeze_model.clone()),
    ];
    write_env_values(&next.settings_file, &values)?;
    state.providers.store(std::sync::Arc::new(providers));
    state.model_config.store(std::sync::Arc::new(next.clone()));
    Ok(settings_view(&next))
}

/// 调用 OpenAI Compatible `GET /models`，返回可供下拉选择的模型 ID。
#[tauri::command]
pub async fn list_models(
    state: State<'_, AppState>,
    setting: ProviderSetting,
    kind: Option<String>,
) -> Result<Vec<String>, String> {
    let base = if setting.base_url.trim().is_empty() {
        provider_base(&setting.provider)
    } else {
        setting.base_url.trim()
    };
    if base.is_empty() {
        return Err("请先填写 OpenAI Compatible Endpoint。".into());
    }
    let configured_key;
    let api_key = match setting.api_key.filter(|key| !key.trim().is_empty()) {
        Some(key) => Some(key),
        None => {
            let config = state.model_config.load();
            configured_key = match kind.as_deref() {
                Some("music") => config.llm3.api_key.clone(),
                Some("voice") if setting.provider == "siliconflow" => {
                    config.siliconflow_tts.api_key.clone()
                }
                Some("voice") => config.qwen_tts.api_key.clone(),
                Some("visual") if config.visual_overrides => config.visual.api_key.clone(),
                _ => config.llm1.api_key.clone(),
            };
            Some(configured_key)
        }
    };
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|error| error.to_string())?;
    let request = client.get(format!("{}/models", base.trim_end_matches('/')));
    let request = match api_key.filter(|key| !key.trim().is_empty()) {
        Some(key) => request.bearer_auth(key),
        None => request,
    };
    let response = request
        .send()
        .await
        .map_err(|_| "无法连接模型服务，请检查 Endpoint 和网络。".to_string())?;
    if !response.status().is_success() {
        return Err(format!("模型列表请求失败：HTTP {}", response.status()));
    }
    let data: Value = response
        .json()
        .await
        .map_err(|_| "模型列表响应不是有效 JSON。".to_string())?;
    let mut ids = std::collections::BTreeSet::new();
    for item in data["data"]
        .as_array()
        .or_else(|| data["models"].as_array())
        .into_iter()
        .flatten()
    {
        if let Some(id) = item["id"].as_str().or_else(|| item["name"].as_str()) {
            ids.insert(id.to_string());
        }
    }
    if ids.is_empty() {
        return Err("服务没有返回可用模型。".into());
    }
    Ok(ids.into_iter().collect())
}

/// 调用 TTS 供应商的 `/audio/voices`，返回用户可选择的声线 ID。
#[tauri::command]
pub async fn list_voices(
    state: State<'_, AppState>,
    setting: ProviderSetting,
) -> Result<Vec<String>, String> {
    if setting.provider == "breeze" {
        return Ok(vec!["voice_warm".into(), "voice_calm".into()]);
    }
    let base = if setting.base_url.trim().is_empty() {
        provider_base(&setting.provider)
    } else {
        setting.base_url.trim()
    };
    if base.is_empty() {
        return Err("请先填写 TTS Endpoint。".into());
    }
    let configured_key;
    let api_key = match setting.api_key.filter(|key| !key.trim().is_empty()) {
        Some(key) => Some(key),
        None => {
            let config = state.model_config.load();
            configured_key = if setting.provider == "siliconflow" {
                config.siliconflow_tts.api_key.clone()
            } else {
                config.qwen_tts.api_key.clone()
            };
            Some(configured_key)
        }
    };
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|error| error.to_string())?;
    let request = client.get(format!("{}/audio/voices", base.trim_end_matches('/')));
    let request = match api_key.filter(|key| !key.trim().is_empty()) {
        Some(key) => request.bearer_auth(key),
        None => request,
    };
    let response = request
        .send()
        .await
        .map_err(|_| "无法连接声线服务，请检查 Endpoint 和网络。".to_string())?;
    if response.status() == StatusCode::NOT_FOUND
        || response.status() == StatusCode::METHOD_NOT_ALLOWED
    {
        let voices = state.model_config.load().tts_voices.clone();
        if !voices.is_empty() {
            return Ok(voices);
        }
    }
    if !response.status().is_success() {
        return Err(format!("声线列表请求失败：HTTP {}", response.status()));
    }
    let data: Value = response
        .json()
        .await
        .map_err(|_| "声线列表响应不是有效 JSON。".to_string())?;
    let mut voices = std::collections::BTreeSet::new();
    for item in data["data"]
        .as_array()
        .or_else(|| data["voices"].as_array())
        .into_iter()
        .flatten()
    {
        if let Some(id) = item.as_str().or_else(|| {
            item["id"]
                .as_str()
                .or_else(|| item["voice_id"].as_str())
                .or_else(|| item["name"].as_str())
        }) {
            voices.insert(id.to_string());
        }
    }
    if voices.is_empty() {
        return Err("TTS 服务没有返回可选声线。".into());
    }
    Ok(voices.into_iter().collect())
}

/// 查询本地 breezeTTS2 模型是否就绪，以及下载目标目录。
#[tauri::command]
pub fn breeze_model_status(state: State<'_, AppState>) -> Result<crate::breeze_model::ModelStatus, String> {
    Ok(crate::breeze_model::status(&state.model_config.load()))
}

/// 下载本地 breezeTTS2 模型（约 5.2 GB）；进度通过 `breeze-model-progress` 事件推送。
#[tauri::command]
pub async fn download_breeze_model(
    app: tauri::AppHandle<tauri::Wry>,
    state: State<'_, AppState>,
    comfyui_dir: Option<String>,
) -> Result<crate::breeze_model::ModelStatus, String> {
    let config = state.model_config.load().clone();
    let (root, source) = match comfyui_dir.filter(|dir| !dir.trim().is_empty()) {
        Some(dir) => (std::path::PathBuf::from(dir.trim()), "config"),
        None => {
            crate::breeze_model::resolve_comfyui_dir(&config).ok_or_else(|| {
                "没有找到 ComfyUI 安装目录，请先填写本地 ComfyUI 路径。".to_string()
            })?
        }
    };
    if !root.is_dir() {
        return Err(format!("ComfyUI 目录不存在：{}", root.display()));
    }
    if source == "config" {
        let path = root.display().to_string();
        if config.comfyui_dir != path {
            write_env_values(&config.settings_file, &[("COMFYUI_DIR", path.clone())])
                .map_err(|error| format!("保存 ComfyUI 目录失败：{error}"))?;
            let mut next = (*config).clone();
            next.comfyui_dir = path;
            state.model_config.store(std::sync::Arc::new(next));
        }
    }
    crate::breeze_model::download(app, root)
        .await
        .map_err(|error| format!("模型下载失败：{error}"))
}

/// 中止正在进行的模型下载；已下载部分保留，下次继续。
#[tauri::command]
pub fn cancel_breeze_download() {
    crate::breeze_model::cancel();
}

/// 自定义媒体协议：在进程内校验签名并读取文件，保留 Range 与下载响应头。
pub async fn media_response(state: AppState, request: Request<Vec<u8>>) -> Response<Vec<u8>> {
    let result = async {
        let response = api::router(state).oneshot(request.map(Body::from)).await;
        let response = match response {
            Ok(response) => response,
            Err(error) => match error {},
        };
        let (parts, body) = response.into_parts();
        let bytes = body.collect().await?.to_bytes().to_vec();
        Ok::<_, Box<dyn std::error::Error>>(Response::from_parts(parts, bytes))
    }
    .await;
    result.unwrap_or_else(|error| {
        Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(error.to_string().into_bytes())
            .expect("媒体错误响应")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 创建隔离的 command 测试状态，不写用户应用数据目录。
    fn test_state() -> AppState {
        let root = std::env::temp_dir().join(format!(
            "explaincraft-command-test-{}",
            uuid::Uuid::new_v4()
        ));
        let mut config = crate::Config::from_env();
        config.database_path = root.join("test.sqlite3");
        config.storage_dir = root.join("storage");
        config.settings_file = root.join(".env");
        config.demo_media_dir =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../shared/public/media");
        config.public_origin = crate::media_origin().into();
        crate::build_state(config).expect("创建 command 测试状态")
    }

    /// command 调度不启动监听器，也能读取真实 SQLite 后端能力。
    #[tokio::test]
    async fn capabilities_are_served_through_the_command_dispatcher() {
        let reply = dispatch(
            test_state(),
            Method::GET,
            "/api/v1/capabilities",
            HeaderMap::new(),
            json!({}),
        )
        .await
        .expect("capabilities command");

        assert_eq!(reply.status, 200);
        assert!(reply.body["data"]["exports"]["resolutions"][0].is_string());
    }

    /// 自定义媒体协议直接读取白名单曲库，不依赖 HTTP 服务。
    #[tokio::test]
    async fn library_media_is_served_through_the_custom_protocol() {
        let request = Request::get("/api/v1/library/ambient.wav")
            .body(vec![])
            .expect("媒体请求");
        let response = media_response(test_state(), request).await;

        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.body().starts_with(b"RIFF"));
    }

    /// 使用 Tauri mock WebView 验证前端 IPC 参数能到达真实 AppState。
    #[tokio::test]
    async fn frontend_ipc_calls_reach_the_shared_backend_state() {
        use tauri::ipc::{CallbackFn, InvokeBody};
        use tauri::webview::InvokeRequest;
        use tauri::Manager;

        /// 按 `@tauri-apps/api/core` 的 JSON 形式调用生产 command handler。
        fn invoke(
            window: &tauri::WebviewWindow<tauri::test::MockRuntime>,
            command: &str,
            body: Value,
            headers: HeaderMap,
        ) -> Result<Value, Value> {
            tauri::test::get_ipc_response(
                window,
                InvokeRequest {
                    cmd: command.into(),
                    callback: CallbackFn(0),
                    error: CallbackFn(1),
                    url: if cfg!(windows) {
                        "http://tauri.localhost"
                    } else {
                        "tauri://localhost"
                    }
                    .parse()
                    .expect("WebView URL"),
                    body: InvokeBody::Json(body),
                    headers,
                    invoke_key: tauri::test::INVOKE_KEY.into(),
                },
            )
            .map(|body| body.deserialize::<Value>().expect("JSON command 响应"))
        }

        let app = tauri::test::mock_builder()
            .manage(test_state())
            .invoke_handler(crate::command_handler())
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("创建 mock Tauri 应用");
        let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("创建 mock WebView");

        let capabilities =
            invoke(&window, "capabilities", json!({}), HeaderMap::new()).expect("capabilities IPC");
        assert_eq!(capabilities["status"], 200);

        let mut headers = HeaderMap::new();
        headers.insert("idempotency-key", "ipc-create-project".parse().unwrap());
        let created =
            invoke(&window, "create_project", json!({}), headers).expect("create_project IPC");
        assert_eq!(created["status"], 201);
        let project_id = created["body"]["data"]["project"]["id"]
            .as_str()
            .expect("项目 ID")
            .to_string();

        let snapshot = invoke(
            &window,
            "get_project",
            json!({ "projectId": project_id }),
            HeaderMap::new(),
        )
        .expect("get_project IPC");
        assert_eq!(snapshot["status"], 200);
        assert_eq!(
            snapshot["body"]["data"]["project"]["id"],
            created["body"]["data"]["project"]["id"]
        );

        let mut headers = HeaderMap::new();
        headers.insert("idempotency-key", "ipc-send-message".parse().unwrap());
        let revision = snapshot["body"]["data"]["project"]["draft_revision"]
            .as_i64()
            .expect("草稿修订号");
        let accepted = invoke(
            &window,
            "send_message",
            json!({
                "projectId": project_id,
                "input": {
                    "intent": "chat",
                    "text": "验证前后端连接",
                    "asset_ids": [],
                    "expected_draft_revision": revision,
                }
            }),
            headers,
        )
        .expect("send_message IPC");
        assert_eq!(accepted["status"], 202);

        let events = invoke(
            &window,
            "project_events",
            json!({ "projectId": project_id, "after": "0" }),
            HeaderMap::new(),
        )
        .expect("project_events IPC");
        assert!(events["cursor"].is_string());
        assert!(events["changed"].is_boolean());

        let settings = invoke(&window, "get_model_settings", json!({}), HeaderMap::new())
            .expect("get_model_settings IPC");
        assert!(settings["script"]["apiKey"].is_null());

        let updated = invoke(
            &window,
            "update_model_settings",
            json!({
                "settings": {
                    "script": {
                        "provider": "custom",
                        "baseUrl": "https://provider.example.test/v1",
                        "model": "gpt-4o-mini",
                        "apiKey": null,
                        "hasApiKey": false
                    },
                    "music": {
                        "provider": "minimax",
                        "baseUrl": "https://api.minimaxi.com/v1",
                        "model": "music-1.5",
                        "apiKey": null,
                        "hasApiKey": false
                    },
                    "voice": {
                        "provider": "qwen",
                        "baseUrl": "https://dashscope.aliyuncs.com/compatible-mode/v1",
                        "model": "qwen-tts",
                        "apiKey": null,
                        "hasApiKey": false,
                        "voiceId": "Cherry"
                    }
                }
            }),
            HeaderMap::new(),
        )
        .expect("update_model_settings IPC");
        assert_eq!(updated["script"]["model"], "gpt-4o-mini");
        assert!(updated["script"]["apiKey"].is_null());
        assert_eq!(
            app.state::<AppState>().model_config.load().llm1.model,
            "gpt-4o-mini"
        );
        assert_eq!(
            app.state::<AppState>().model_config.load().llm1_base,
            "https://provider.example.test/v1"
        );

        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("绑定模型列表测试服务");
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            use std::io::{Read, Write};
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().expect("接收模型或声线请求");
                let mut buffer = [0_u8; 2048];
                let _ = stream.read(&mut buffer);
                let request = String::from_utf8_lossy(&buffer);
                let body = if request.contains("/audio/voices") {
                    r#"{"data":[{"id":"Cherry"},{"id":"Ethan"}]}"#
                } else {
                    r#"{"data":[{"id":"gpt-4o-mini"},{"id":"gpt-4.1"}]}"#
                };
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
                .unwrap();
            }
        });
        let models = invoke(
            &window,
            "list_models",
            json!({
                "setting": {
                    "provider": "custom",
                    "baseUrl": format!("http://{address}"),
                    "model": "",
                    "apiKey": "test-key",
                    "hasApiKey": true
                }
            }),
            HeaderMap::new(),
        )
        .expect("list_models IPC");
        assert_eq!(models, json!(["gpt-4.1", "gpt-4o-mini"]));

        let voices = invoke(
            &window,
            "list_voices",
            json!({
                "setting": {
                    "provider": "qwen",
                    "baseUrl": format!("http://{address}"),
                    "model": "qwen-tts",
                    "apiKey": "test-key",
                    "hasApiKey": true,
                    "voiceId": ""
                }
            }),
            HeaderMap::new(),
        )
        .expect("list_voices IPC");
        assert_eq!(voices, json!(["Cherry", "Ethan"]));
    }
}
