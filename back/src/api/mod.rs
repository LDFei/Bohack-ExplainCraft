//! HTTP 接口（docs/API.md 第 4～6 节），基础路径 /api/v1。

mod assets;
mod events;
mod jobs;
mod projects;

use axum::extract::{DefaultBodyLimit, FromRequestParts};
use axum::http::request::Parts;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use rusqlite::{params, Connection, OptionalExtension};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::config::AuthMode;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use crate::util::{elapsed, iso, now, now_iso, request_id};

/// HTTP 访问日志中间件：打印方法、路径、状态码与耗时。
///
/// - 写操作（POST / PATCH / DELETE）和所有出错的请求打印为 info / warn / error；
/// - 成功的 GET 打印为 debug：前端每收到一个事件都会重新拉取快照，逐条打印会淹没流程日志；
/// - 只记录路径，不记录查询串：媒体地址的签名、游标等不进入日志；
/// - SSE 连接在建立时就返回 200，耗时只代表建立连接所用的时间。
pub async fn access_log(request: axum::extract::Request, next: axum::middleware::Next) -> Response {
    let method = request.method().clone();
    let path = request.uri().path().to_string();
    let started = std::time::Instant::now();
    let response = next.run(request).await;
    let status = response.status();
    if path == "/health" {
        return response;
    }
    let took = elapsed(started);
    if status.is_server_error() {
        tracing::error!(target: "http", "{method} {path} → {} ({took})", status.as_u16());
    } else if status.is_client_error() {
        tracing::warn!(target: "http", "{method} {path} → {} ({took})", status.as_u16());
    } else if method == axum::http::Method::GET {
        tracing::debug!(target: "http", "{method} {path} → {} ({took})", status.as_u16());
    } else {
        tracing::info!(target: "http", "{method} {path} → {} ({took})", status.as_u16());
    }
    response
}

/// 构建路由。业务接口挂在 `PUBLIC_API_PREFIX`（默认 `/api/v1`）下，`/health` 用于健康检查。
///
/// 上传接口的请求体最大约 110MB，覆盖 100MB 的视频上限和 multipart 开销。
pub fn router(state: AppState) -> Router {
    let api = Router::new()
        .route("/capabilities", get(projects::capabilities))
        .route("/projects", get(projects::list).post(projects::create))
        .route(
            "/projects/{project_id}",
            get(projects::snapshot)
                .patch(projects::rename)
                .delete(projects::remove),
        )
        .route(
            "/projects/{project_id}/messages",
            get(projects::messages).post(projects::send_message),
        )
        .route(
            "/projects/{project_id}/draft",
            axum::routing::patch(projects::patch_draft),
        )
        .route(
            "/projects/{project_id}/assets",
            get(assets::list).post(assets::upload),
        )
        .route("/projects/{project_id}/events", get(events::stream))
        .route("/projects/{project_id}/versions", get(jobs::versions))
        .route(
            "/projects/{project_id}/versions/{version_id}",
            get(jobs::version),
        )
        .route(
            "/projects/{project_id}/versions/{version_id}/exports",
            post(jobs::export),
        )
        .route("/assets/{asset_id}", get(assets::get))
        .route("/assets/{asset_id}/download", get(assets::download))
        .route("/jobs/{job_id}", get(jobs::get))
        .route("/jobs/{job_id}/answers", post(jobs::answer))
        .route("/jobs/{job_id}/cancel", post(jobs::cancel))
        .route("/jobs/{job_id}/retry", post(jobs::retry))
        // 以下两个地址供 <video>/<img>/<audio> 直接加载：媒体靠签名鉴权，示例曲库公开。
        .route("/media/{asset_id}", get(assets::media))
        .route("/library/{file}", get(assets::library))
        .layer(DefaultBodyLimit::max(110 * 1024 * 1024));
    let prefix = state.config.public_prefix.clone();
    Router::new()
        .route("/health", get(|| async { "ok" }))
        .nest(&prefix, api)
        .fallback(|| async { ApiError::not_found("接口不存在。") })
        .with_state(state)
}

// ---------- 当前用户 ----------

/// 当前用户。登录模块接入前：dev 模式下无令牌视为 local_user，
/// 有令牌则按令牌区分用户（便于验证跨用户隔离）；strict 模式必须携带令牌。
pub struct CurrentUser(pub String);

impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = parts
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(str::trim)
            .filter(|t| !t.is_empty());
        match (token, state.config.auth_mode) {
            (Some(token), _) => {
                let digest = Sha256::digest(token.as_bytes());
                Ok(CurrentUser(format!("usr_{}", hex::encode(&digest[..8]))))
            }
            (None, AuthMode::Dev) => Ok(CurrentUser("local_user".into())),
            (None, AuthMode::Strict) => Err(ApiError::unauthorized()),
        }
    }
}

