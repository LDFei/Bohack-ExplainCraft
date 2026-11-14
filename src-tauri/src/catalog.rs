//! 能力目录：`GET /capabilities` 的返回值，以及新项目的默认草稿。
//!
//! 这里只列出已经接通的能力（文档 5.1：后端只能返回已接通的能力，前端不写死选项）。
//! 接入真实供应商后，在这里同步音色、风格、曲库和导出规格。

use serde_json::{json, Value};

use crate::models::*;

/// 可选参数目录。列表项为 `(id, 展示名称)`。
pub struct Catalog {
    /// 视频风格。
    pub styles: Vec<(&'static str, &'static str)>,
    /// 曲库中的背景音乐。
    pub music_tracks: Vec<(&'static str, &'static str)>,
    /// 调色预设。
    pub color_presets: Vec<(&'static str, &'static str)>,
    /// 特效预设。
    pub effect_presets: Vec<(&'static str, &'static str)>,
    /// 支持的画面比例。
    pub aspect_ratios: Vec<&'static str>,
    /// 支持的语言。
    pub languages: Vec<&'static str>,
    /// 目标时长下限（秒）。
    pub duration_min: u32,
    /// 目标时长上限（秒）。
    pub duration_max: u32,
    /// 可导出的分辨率。
    pub export_resolutions: Vec<&'static str>,
    /// 每个项目最多上传的素材数。
    pub max_uploaded_files: i64,
}

/// 某类上传文件的限制。
pub struct UploadLimit {
    /// 单个文件最大字节数。
    pub max_bytes: i64,
    /// 允许的真实 MIME 类型（按文件头识别）。
    pub mime_types: &'static [&'static str],
}

/// 图片上传限制：20MB，JPG / PNG / WebP。
pub const IMAGE_LIMIT: UploadLimit = UploadLimit {
    max_bytes: 20 * 1024 * 1024,
    mime_types: &["image/jpeg", "image/png", "image/webp"],
};
/// 视频上传限制：100MB，MP4 / WebM。
pub const VIDEO_LIMIT: UploadLimit = UploadLimit {
    max_bytes: 100 * 1024 * 1024,
    mime_types: &["video/mp4", "video/webm"],
};
/// 音频上传限制：50MB，MP3 / WAV。
pub const AUDIO_LIMIT: UploadLimit = UploadLimit {
    max_bytes: 50 * 1024 * 1024,
    mime_types: &["audio/mpeg", "audio/wav"],
};

/// 示例曲库允许公开访问的文件（白名单，防止路径穿越）。
pub const LIBRARY_FILES: &[&str] = &["ambient.wav"];

/// 默认目录与前端 `src/domain/content.ts` 的初始选项保持一致。
impl Default for Catalog {
    fn default() -> Self {
        Self {
            styles: vec![
                ("warm", "温暖治愈"),
                ("cinematic", "电影质感"),
                ("minimal", "简约高级"),
                ("energetic", "活力轻快"),
            ],
            music_tracks: vec![("piano", "午后钢琴"), ("light", "轻松日常")],
            color_presets: vec![
                ("natural", "自然原色"),
                ("warm", "暖调胶片"),
                ("cool", "清冷电影"),
            ],
            effect_presets: vec![("none", "简洁无特效"), ("fade", "柔和淡入淡出")],
            aspect_ratios: vec!["9:16", "16:9", "1:1"],
            languages: vec!["zh-CN"],
            duration_min: 15,
            duration_max: 300,
            export_resolutions: vec!["720p"],
            max_uploaded_files: 20,
        }
    }
}

/// 判断 `(id, 名称)` 列表中是否存在指定 ID。
fn has(list: &[(&str, &str)], id: &str) -> bool {
    list.iter().any(|(key, _)| *key == id)
}

impl Catalog {
    /// 风格 ID 是否可用。
    pub fn has_style(&self, id: &str) -> bool {
        has(&self.styles, id)
    }

