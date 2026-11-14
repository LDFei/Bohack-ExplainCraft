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
use std::time::{Duration, Instant};

use async_trait::async_trait;
use quercus_engine_rs::codec::{Codec, Format};
use quercus_engine_rs::edit::EditCommand;
use quercus_engine_rs::editor::{EditorSession, TrackKind};
use quercus_engine_rs::node::NodeId;
use quercus_engine_rs::render::{
    gpu_init, gpu_kind, manager_init, manager_is_initialized, GpuBackend, RenderBackend,
};
use quercus_engine_rs::task::{export, save_otio, EncodingParams, Task, TaskState};
use quercus_engine_rs::types::Rational;

use super::edit_ops::{ms_to_frames, ops_from_plan, srt_from_plan, EditOp, TrackType};
use super::*;

/// 串行化全部剪辑会话：quercus 的撤销栈与渲染管理器是进程级共享状态。
static ENGINE_LOCK: Mutex<()> = Mutex::new(());

/// FFmpeg 的立体声声道布局（`AV_CH_LAYOUT_STEREO` = 左 | 右）。
const STEREO_LAYOUT: u64 = 0x3;

/// quercus 渲染器。预览与导出都通过引擎重新编码（目前只开放 720p）。
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

/// 已搭建的 Quercus 工程，既可直接渲染，也可保存为人工剪辑工程。
struct BuiltSession {
    session: EditorSession,
    sequence: NodeId,
    width: i32,
    height: i32,
    fps: i32,
}

