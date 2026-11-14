//! 默认成片输出：将已生成的 SVG/HTML 镜头文件按确认的顺序拼接，不调用剪辑模型或 Quercus。
//! 字幕由 resvg 绘制为透明图层，兼容没有 libass/drawtext 的 FFmpeg 安装。

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use async_trait::async_trait;

use super::*;

/// 原生输出器；只消费已确认的镜头、音频和字幕，不执行模型生成的剪辑指令。
pub struct DirectRenderer {
    /// 输出和临时目录均放在应用媒体目录内，方便后续原子移动入库。
    work_dir: PathBuf,
    /// 输出规格一致时复用源文件，避免不必要的有损重编码。
    ffprobe: String,
}

/// 未完成的媒体文件随 future 取消一并清理。
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl DirectRenderer {
    /// 创建默认输出器，不初始化任何 Quercus 编辑会话。
    pub fn new(config: &crate::config::Config) -> Self {
        Self {
            work_dir: config.storage_dir.join("_render"),
            ffprobe: config.ffprobe.clone(),
        }
    }
}

/// 内部错误只写日志，界面收到稳定中文说明。
fn failed(label: &str, detail: impl std::fmt::Display) -> ProviderError {
    tracing::warn!(target: "job", "直接输出 {label}失败：{detail}");
    ProviderError::render_failed(format!("视频输出失败（{label}），请检查 FFmpeg 后重试。"))
}

/// 使用参数数组调用 FFmpeg；用户文本和文件名不会进入 shell。
fn ffmpeg() -> Command {
    let mut command =
        Command::new(std::env::var("FFMPEG_PATH").unwrap_or_else(|_| "ffmpeg".into()));
    command.args(["-hide_banner", "-loglevel", "error", "-nostdin", "-y"]);
    command
}

/// 子进程有时间上限，取消任务时立即杀掉正在编码的子进程。
async fn run(command: Command, label: &str) -> ProviderResult<()> {
    let mut command = tokio::process::Command::from(command);
    let result = tokio::time::timeout(
        Duration::from_secs(600),
        command.kill_on_drop(true).output(),
    )
    .await
    .map_err(|_| failed(label, "输出超过十分钟"))?
    .map_err(|error| failed(label, error))?;
    if !result.status.success() {
        return Err(failed(label, String::from_utf8_lossy(&result.stderr)));
    }
    Ok(())
}

/// 优先硬件编码，真正失败才用软件兜底；每次重试都重新构造命令。
async fn encode(
    mut build: impl FnMut() -> Command,
    output: &Path,
    label: &str,
) -> ProviderResult<()> {
    let mut last = None;
    for encoder in super::encoding::candidates() {
        let mut command = build();
        encoder.apply(&mut command);
        command
            .args(["-pix_fmt", "yuv420p", "-r", "30", "-movflags", "+faststart"])
            .arg(output);
        tracing::info!(target: "job", "{label}：{}", encoder.name());
        match run(command, label).await {
            Ok(()) => return Ok(()),
            Err(error) => last = Some(error),
        }
    }
    Err(last.unwrap_or_else(|| failed(label, "没有可用编码器")))
}

/// 秒数用于 FFmpeg 的时间参数，保留毫秒精度。
fn seconds(ms: i64) -> String {
    format!("{:.3}", ms.max(1) as f64 / 1000.0)
}

