//! 剪辑指令：llm1 在“剪辑”阶段输出、剪辑库逐条执行的操作序列。
//!
//! 指令是给 LLM 使用的“工具调用”格式（JSON，`op` 字段区分类型），刻意保持精简，
//! 只覆盖短视频剪辑最常用的操作；执行器（如 `providers::quercus`）把它们翻译为
//! quercus-engine 的 `EditCommand`。真实模型接入时，可以把本文件的结构作为
//! function calling / JSON Schema 交给模型，要求它直接输出指令数组。
//!
//! 约定：
//! - 素材用 `asset_id` 引用，执行器负责把它解析为本地文件；
//! - 时间统一用毫秒，执行器按序列帧率换算为帧；
//! - `ref`/`clip` 是指令内部的别名，后续指令用它引用前面导入的素材或放置的片段。
//!
//! 示例（一张图片配一段旁白）：
//!
//! ```json
//! [
//!   {"op": "create_sequence", "width": 720, "height": 1280, "fps": 30},
//!   {"op": "import_media", "ref": "img1", "asset_id": "ast_…"},
//!   {"op": "import_media", "ref": "voice1", "asset_id": "ast_…"},
//!   {"op": "place_clip", "media": "img1", "track": "video", "track_index": 0,
//!    "start_ms": 0, "end_ms": 7000, "clip": "shot1"},
//!   {"op": "place_clip", "media": "voice1", "track": "audio", "track_index": 0,
//!    "start_ms": 0, "end_ms": 7000}
//! ]
//! ```

use serde::{Deserialize, Serialize};

/// 片段所在的轨道类型。
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TrackType {
    /// 视频轨（画面）。
    Video,
    /// 音频轨（配音、背景音乐）。
    Audio,
}

/// 一条剪辑指令。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum EditOp {
    /// 创建序列（画布）。必须是第一条指令。
    CreateSequence {
        /// 画面宽度（像素）。
        width: i32,
        /// 画面高度（像素）。
        height: i32,
        /// 帧率。
        fps: i32,
    },
    /// 把项目素材导入剪辑工程，后续用 `ref` 引用。
    ImportMedia {
        /// 本次剪辑内的素材别名。
        #[serde(rename = "ref")]
        alias: String,
        /// 项目素材 ID。
        asset_id: String,
    },
    /// 在轨道上放置一个片段。
    PlaceClip {
        /// 引用 `import_media` 的别名。
        media: String,
        /// 轨道类型。
        track: TrackType,
        /// 同类轨道中的序号：视频 0 为主画面；音频 0 为配音、1 为背景音乐。
        track_index: usize,
        /// 在成片中的开始时间（毫秒）。
        start_ms: i64,
        /// 在成片中的结束时间（毫秒，不含）。
        end_ms: i64,
        /// 从素材的哪个时间点开始取（毫秒），默认 0。
        #[serde(default)]
        media_in_ms: i64,
        /// 片段别名，供转场等指令引用。
        #[serde(default)]
        clip: Option<String>,
    },
    /// 在两个相邻片段之间加交叉淡化转场。
    AddCrossfade {
        /// 前一个片段的别名。
        previous: String,
        /// 后一个片段的别名。
        next: String,
        /// 转场在接缝两侧各延伸的时长（毫秒）。
        half_ms: i64,
    },
}

/// 毫秒换算为指定帧率下的帧序号（四舍五入）。
pub fn ms_to_frames(ms: i64, fps: i32) -> i64 {
    (ms * fps as i64 + 500) / 1000
}

/// 成片帧率。
pub const DEFAULT_FPS: i32 = 30;

