//! SSE：聊天文本与制作进度（文档第 6 节）。
//! 事件随状态变更一起落库，连接只负责按游标补发和推送，断线重连不会丢事件。

use std::collections::VecDeque;
use std::convert::Infallible;
use std::time::Duration;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use futures::stream::{self, Stream};
use serde::Deserialize;
use tokio::sync::broadcast::{error::RecvError, Receiver};

use super::CurrentUser;
use crate::db;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

/// 事件流查询参数。
#[derive(Deserialize)]
pub struct EventsQuery {
    /// 排他游标：只推送序号大于它的事件。
    after: Option<String>,
}

/// 一条 SSE 连接的推送状态。
struct Cursor {
    /// 全局状态。
    state: AppState,
    /// 订阅的项目。
    project_id: String,
    /// 已推送到的事件序号。
    after: i64,
    /// 项目有新事件的通知。
    rx: Receiver<String>,
    /// 已从数据库读出、尚未发送的事件。
    buffer: VecDeque<Event>,
}

/// `GET /projects/{id}/events?after=<cursor>`：SSE 事件流（文档第 6 节）。
///
/// - 必须携带游标；`after` 与 `Last-Event-ID` 同时存在且不一致时返回 400；
/// - 游标位于未来返回 400，早于已清理的位置返回 `410 EVENT_CURSOR_EXPIRED`；
/// - 每 15 秒发送 `:ping` 注释保持连接，并关闭代理缓冲。
pub async fn stream(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(project_id): Path<String>,
    Query(query): Query<EventsQuery>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    let last_event_id = headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let raw = match (query.after, last_event_id) {
        (Some(after), Some(last)) if after != last => {
            return Err(ApiError::bad_request("after 与 Last-Event-ID 不一致。"));
        }
        (Some(after), _) => after,
        (None, Some(last)) => last,
        (None, None) => {
            return Err(ApiError::bad_request(
                "请先获取项目快照，并携带 after 游标订阅事件。",
            ))
        }
    };
    let after: i64 = raw
        .parse()
        .ok()
        .filter(|v| *v >= 0)
        .ok_or_else(|| ApiError::bad_request("事件游标无效。"))?;
    // 先订阅再检查，避免检查与订阅之间漏掉通知。
    // 先订阅再读取项目状态，避免两者之间产生的事件通知被漏掉。
    let rx = state.db.subscribe();
    let project = state
        .db
        .read(|conn| db::project_for(conn, &project_id, &user))?;
    // 游标位于未来：不属于该项目或被篡改。
    if after > project.event_seq {
        return Err(ApiError::bad_request("事件游标无效。"));
    }
    // 游标之后的部分事件已被清理，无法补全：前端需要重新获取快照。
    if after < project.pruned_through {
        return Err(ApiError::new(
            axum::http::StatusCode::GONE,
            "EVENT_CURSOR_EXPIRED",
            "连接中断时间较长，正在重新同步。",
        ));
    }
    let log_id = crate::util::short(&project_id).to_string();
    let cursor = Cursor {
        state,
        project_id,
        after,
        rx,
        buffer: VecDeque::new(),
    };
    let sse = Sse::new(events(cursor)).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("ping"),
    );
    let mut response = sse.into_response();
    response
        .headers_mut()
        .insert("x-accel-buffering", HeaderValue::from_static("no"));
    response
        .headers_mut()
        .insert("cache-control", HeaderValue::from_static("no-cache"));
    tracing::debug!(target: "api", "项目 {} 建立事件流，游标 {after}", log_id);
    Ok(response)
}

/// 事件流本体：先补发游标之后的历史事件，再等待新事件。
///
/// 事件来自数据库而不是内存，因此断线重连、多实例部署都不会丢事件；
/// 广播通知只用来尽快唤醒，每 20 秒也会主动复查一次。项目被删除时结束连接。
fn events(cursor: Cursor) -> impl Stream<Item = Result<Event, Infallible>> {
    stream::unfold(cursor, |mut c| async move {
        loop {
            if let Some(event) = c.buffer.pop_front() {
                return Some((Ok(event), c));
            }
            let fetched = c.state.db.read(|conn| {
                if db::load_project(conn, &c.project_id)?.is_none() {
                    return Ok(None); // 项目已删除：结束连接
                }
                db::events_after(conn, &c.project_id, c.after, 100).map(Some)
            });
            match fetched {
                Ok(Some(rows)) if !rows.is_empty() => {
                    for row in rows {
                        c.after = row.seq;
                        c.buffer.push_back(
                            Event::default()
                                .id(row.seq.to_string())
                                .event(row.event)
                                .data(row.data),
                        );
                    }
                    continue;
                }
                Ok(Some(_)) => {}
                _ => return None,
            }
            // 等待本项目的新事件；定期醒来复查，防止通知丢失。
            loop {
                match tokio::time::timeout(Duration::from_secs(20), c.rx.recv()).await {
                    Ok(Ok(project)) if project == c.project_id => break,
                    Ok(Ok(_)) => continue,
                    Ok(Err(RecvError::Lagged(_))) | Err(_) => break,
                    Ok(Err(RecvError::Closed)) => return None,
                }
            }
        }
    })
}
