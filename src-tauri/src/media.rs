//! 文件存储、签名访问地址、媒体类型识别与元数据解析。
//! 前端只拿到带有效期的签名地址，不暴露服务器文件路径。

use std::path::{Path, PathBuf};
use std::time::Duration;

use hmac::{Hmac, Mac};
use rusqlite::Connection;
use sha2::Sha256;

use crate::config::Config;
use crate::db::{self, AssetRow, VersionRow};
use crate::error::ApiResult;
use crate::models::{Asset, Draft, VideoVersion};
use crate::providers::{MediaData, MediaOutput, ProviderError, ProviderResult, NARRATION_PAUSE_MS};
use crate::util::{iso, now};

/// 媒体服务：负责项目文件的存储、签名访问地址和元数据解析。
pub struct Media {
    /// 文件存储根目录。
    pub storage_dir: PathBuf,
    /// HMAC 签名密钥。
    secret: Vec<u8>,
    /// 签名地址有效期（秒）。
    ttl_seconds: i64,
    /// 接口前缀，用于拼接地址。
    prefix: String,
    /// ffprobe 可执行文件。
    ffprobe: String,
}

/// ffprobe 的解析结果。
#[derive(Default, Debug)]
pub struct Probe {
    /// 宽度（像素）。
    pub width: Option<i64>,
    /// 高度（像素）。
    pub height: Option<i64>,
    /// 时长（毫秒）。
    pub duration_ms: Option<i64>,
    /// 文件能否被解码。ffprobe 不可用时也为 `true`（跳过校验，不阻塞上传）。
    pub ok: bool,
}

/// 配音对齐的临时目录；失败或取消时同样清理，避免遗留半成品音频。
struct VoiceScratch(PathBuf);

impl Drop for VoiceScratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl Media {
    /// 创建媒体服务并确保存储目录存在。未配置密钥时随机生成。
    pub fn new(config: &Config) -> anyhow::Result<Self> {
        std::fs::create_dir_all(&config.storage_dir)?;
        let secret = match &config.media_secret {
            Some(secret) => secret.as_bytes().to_vec(),
            None => uuid::Uuid::new_v4().as_bytes().to_vec(),
        };
        Ok(Self {
            storage_dir: config.storage_dir.clone(),
            secret,
            ttl_seconds: config.media_url_ttl_seconds,
            prefix: config.public_base(),
            ffprobe: config.ffprobe.clone(),
        })
    }

