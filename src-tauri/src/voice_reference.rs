//! 用户明确上传的本地配音参考音，校验后统一转换；不要求用户手写本机路径。
use crate::{state::AppState, util::make_id};
use std::path::PathBuf;

/// 未提交参考音目录的清理守卫。
struct Pending(PathBuf, bool);
impl Drop for Pending {
    fn drop(&mut self) {
        if !self.1 {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}
/// 保存3～20秒参考音与准确转写，成功返回内部路径，不向前端泄露路径。
pub async fn store(state: &AppState, bytes: &[u8], transcript: &str) -> Result<PathBuf, String> {
    if bytes.len() > 10 * 1024 * 1024
        || transcript.trim().is_empty()
        || transcript.chars().count() > 2000
    {
        return Err("请提供10MB以内的3～20秒参考音，并填写准确转写（最多2000字）。".into());
    }
    let (_, kind, ext) =
        crate::media::sniff(bytes).ok_or("不支持该参考音格式，请使用MP3、WAV或M4A。")?;
    if kind != "audio" {
        return Err("参考文件必须是音频。".into());
    }
    let dir = state
        .config
        .storage_dir
        .join("_voices")
        .join(make_id("reference"));
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|_| "无法创建参考音目录。")?;
    let mut pending = Pending(dir.clone(), false);
    let source = dir.join(format!("source.{ext}"));
    tokio::fs::write(&source, bytes)
        .await
        .map_err(|_| "无法保存参考音。")?;
    let probe = state.media.probe(&source).await;
    if !probe.ok
        || !probe
            .duration_ms
            .is_some_and(|d| (3000..=20000).contains(&d))
    {
        return Err("请截取3～20秒、清晰无背景音乐的参考音。".into());
    }
    let target = dir.join("reference.wav");
    let output = tokio::process::Command::new(
        std::env::var("FFMPEG_PATH").unwrap_or_else(|_| "ffmpeg".into()),
    )
    .args(["-v", "error", "-nostdin", "-i"])
    .arg(&source)
    .args(["-ar", "24000", "-ac", "1", "-c:a", "pcm_s16le"])
    .arg(&target)
    .kill_on_drop(true)
    .output();
    let out = tokio::time::timeout(std::time::Duration::from_secs(20), output)
        .await
        .map_err(|_| "参考音转换超时。")?
        .map_err(|_| "无法转换参考音。")?;
    if !out.status.success() {
        return Err("参考音转换失败，请使用MP3或WAV。".into());
    }
    tokio::fs::write(target.with_extension("txt"), transcript.trim())
        .await
        .map_err(|_| "无法保存参考音转写。")?;
    let _ = tokio::fs::remove_file(source).await;
    pending.1 = true;
    Ok(target)
}