/// 转义字幕 XML，文字不能注入新的 SVG 节点或外部引用。
fn xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// 生成透明字幕图层，原画面保持透明；不依赖 FFmpeg 的可选字体过滤器。
fn subtitle_png(text: &str, width: u32, height: u32, path: &Path) -> ProviderResult<()> {
    let font_size = (width.min(height) as f64 * 0.046).round().max(18.0);
    let line_chars = ((width as f64 * 0.86) / font_size).floor().max(8.0) as usize;
    let chars: Vec<char> = text.chars().collect();
    let lines: Vec<String> = chars
        .chunks(line_chars)
        .map(|chunk| chunk.iter().collect())
        .collect();
    let line_height = font_size * 1.45;
    let box_height = line_height * lines.len().max(1) as f64 + font_size * 0.65;
    if box_height > height as f64 * 0.45 {
        return Err(ProviderError::invalid(
            "单个镜头字幕过长，请缩短字幕或增加镜头后重试。",
        ));
    }
    let top = height as f64 - box_height - font_size * 0.7;
    let labels = lines.iter().enumerate().map(|(index, line)| {
        format!("<text x=\"{}\" y=\"{}\" text-anchor=\"middle\" font-family=\"sans-serif\" font-size=\"{font_size}\" fill=\"white\">{}</text>",
            width / 2, top + font_size * 1.05 + index as f64 * line_height, xml(line))
    }).collect::<String>();
    let svg = format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\"><rect x=\"{}\" y=\"{top}\" width=\"{}\" height=\"{box_height}\" rx=\"8\" fill=\"black\" opacity=\"0.68\"/>{labels}</svg>", width as f64 * 0.04, width as f64 * 0.92);
    let options = crate::svg_animation::svg_options();
    let tree = usvg::Tree::from_str(&svg, &options).map_err(|error| failed("绘制字幕", error))?;
    let mut pixmap =
        tiny_skia::Pixmap::new(width, height).ok_or_else(|| failed("字幕画布", "尺寸无效"))?;
    resvg::render(
        &tree,
        tiny_skia::Transform::identity(),
        &mut pixmap.as_mut(),
    );
    pixmap
        .save_png(path)
        .map_err(|error| failed("保存字幕", error))
}

/// 查找已确认的素材，只接受真实存在的本地文件。
fn input<'a>(request: &'a RenderRequest, id: &str) -> ProviderResult<&'a Path> {
    request
        .inputs
        .get(id)
        .filter(|path| path.is_file())
        .map(PathBuf::as_path)
        .ok_or_else(|| ProviderError::invalid("镜头或音频素材缺失，请重新生成该步骤。"))
}

/// 成片输出的统一元信息。
fn video_output(
    path: PathBuf,
    aspect_ratio: &str,
    duration_ms: i64,
    resolution: &str,
) -> MediaOutput {
    let (width, height) = crate::util::output_dimensions(aspect_ratio, resolution);
    MediaOutput {
        data: MediaData::Path(path),
        file_name: "render.mp4".into(),
        mime_type: "video/mp4".into(),
        kind: "video".into(),
        source: "rendered".into(),
        width: Some(width),
        height: Some(height),
        duration_ms: Some(duration_ms),
    }
}