    /// 计算签名：`HMAC-SHA256(asset_id | 过期时间 | 是否下载)`。
    ///
    /// 把“是否下载”纳入签名，防止把播放链接改成下载链接。
    fn signature(&self, asset_id: &str, expires: i64, download: bool) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.secret).expect("hmac key");
        mac.update(format!("{asset_id}|{expires}|{}", download as u8).as_bytes());
        hex::encode(mac.finalize().into_bytes())
    }

    /// 生成带签名的临时访问地址，返回 (url, expires_at)。
    pub fn signed_url(&self, asset_id: &str, download: bool) -> (String, String) {
        // 过期时间写进地址并参与签名，前端无法自行延长有效期。
        let expires_at = now() + chrono::Duration::seconds(self.ttl_seconds);
        let expires = expires_at.timestamp();
        let sig = self.signature(asset_id, expires, download);
        let dl = if download { "&dl=1" } else { "" };
        (
            format!(
                "{}/media/{asset_id}?exp={expires}&sig={sig}{dl}",
                self.prefix
            ),
            iso(expires_at),
        )
    }

    /// 校验签名地址：未过期且签名一致。
    pub fn verify(&self, asset_id: &str, expires: i64, download: bool, sig: &str) -> bool {
        if expires < now().timestamp() {
            return false;
        }
        let expected = self.signature(asset_id, expires, download);
        // 定长比较，避免时序差异。
        // 逐字节异或后汇总，比较耗时与内容无关，避免时序攻击。
        expected.len() == sig.len()
            && expected
                .bytes()
                .zip(sig.bytes())
                .fold(0u8, |acc, (a, b)| acc | (a ^ b))
                == 0
    }

    /// 素材在存储目录中的路径：`<项目>/<素材ID>.<扩展名>`。
    ///
    /// 只取文件名的扩展名，不使用用户提供的名称，避免路径穿越。
    pub fn path_for(&self, project_id: &str, asset_id: &str, file_name: &str) -> PathBuf {
        let ext = Path::new(file_name)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("bin");
        self.storage_dir
            .join(project_id)
            .join(format!("{asset_id}.{ext}"))
    }

    /// 把供应商产出保存进项目存储，返回 (路径, 字节数)。
    pub async fn store_output(
        &self,
        project_id: &str,
        asset_id: &str,
        output: &MediaOutput,
    ) -> ApiResult<(PathBuf, i64)> {
        let path = self.path_for(project_id, asset_id, &output.file_name);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        match &output.data {
            // 渲染器写在存储目录 `_render/` 下的临时成片直接移动（同一磁盘，免去复制）；
            // 其他来源（示例素材、用户指定路径）只复制，不改动原文件。
            MediaData::Path(source) if source.starts_with(self.storage_dir.join("_render")) => {
                if tokio::fs::rename(source, &path).await.is_err() {
                    tokio::fs::copy(source, &path).await?;
                    let _ = tokio::fs::remove_file(source).await;
                }
            }
            MediaData::Path(source) => {
                tokio::fs::copy(source, &path).await?;
            }
            MediaData::Bytes(bytes) => tokio::fs::write(&path, bytes).await?,
        }
        let size = tokio::fs::metadata(&path).await?.len() as i64;
        Ok((path, size))
    }

    /// 将实际过长的 TTS 音频变速到镜头预算内，保留完整语句与音高。
    ///
    /// 字数只能预估时长，部分供应商也会忽略 speed；必须以真实音频再次校验。
    /// 不截掉句尾，不修改上传文件，短配音保持原样。
    pub async fn fit_narration(
        &self,
        output: MediaOutput,
        shot_duration_ms: i64,
    ) -> ProviderResult<MediaOutput> {
        let fit = async {
            let dir = self
                .storage_dir
                .join("_render")
                .join(crate::util::make_id("voice-fit"));
            tokio::fs::create_dir_all(&dir).await?;
            let _scratch = VoiceScratch(dir.clone());
            let input = dir.join("input.audio");
            match &output.data {
                MediaData::Bytes(bytes) => tokio::fs::write(&input, bytes).await?,
                MediaData::Path(path) => {
                    tokio::fs::copy(path, &input).await?;
                }
            }
            let actual = self
                .probe(&input)
                .await
                .duration_ms
                .filter(|duration| *duration > 0)
                .ok_or_else(|| anyhow::anyhow!("无法测量配音时长"))?;
            // 额外预留 20ms，吸收 atempo 和采样边界的毫秒取整误差。
            let budget = (shot_duration_ms - NARRATION_PAUSE_MS - 20).max(200);
            if actual <= budget {
                return Ok(output);
            }
            let mut factor = actual as f64 / budget as f64;
            let mut filters = Vec::new();
            // 每级 atempo 不超过 2，避免大倍率时跳过音频采样；所有文字仍完整保留。
            while factor > 2.0 {
                filters.push("atempo=2".to_string());
                factor /= 2.0;
            }
            filters.push(format!("atempo={factor:.8}"));
            let path = dir.join("aligned.wav");
            let result = tokio::time::timeout(
                Duration::from_secs(60),
                tokio::process::Command::new(
                    std::env::var("FFMPEG_PATH").unwrap_or_else(|_| "ffmpeg".into()),
                )
                .args(["-v", "error", "-nostdin", "-y", "-i"])
                .arg(&input)
                .args(["-vn", "-af", &filters.join(","), "-c:a", "pcm_s16le"])
                .arg(&path)
                .kill_on_drop(true)
                .output(),
            )
            .await??;
            if !result.status.success() {
                anyhow::bail!(
                    "FFmpeg 配音对齐失败：{}",
                    String::from_utf8_lossy(&result.stderr)
                );
            }
            let duration = self
                .probe(&path)
                .await
                .duration_ms
                .filter(|duration| *duration > 0 && *duration <= budget + 40)
                .ok_or_else(|| anyhow::anyhow!("配音对齐结果不在镜头预算内"))?;
            tracing::info!(target: "job", "配音按镜头对齐：{actual}ms → {duration}ms，镜头 {shot_duration_ms}ms");
            Ok(MediaOutput {
                data: MediaData::Bytes(tokio::fs::read(path).await?),
                file_name: "narration.wav".into(),
                mime_type: "audio/wav".into(),
                duration_ms: Some(duration),
                ..output
            })
        };
        fit.await.map_err(|error: anyhow::Error| {
            tracing::warn!(target: "job", "配音时长对齐失败：{error:#}");
            ProviderError::render_failed("配音未能对齐镜头时长，请检查 FFmpeg 后重试。")
        })
    }

    /// 用 ffprobe 解析真实宽高与时长；ffprobe 不可用时返回 ok=false。
    pub async fn probe(&self, path: &Path) -> Probe {
        let run = tokio::process::Command::new(&self.ffprobe)
            .args([
                "-v",
                "error",
                "-show_entries",
                "stream=width,height:format=duration",
                "-of",
                "json",
            ])
            .arg(path)
            .kill_on_drop(true)
            .output();
        let output = match tokio::time::timeout(Duration::from_secs(10), run).await {
            Ok(Ok(output)) if output.status.success() => output,
            Ok(Ok(_)) => {
                return Probe {
                    ok: false,
                    ..Default::default()
                }
            }
            // ffprobe 无法启动（未安装）：跳过解析，不阻塞上传。
            Ok(Err(error)) => {
                tracing::warn!("ffprobe unavailable: {error}");
                return Probe {
                    ok: true,
                    ..Default::default()
                };
            }
            Err(_) => {
                return Probe {
                    ok: false,
                    ..Default::default()
                }
            }
        };
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(&output.stdout) else {
            return Probe {
                ok: false,
                ..Default::default()
            };
        };
        let stream = value["streams"]
            .as_array()
            .and_then(|streams| streams.iter().find(|s| s["width"].is_number()).cloned());
        let duration = value["format"]["duration"]
            .as_str()
            .and_then(|d| d.parse::<f64>().ok())
            .map(|d| (d * 1000.0).round() as i64);
        Probe {
            width: stream.as_ref().and_then(|s| s["width"].as_i64()),
            height: stream.as_ref().and_then(|s| s["height"].as_i64()),
            duration_ms: duration,
            ok: true,
        }
    }

    // ---------- 响应补全：把 asset_id 换成临时地址 ----------

    /// 转为接口对象：图片素材补上临时缩略图地址。
    pub fn asset_api(&self, row: &AssetRow) -> Asset {
        let mut asset = row.asset.clone();
        if asset.kind == "image" && asset.status == "ready" {
            asset.thumbnail_url = Some(self.signed_url(&asset.id, false).0);
        }
        asset
    }

    /// 在一组素材中找到第一张可用图片，返回它的临时地址。
    fn image_url(&self, conn: &Connection, asset_ids: &[String]) -> ApiResult<Option<String>> {
        for id in asset_ids {
            if let Some(row) = db::load_asset(conn, id)? {
                if row.asset.kind == "image" && row.asset.status == "ready" {
                    return Ok(Some(self.signed_url(id, false).0));
                }
            }
        }
        Ok(None)
    }

    /// 为分镜补上缩略图地址。
    pub fn hydrate_draft(&self, conn: &Connection, draft: &mut Draft) -> ApiResult<()> {
        for shot in &mut draft.shots {
            shot.thumbnail_url = self.image_url(conn, &shot.asset_ids)?;
        }
        Ok(())
    }

    /// 转为接口对象。full=false 时省略快照、播放地址和导出列表（列表接口）。
    pub fn version_api(
        &self,
        conn: &Connection,
        row: VersionRow,
        full: bool,
    ) -> ApiResult<VideoVersion> {
        let thumbnail_url = match &row.thumbnail_asset_id {
            Some(id) => self.image_url(conn, std::slice::from_ref(id))?,
            None => None,
        };
        let (playback_url, url_expires_at, snapshot, exports) = if full {
            let (url, expires) = self.signed_url(&row.video_asset_id, false);
            let mut snapshot = row.snapshot.clone();
            self.hydrate_draft(conn, &mut snapshot)?;
            (
                Some(url),
                Some(expires),
                Some(snapshot),
                Some(row.exports.clone()),
            )
        } else {
            (None, None, None, None)
        };
        Ok(VideoVersion {
            id: row.id,
            project_id: row.project_id,
            version_number: row.version_number,
            source_draft_revision: row.source_draft_revision,
            source_job_id: row.source_job_id,
            created_at: row.created_at,
            video_asset_id: row.video_asset_id,
            thumbnail_url,
            duration_ms: row.duration_ms,
            width: row.width,
            height: row.height,
            playback_url,
            url_expires_at,
            snapshot,
            exports,
        })
    }
}

