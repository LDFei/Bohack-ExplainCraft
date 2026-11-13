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

/// 导出指定版本为 MP4。
#[tauri::command]
pub async fn export_video(
    state: State<'_, AppState>,
    request: IpcRequest<'_>,
    project_id: String,
    version_id: String,
    resolution: String,
) -> Result<CommandReply, String> {
    json_command(
        &state,
        Method::POST,
        format!("/api/v1/projects/{project_id}/versions/{version_id}/exports"),
        &request,
        json!({ "format": "mp4", "resolution": resolution, "fps": 30 }),
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
        config.demo_media_dir =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../front/public/media");
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
        assert_eq!(reply.body["data"]["demo_mode"], true);
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
}