#[async_trait]
impl Renderer for DirectRenderer {
    async fn render(&self, request: RenderRequest) -> ProviderResult<MediaOutput> {
        if request.plan.clips.is_empty() || request.plan.total_ms <= 0 {
            return Err(ProviderError::invalid(
                "还没有可输出的镜头，请先完成画面制作。",
            ));
        }
        let id = crate::util::make_id("direct");
        let directory = self.work_dir.join(&id);
        tokio::fs::create_dir_all(&directory)
            .await
            .map_err(|error| failed("创建输出目录", error))?;
        let _scratch = Scratch(directory.clone());
        let (width, height) = crate::util::output_dimensions(
            &request.plan.aspect_ratio,
            &request.plan.output_resolution,
        );
        let mut files = Vec::new();
        let mut end = 0;
        for (index, clip) in request.plan.clips.iter().enumerate() {
            if clip.start_ms != end || clip.duration_ms <= 0 {
                return Err(ProviderError::invalid(
                    "基础输出需要连续的正时长镜头，请重新确认分镜。",
                ));
            }
            end += clip.duration_ms;
            let visual_id = clip
                .visual_asset_id
                .as_deref()
                .ok_or_else(|| ProviderError::invalid("镜头还没有画面，请先生成画面。"))?;
            let source = input(&request, visual_id)?;
            let still = matches!(
                source
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .unwrap_or_default()
                    .to_ascii_lowercase()
                    .as_str(),
                "png" | "jpg" | "jpeg" | "webp"
            );
            let reference =
                request.ctx.settings.render_profile == "vibe" && clip.camera_motion == "reference";
            let caption = if request.plan.subtitles_enabled || reference {
                clip.subtitle
                    .as_deref()
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
            } else {
                None
            };
            let mut overlays: Vec<(PathBuf, Option<(f64, f64)>)> = vec![];
            if reference {
                let overlay = directory.join(format!("reference-{index}.png"));
                let svg = crate::vibe::reference_svg(
                    caption.unwrap_or("参考素材"),
                    width as u32,
                    height as u32,
                );
                let tree = usvg::Tree::from_str(&svg, &crate::svg_animation::svg_options())
                    .map_err(|e| failed("参考素材框架", e))?;
                let mut pixmap = tiny_skia::Pixmap::new(width as u32, height as u32)
                    .ok_or_else(|| failed("参考素材框架", "尺寸无效"))?;
                resvg::render(
                    &tree,
                    tiny_skia::Transform::identity(),
                    &mut pixmap.as_mut(),
                );
                pixmap
                    .save_png(&overlay)
                    .map_err(|e| failed("参考素材框架", e))?;
                overlays.push((overlay, None));
            }
            if let Some(caption) = caption {
                let font_size = (height as f32 * 0.034).clamp(24., 44.);
                let columns = (width as f32 * 0.84 / font_size).floor().max(8.) as usize;
                let chars = caption.chars().collect::<Vec<_>>();
                let pages = chars.chunks(columns * 2).collect::<Vec<_>>();
                for (page, chars) in pages.iter().enumerate() {
                    let path = directory.join(format!("caption-{index}-{page}.png"));
                    let value = chars.iter().collect::<String>();
                    subtitle_png(&value, width as u32, height as u32, &path)?;
                    overlays.push((
                        path,
                        Some((
                            clip.duration_ms as f64 / 1000. * page as f64 / pages.len() as f64,
                            clip.duration_ms as f64 / 1000. * (page + 1) as f64
                                / pages.len() as f64,
                        )),
                    ));
                }
            }
            let target = directory.join(format!("clip-{index}.mp4"));
            let duration = seconds(clip.duration_ms);
            tracing::info!(target: "job", "输出镜头 {}/{}，时长 {} 秒", index + 1, request.plan.clips.len(), duration);
            crate::metrics::production_log(format!(
                "正在编码镜头 {}/{}，{} 秒",
                index + 1,
                request.plan.clips.len(),
                duration
            ));
            encode(|| {
                let mut command = ffmpeg();
                if still { command.args(["-loop", "1", "-framerate", "30"]); }
                else { command.args(["-stream_loop", "-1"]); }
                if !still && clip.source_in_ms>0 {command.args(["-ss",&seconds(clip.source_in_ms)]);}
                command.arg("-i").arg(source);
                for (path,_) in &overlays {command.args(["-loop","1","-i"]).arg(path);}
                let fit = if reference {format!("scale={}:{}:force_original_aspect_ratio=decrease,pad={width}:{height}:(ow-iw)/2:oh*0.32+(oh*0.44-ih)/2:color=0x0B1120,setsar=1,fps=30",width*84/100,height*44/100)}else{format!("scale={width}:{height}:force_original_aspect_ratio=decrease,pad={width}:{height}:(ow-iw)/2:(oh-ih)/2:color=0x0B1120,setsar=1,fps=30")};
                if !overlays.is_empty(){
                    let mut graph=format!("[0:v]{fit}[base0]");
                    for(i,(_,range))in overlays.iter().enumerate(){let enable=range.map(|(a,b)|format!(":enable='gte(t,{a:.4})*lt(t,{b:.4})'")).unwrap_or_default();graph.push_str(&format!(";[base{i}][{}:v]overlay=0:0:format=auto{enable}[base{}]",i+1,i+1));}
                    command.args(["-filter_complex",&graph,"-map",&format!("[base{}]",overlays.len())]);
                }else{command.args(["-vf",&fit,"-map","0:v:0"]);}
                command.args(["-an", "-t", &duration]);
                command
            }, &target, "镜头编码").await?;
            files.push(target);
        }
        if (end - request.plan.total_ms).abs() > 2 {
            return Err(ProviderError::invalid(
                "分镜总时长与输出时长不一致，请重新确认时长。",
            ));
        }
        // concat 文件只引用本次生成的数字命名文件，不包含用户提供的路径或文本。
        let concat = directory.join("clips.txt");
        let listing = (0..files.len())
            .map(|index| format!("file 'clip-{index}.mp4'\n"))
            .collect::<String>();
        tokio::fs::write(&concat, listing)
            .await
            .map_err(|error| failed("保存镜头清单", error))?;
        let mut command = ffmpeg();
        command
            .args(["-f", "concat", "-safe", "1", "-i"])
            .arg(&concat);
        command.args(["-f", "lavfi", "-i", "anullsrc=r=48000:cl=stereo"]);
        let total = seconds(request.plan.total_ms);
        let mut filters = vec![format!(
            "[1:a]atrim=duration={total},asetpts=PTS-STARTPTS[base]"
        )];
        let mut mix = String::from("[base]");
        let mut audio_index = 2;
        for clip in &request.plan.clips {
            if let Some(id) = clip.narration_asset_id.as_deref() {
                command.arg("-i").arg(input(&request, id)?);
                filters.push(format!("[{audio_index}:a]atrim=duration={},asetpts=PTS-STARTPTS,adelay={}:all=1,volume={}[a{audio_index}]", seconds(clip.duration_ms), clip.start_ms, request.plan.narration_volume.clamp(0.0, 1.0)));
                mix.push_str(&format!("[a{audio_index}]"));
                audio_index += 1;
            }
        }
        if let Some(id) = request.plan.music_asset_id.as_deref() {
            command
                .args(["-stream_loop", "-1", "-i"])
                .arg(input(&request, id)?);
            filters.push(format!("[{audio_index}:a]atrim=duration={total},asetpts=PTS-STARTPTS,volume={}[a{audio_index}]", request.plan.music_volume.clamp(0.0, 1.0)));
            mix.push_str(&format!("[a{audio_index}]"));
            audio_index += 1;
        }
        filters.push(format!("{mix}amix=inputs={}:duration=first:dropout_transition=0:normalize=0,alimiter=limit=0.95[mixed]", audio_index - 1));
        let output = directory.join("render.mp4");
        command
            .args([
                "-filter_complex",
                &filters.join(";"),
                "-map",
                "0:v:0",
                "-map",
                "[mixed]",
                "-c:v",
                "copy",
                "-c:a",
                "aac",
                "-ar",
                "48000",
                "-ac",
                "2",
                "-t",
                &total,
                "-movflags",
                "+faststart",
            ])
            .arg(&output);
        tracing::info!(target: "job", "合成全部旁白、背景音乐和字幕，不调用剪辑库");
        run(command, "音画合成").await?;
        let final_path = self.work_dir.join(format!("{id}.mp4"));
        tokio::fs::rename(output, &final_path)
            .await
            .map_err(|error| failed("保存成片", error))?;
        Ok(video_output(
            final_path,
            &request.plan.aspect_ratio,
            request.plan.total_ms,
            &request.plan.output_resolution,
        ))
    }

