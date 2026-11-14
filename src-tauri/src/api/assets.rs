//! 素材上传、查询、下载，以及签名媒体地址（文档 5.5、5.8）。

use axum::body::Body;
use axum::extract::{Multipart, Path, Query, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use rusqlite::Connection;
use serde::Deserialize;
use serde_json::json;
use tower::ServiceExt;
use tower_http::services::ServeFile;

use super::{ok, page_limit, parse_cursor, reply, CurrentUser, Idempotency};
use crate::catalog::{AUDIO_LIMIT, IMAGE_LIMIT, LIBRARY_FILES, VIDEO_LIMIT};
use crate::db::{self, AssetRow};
use crate::error::{ApiError, ApiResult};
use crate::media::sniff;
use crate::models::Asset;
use crate::state::AppState;
use crate::util::{make_id, short};

/// 分页查询参数。
#[derive(Deserialize)]
pub struct ListQuery {
    /// 上一页返回的 `next_cursor`。
    cursor: Option<String>,
    /// 每页数量。
    limit: Option<i64>,
}

/// `GET /projects/{id}/assets`：素材面板列表。
pub async fn list(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(project_id): Path<String>,
    Query(query): Query<ListQuery>,
) -> ApiResult<Response> {
    let before = parse_cursor(query.cursor.as_deref())?;
    let limit = page_limit(query.limit, 50);
    let (rows, next_cursor) = state.db.read(|conn| {
        db::project_for(conn, &project_id, &user)?;
        // 素材面板展示用户上传的文件；生成的中间产物通过分镜与成片体现。
        db::list_assets(conn, &project_id, before, limit, true)
    })?;
    let items: Vec<Asset> = rows.iter().map(|r| state.media.asset_api(r)).collect();
    Ok(ok(json!({ "items": items, "next_cursor": next_cursor })))
}

/// 读取素材并校验它所在的项目属于当前用户；否则统一返回 404。
fn asset_for(conn: &Connection, asset_id: &str, user: &str) -> ApiResult<AssetRow> {
    let row = db::load_asset(conn, asset_id)?
        .ok_or_else(|| ApiError::not_found("文件不存在或已被删除。"))?;
    db::project_for(conn, &row.asset.project_id, user)
        .map_err(|_| ApiError::not_found("文件不存在或已被删除。"))?;
    Ok(row)
}

/// `GET /assets/{id}`：查询素材处理状态。
pub async fn get(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(asset_id): Path<String>,
) -> ApiResult<Response> {
    let row = state.db.read(|conn| asset_for(conn, &asset_id, &user))?;
    Ok(ok(json!({ "asset": state.media.asset_api(&row) })))
}

/// `POST /projects/{id}/assets`：上传一个文件（multipart 字段 `file`，幂等）。
///
/// 校验顺序：文件非空 → 按文件头识别真实类型（不信任扩展名与客户端 MIME）→
/// 大小上限 → 项目素材数上限 → 落盘 → ffprobe 确认可解码并解析宽高时长 → 写库。
pub async fn upload(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(project_id): Path<String>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> ApiResult<Response> {
    state
        .db
        .read(|conn| db::project_for(conn, &project_id, &user))?;

    let mut file: Option<(String, Vec<u8>)> = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| ApiError::bad_request("上传内容不完整，请重新选择文件。"))?
    {
        if field.name() == Some("file") {
            let name = field.file_name().unwrap_or("upload").to_string();
            let bytes = field.bytes().await.map_err(|_| {
                ApiError::new(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "FILE_TOO_LARGE",
                    "文件过大，请压缩后再上传。",
                )
            })?;
            file = Some((name, bytes.to_vec()));
            break;
        }
    }
    let (file_name, bytes) =
        file.ok_or_else(|| ApiError::validation("请选择要上传的文件（字段名 file）。"))?;
    if bytes.is_empty() {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "INVALID_FILE",
            "这个文件是空的，请重新选择。",
        ));
    }
    // 按文件头识别真实类型，改扩展名的伪装文件会被拒绝。
    let (mime, kind, ext) = sniff(&bytes).ok_or_else(|| {
        ApiError::new(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "UNSUPPORTED_FILE",
            "支持 JPG、PNG、WebP、MP4、WebM、MP3 和 WAV 文件。",
        )
    })?;
    let limit = match kind {
        "image" => IMAGE_LIMIT,
        "video" => VIDEO_LIMIT,
        _ => AUDIO_LIMIT,
    };
    if !limit.mime_types.contains(&mime) {
        return Err(ApiError::new(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "UNSUPPORTED_FILE",
            "暂不支持这种文件格式。",
        ));
    }
    if bytes.len() as i64 > limit.max_bytes {
        return Err(ApiError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "FILE_TOO_LARGE",
            format!(
                "文件超过 {}MB，请压缩后再上传。",
                limit.max_bytes / 1024 / 1024
            ),
        ));
    }

    // 幂等指纹：文件名 + 文件内容。
    // 幂等指纹：文件名 + 文件内容。
    let mut fingerprint = file_name.clone().into_bytes();
    fingerprint.extend_from_slice(&bytes);
    let idem = Idempotency::from_headers(
        &headers,
        &user,
        "POST",
        format!("/projects/{project_id}/assets"),
        &fingerprint,
    )?;
    if let Some(response) = state.db.read(|conn| idem.replay(conn))? {
        return Ok(response);
    }
    let uploaded = state.db.read(|conn| db::count_uploads(conn, &project_id))?;
    if uploaded >= state.catalog.max_uploaded_files {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "FILE_LIMIT",
            format!(
                "每个项目最多上传 {} 个素材。",
                state.catalog.max_uploaded_files
            ),
        ));
    }

    let asset_id = make_id("ast");
    let path = state
        .media
        .path_for(&project_id, &asset_id, &format!("x.{ext}"));
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(&path, &bytes).await?;
    // 先落盘再解析：损坏或无法解码的文件删除后返回 422。
    let probe = state.media.probe(&path).await;
    if !probe.ok {
        let _ = tokio::fs::remove_file(&path).await;
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "INVALID_FILE",
            "此媒体已损坏或无法解码，请选择其他文件。",
        ));
    }
    let row = AssetRow {
        asset: Asset {
            id: asset_id,
            project_id: project_id.clone(),
            kind: kind.into(),
            source: "upload".into(),
            status: "ready".into(),
            file_name,
            mime_type: mime.into(),
            size_bytes: bytes.len() as i64,
            duration_ms: if kind == "image" {
                None
            } else {
                probe.duration_ms
            },
            width: probe.width,
            height: probe.height,
            thumbnail_url: None,
            error: None,
        },
        storage_path: path.to_string_lossy().into_owned(),
    };
    let response = state.db.write(|t| {
        // 并发重发时另一请求已先完成：返回它的结果，并删除本次多写的文件。
        if let Some(response) = idem.replay(t)? {
            return Ok(Err(response));
        }
        db::project_for(t, &project_id, &user)?;
        db::insert_asset(t, &row)?;
        // 返回值里的缩略图地址是临时的；幂等重放时重新签发。
        let data = json!({ "asset": state.media.asset_api(&row) });
        idem.store(t, StatusCode::CREATED, &data)?;
        tracing::info!(
            target: "api",
            "项目 {} 上传素材 {}「{}」{} {:.1}MB{}{}",
            short(&project_id),
            short(&row.asset.id),
            row.asset.file_name,
            row.asset.mime_type,
            row.asset.size_bytes as f64 / 1024.0 / 1024.0,
            match (row.asset.width, row.asset.height) {
                (Some(w), Some(h)) => format!(" {w}×{h}"),
                _ => String::new(),
            },
            row.asset.duration_ms.map(|ms| format!(" {:.1}s", ms as f64 / 1000.0)).unwrap_or_default()
        );
        Ok(Ok(reply(StatusCode::CREATED, data)))
    })?;
    match response {
        Ok(response) => Ok(response),
        Err(replayed) => {
            let _ = tokio::fs::remove_file(&path).await;
            Ok(replayed)
        }
    }
}