/// 根据文件头识别真实类型，不信任扩展名和客户端 MIME。
/// 返回 (mime_type, kind, 扩展名)。
pub fn sniff(bytes: &[u8]) -> Option<(&'static str, &'static str, &'static str)> {
    let starts = |prefix: &[u8]| bytes.starts_with(prefix);
    if starts(&[0xFF, 0xD8, 0xFF]) {
        Some(("image/jpeg", "image", "jpg"))
    } else if starts(b"\x89PNG\r\n\x1a\n") {
        Some(("image/png", "image", "png"))
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some(("image/webp", "image", "webp"))
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WAVE" {
        Some(("audio/wav", "audio", "wav"))
    } else if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        Some(("video/mp4", "video", "mp4"))
    } else if starts(&[0x1A, 0x45, 0xDF, 0xA3]) {
        Some(("video/webm", "video", "webm"))
    } else if starts(b"ID3") || (bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] & 0xE0 == 0xE0) {
        Some(("audio/mpeg", "audio", "mp3"))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 使用真实音频验证超长配音会收敛到镜头预算，原始文件与短配音不被改写。
    #[tokio::test]
    async fn long_narration_fits_shot_without_mutating_source() {
        let root = std::env::temp_dir().join(crate::util::make_id("voice-fit-test"));
        let mut config = Config::from_env();
        config.storage_dir = root.clone();
        let media = Media::new(&config).unwrap();
        let source = root.join("original.wav");
        let result = tokio::process::Command::new(
            std::env::var("FFMPEG_PATH").unwrap_or_else(|_| "ffmpeg".into()),
        )
        .args([
            "-v",
            "error",
            "-nostdin",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=3",
        ])
        .arg(&source)
        .output()
        .await
        .unwrap();
        assert!(result.status.success());
        let original = tokio::fs::read(&source).await.unwrap();
        let fitted = media
            .fit_narration(
                MediaOutput {
                    data: MediaData::Path(source.clone()),
                    file_name: "original.wav".into(),
                    mime_type: "audio/wav".into(),
                    kind: "audio".into(),
                    source: "generated".into(),
                    width: None,
                    height: None,
                    duration_ms: None,
                },
                2000,
            )
            .await
            .unwrap();
        assert!(fitted.duration_ms.unwrap() <= 1620);
        assert_eq!(tokio::fs::read(source).await.unwrap(), original);
        let MediaData::Bytes(first) = &fitted.data else {
            panic!("对齐后应返回音频字节")
        };
        assert!(first.len() > 1000);
        let fitted_again = media.fit_narration(fitted.clone(), 2000).await.unwrap();
        let MediaData::Bytes(second) = fitted_again.data else {
            panic!("短配音应保持字节不变")
        };
        assert_eq!(*first, second);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn sniffs_common_types() {
        assert_eq!(sniff(&[0xFF, 0xD8, 0xFF, 0xE0]).unwrap().0, "image/jpeg");
        assert_eq!(sniff(b"RIFF\0\0\0\0WAVEfmt ").unwrap().0, "audio/wav");
        assert_eq!(sniff(b"\0\0\0\x18ftypmp42").unwrap().0, "video/mp4");
        assert!(sniff(b"hello world").is_none());
    }
}
