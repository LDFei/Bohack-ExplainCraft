//! 安装后的离线自检：加载真实程序和媒体依赖，不打开窗口、不读取用户配置。
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;

/// 在独立临时目录中执行真实 H.264/AAC 编解码，结束后清理测试文件。
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// 从安装资源中定位工具；Linux 安装包明确依赖 Ubuntu 的系统 FFmpeg。
fn tool(resources: &Path, name: &str) -> Result<PathBuf> {
    if cfg!(target_os = "linux") {
        let path = PathBuf::from("/usr/bin").join(name);
        if path.is_file() {
            return Ok(path);
        }
    } else {
        let name = if cfg!(windows) {
            format!("{name}.exe")
        } else {
            name.into()
        };
        for path in [resources.join("runtime").join(&name), resources.join(&name)] {
            if path.is_file() {
                return Ok(path);
            }
        }
    }
    bail!("安装包缺少媒体工具：{name}")
}

/// 避免开发机的动态库环境掩盖安装包遗漏的依赖。
fn command(executable: &Path) -> Command {
    let mut command = Command::new(executable);
    for key in [
        "DYLD_LIBRARY_PATH",
        "DYLD_FALLBACK_LIBRARY_PATH",
        "LD_LIBRARY_PATH",
    ] {
        command.env_remove(key);
    }
    command
}

/// 检查正式安装资源并输出机器可读证据；失败时返回非零状态，禁止上传产物。
pub fn run() -> Result<Value> {
    let context = crate::app_context();
    let resources =
        tauri::utils::platform::resource_dir(context.package_info(), &tauri::Env::default())?;
    if !resources.join("media/ambient.wav").is_file() {
        bail!("安装包缺少内置音乐资源")
    }
    let ffmpeg = tool(&resources, "ffmpeg")?;
    let ffprobe = tool(&resources, "ffprobe")?;
    let scratch = Scratch(std::env::temp_dir().join(format!(
        "explaincraft-package-check-{}",
        uuid::Uuid::new_v4()
    )));
    std::fs::create_dir_all(&scratch.0)?;
    let video = scratch.0.join("check.mp4");
    let encoded = command(&ffmpeg)
        .args([
            "-nostdin",
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=160x90:rate=10",
            "-f",
            "lavfi",
            "-i",
            "anullsrc=r=44100:cl=mono",
            "-t",
            "0.3",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-y",
        ])
        .arg(&video)
        .output()
        .context("无法启动安装包的 FFmpeg")?;
    if !encoded.status.success() {
        bail!(
            "安装包媒体编码失败：{}",
            String::from_utf8_lossy(&encoded.stderr)
        )
    }
    let probed = command(&ffprobe)
        .args(["-v", "error", "-show_streams", "-of", "json"])
        .arg(&video)
        .output()
        .context("无法启动安装包的 FFprobe")?;
    if !probed.status.success() {
        bail!(
            "安装包媒体检查失败：{}",
            String::from_utf8_lossy(&probed.stderr)
        )
    }
    let streams: Value = serde_json::from_slice(&probed.stdout)?;
    let streams = streams["streams"].as_array().context("没有媒体流")?;
    if !streams
        .iter()
        .any(|s| s["codec_name"] == "h264" && s["width"] == 160 && s["height"] == 90)
        || !streams.iter().any(|s| s["codec_name"] == "aac")
    {
        bail!("安装包未能生成可验证的 H.264/AAC 视频")
    }
    Ok(json!({
        "status": "passed",
        "version": context.package_info().version.to_string(),
        "identifier": context.config().identifier,
        "productName": context.config().product_name,
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "media": "h264+aac",
        "bundledMusic": true,
    }))
}