// ---------- 响应与请求工具 ----------

/// 成功响应：`{"data": ..., "request_id": "..."}`。
pub fn reply(status: StatusCode, data: Value) -> Response {
    (
        status,
        Json(json!({ "data": data, "request_id": request_id() })),
    )
        .into_response()
}

/// 200 成功响应。
pub fn ok(data: Value) -> Response {
    reply(StatusCode::OK, data)
}

/// 解析 JSON 请求体：语法错误 400，结构或字段不合法 422。
pub fn parse_body<T: DeserializeOwned>(body: &[u8]) -> ApiResult<T> {
    let body = if body.iter().all(u8::is_ascii_whitespace) {
        b"{}".as_slice()
    } else {
        body
    };
    serde_json::from_slice(body).map_err(|error| {
        if error.is_syntax() || error.is_eof() {
            ApiError::bad_request("请求内容不是有效的 JSON。")
        } else {
            ApiError::validation("请求字段不完整或格式不正确。")
                .details(json!({ "fields": [{ "path": "body", "reason": error.to_string() }] }))
        }
    })
}

/// 分页大小：未传时用默认值，限制在 1～100。
pub fn page_limit(limit: Option<i64>, default: i64) -> i64 {
    limit.unwrap_or(default).clamp(1, 100)
}

/// 解析数字游标；为空表示第一页。
pub fn parse_cursor(cursor: Option<&str>) -> ApiResult<Option<i64>> {
    cursor
        .filter(|c| !c.is_empty())
        .map(|c| {
            c.parse::<i64>()
                .map_err(|_| ApiError::bad_request("分页游标无效。"))
        })
        .transpose()
}

// ---------- 幂等 ----------

/// 一次需要幂等的请求（文档 2.1）。
///
/// 以 `(用户, 方法, 路径, Idempotency-Key)` 为键、请求体哈希为指纹：
/// 同一键 24 小时内只执行一次，重发返回原响应；内容不同返回 `409 IDEMPOTENCY_CONFLICT`。
pub struct Idempotency {
    /// 当前用户。
    user_id: String,
    /// HTTP 方法。
    method: &'static str,
    /// 请求路径（不含前缀）。
    path: String,
    /// 前端生成的 `Idempotency-Key`。
    key: String,
    /// 请求内容的 SHA-256。
    fingerprint: String,
}

impl Idempotency {
    /// 从请求头读取 `Idempotency-Key` 并计算指纹；缺少请求头返回 400。
    pub fn from_headers(
        headers: &HeaderMap,
        user_id: &str,
        method: &'static str,
        path: String,
        body: &[u8],
    ) -> ApiResult<Self> {
        let key = headers
            .get("idempotency-key")
            .and_then(|v| v.to_str().ok())
            .map(str::trim)
            .filter(|k| !k.is_empty() && k.len() <= 200)
            .ok_or_else(|| ApiError::bad_request("缺少 Idempotency-Key 请求头。"))?;
        let mut hasher = Sha256::new();
        hasher.update(body);
        Ok(Self {
            user_id: user_id.into(),
            method,
            path,
            key: key.into(),
            fingerprint: hex::encode(hasher.finalize()),
        })
    }

    /// 已受理过：内容相同返回原响应；内容不同返回 409。
    pub fn replay(&self, conn: &Connection) -> ApiResult<Option<Response>> {
        let cutoff = iso(now() - chrono::Duration::hours(24));
        let found: Option<(String, u16, String)> = conn
            .query_row(
                "SELECT fingerprint, status_code, response FROM idempotency
                 WHERE user_id = ?1 AND method = ?2 AND path = ?3 AND key = ?4 AND created_at > ?5",
                params![self.user_id, self.method, self.path, self.key, cutoff],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        match found {
            None => Ok(None),
            Some((fingerprint, _, _)) if fingerprint != self.fingerprint => {
                Err(ApiError::conflict(
                    "IDEMPOTENCY_CONFLICT",
                    "相同的请求标识被用于不同内容，请为新的请求使用新的标识。",
                ))
            }
            Some((_, status, response)) => {
                let data: Value = serde_json::from_str(&response)?;
                Ok(Some(reply(
                    StatusCode::from_u16(status).unwrap_or(StatusCode::OK),
                    data,
                )))
            }
        }
    }

    /// 记录本次受理的响应。应与业务写入在同一事务中调用，保证“受理”与“记录”同时生效。
    pub fn store(&self, conn: &Connection, status: StatusCode, data: &Value) -> ApiResult<()> {
        conn.execute(
            "INSERT OR REPLACE INTO idempotency (user_id, method, path, key, fingerprint, status_code,
                response, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                self.user_id,
                self.method,
                self.path,
                self.key,
                self.fingerprint,
                status.as_u16(),
                data.to_string(),
                now_iso()
            ],
        )?;
        Ok(())
    }
}
