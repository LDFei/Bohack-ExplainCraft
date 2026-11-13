//! 统一错误类型。
//!
//! 对外输出文档第 7 节约定的 `error` 对象（`code / message / retryable / details`）。
//! 数据库、IO 等内部错误统一转换为 `INTERNAL_ERROR`，详细原因只写入日志，不返回给前端。

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};

use crate::util::request_id;

/// 接口错误。处理函数返回它即可自动转换为带正确 HTTP 状态码的 JSON 响应。
#[derive(Debug)]
pub struct ApiError {
    /// HTTP 状态码。
    pub status: StatusCode,
    /// 业务错误码，如 `PROJECT_BUSY`、`REVISION_CONFLICT`。
    pub code: &'static str,
    /// 面向用户的中文说明，可直接展示。
    pub message: String,
    /// 前端是否可以原样重试。
    pub retryable: bool,
    /// 附加信息，如冲突时的当前版本号、校验失败的字段路径。
    pub details: Option<Value>,
}

/// 接口层与数据层通用的结果类型。
pub type ApiResult<T> = Result<T, ApiError>;

impl ApiError {
    /// 创建错误，默认不可重试、无附加信息。
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
            retryable: false,
            details: None,
        }
    }

    /// 附加 `details` 信息。
    pub fn details(mut self, details: Value) -> Self {
        self.details = Some(details);
        self
    }

    /// 标记为可重试。
    pub fn retryable(mut self) -> Self {
        self.retryable = true;
        self
    }

    /// 400：请求结构错误（JSON 语法、缺少请求头、游标无效等）。
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "BAD_REQUEST", message)
    }

    /// 422：参数或字段内容不合法。
    pub fn validation(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "VALIDATION_ERROR",
            message,
        )
    }

    /// 404：资源不存在。跨用户访问也使用 404，避免泄露资源是否存在。
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, "NOT_FOUND", message)
    }

    /// 409：状态或版本冲突，错误码由调用方指定。
    pub fn conflict(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, code, message)
    }

    /// 401：未登录（仅 strict 鉴权模式下出现）。
    pub fn unauthorized() -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            "UNAUTHORIZED",
            "请先登录后再继续。",
        )
    }

    /// 500：内部错误。原因写入日志，对外只返回通用说明。
    pub fn internal(message: impl std::fmt::Display) -> Self {
        tracing::error!("internal error: {message}");
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "INTERNAL_ERROR",
            "服务暂时出了点问题，请稍后再试。",
        )
        .retryable()
    }

    /// 工作线程发现自己的执行尝试已被取消或取代（fencing token 不匹配）。
    ///
    /// 只在后端内部流转，用来中止写入，不会返回给前端。
    pub fn fenced() -> Self {
        Self::new(StatusCode::CONFLICT, "FENCED", "任务执行尝试已失效")
    }

    /// 是否为 [`ApiError::fenced`] 产生的错误。
    pub fn is_fenced(&self) -> bool {
        self.code == "FENCED"
    }

    /// 409 `PROJECT_BUSY`：项目已有活动任务，附带当前任务 ID。
    pub fn project_busy(active_job_id: Option<&str>) -> Self {
        Self::conflict("PROJECT_BUSY", "当前视频正在制作，请完成或停止后再修改。")
            .details(json!({ "active_job_id": active_job_id }))
    }
}

/// 数据库错误视为内部错误。
impl From<rusqlite::Error> for ApiError {
    fn from(error: rusqlite::Error) -> Self {
        Self::internal(format!("sqlite: {error}"))
    }
}

/// JSON 列序列化失败视为内部错误（请求体解析错误另由 `parse_body` 处理）。
impl From<serde_json::Error> for ApiError {
    fn from(error: serde_json::Error) -> Self {
        Self::internal(format!("json: {error}"))
    }
}

/// 文件读写错误视为内部错误。
impl From<std::io::Error> for ApiError {
    fn from(error: std::io::Error) -> Self {
        Self::internal(format!("io: {error}"))
    }
}

/// 其他内部错误。
impl From<anyhow::Error> for ApiError {
    fn from(error: anyhow::Error) -> Self {
        Self::internal(error)
    }
}

/// 转换为统一的失败响应：`{"error": {...}, "request_id": "..."}`。
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = json!({
            "error": {
                "code": self.code,
                "message": self.message,
                "retryable": self.retryable,
                "details": self.details,
            },
            "request_id": request_id(),
        });
        // 业务错误码打印到终端，配合访问日志的状态码定位问题（500 的原因已在 internal() 中记录）。
        if self.status.is_client_error() {
            tracing::warn!(target: "api", "请求被拒绝 {}：{}", self.code, self.message);
        }
        (self.status, Json(body)).into_response()
    }
}