/// 下载文件名：成片使用“标题-v版本号.mp4”，其他素材使用原文件名。
fn download_name(conn: &Connection, row: &AssetRow) -> ApiResult<String> {
    if row.asset.source == "rendered" {
        let (versions, _) = db::list_versions(conn, &row.asset.project_id, None, 100)?;
        for version in versions {
            let used = version.video_asset_id == row.asset.id
                || version.exports.iter().any(|e| e.asset_id == row.asset.id);
            if used {
                let title = version
                    .snapshot
                    .script
                    .as_ref()
                    .map(|s| s.title.clone())
                    .unwrap_or_else(|| "我的短片".into());
                let extension = if row.asset.mime_type == "application/zip" {
                    "zip"
                } else {
                    "mp4"
                };
                return Ok(format!("{title}-v{}.{extension}", version.version_number));
            }
        }
    }
    Ok(row.asset.file_name.clone())
}

/// `GET /assets/{id}/download`：签发短期下载地址。
pub async fn download(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(asset_id): Path<String>,
) -> ApiResult<Response> {
    let (row, file_name) = state.db.read(|conn| {
        let row = asset_for(conn, &asset_id, &user)?;
        let name = download_name(conn, &row)?;
        Ok((row, name))
    })?;
    let (url, expires_at) = state.media.signed_url(&row.asset.id, true);
    Ok(ok(json!({
        "asset_id": row.asset.id,
        "file_name": file_name,
        "mime_type": row.asset.mime_type,
        "size_bytes": row.asset.size_bytes,
        "download_url": url,
        "expires_at": expires_at,
    })))
}

