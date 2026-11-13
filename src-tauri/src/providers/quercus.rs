//! 基于 quercus-engine 的剪辑渲染器：逐条执行 llm1 输出的剪辑指令（[`EditOp`]），
//! 在 quercus 的非线性剪辑工程里搭出时间线，再导出 H.264/AAC 的 MP4 成片。
//!
//! - 需要启用 `quercus` feature，并在编译期提供 FFmpeg 开发包（`FFMPEG_DIR`）；
//! - quercus 的调用是同步阻塞的，全部放在 `spawn_blocking` 中执行，不占用异步线程；
//! - 引擎的撤销栈是进程级共享的，这里用一把全局锁串行执行剪辑，避免并发任务互相干扰；
//! - 指令中的错误（引用不存在的素材、时间区间非法）会转换为可读的 [`ProviderError`]，
//!   任务失败后用户可以重试。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use async_trait::async_trait;
use quercus_engine_rs::codec::{Codec, Format};
use quercus_engine_rs::edit::EditCommand;
use quercus_engine_rs::editor::{EditorSession, TrackKind};
use quercus_engine_rs::node::NodeId;
use quercus_engine_rs::render::{manager_init, manager_is_initialized, RenderBackend};
use quercus_engine_rs::task::{export, EncodingParams, TaskState};
use quercus_engine_rs::types::Rational;

use super::edit_ops::{ms_to_frames, ops_from_plan, EditOp, TrackType};
use super::*;

/// 串行化全部剪辑会话：quercus 的撤销栈与渲染管理器是进程级共享状态。
static ENGINE_LOCK: Mutex<()> = Mutex::new(());

/// FFmpeg 的立体声声道布局（`AV_CH_LAYOUT_STEREO` = 左 | 右）。
const STEREO_LAYOUT: u64 = 0x3;

/// quercus 渲染器。导出规格的转换仍复用原文件（目前只开放 720p）。
pub struct QuercusRenderer {
    /// 渲染输出的临时目录。
    work_dir: PathBuf,
}

impl QuercusRenderer {
    /// 创建渲染器，输出文件写入存储目录下的 `_render/`。
    pub fn new(config: &crate::config::Config) -> Self {
        Self {
            work_dir: config.storage_dir.join("_render"),
        }
    }
}

/// 把引擎错误统一转换为任务可用的失败原因（原始信息写日志，不直接给用户看）。
fn render_error(context: &str, detail: impl std::fmt::Display) -> ProviderError {
    tracing::warn!(target: "job", "quercus {context}失败：{detail}");
    ProviderError::render_failed(format!("剪辑合成失败（{context}），请稍后重试。"))
}

/// 把 quercus 0.7.1 解码不了的图片格式转成 PNG，返回实际交给引擎的路径。
///
/// quercus 的 mjpeg 解码在“单帧图片当视频读取”的路径上取不到帧（导出时报
/// `unexpected codec EOF`），PNG 走同一路径正常；WebP 一并转换以免同类问题。
fn engine_friendly(path: &Path, work_dir: &Path) -> ProviderResult<PathBuf> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    if !matches!(ext.as_str(), "jpg" | "jpeg" | "webp") {
        return Ok(path.to_path_buf());
    }
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("image");
    let target = work_dir.join(format!("{stem}.png"));
    if !target.is_file() {
        std::fs::create_dir_all(work_dir).map_err(|e| render_error("准备素材", e))?;
        image::open(path)
            .map_err(|e| render_error("读取图片", e))?
            .save(&target)
            .map_err(|e| render_error("转换图片", e))?;
    }
    Ok(target)
}