    async fn export(&self, request: ExportRequest) -> ProviderResult<MediaOutput> {
        if !["720p", "1080p"].contains(&request.resolution.as_str())
            || request.format != "mp4"
            || request.fps != 30
        {
            return Err(ProviderError::invalid(
                "当前直接输出支持 720p、30 FPS 的 MP4。",
            ));
        }
        tokio::fs::create_dir_all(&self.work_dir)
            .await
            .map_err(|error| failed("创建导出目录", error))?;
        let path = self
            .work_dir
            .join(format!("{}.mp4", crate::util::make_id("export")));
        let (w, h) = crate::util::output_dimensions(&request.aspect_ratio, &request.resolution);
        let probe = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            tokio::process::Command::new(&self.ffprobe)
                .args([
                    "-v",
                    "error",
                    "-select_streams",
                    "v:0",
                    "-show_entries",
                    "stream=width,height,r_frame_rate",
                    "-of",
                    "json",
                ])
                .arg(&request.source)
                .kill_on_drop(true)
                .output(),
        )
        .await;
        let compatible = probe
            .ok()
            .and_then(Result::ok)
            .and_then(|o| serde_json::from_slice::<serde_json::Value>(&o.stdout).ok())
            .is_some_and(|d| {
                d["streams"][0]["width"] == w
                    && d["streams"][0]["height"] == h
                    && d["streams"][0]["r_frame_rate"] == "30/1"
            });
        if compatible {
            tokio::fs::copy(&request.source, &path)
                .await
                .map_err(|e| failed("复制成片", e))?;
        } else {
            encode(
                || {
                    let mut cmd = ffmpeg();
                    cmd.arg("-i").arg(&request.source).args([
                        "-vf",
                        &format!("scale={w}:{h}"),
                        "-c:a",
                        "aac",
                    ]);
                    cmd
                },
                &path,
                "导出成片",
            )
            .await?;
        }
        Ok(video_output(
            path,
            &request.aspect_ratio,
            request.duration_ms,
            &request.resolution,
        ))
    }

    async fn export_bundle(&self, _request: ProjectBundleRequest) -> ProviderResult<MediaOutput> {
        Err(ProviderError::invalid(
            "请先在成片后的选项中同意使用剪辑库，再导出可编辑工程包。",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// 独立媒体目录，不接触用户上传或应用数据库。
    struct Fixture(PathBuf);

    impl Fixture {
        /// 准备浅色画面，便于验证字幕没有遮黑原画。
        fn new() -> Self {
            let root = std::env::temp_dir().join(crate::util::make_id("direct-test"));
            std::fs::create_dir_all(&root).unwrap();
            image::RgbImage::from_pixel(320, 180, image::Rgb([240, 240, 240]))
                .save(root.join("first.png"))
                .unwrap();
            image::RgbImage::from_pixel(320, 180, image::Rgb([190, 220, 245]))
                .save(root.join("second.png"))
                .unwrap();
            Self(root)
        }

        /// 创建真实 PCM 正弦波；不同段落各有独立素材 ID。
        fn tone(&self, name: &str, frequency: f64, duration_ms: u32) -> PathBuf {
            let rate = 48_000_u32;
            let samples = rate * duration_ms / 1000;
            let mut bytes = Vec::new();
            bytes.extend_from_slice(b"RIFF");
            bytes.extend_from_slice(&(36 + samples * 4).to_le_bytes());
            bytes.extend_from_slice(b"WAVEfmt ");
            bytes.extend_from_slice(&16_u32.to_le_bytes());
            bytes.extend_from_slice(&1_u16.to_le_bytes());
            bytes.extend_from_slice(&2_u16.to_le_bytes());
            bytes.extend_from_slice(&rate.to_le_bytes());
            bytes.extend_from_slice(&(rate * 4).to_le_bytes());
            bytes.extend_from_slice(&4_u16.to_le_bytes());
            bytes.extend_from_slice(&16_u16.to_le_bytes());
            bytes.extend_from_slice(b"data");
            bytes.extend_from_slice(&(samples * 4).to_le_bytes());
            for index in 0..samples {
                let sample = (0.5
                    * i16::MAX as f64
                    * (std::f64::consts::TAU * frequency * index as f64 / rate as f64).sin())
                    as i16;
                bytes.extend_from_slice(&sample.to_le_bytes());
                bytes.extend_from_slice(&sample.to_le_bytes());
            }
            let path = self.0.join(name);
            std::fs::write(&path, bytes).unwrap();
            path
        }

        /// 测试和生产使用同样的输出目录构造。
        fn renderer(&self) -> DirectRenderer {
            let mut config = crate::Config::from_env();
            config.storage_dir = self.0.join("storage");
            DirectRenderer::new(&config)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// 构造已确认的制作上下文，测试不会请求任何模型。
    fn context() -> ProductionContext {
        ProductionContext {
            brief: None,
            settings: crate::catalog::empty_draft().settings,
            user_text: "测试确定性直接输出".into(),
            answers: vec![],
            speech_rate: 1.0,
        }
    }

    /// 检测每个时间区间的声音，避免只混入首段旁白的回归。
    async fn volume(path: &Path, start: f64) -> f64 {
        let mut command = ffmpeg();
        command
            .args(["-loglevel", "info", "-ss", &start.to_string(), "-i"])
            .arg(path)
            .args([
                "-t",
                "0.3",
                "-map",
                "0:a:0",
                "-af",
                "volumedetect",
                "-f",
                "null",
                "-",
            ]);
        let result = tokio::process::Command::from(command)
            .output()
            .await
            .unwrap();
        let log = String::from_utf8_lossy(&result.stderr);
        log.split("mean_volume:")
            .nth(1)
            .and_then(|tail| tail.split_whitespace().next())
            .and_then(|value| value.parse().ok())
            .unwrap_or_else(|| panic!("{log}"))
    }

    /// 两段画面、两段配音和烧录字幕均进入真正可解码的 MP4。
    #[tokio::test]
    async fn direct_output_contains_every_narration_and_real_subtitles() {
        let fixture = Fixture::new();
        let mut inputs = HashMap::from([
            ("first".into(), fixture.0.join("first.png")),
            ("second".into(), fixture.0.join("second.png")),
            ("voice1".into(), fixture.tone("voice1.wav", 440.0, 1000)),
            ("voice2".into(), fixture.tone("voice2.wav", 880.0, 1000)),
        ]);
        let clips = (0..2)
            .map(|index| Clip {
                shot_id: format!("shot{index}"),
                source_in_ms: 0,
                start_ms: index * 1000,
                duration_ms: 1000,
                visual_asset_id: Some(if index == 0 { "first" } else { "second" }.into()),
                narration_asset_id: Some(format!("voice{}", index + 1)),
                subtitle: Some(
                    if index == 0 {
                        "第一段字幕"
                    } else {
                        "第二段字幕"
                    }
                    .into(),
                ),
                ..Default::default()
            })
            .collect();
        let plan = EditPlan {
            aspect_ratio: "16:9".into(),
            total_ms: 2000,
            clips,
            narration_volume: 1.0,
            subtitles_enabled: true,
            ..Default::default()
        };
        let result = fixture
            .renderer()
            .render(RenderRequest {
                ctx: context(),
                plan,
                inputs: std::mem::take(&mut inputs),
            })
            .await
            .unwrap();
        let MediaData::Path(path) = result.data else {
            panic!("输出路径")
        };
        assert_eq!(result.duration_ms, Some(2000));
        for start in [0.2, 1.2] {
            assert!(volume(&path, start).await > -35.0);
        }
        let mut command = ffmpeg();
        command.args(["-ss", "0.3", "-i"]).arg(&path).args([
            "-frames:v",
            "1",
            "-f",
            "image2pipe",
            "-c:v",
            "png",
            "-",
        ]);
        let frame = tokio::process::Command::from(command)
            .output()
            .await
            .unwrap();
        assert!(frame.status.success());
        let image = image::load_from_memory(&frame.stdout).unwrap().to_rgb8();
        assert_eq!(image.dimensions(), (1280, 720));
        assert!(image.get_pixel(100, 100).0.iter().all(|value| *value > 220));
        assert!(image.get_pixel(100, 650).0.iter().all(|value| *value < 150));
        let bright_letters = (630..690)
            .flat_map(|y| (200..1080).map(move |x| (x, y)))
            .filter(|(x, y)| image.get_pixel(*x, *y).0.iter().all(|value| *value > 230))
            .count();
        assert!(bright_letters > 100, "透明字幕层没有真实文字");
    }

    /// 纯 BGM 也有完整音轨，短音乐会循环；导出 MP4 不会调用剪辑库。
    #[tokio::test]
    async fn bgm_only_output_loops_music_and_exports_without_editor() {
        let fixture = Fixture::new();
        let renderer = fixture.renderer();
        let plan = EditPlan {
            aspect_ratio: "16:9".into(),
            total_ms: 1000,
            clips: vec![Clip {
                shot_id: "shot".into(),
                source_in_ms: 0,
                start_ms: 0,
                duration_ms: 1000,
                visual_asset_id: Some("visual".into()),
                ..Default::default()
            }],
            music_asset_id: Some("music".into()),
            music_volume: 0.5,
            ..Default::default()
        };
        let result = renderer
            .render(RenderRequest {
                ctx: context(),
                plan: plan.clone(),
                inputs: HashMap::from([
                    ("visual".into(), fixture.0.join("first.png")),
                    ("music".into(), fixture.tone("bgm.wav", 220.0, 250)),
                ]),
            })
            .await
            .unwrap();
        let MediaData::Path(path) = result.data else {
            panic!("输出路径")
        };
        for start in [0.1, 0.7] {
            assert!(volume(&path, start).await > -40.0);
        }
        let exported = renderer
            .export(ExportRequest {
                source: path.clone(),
                duration_ms: 1000,
                aspect_ratio: "16:9".into(),
                resolution: "720p".into(),
                fps: 30,
                format: "mp4".into(),
            })
            .await
            .unwrap();
        let MediaData::Path(copy) = exported.data else {
            panic!("导出路径")
        };
        assert_eq!(std::fs::read(copy).unwrap(), std::fs::read(&path).unwrap());
        assert!(renderer
            .export_bundle(ProjectBundleRequest {
                source: path,
                plan,
                assets: vec![],
                title: "尚未选择剪辑库".into(),
                version_number: 1
            })
            .await
            .is_err());
    }
}