/// 轮询引擎任务并定期输出进度；超时主动取消，避免导出永久卡在 `task.wait()`。
fn wait_for_task(task: &Task, context: &str, timeout: Duration) -> ProviderResult<()> {
    let started = Instant::now();
    let mut last_log = Instant::now() - Duration::from_secs(5);
    loop {
        match task.state() {
            TaskState::Succeeded => {
                tracing::info!(target: "job", "  · quercus {context}完成，用时 {:.1}s", started.elapsed().as_secs_f64());
                return Ok(());
            }
            TaskState::Failed | TaskState::Cancelled => {
                return Err(render_error(context, task.error()));
            }
            TaskState::Running => {}
        }
        if started.elapsed() >= timeout {
            task.cancel();
            return Err(render_error(
                context,
                format!("等待超过 {} 秒，已请求引擎取消", timeout.as_secs()),
            ));
        }
        if last_log.elapsed() >= Duration::from_secs(5) {
            tracing::info!(target: "job", "  · quercus {context}中：{:.0}%", task.progress() * 100.0);
            last_log = Instant::now();
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// 执行剪辑指令搭建画面、声音和透明字幕轨。调用方必须持有 [`ENGINE_LOCK`]。
fn build_session(
    ops: &[EditOp],
    inputs: &HashMap<String, PathBuf>,
    work_dir: &Path,
    subtitles: Option<&str>,
) -> ProviderResult<BuiltSession> {
    // 在首帧前显式创建共享 GPU 上下文，避免时间线合成停留在 CPU/读回路径。
    let gpu_available = gpu_init(GpuBackend::Auto);
    tracing::info!(target: "job", "  · quercus GPU 合成初始化：available={gpu_available}，backend={:?}", gpu_kind());
    // 多线程渲染后端：不依赖外部 worker 进程；只需初始化一次。
    if !manager_is_initialized() && !manager_init(RenderBackend::Threads) {
        return Err(render_error("初始化渲染管理器", "manager_init 返回 false"));
    }
    std::fs::create_dir_all(work_dir).map_err(|e| render_error("准备工作目录", e))?;
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
                tracing::info!(target: "job", "  · quercus 导入素材 {alias}（{asset_id}）");
                let path = inputs
                    .get(asset_id)
                    .ok_or_else(|| render_error(&step, format!("素材 {asset_id} 不存在")))?;
                let path = engine_friendly(path, &work_dir.join("inputs"))?;
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
                tracing::info!(target: "job", "  · quercus 放置 {alias} → {:?} 轨 {track_index}，{}～{}ms", track, start_ms, end_ms);
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
    if let Some(srt) = subtitles {
        let cues = parse_srt(srt)?;
        for (index, cue) in cues.iter().enumerate() {
            let overlay = work_dir.join("subtitles").join(format!("{index:03}.png"));
            crate::svg_animation::write_subtitle_overlay(
                &cue.text,
                width as u32,
                height as u32,
                &overlay,
            )?;
            let footage = session
                .project()
                .import_footage(&overlay.to_string_lossy())
                .ok_or_else(|| {
                    render_error("导入透明字幕", format!("无法导入 {}", overlay.display()))
                })?;
            session
                .execute(EditCommand::PlaceFootageClip {
                    footage,
                    kind: TrackKind::Video,
                    track_index: 1,
                    in_ts: ms_to_frames(cue.start_ms, fps),
                    out_ts: ms_to_frames(cue.end_ms, fps),
                    media_in_ts: 0,
                })
                .map_err(|error| render_error("放置透明字幕", error))?;
            tracing::info!(target: "job", "  · quercus 字幕 {} 放到视频轨 1：{}～{}ms", index + 1, cue.start_ms, cue.end_ms);
        }
    }
    Ok(BuiltSession {
        session,
        sequence,
        width,
        height,
        fps,
    })
}

/// 渲染已搭建的工程。引擎任务是后台线程，这里轮询状态并限制最长等待时间。
fn export_built_session(built: &BuiltSession, output: &Path, total_ms: i64) -> ProviderResult<()> {
    let params = EncodingParams {
        format: Format::MPEG4Video as i32,
        video_enabled: true,
        video_codec: Codec::H264 as i32,
        video_width: built.width,
        video_height: built.height,
        // 注意是“每帧时长”：30fps 对应 1/30。
        video_time_base_num: 1,
        video_time_base_den: built.fps,
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
    tracing::info!(target: "job", "  · quercus 编码任务开始 → {}", output.display());
    let task = export(
        &built.session.project(),
        built.sequence,
        &output.to_string_lossy(),
        &params,
    )
    .ok_or_else(|| render_error("导出", "无法创建导出任务"))?;
    // 编码耗时取决于素材数量和机器性能，与成片时长非线性相关：40 秒多镜头在未开硬件编码
    // 的机器上实测超过 3 分钟，因此下限取 600 秒，避免正常成片被判超时取消。
    let timeout = Duration::from_secs((total_ms.max(1000) as u64 / 250).clamp(600, 1800));
    wait_for_task(&task, "视频编码", timeout)?;
    if !output.is_file() {
        return Err(render_error("导出", "没有生成输出文件"));
    }
    Ok(())
}

/// 一条待烧录字幕的时间区间与文本。
struct SubtitleCue {
    start_ms: i64,
    end_ms: i64,
    text: String,
}

/// 解析由 `srt_from_plan` 生成的 SubRip，避免引入另一套字幕时间来源。
fn parse_srt(srt: &str) -> ProviderResult<Vec<SubtitleCue>> {
    fn timestamp(text: &str) -> Option<i64> {
        let (clock, millis) = text.trim().split_once(',')?;
        let mut parts = clock.split(':');
        let hours = parts.next()?.parse::<i64>().ok()?;
        let minutes = parts.next()?.parse::<i64>().ok()?;
        let seconds = parts.next()?.parse::<i64>().ok()?;
        let millis = millis.parse::<i64>().ok()?;
        Some(((hours * 60 + minutes) * 60 + seconds) * 1000 + millis)
    }

    let normalized = srt.replace("\r\n", "\n").replace('\r', "\n");
    let mut cues = Vec::new();
    for block in normalized
        .split("\n\n")
        .filter(|block| !block.trim().is_empty())
    {
        let lines: Vec<&str> = block.lines().collect();
        let timing = lines
            .get(1)
            .ok_or_else(|| ProviderError::invalid("字幕时间轴不完整。"))?;
        let (start, end) = timing
            .split_once(" --> ")
            .ok_or_else(|| ProviderError::invalid("字幕时间轴格式错误。"))?;
        let start_ms =
            timestamp(start).ok_or_else(|| ProviderError::invalid("字幕开始时间错误。"))?;
        let end_ms = timestamp(end).ok_or_else(|| ProviderError::invalid("字幕结束时间错误。"))?;
        let text = lines.get(2..).unwrap_or_default().join("\n");
        if end_ms > start_ms && !text.trim().is_empty() {
            cues.push(SubtitleCue {
                start_ms,
                end_ms,
                text,
            });
        }
    }
    Ok(cues)
}

/// 在 quercus 中执行一组剪辑指令并导出成片。
fn execute_ops(
    ops: &[EditOp],
    inputs: &HashMap<String, PathBuf>,
    output: &Path,
    total_ms: i64,
    subtitles: Option<&str>,
) -> ProviderResult<()> {
    let started = Instant::now();
    tracing::info!(target: "job", "  · quercus 等待剪辑引擎锁");
    let _guard = ENGINE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    tracing::info!(target: "job", "  · quercus 已获得剪辑引擎锁，开始搭建时间线");
    let work_dir = output.with_extension("work");
    let built = build_session(ops, inputs, &work_dir, subtitles)?;
    tracing::info!(target: "job", "  · quercus 时间线搭建完成，用时 {:.1}s；开始单次成片编码", started.elapsed().as_secs_f64());
    export_built_session(&built, output, total_ms)?;
    tracing::info!(target: "job", "  · quercus 成片完成，总耗时 {:.1}s（无字幕二次编码）", started.elapsed().as_secs_f64());
    let _ = std::fs::remove_dir_all(work_dir);
    Ok(())
}

/// 清理归档文件名，避免素材原名带来路径分隔符或控制字符。
fn safe_file_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|ch| {
            if ch.is_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "asset.bin".into()
    } else {
        cleaned
    }
}

/// 保存一种剪辑交换格式，并确认引擎确实写出了文件。
fn save_interchange(built: &BuiltSession, path: &Path, label: &str) -> ProviderResult<()> {
    let filename = path
        .to_str()
        .ok_or_else(|| render_error(label, "路径不是有效 UTF-8"))?;
    tracing::info!(target: "job", "  · quercus 开始保存 {label}：{filename}");
    let task = save_otio(&built.session.project(), filename)
        .ok_or_else(|| render_error(label, "无法创建交换格式导出任务"))?;
    wait_for_task(&task, label, Duration::from_secs(120))?;
    if !path.is_file() {
        return Err(render_error(label, "没有生成文件"));
    }
    Ok(())
}

/// 把工程内的临时绝对路径改为归档根目录相对路径，解压后仍可找到素材。
fn make_project_portable(path: &Path, staging: &Path) -> ProviderResult<()> {
    let text = std::fs::read_to_string(path).map_err(|e| render_error("读取工程交换文件", e))?;
    let root = staging.to_string_lossy();
    let portable = text
        .replace(root.as_ref(), ".")
        .replace(&root.replace('\\', "/"), ".");
    std::fs::write(path, portable).map_err(|e| render_error("写入可移植工程", e))
}

/// 递归把目录写入 ZIP，媒体直接存储，文本与工程文件使用 Deflate 压缩。
fn zip_directory(staging: &Path, output: &Path) -> ProviderResult<()> {
    use zip::write::SimpleFileOptions;

    fn collect(root: &Path, directory: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
        let mut entries = std::fs::read_dir(directory)?.collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            if path.is_dir() {
                collect(root, &path, files)?;
            } else if path.is_file() {
                files.push(path.strip_prefix(root).unwrap_or(&path).to_path_buf());
            }
        }
        Ok(())
    }

    let mut files = Vec::new();
    collect(staging, staging, &mut files).map_err(|e| render_error("读取工程包文件", e))?;
    let file = std::fs::File::create(output).map_err(|e| render_error("创建 ZIP", e))?;
    let mut zip = zip::ZipWriter::new(file);
    for relative in files {
        let name = relative.to_string_lossy().replace('\\', "/");
        let extension = relative
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let compressed = matches!(
            extension.as_str(),
            "ove" | "otio" | "fcpxml" | "srt" | "json" | "txt"
        );
        let options = SimpleFileOptions::default().compression_method(if compressed {
            zip::CompressionMethod::Deflated
        } else {
            zip::CompressionMethod::Stored
        });
        zip.start_file(&name, options)
            .map_err(|e| render_error("写入 ZIP 条目", e))?;
        let mut source = std::fs::File::open(staging.join(&relative))
            .map_err(|e| render_error("读取 ZIP 素材", e))?;
        std::io::copy(&mut source, &mut zip).map_err(|e| render_error("压缩工程包", e))?;
        tracing::info!(target: "job", "  · 工程包已加入 {name}");
    }
    zip.finish().map_err(|e| render_error("完成 ZIP", e))?;
    Ok(())
}

/// 重建带相对素材路径的工程，保存 OVE/OTIO/FCPXML 并打包当前版本全部素材。
fn export_project_bundle(
    request: ProjectBundleRequest,
    work_dir: &Path,
) -> ProviderResult<PathBuf> {
    let bundle_id = crate::util::make_id("bundle");
    let staging = work_dir.join(&bundle_id);
    let assets_dir = staging.join("assets");
    std::fs::create_dir_all(&assets_dir).map_err(|e| render_error("创建工程包目录", e))?;
    let output = work_dir.join(format!("{bundle_id}.zip"));
    std::fs::copy(&request.source, staging.join("video.mp4"))
        .map_err(|e| render_error("复制成片到工程包", e))?;

    let mut staged_inputs = HashMap::new();
    let mut manifest_assets = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for asset in request.assets {
        if !seen.insert(asset.id.clone()) {
            continue;
        }
        let archived = format!("{}-{}", asset.id, safe_file_name(&asset.file_name));
        let target = assets_dir.join(&archived);
        std::fs::copy(&asset.path, &target)
            .map_err(|e| render_error("复制工程素材", format!("{}：{e}", asset.path.display())))?;
        staged_inputs.insert(asset.id.clone(), target);
        manifest_assets.push(serde_json::json!({
            "asset_id": asset.id,
            "file": format!("assets/{archived}"),
            "original_name": asset.file_name,
        }));
    }
    let ops = if request.plan.ops.is_empty() {
        ops_from_plan(&request.plan)
    } else {
        request.plan.ops.clone()
    };
    let subtitles = srt_from_plan(&request.plan);
    if let Some(srt) = &subtitles {
        std::fs::write(staging.join("subtitles.srt"), srt)
            .map_err(|e| render_error("写入工程包字幕", e))?;
    }
    tracing::info!(target: "job", "  · quercus 等待剪辑引擎锁以保存工程包");
    let _guard = ENGINE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let built = build_session(&ops, &staged_inputs, &staging, subtitles.as_deref())?;
    let ove = staging.join("project.ove");
    let ove_name = ove
        .to_str()
        .ok_or_else(|| render_error("保存 OVE", "路径不是有效 UTF-8"))?;
    tracing::info!(target: "job", "  · quercus 开始保存 OVE：{ove_name}");
    built
        .session
        .save(ove_name)
        .map_err(|e| render_error("保存 OVE", format!("{e:?}")))?;
    crate::oak::sidecar::write(ove_name, &built.session.snapshot())
        .map_err(|e| render_error("保存 OVE 格式信息", e))?;
    let otio = staging.join("project.otio");
    let fcpxml = staging.join("project.fcpxml");
    save_interchange(&built, &otio, "保存 OTIO")?;
    save_interchange(&built, &fcpxml, "保存 FCPXML")?;
    drop(built);
    drop(_guard);

    for path in [&ove, &otio, &fcpxml] {
        make_project_portable(path, &staging)?;
    }
    std::fs::write(
        staging.join("manifest.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "title": request.title,
            "version": request.version_number,
            "video": "video.mp4",
            "project_files": ["project.ove", "project.otio", "project.fcpxml"],
            "subtitles": subtitles.is_some().then_some("subtitles.srt"),
            "timeline": request.plan,
            "assets": manifest_assets,
        }))
        .map_err(|e| render_error("生成素材清单", e))?,
    )
    .map_err(|e| render_error("写入素材清单", e))?;
    std::fs::write(
        staging.join("README.txt"),
        "灵镜工程包\n\nvideo.mp4：当前版本成片。\nassets/：时间线素材和用户上传素材。\ninputs/：为剪辑引擎转换的图片（如有）。\nsubtitles.srt：独立字幕（启用字幕时）。\nproject.ove：Quercus 原生工程。\nproject.otio：OpenTimelineIO 交换工程。\nproject.fcpxml：Final Cut Pro XML 交换工程。\nmanifest.json：素材 ID、归档路径和完整时间线。\n\n工程使用相对素材路径，请完整解压后再打开，不要只移动工程文件。\n",
    )
    .map_err(|e| render_error("写入工程包说明", e))?;
    tracing::info!(target: "job", "  · 开始压缩工程包 → {}", output.display());
    zip_directory(&staging, &output)?;
    std::fs::remove_dir_all(&staging).map_err(|e| render_error("清理工程包临时目录", e))?;
    tracing::info!(target: "job", "  · 工程包导出完成：{}", output.display());
    Ok(output)
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
        let subtitles = srt_from_plan(&plan);
        let output = self
            .work_dir
            .join(format!("{}.mp4", crate::util::make_id("render")));
        let (width, height) = crate::util::dimensions(&plan.aspect_ratio);
        let total_ms = plan.total_ms.max(1000);
        let inputs = request.inputs;
        let target = output.clone();
        let ops_count = ops.len();
        tracing::info!(target: "job", "  · quercus 开始剪辑：{ops_count} 条指令，成片 {}×{}，{:.1}s", width, height, total_ms as f64 / 1000.0);
        let result = tokio::task::spawn_blocking(move || {
            execute_ops(&ops, &inputs, &target, total_ms, subtitles.as_deref())
        })
        .await
        .map_err(|e| render_error("剪辑线程", e));
        // 清理为引擎转换格式的临时图片（成功或失败都清理）。
        let _ = tokio::fs::remove_dir_all(output.with_extension("inputs")).await;
        let _ = tokio::fs::remove_dir_all(output.with_extension("work")).await;
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
        let (width, height) = crate::util::dimensions(&request.aspect_ratio);
        let total_ms = request.duration_ms.max(1000);
        let output = self
            .work_dir
            .join(format!("{}.mp4", crate::util::make_id("export")));
        // 当前唯一开放的规格与预览规格相同，直接复制原始成片，避免二次编码和
        // Quercus 再次解码已有 MP4；这也不会降低画质或弄丢已经混好的多段音轨。
        tracing::info!(target: "job", "  · quercus 导出规格与预览一致，直接复制成片，不重复编码");
        if let Some(parent) = output.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| render_error("创建导出目录", e))?;
        }
        tokio::fs::copy(&request.source, &output)
            .await
            .map_err(|e| render_error("写出导出文件", e))?;
        tracing::info!(target: "job", "  · quercus 导出文件已就绪：{}", output.display());
        Ok(MediaOutput {
            data: MediaData::Path(output),
            file_name: "export.mp4".into(),
            mime_type: "video/mp4".into(),
            kind: "video".into(),
            source: "rendered".into(),
            width: Some(width),
            height: Some(height),
            duration_ms: Some(total_ms),
        })
    }

    async fn export_bundle(&self, request: ProjectBundleRequest) -> ProviderResult<MediaOutput> {
        let (width, height) = crate::util::dimensions(&request.plan.aspect_ratio);
        let duration_ms = request.plan.total_ms.max(1000);
        let work_dir = self.work_dir.clone();
        let output = tokio::task::spawn_blocking(move || export_project_bundle(request, &work_dir))
            .await
            .map_err(|e| render_error("工程包导出线程", e))??;
        Ok(MediaOutput {
            data: MediaData::Path(output),
            file_name: "project-bundle.zip".into(),
            mime_type: "application/zip".into(),
            kind: "file".into(),
            source: "rendered".into(),
            width: Some(width),
            height: Some(height),
            duration_ms: Some(duration_ms),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 首帧前应能建立非 CPU 的共享合成后端；无 GPU 的构建环境允许回退。
    #[test]
    fn initializes_gpu_compositing_backend() {
        let available = gpu_init(GpuBackend::Auto);
        println!("GPU available={available}, backend={:?}", gpu_kind());
        if available {
            assert_ne!(gpu_kind(), Some(GpuBackend::Cpu));
        }
    }

    /// 生成一秒双声道正弦波，便于逐段确认每一路旁白都进入最终混音。
    fn write_tone(path: &Path, frequency: f64) {
        let sample_rate = 48_000_u32;
        let samples = sample_rate as usize * 2;
        let mut bytes = Vec::with_capacity(44 + samples * 2);
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&((36 + samples * 2) as u32).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&2_u16.to_le_bytes());
        bytes.extend_from_slice(&sample_rate.to_le_bytes());
        bytes.extend_from_slice(&(sample_rate * 4).to_le_bytes());
        bytes.extend_from_slice(&4_u16.to_le_bytes());
        bytes.extend_from_slice(&16_u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&(samples as u32 * 2).to_le_bytes());
        for index in 0..sample_rate as usize {
            let value = (0.65
                * (std::f64::consts::TAU * frequency * index as f64 / sample_rate as f64).sin())
                as f32;
            let pcm = (value * i16::MAX as f32) as i16;
            bytes.extend_from_slice(&pcm.to_le_bytes());
            bytes.extend_from_slice(&pcm.to_le_bytes());
        }
        std::fs::write(path, bytes).expect("测试音频");
    }

    /// 用 FFmpeg 测量成片指定区间的平均音量；无声区间约为 -91 dB。
    fn mean_volume(path: &Path, start: f64) -> f64 {
        let output = std::process::Command::new(
            std::env::var("FFMPEG_PATH").unwrap_or_else(|_| "ffmpeg".into()),
        )
        .args(["-hide_banner", "-i"])
        .arg(path)
        .args([
            "-ss",
            &start.to_string(),
            "-t",
            "0.6",
            "-map",
            "0:a:0",
            "-af",
            "volumedetect",
            "-f",
            "null",
            "-",
        ])
        .output()
        .expect("运行 FFmpeg 音量检测");
        let log = String::from_utf8_lossy(&output.stderr);
        log.split("mean_volume:")
            .nth(1)
            .and_then(|tail| tail.trim_start().split_whitespace().next())
            .and_then(|value| value.parse().ok())
            .unwrap_or_else(|| panic!("没有检测到音量：{log}"))
    }

    /// 读取成片中间帧的平均 RGB 亮度，防止字幕层把整幅画面覆盖成黑色。
    fn frame_brightness(path: &Path, root: &Path, start: f64) -> f64 {
        let frame = root.join("frame.png");
        let status = std::process::Command::new(
            std::env::var("FFMPEG_PATH").unwrap_or_else(|_| "ffmpeg".into()),
        )
        .args(["-v", "error", "-y", "-ss", &start.to_string(), "-i"])
        .arg(path)
        .args(["-frames:v", "1"])
        .arg(&frame)
        .status()
        .expect("提取成片帧");
        assert!(status.success());
        let image = image::open(frame).expect("读取成片帧").to_rgb8();
        image
            .pixels()
            .flat_map(|pixel| pixel.0)
            .map(|channel| channel as f64)
            .sum::<f64>()
            / (image.width() * image.height() * 3) as f64
    }

    /// SVG 镜头可直接进入 Quercus 时间线并输出 MP4。
    #[tokio::test]
    async fn renders_an_svg_shot_through_quercus() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><rect width="64" height="64" fill="#eeeeee"/><rect x="4" y="4" width="16" height="16" fill="#337799"><animate attributeName="x" values="4;40;4" dur="1s"/></rect></svg>"##;
        let shot = crate::svg_animation::render(svg, 1000, 64, 64, 0)
            .await
            .expect("SVG 镜头");
        let MediaData::Path(path) = shot.data else {
            panic!("镜头路径")
        };
        let output =
            std::env::temp_dir().join(format!("quercus-test-{}.mp4", uuid::Uuid::new_v4()));
        let ops = vec![
            EditOp::CreateSequence {
                width: 64,
                height: 64,
                fps: 30,
            },
            EditOp::ImportMedia {
                alias: "shot".into(),
                asset_id: "shot".into(),
            },
            EditOp::PlaceClip {
                media: "shot".into(),
                track: TrackType::Video,
                track_index: 0,
                start_ms: 0,
                end_ms: 1000,
                media_in_ms: 0,
                clip: Some("shot".into()),
            },
        ];
        let inputs = HashMap::from([("shot".into(), path.clone())]);
        execute_ops(&ops, &inputs, &output, 1000, None).expect("Quercus 输出");
        assert!(output.is_file());
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(output);
    }

    /// 三段旁白都进入混音，三段字幕正常显示且不会把画面覆盖成黑色。
    #[tokio::test]
    async fn mixes_every_narration_and_overlays_subtitles_without_black_frames() {
        let root = std::env::temp_dir().join(format!("quercus-multi-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("测试目录");
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 320 568"><rect width="320" height="568" fill="#eeeeee"/></svg>"##;
        let shot = crate::svg_animation::render(svg, 3000, 320, 568, 0)
            .await
            .expect("测试画面");
        let MediaData::Path(video) = shot.data else {
            panic!("镜头路径")
        };
        let mut inputs = HashMap::from([("visual".into(), video.clone())]);
        let mut ops = vec![
            EditOp::CreateSequence {
                width: 320,
                height: 568,
                fps: 30,
            },
            EditOp::ImportMedia {
                alias: "visual".into(),
                asset_id: "visual".into(),
            },
            EditOp::PlaceClip {
                media: "visual".into(),
                track: TrackType::Video,
                track_index: 0,
                start_ms: 0,
                end_ms: 3000,
                media_in_ms: 0,
                clip: Some("shot".into()),
            },
        ];
        for (index, frequency) in [220.0, 440.0, 880.0].into_iter().enumerate() {
            let id = format!("voice{index}");
            let wav = root.join(format!("{id}.wav"));
            let path = root.join(format!("{id}.mp3"));
            write_tone(&wav, frequency);
            let status = std::process::Command::new(
                std::env::var("FFMPEG_PATH").unwrap_or_else(|_| "ffmpeg".into()),
            )
            .args(["-v", "error", "-y", "-i"])
            .arg(&wav)
            .args(["-codec:a", "libmp3lame", "-q:a", "5"])
            .arg(&path)
            .status()
            .expect("生成 MP3 旁白");
            assert!(status.success());
            let _ = std::fs::remove_file(wav);
            inputs.insert(id.clone(), path);
            ops.push(EditOp::ImportMedia {
                alias: id.clone(),
                asset_id: id.clone(),
            });
            ops.push(EditOp::PlaceClip {
                media: id.clone(),
                track: TrackType::Audio,
                track_index: 0,
                start_ms: index as i64 * 1000,
                end_ms: (index as i64 + 1) * 1000,
                media_in_ms: 0,
                clip: Some(format!("voice_clip{index}")),
            });
        }
        let srt = "1\n00:00:00,000 --> 00:00:01,000\n第一段字幕\n\n2\n00:00:01,000 --> 00:00:02,000\n第二段字幕\n\n3\n00:00:02,000 --> 00:00:03,000\n第三段字幕\n\n";
        let output = root.join("mixed.mp4");
        execute_ops(&ops, &inputs, &output, 3000, Some(srt)).expect("多轨字幕成片");
        for start in [0.5, 1.5, 2.5] {
            let brightness = frame_brightness(&output, &root, start);
            assert!(
                (120.0..235.0).contains(&brightness),
                "{start}s 字幕黑屏或未实际叠加，平均亮度为 {brightness}"
            );
        }
        for start in [0.2, 1.2, 2.2] {
            let volume = mean_volume(&output, start);
            assert!(
                volume > -45.0,
                "{start}s 的旁白没有混入，平均音量 {volume} dB"
            );
        }
        let _ = std::fs::remove_file(video);
        let _ = std::fs::remove_dir_all(root);
    }

    /// 工程包包含可移植工程、全部素材和独立字幕，便于人工接管。
    #[tokio::test]
    async fn exports_portable_project_bundle() {
        use std::io::Read;

        let root = std::env::temp_dir().join(format!("quercus-bundle-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("测试目录");
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><rect width="64" height="64" fill="#345678"/></svg>"##;
        let shot = crate::svg_animation::render(svg, 1000, 64, 64, 0)
            .await
            .expect("测试画面");
        let MediaData::Path(video) = shot.data else {
            panic!("镜头路径")
        };
        let source = root.join("source.mp4");
        let visual_ops = vec![
            EditOp::CreateSequence {
                width: 64,
                height: 64,
                fps: 30,
            },
            EditOp::ImportMedia {
                alias: "visual".into(),
                asset_id: "visual".into(),
            },
            EditOp::PlaceClip {
                media: "visual".into(),
                track: TrackType::Video,
                track_index: 0,
                start_ms: 0,
                end_ms: 1000,
                media_in_ms: 0,
                clip: Some("shot0".into()),
            },
        ];
        execute_ops(
            &visual_ops,
            &HashMap::from([("visual".into(), video.clone())]),
            &source,
            1000,
            None,
        )
        .expect("原始成片");
        let voice = root.join("voice.wav");
        write_tone(&voice, 330.0);
        let upload = root.join("notes.txt");
        std::fs::write(&upload, "人工剪辑备注").expect("上传素材");
        let plan = EditPlan {
            aspect_ratio: "16:9".into(),
            total_ms: 1000,
            clips: vec![Clip {
                shot_id: "shot0".into(),
                start_ms: 0,
                duration_ms: 1000,
                visual_asset_id: Some("visual".into()),
                narration_asset_id: Some("voice".into()),
                subtitle: Some("工程包字幕".into()),
                ..Default::default()
            }],
            subtitles_enabled: true,
            ops: visual_ops,
            ..Default::default()
        };
        let archive = export_project_bundle(
            ProjectBundleRequest {
                source: source.clone(),
                plan,
                assets: vec![
                    BundleAsset {
                        id: "visual".into(),
                        file_name: "shot.mp4".into(),
                        path: video.clone(),
                    },
                    BundleAsset {
                        id: "voice".into(),
                        file_name: "voice.wav".into(),
                        path: voice,
                    },
                    BundleAsset {
                        id: "upload".into(),
                        file_name: "notes.txt".into(),
                        path: upload,
                    },
                ],
                title: "测试工程".into(),
                version_number: 1,
            },
            &root,
        )
        .expect("工程包");
        let file = std::fs::File::open(&archive).expect("打开 ZIP");
        let mut zip = zip::ZipArchive::new(file).expect("ZIP 结构");
        let names: Vec<String> = (0..zip.len())
            .map(|index| zip.by_index(index).expect("ZIP 条目").name().to_string())
            .collect();
        for required in [
            "video.mp4",
            "project.ove",
            "project.otio",
            "project.fcpxml",
            "subtitles.srt",
            "manifest.json",
            "README.txt",
        ] {
            assert!(names.iter().any(|name| name == required), "缺少 {required}");
        }
        assert!(names.iter().any(|name| name.starts_with("assets/visual-")));
        assert!(names.iter().any(|name| name.starts_with("assets/voice-")));
        assert!(names.iter().any(|name| name.starts_with("assets/upload-")));
        for project in ["project.ove", "project.otio", "project.fcpxml"] {
            let mut text = String::new();
            zip.by_name(project)
                .expect("工程条目")
                .read_to_string(&mut text)
                .expect("工程文本");
            assert!(!text.trim().is_empty());
            assert!(
                !text.contains(root.to_string_lossy().as_ref()),
                "{project} 仍引用临时绝对路径"
            );
        }
        let _ = std::fs::remove_file(video);
        let _ = std::fs::remove_dir_all(root);
    }
}