/// 把剪辑方案的时间线翻译成标准指令序列。
///
/// 演示版 llm1 用它生成指令；真实模型可以直接输出指令，不经过这里。
/// 轨道约定：视频 0 放镜头画面，音频 0 放配音，音频 1 放背景音乐。
pub fn ops_from_plan(plan: &super::EditPlan) -> Vec<EditOp> {
    let (width, height) = crate::util::dimensions(&plan.aspect_ratio);
    let mut ops = vec![EditOp::CreateSequence {
        width: width as i32,
        height: height as i32,
        fps: DEFAULT_FPS,
    }];
    for (index, clip) in plan.clips.iter().enumerate() {
        let end_ms = clip.start_ms + clip.duration_ms;
        if let Some(asset) = &clip.visual_asset_id {
            let alias = format!("visual{index}");
            ops.push(EditOp::ImportMedia {
                alias: alias.clone(),
                asset_id: asset.clone(),
            });
            ops.push(EditOp::PlaceClip {
                media: alias,
                track: TrackType::Video,
                track_index: 0,
                start_ms: clip.start_ms,
                end_ms,
                media_in_ms: 0,
                clip: Some(format!("shot{index}")),
            });
        }
        if let Some(asset) = &clip.narration_asset_id {
            let alias = format!("voice{index}");
            ops.push(EditOp::ImportMedia {
                alias: alias.clone(),
                asset_id: asset.clone(),
            });
            ops.push(EditOp::PlaceClip {
                media: alias,
                track: TrackType::Audio,
                track_index: 0,
                start_ms: clip.start_ms,
                end_ms,
                media_in_ms: 0,
                clip: None,
            });
        }
    }
    if let Some(asset) = &plan.music_asset_id {
        ops.push(EditOp::ImportMedia {
            alias: "music".into(),
            asset_id: asset.clone(),
        });
        // 背景音乐比成片短时首尾相接重复铺满（如 MiniMax-H3 生成的音轨只有约 5 秒）。
        let piece = plan
            .music_duration_ms
            .filter(|ms| *ms >= 1000)
            .unwrap_or(plan.total_ms);
        let mut start = 0;
        while start < plan.total_ms {
            let end = (start + piece).min(plan.total_ms);
            ops.push(EditOp::PlaceClip {
                media: "music".into(),
                track: TrackType::Audio,
                track_index: 1,
                start_ms: start,
                end_ms: end,
                media_in_ms: 0,
                clip: None,
            });
            start = end;
        }
    }
    set_crossfades(&mut ops, plan.effect_preset_id == "fade", 300);
    ops
}

/// 按特效预设整理转场：先移除全部交叉淡化，开启时再为主画面轨上相邻的镜头补上。
///
/// 在“调色与特效”阶段调用，保证用户在右侧修改预设后，指令与设置一致。
pub fn set_crossfades(ops: &mut Vec<EditOp>, enabled: bool, half_ms: i64) {
    ops.retain(|op| !matches!(op, EditOp::AddCrossfade { .. }));
    if !enabled {
        return;
    }
    let shots: Vec<String> = ops
        .iter()
        .filter_map(|op| match op {
            EditOp::PlaceClip {
                track: TrackType::Video,
                track_index: 0,
                clip: Some(alias),
                ..
            } => Some(alias.clone()),
            _ => None,
        })
        .collect();
    for pair in shots.windows(2) {
        ops.push(EditOp::AddCrossfade {
            previous: pair[0].clone(),
            next: pair[1].clone(),
            half_ms,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ops_round_trip_as_tagged_json() {
        let ops = vec![
            EditOp::CreateSequence {
                width: 720,
                height: 1280,
                fps: 30,
            },
            EditOp::ImportMedia {
                alias: "img1".into(),
                asset_id: "ast_1".into(),
            },
            EditOp::PlaceClip {
                media: "img1".into(),
                track: TrackType::Video,
                track_index: 0,
                start_ms: 0,
                end_ms: 7000,
                media_in_ms: 0,
                clip: Some("shot1".into()),
            },
        ];
        let text = serde_json::to_string(&ops).unwrap();
        assert!(text.contains(r#""op":"create_sequence""#));
        assert!(text.contains(r#""ref":"img1""#));
        let back: Vec<EditOp> = serde_json::from_str(&text).unwrap();
        assert_eq!(back, ops);
    }

    #[test]
    fn converts_milliseconds_to_frames() {
        assert_eq!(ms_to_frames(1000, 30), 30);
        assert_eq!(ms_to_frames(7500, 30), 225);
        assert_eq!(ms_to_frames(16, 30), 0);
    }
}