/// 在 quercus 中执行一组剪辑指令并导出成片，返回输出文件路径与时长（毫秒）。
///
/// 这是阻塞函数，调用方需放在 `spawn_blocking` 中。
fn execute_ops(
    ops: &[EditOp],
    inputs: &HashMap<String, PathBuf>,
    output: &Path,
    total_ms: i64,
) -> ProviderResult<()> {
    let _guard = ENGINE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // 多线程渲染后端：不依赖外部 worker 进程；只需初始化一次。
    if !manager_is_initialized() && !manager_init(RenderBackend::Threads) {
        return Err(render_error("初始化渲染管理器", "manager_init 返回 false"));
    }
    let session =
        EditorSession::new().map_err(|e| render_error("创建剪辑工程", format!("{e:?}")))?;

    let mut sequence: Option<(NodeId, i32, i32, i32)> = None;
    let mut media: HashMap<String, NodeId> = HashMap::new();
    let mut clips: HashMap<String, NodeId> = HashMap::new();

    for (index, op) in ops.iter().enumerate() {
        let step = format!("第 {} 条指令", index + 1);
        match op {
            EditOp::CreateSequence { width, height, fps } => {
                let result = session
                    .execute(EditCommand::CreateSequence {
                        name: "ExplainCraft".into(),
                        width: Some(*width),
                        height: Some(*height),
                        rate: Some(Rational::new(*fps as i64, 1)),
                        interlaced: Some(false),
                    })
                    .map_err(|e| render_error(&step, e))?;
                let id = result
                    .primary
                    .ok_or_else(|| render_error(&step, "未返回序列"))?;
                session.set_current_sequence(id);
                sequence = Some((id, *width, *height, *fps));
            }
            EditOp::ImportMedia { alias, asset_id } => {
                let path = inputs
                    .get(asset_id)
                    .ok_or_else(|| render_error(&step, format!("素材 {asset_id} 不存在")))?;
                let path = engine_friendly(path, &output.with_extension("inputs"))?;
                let footage = session
                    .project()
                    .import_footage(&path.to_string_lossy())
                    .ok_or_else(|| render_error(&step, format!("无法导入 {}", path.display())))?;
                media.insert(alias.clone(), footage);
            }
            EditOp::PlaceClip {
                media: alias,
                track,
                track_index,
                start_ms,
                end_ms,
                media_in_ms,
                clip,
            } => {
                let (_, _, _, fps) =
                    sequence.ok_or_else(|| render_error(&step, "缺少 create_sequence"))?;
                let footage = *media
                    .get(alias)
                    .ok_or_else(|| render_error(&step, format!("未导入素材 {alias}")))?;
                if end_ms <= start_ms {
                    return Err(render_error(&step, "片段结束时间早于开始时间"));
                }
                let result = session
                    .execute(EditCommand::PlaceFootageClip {
                        footage,
                        kind: match track {
                            TrackType::Video => TrackKind::Video,
                            TrackType::Audio => TrackKind::Audio,
                        },
                        track_index: *track_index,
                        in_ts: ms_to_frames(*start_ms, fps),
                        out_ts: ms_to_frames(*end_ms, fps),
                        media_in_ts: ms_to_frames(*media_in_ms, fps),
                    })
                    .map_err(|e| render_error(&step, e))?;
                if let (Some(name), Some(id)) = (clip, result.primary) {
                    clips.insert(name.clone(), id);
                }
            }
            EditOp::AddCrossfade {
                previous,
                next,
                half_ms,
            } => {
                let (Some(previous), Some(next)) = (clips.get(previous), clips.get(next)) else {
                    // 转场是锦上添花：引用不到片段时跳过，不让整次剪辑失败。
                    tracing::warn!(target: "job", "quercus 跳过转场：片段 {previous}/{next} 不存在");
                    continue;
                };
                if let Err(error) = session.execute(EditCommand::AddTransitionAtSeam {
                    previous: *previous,
                    next: *next,
                    half: Rational::new(*half_ms, 1000),
                }) {
                    tracing::warn!(target: "job", "quercus 跳过转场：{error}");
                }
            }
        }
    }

    let (sequence, width, height, fps) =
        sequence.ok_or_else(|| render_error("导出", "指令中没有 create_sequence"))?;
    let params = EncodingParams {
        format: Format::MPEG4Video as i32,
        video_enabled: true,
        video_codec: Codec::H264 as i32,
        video_width: width,
        video_height: height,
        // 注意是“每帧时长”：30fps 对应 1/30。
        video_time_base_num: 1,
        video_time_base_den: fps,
        audio_enabled: true,
        audio_codec: Codec::AAC as i32,
        audio_sample_rate: 48_000,
        audio_channel_layout: STEREO_LAYOUT,
        export_length_num: total_ms as i32,
        export_length_den: 1000,
        ..EncodingParams::default()
    };
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent).map_err(|e| render_error("创建输出目录", e))?;
    }
    let task = export(
        &session.project(),
        sequence,
        &output.to_string_lossy(),
        &params,
    )
    .ok_or_else(|| render_error("导出", "无法创建导出任务"))?;
    // 阻塞等待编码完成（已在 spawn_blocking 中）。
    let ok = task.wait();
    if !ok || task.state() != TaskState::Succeeded {
        return Err(render_error("导出", task.error()));
    }
    if !output.is_file() {
        return Err(render_error("导出", "没有生成输出文件"));
    }
    Ok(())
}

#[async_trait]
impl Renderer for QuercusRenderer {
    async fn render(&self, request: RenderRequest) -> ProviderResult<MediaOutput> {
        let plan = request.plan;
        // 剪辑指令由 llm1 输出；为空时（例如旧检查点）按时间线补出标准指令。
        let ops = if plan.ops.is_empty() {
            ops_from_plan(&plan)
        } else {
            plan.ops.clone()
        };
        let output = self
            .work_dir
            .join(format!("{}.mp4", crate::util::make_id("render")));
        let (width, height) = crate::util::dimensions(&plan.aspect_ratio);
        let total_ms = plan.total_ms.max(1000);
        let inputs = request.inputs;
        let target = output.clone();
        let ops_count = ops.len();
        tracing::info!(target: "job", "  · quercus 开始剪辑：{ops_count} 条指令，成片 {}×{}，{:.1}s", width, height, total_ms as f64 / 1000.0);
        let result =
            tokio::task::spawn_blocking(move || execute_ops(&ops, &inputs, &target, total_ms))
                .await
                .map_err(|e| render_error("剪辑线程", e));
        // 清理为引擎转换格式的临时图片（成功或失败都清理）。
        let _ = tokio::fs::remove_dir_all(output.with_extension("inputs")).await;
        result??;
        Ok(MediaOutput {
            data: MediaData::Path(output),
            file_name: "render.mp4".into(),
            mime_type: "video/mp4".into(),
            kind: "video".into(),
            source: "rendered".into(),
            width: Some(width),
            height: Some(height),
            duration_ms: Some(total_ms),
        })
    }

    async fn export(&self, request: ExportRequest) -> ProviderResult<MediaOutput> {
        if request.resolution != "720p" || request.format != "mp4" {
            return Err(ProviderError::invalid("目前只支持 720p MP4 导出。"));
        }
        // 预览成片本身就是 720p MP4，直接复用。
        let (width, height) = crate::util::dimensions(&request.aspect_ratio);
        Ok(MediaOutput {
            data: MediaData::Path(request.source),
            file_name: "export.mp4".into(),
            mime_type: "video/mp4".into(),
            kind: "video".into(),
            source: "rendered".into(),
            width: Some(width),
            height: Some(height),
            duration_ms: None,
        })
    }
}