    /// 曲库 ID 是否可用。
    pub fn has_music(&self, id: &str) -> bool {
        has(&self.music_tracks, id)
    }

    /// 调色预设 ID 是否可用。
    pub fn has_color(&self, id: &str) -> bool {
        has(&self.color_presets, id)
    }

    /// 特效预设 ID 是否可用。
    pub fn has_effect(&self, id: &str) -> bool {
        has(&self.effect_presets, id)
    }

    /// 生成 `/capabilities` 响应的 `data`。
    ///
    /// - `prefix`：接口前缀，用于拼接曲库试听地址；
    /// - `event_retention_seconds`：事件保留时长，前端据此判断断线恢复策略；
    pub fn capabilities(
        &self,
        prefix: &str,
        event_retention_seconds: i64,
        default_voice: &str,
        voice_options: &[String],
    ) -> Value {
        let named = |list: &[(&str, &str)]| -> Vec<Value> {
            list.iter()
                .map(|(id, name)| json!({ "id": id, "name": name }))
                .collect()
        };
        let limit =
            |l: &UploadLimit| json!({ "max_bytes": l.max_bytes, "mime_types": l.mime_types });
        json!({
            "defaults": {
                "target_duration_seconds": 30,
                "aspect_ratio": "9:16",
                "language": "zh-CN",
                "style_id": "warm",
                "voice_id": default_voice,
                "music_mode": "auto",
                "color_preset_id": "natural",
                "effect_preset_id": "none",
                "subtitles_enabled": true
            },
            "duration_range_seconds": { "min": self.duration_min, "max": self.duration_max },
            "aspect_ratios": self.aspect_ratios,
            "languages": self.languages,
            "styles": named(&self.styles),
            // 音色试听需要真实 TTS，未配置时不提供。
            "voices": voice_options.iter()
                .map(|voice| json!({ "id": voice, "name": voice, "sample_url": null }))
                .collect::<Vec<_>>(),
            // 当前曲库共用一段本地背景音。
            "music_tracks": self.music_tracks.iter()
                .map(|(id, name)| json!({
                    "id": id,
                    "name": name,
                    "sample_url": format!("{prefix}/library/{}", LIBRARY_FILES[0]),
                }))
                .collect::<Vec<_>>(),
            "color_presets": named(&self.color_presets),
            "effect_presets": named(&self.effect_presets),
            "exports": {
                "formats": ["mp4"],
                "resolutions": self.export_resolutions,
                "fps": [30],
                "preview_resolution": "720p"
            },
            "upload_limits": {
                "max_uploaded_files_per_project": self.max_uploaded_files,
                "image": limit(&IMAGE_LIMIT),
                "video": limit(&VIDEO_LIMIT),
                "audio": limit(&AUDIO_LIMIT),
            },
            "event_retention_seconds": event_retention_seconds,
        })
    }
}

/// 新项目的初始草稿：默认设置，全部阶段待计算（文档 5.2）。
pub fn empty_draft() -> Draft {
    Draft {
        brief: None,
        settings: Settings {
            target_duration_seconds: 30,
            aspect_ratio: "9:16".into(),
            language: "zh-CN".into(),
            style_id: "warm".into(),
        },
        script: None,
        shots: vec![],
        audio: AudioSettings {
            voice_id: "voice_warm".into(),
            speech_rate: 1.0,
            narration_enabled: true,
            music_mode: "auto".into(),
            music_id: None,
            music_asset_id: None,
            resolved_music_asset_id: None,
            narration_volume: 1.0,
            music_volume: 0.2,
        },
        finishing: Finishing {
            color_preset_id: "natural".into(),
            effect_preset_id: "none".into(),
            subtitles_enabled: true,
        },
        invalidated_stages: StageKey::ALL.to_vec(),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn procedural_background_music_is_enabled_by_default() {
        let draft = super::empty_draft();
        assert_eq!(draft.audio.music_mode, "auto");
        assert!(draft.audio.resolved_music_asset_id.is_none());
    }
}
