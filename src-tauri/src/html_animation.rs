//! 离线 HTML/CSS/内联 SVG 镜头输出；生成内容没有脚本、外链或文件访问能力。
use crate::providers::{MediaData, MediaOutput, ProviderError, ProviderResult};
use std::path::PathBuf;
use std::time::Duration;

/// 同时至多两个浏览器，避免多镜头占满桌面内存。
static BROWSERS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);
/// 本次工作目录和专属进程组，取消、失败、成功使用相同的清理路径。
struct Scratch {
    dir: PathBuf,
    group: Option<u32>,
}
impl Drop for Scratch {
    fn drop(&mut self) {
        if let Some(pid) = self.group {
            #[cfg(unix)]
            {
                let _ = std::process::Command::new("kill")
                    .args(["-TERM", "--", &format!("-{pid}")])
                    .output();
            }
            #[cfg(windows)]
            {
                let _ = std::process::Command::new("taskkill")
                    .args(["/PID", &pid.to_string(), "/T", "/F"])
                    .output();
            }
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
/// 拒绝明显的可执行内容；可信浏览器驱动还会对解析后的 DOM/CSS 再次校验。
fn validate(html: &str) -> ProviderResult<()> {
    // 标准 SVG 命名空间是标识符；浏览器层仍拒绝所有 src、非锚点 href 和外链 CSS。
    let lower = html
        .to_ascii_lowercase()
        .replace("http://www.w3.org/2000/svg", "")
        .replace("http://www.w3.org/1999/xlink", "");
    if html.len() > 300000
        || !lower.contains('<')
        || [
            "<script",
            "<iframe",
            "<object",
            "<embed",
            "javascript:",
            "file:",
            "http:",
            "https:",
            "<link",
            "@import",
            " onload",
            " onerror",
            " onclick",
            "<base",
            "<meta",
        ]
        .iter()
        .any(|token| lower.contains(token))
    {
        return Err(ProviderError::invalid(
            "HTML 镜头只允许离线文字、CSS 和内联 SVG；请移除脚本、事件与外部资源。",
        ));
    }
    Ok(())
}
/// 使用系统 Node 24+ 和 Chrome 定帧编码，依赖缺失时给出可操作说明而非假成片。
pub async fn render(
    html: &str,
    duration_ms: i64,
    width: u32,
    height: u32,
    index: usize,
) -> ProviderResult<MediaOutput> {
    validate(html)?;
    if !(1..=60000).contains(&duration_ms)
        || width < 16
        || height < 16
        || width as u64 * height as u64 > 2073600
    {
        return Err(ProviderError::invalid(
            "HTML 镜头尺寸或时长超出本地输出范围。",
        ));
    }
    let _permit = BROWSERS
        .acquire()
        .await
        .map_err(|_| ProviderError::render_failed("HTML 输出器已关闭，请重启应用。"))?;
    let dir = std::env::temp_dir().join(crate::util::make_id("html-shot"));
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|_| ProviderError::render_failed("无法准备 HTML 镜头目录。"))?;
    let mut scratch = Scratch {
        dir: dir.clone(),
        group: None,
    };
    let source = dir.join("scene.html");
    let script = dir.join("render-html.mjs");
    let output = dir.join("scene.mp4");
    tokio::fs::write(&source, html)
        .await
        .map_err(|_| ProviderError::render_failed("无法保存 HTML 镜头。"))?;
    tokio::fs::write(&script, include_str!("../scripts/render-html.mjs"))
        .await
        .map_err(|_| ProviderError::render_failed("无法准备 HTML 输出器。"))?;
    let mut command = std::process::Command::new(
        std::env::var("NODE_PATH_BIN").unwrap_or_else(|_| "node".into()),
    );
    command
        .arg(&script)
        .arg(&source)
        .arg(&output)
        .arg(dir.join("chrome"))
        .args([
            width.to_string(),
            height.to_string(),
            duration_ms.to_string(),
        ]);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut command = tokio::process::Command::from(command);
    let child = command
        .kill_on_drop(true)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|_| {
            ProviderError::unavailable(
                "HTML 画面需要本地 Node.js 24+ 和 Chrome；请安装后重试，或重新选择 SVG。",
            )
        })?;
    scratch.group = child.id();
    crate::metrics::production_log(format!(
        "HTML 离线浏览器开始输出镜头 {}，{}ms",
        index + 1,
        duration_ms
    ));
    let result = tokio::time::timeout(Duration::from_secs(240), child.wait_with_output())
        .await
        .map_err(|_| {
            ProviderError::render_failed("HTML 镜头输出超时，请减少画面复杂度或选择 SVG。")
        })?
        .map_err(|_| ProviderError::render_failed("HTML 输出进程中断，请重试。"))?;
    if !result.status.success() {
        tracing::warn!(target:"job","HTML 输出器失败：{}",String::from_utf8_lossy(&result.stderr));
        return Err(ProviderError::render_failed(
            "HTML 输出失败：请确认本地 Node.js 24+、Chrome 和 FFmpeg 可用，且画面没有脚本或外链。",
        ));
    }
    let bytes = tokio::fs::read(&output)
        .await
        .map_err(|_| ProviderError::render_failed("未能读取 HTML 镜头输出。"))?;
    if bytes.len() < 100 {
        return Err(ProviderError::render_failed("HTML 镜头输出为空，请重试。"));
    }
    Ok(MediaOutput {
        data: MediaData::Bytes(bytes),
        file_name: format!("html-shot-{}.mp4", index + 1),
        mime_type: "video/mp4".into(),
        kind: "video".into(),
        source: "generated".into(),
        width: Some(width as i64),
        height: Some(height as i64),
        duration_ms: Some(duration_ms),
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    /// 生成页面不得访问脚本、网络或本机文件。
    #[test]
    fn rejects_active_and_external_html() {
        assert!(
            validate("<svg xmlns=\"http://www.w3.org/2000/svg\"><circle r='10'/></svg>").is_ok()
        );
        for html in [
            "<script>alert(1)</script>",
            "<img src='file:///etc/passwd'>",
            "<style>@import 'https://x';</style>",
            "<div onclick='fetch()'>x</div>",
        ] {
            assert!(validate(html).is_err());
        }
    }
    /// 真实 Chrome 输出 CSS 动画，并验证中间帧发生变化。
    #[tokio::test]
    async fn renders_css_animation_to_real_video() {
        let result=render("<style>@keyframes move{from{transform:translateX(0)}to{transform:translateX(200px)}}.box{width:80px;height:80px;animation:move 1s linear both}</style><svg xmlns='http://www.w3.org/2000/svg' class='box'><rect width='80' height='80' fill='red'/></svg>",1000,320,180,0).await.unwrap();
        let MediaData::Bytes(bytes) = result.data else {
            panic!("应返回真实视频字节")
        };
        assert!(bytes.len() > 1000);
        let root = std::env::temp_dir().join(crate::util::make_id("html-test"));
        std::fs::create_dir_all(&root).unwrap();
        let video = root.join("test.mp4");
        std::fs::write(&video, bytes).unwrap();
        let mut cfg = crate::Config::from_env();
        cfg.storage_dir = root.join("storage");
        let media = crate::media::Media::new(&cfg).unwrap();
        let probe = media.probe(&video).await;
        assert_eq!(probe.width, Some(320));
        assert!((probe.duration_ms.unwrap() - 1000).abs() < 100);
        let frame = |at: &str| {
            std::process::Command::new(
                std::env::var("FFMPEG_PATH").unwrap_or_else(|_| "ffmpeg".into()),
            )
            .args(["-v", "error", "-ss", at, "-i"])
            .arg(&video)
            .args(["-frames:v", "1", "-f", "image2pipe", "-vcodec", "png", "-"])
            .output()
            .unwrap()
            .stdout
        };
        assert_ne!(frame("0.1"), frame("0.8"));
        let _ = std::fs::remove_dir_all(root);
    }
}
