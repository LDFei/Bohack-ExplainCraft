//! 成片发布前的真实媒体检查；失败不能显示为成功交付。
use crate::providers::{ProviderError, ProviderResult};
use serde_json::{json, Value};
use std::{path::Path, time::Duration};

/// 检查真实流、帧率、尺寸、时长与音轨，并抽样完整解码；失败提供具体可修复原因。
pub async fn inspect(
    path: &Path,
    expected_ms: i64,
    width: i64,
    height: i64,
    audio_required: bool,
) -> ProviderResult<Value> {
    let out = tokio::process::Command::new(
        std::env::var("FFPROBE_PATH").unwrap_or_else(|_| "ffprobe".into()),
    )
    .args([
        "-v",
        "error",
        "-show_streams",
        "-show_format",
        "-of",
        "json",
    ])
    .arg(path)
    .kill_on_drop(true)
    .output();
    let out = tokio::time::timeout(Duration::from_secs(20), out)
        .await
        .map_err(|_| ProviderError::render_failed("成片检查超时。"))?
        .map_err(|_| ProviderError::render_failed("缺少 ffprobe，无法验证成片。"))?;
    let data: Value = serde_json::from_slice(&out.stdout)
        .map_err(|_| ProviderError::render_failed("成片文件无法解析。"))?;
    let streams = data["streams"]
        .as_array()
        .ok_or_else(|| ProviderError::render_failed("成片没有可用媒体流。"))?;
    let video = streams
        .iter()
        .find(|s| s["codec_type"] == "video")
        .ok_or_else(|| ProviderError::render_failed("成片缺少画面。"))?;
    let duration = data["format"]["duration"]
        .as_str()
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(0.)
        * 1000.;
    if video["width"] != width
        || video["height"] != height
        || (duration - expected_ms as f64).abs() > 200.
    {
        return Err(ProviderError::render_failed(
            "成片规格或时长与确认的时间线不一致。",
        ));
    }
    let has_audio = streams.iter().any(|s| s["codec_type"] == "audio");
    if audio_required && !has_audio {
        return Err(ProviderError::render_failed("成片缺少应有的音轨。"));
    }
    let decode = tokio::process::Command::new(
        std::env::var("FFMPEG_PATH").unwrap_or_else(|_| "ffmpeg".into()),
    )
    .args(["-v", "error", "-nostdin", "-i"])
    .arg(path)
    .args(["-f", "null", "-"])
    .kill_on_drop(true)
    .output();
    let decode = tokio::time::timeout(Duration::from_secs(120), decode)
        .await
        .map_err(|_| ProviderError::render_failed("成片解码检查超时。"))?
        .map_err(|_| ProviderError::render_failed("无法启动成片解码检查。"))?;
    if !decode.status.success() {
        return Err(ProviderError::render_failed("成片解码失败，文件可能损坏。"));
    }
    // 抽取实际像素和音频电平，识别有媒体流却全黑、全静音的无效交付。
    let mut command = tokio::process::Command::new(
        std::env::var("FFMPEG_PATH").unwrap_or_else(|_| "ffmpeg".into()),
    );
    command
        .args(["-v", "info", "-nostdin", "-i"])
        .arg(path)
        .args([
            "-t",
            "30",
            "-vf",
            "fps=1/3:start_time=0,signalstats,metadata=print",
        ]);
    if has_audio {
        command.args(["-af", "volumedetect"]);
    }
    command.args(["-f", "null", "-"]).kill_on_drop(true);
    let sample = tokio::time::timeout(Duration::from_secs(45), command.output())
        .await
        .map_err(|_| ProviderError::render_failed("画面与音量抽检超时。"))?
        .map_err(|_| ProviderError::render_failed("无法抽检成片画面与音量。"))?;
    if !sample.status.success() {
        return Err(ProviderError::render_failed("画面与音量抽检失败。"));
    }
    let log = String::from_utf8_lossy(&sample.stderr);
    let values = |name: &str| -> Vec<f64> {
        log.lines()
            .filter_map(|line| line.split_once(name)?.1.trim().parse().ok())
            .collect()
    };
    let brightness = values("lavfi.signalstats.YAVG=");
    let maxima = values("lavfi.signalstats.YMAX=");
    // MPEG 的黑电平约为16；有字/图形的深色Vibe画面不会被误判成全黑。
    if maxima.is_empty() || maxima.iter().all(|v| *v <= 20.) {
        return Err(ProviderError::render_failed(
            "抽检画面全黑，请重新生成画面。",
        ));
    }
    let mean_db = log.lines().find_map(|line| {
        line.split_once("mean_volume:")?
            .1
            .trim()
            .split_whitespace()
            .next()?
            .parse::<f64>()
            .ok()
    });
    if audio_required && mean_db.is_none_or(|v| !v.is_finite() || v < -65.) {
        return Err(ProviderError::render_failed(
            "抽检音轨接近静音，请更换音乐或重新生成配音。",
        ));
    }
    Ok(
        json!({"passed":true,"duration_ms":duration.round()as i64,"width":width,"height":height,"fps":video["avg_frame_rate"],"has_audio":has_audio,"decoded":true,"brightness_samples":brightness,"audio_mean_db":mean_db.filter(|v|v.is_finite()),"visual_review_required":true}),
    )
}
