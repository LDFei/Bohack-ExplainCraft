//! FFmpeg H.264 编码器选择：硬件优先、软件兜底，并明确记录实际使用的编码器。

use std::process::{Command, ExitStatus};

use super::{ProviderError, ProviderResult};

/// 项目支持的 H.264 编码后端。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum H264Encoder {
    /// Apple VideoToolbox。
    VideoToolbox,
    /// NVIDIA NVENC。
    Nvenc,
    /// Intel Quick Sync Video。
    Qsv,
    /// AMD Advanced Media Framework。
    Amf,
    /// x264 软件编码。
    Software,
}

impl H264Encoder {
    /// FFmpeg 命令行中的编码器名称。
    pub fn name(self) -> &'static str {
        match self {
            Self::VideoToolbox => "h264_videotoolbox",
            Self::Nvenc => "h264_nvenc",
            Self::Qsv => "h264_qsv",
            Self::Amf => "h264_amf",
            Self::Software => "libx264",
        }
    }

    /// 按用户显式指定的名称解析后端；`auto` 和 `software` 有特殊含义。
    fn from_name(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "h264_videotoolbox" | "videotoolbox" => Some(Self::VideoToolbox),
            "h264_nvenc" | "nvenc" => Some(Self::Nvenc),
            "h264_qsv" | "qsv" => Some(Self::Qsv),
            "h264_amf" | "amf" => Some(Self::Amf),
            "libx264" | "software" | "cpu" => Some(Self::Software),
            _ => None,
        }
    }

    /// 把该后端的编码参数写入命令，调用方继续添加像素格式和输出参数。
    pub fn apply(self, command: &mut Command) {
        command.arg("-c:v").arg(self.name());
        match self {
            Self::VideoToolbox => {
                command.args([
                    "-b:v",
                    "8M",
                    "-maxrate",
                    "10M",
                    "-bufsize",
                    "16M",
                    "-prio_speed",
                    "1",
                    "-allow_sw",
                    "0",
                ]);
            }
            Self::Nvenc => {
                command.args(["-preset", "p3", "-cq", "19"]);
            }
            Self::Qsv => {
                command.args(["-global_quality", "19"]);
            }
            Self::Amf => {
                command.args([
                    "-quality", "speed", "-rc", "cqp", "-qp_i", "19", "-qp_p", "19",
                ]);
            }
            Self::Software => {
                command.args(["-preset", "veryfast", "-crf", "18"]);
            }
        }
    }
}

/// 返回本次编码按优先级尝试的后端。
///
/// `FFMPEG_HW_ENCODER` 可设为 `auto`、`software`、`videotoolbox`、`nvenc`、`qsv`
/// 或 `amf`。默认先用当前平台的硬件后端，全部不可用时使用 x264。
pub fn candidates() -> Vec<H264Encoder> {
    if let Ok(requested) = std::env::var("FFMPEG_HW_ENCODER") {
        let requested = requested.trim();
        if !requested.is_empty() && !requested.eq_ignore_ascii_case("auto") {
            if let Some(encoder) = H264Encoder::from_name(requested) {
                return if encoder == H264Encoder::Software {
                    vec![H264Encoder::Software]
                } else {
                    vec![encoder, H264Encoder::Software]
                };
            }
            tracing::warn!(target: "job", "未知 FFMPEG_HW_ENCODER={requested}，改用自动选择");
        }
    }
    #[cfg(target_os = "macos")]
    let hardware = vec![H264Encoder::VideoToolbox];
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    let hardware = vec![H264Encoder::Nvenc, H264Encoder::Qsv, H264Encoder::Amf];
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    let hardware = vec![];
    hardware
        .into_iter()
        .chain([H264Encoder::Software])
        .collect()
}

/// 逐个尝试编码后端；只有进程成功退出才停止，失败时记录原因并继续回退。
pub fn encode_h264(
    label: &str,
    mut run: impl FnMut(H264Encoder) -> std::io::Result<ExitStatus>,
) -> ProviderResult<()> {
    for encoder in candidates() {
        tracing::info!(target: "job", "  · {label} 使用 {} 编码", encoder.name());
        match run(encoder) {
            Ok(status) if status.success() => {
                tracing::info!(target: "job", "  · {label} 编码完成（{}）", encoder.name());
                return Ok(());
            }
            Ok(status) => {
                let detail = format!("{} 退出状态 {status}", encoder.name());
                tracing::warn!(target: "job", "  · {label} {detail}，尝试下一编码器");
            }
            Err(error) => {
                let detail = format!("{} 无法启动：{error}", encoder.name());
                tracing::warn!(target: "job", "  · {label} {detail}，尝试下一编码器");
            }
        }
    }
    Err(ProviderError::render_failed(format!(
        "{label}失败，硬件与软件编码均不可用，请检查 FFmpeg 安装。"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 硬件参数不会误入 x264，且禁止 VideoToolbox 静默改用软件。
    #[test]
    fn hardware_arguments_select_the_real_encoder() {
        let mut command = Command::new("ffmpeg");
        H264Encoder::VideoToolbox.apply(&mut command);
        let args: Vec<String> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert!(args
            .windows(2)
            .any(|pair| pair == ["-c:v", "h264_videotoolbox"]));
        assert!(args.windows(2).any(|pair| pair == ["-allow_sw", "0"]));
        assert!(!args.iter().any(|arg| arg == "libx264"));
    }

    /// 软件兜底使用快速预设，避免默认 medium 造成不必要的等待。
    #[test]
    fn software_arguments_use_the_fast_preset() {
        let mut command = Command::new("ffmpeg");
        H264Encoder::Software.apply(&mut command);
        let args: Vec<String> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert!(args.windows(2).any(|pair| pair == ["-c:v", "libx264"]));
        assert!(args.windows(2).any(|pair| pair == ["-preset", "veryfast"]));
    }
}