/// 签名媒体地址的查询参数。
#[derive(Deserialize)]
pub struct MediaQuery {
    /// 过期时间（Unix 秒）。
    exp: Option<i64>,
    /// 签名。
    sig: Option<String>,
    /// 为 1 时以附件形式下载。
    dl: Option<u8>,
}

/// 生成下载用的 `Content-Disposition`。
///
/// 中文文件名按 RFC 5987 编码到 `filename*`，并提供 ASCII 的 `filename` 作为兜底。
fn content_disposition(name: &str) -> String {
    let encoded: String = name
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect();
    format!("attachment; filename=\"video.mp4\"; filename*=UTF-8''{encoded}")
}

/// 签名媒体地址：支持 Range，供播放器拖动进度；dl=1 时以附件下载。
pub async fn media(
    State(state): State<AppState>,
    Path(asset_id): Path<String>,
    Query(query): Query<MediaQuery>,
    request: Request,
) -> Result<Response, ApiError> {
    let download = query.dl == Some(1);
    let valid = match (query.exp, query.sig.as_deref()) {
        (Some(exp), Some(sig)) => state.media.verify(&asset_id, exp, download, sig),
        _ => false,
    };
    if !valid {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "URL_EXPIRED",
            "链接已过期，请刷新后重试。",
        ));
    }
    let (row, name) = state.db.read(|conn| {
        let row = db::load_asset(conn, &asset_id)?
            .ok_or_else(|| ApiError::not_found("文件不存在或已被删除。"))?;
        let name = download_name(conn, &row)?;
        Ok((row, name))
    })?;
    let mut response = ServeFile::new(&row.storage_path)
        .oneshot(request)
        .await
        .map_err(|_| ApiError::not_found("文件不存在或已被删除。"))?
        .map(Body::new);
    if response.status() == StatusCode::NOT_FOUND {
        return Err(ApiError::not_found("文件不存在或已被删除。"));
    }
    let headers = response.headers_mut();
    if let Ok(value) = HeaderValue::from_str(&row.asset.mime_type) {
        headers.insert(header::CONTENT_TYPE, value);
    }
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=300"),
    );
    if download {
        if let Ok(value) = HeaderValue::from_str(&content_disposition(&name)) {
            headers.insert(header::CONTENT_DISPOSITION, value);
        }
    }
    Ok(response)
}

/// 示例曲库试听（公开、只允许白名单文件）。
pub async fn library(
    State(state): State<AppState>,
    Path(file): Path<String>,
    request: Request,
) -> Response {
    if !LIBRARY_FILES.contains(&file.as_str()) {
        return ApiError::not_found("曲目不存在。").into_response();
    }
    let path = state.config.demo_media_dir.join(&file);
    match ServeFile::new(path).oneshot(request).await {
        Ok(response) => response.map(Body::new),
        Err(_) => ApiError::not_found("曲目不存在。").into_response(),
    }
}
