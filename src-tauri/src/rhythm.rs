//! 从实际音轨分析节奏，保存可审查的估计结果；没有足够节奏信号时不冒充卡点成功。
use crate::providers::{ProviderError, ProviderResult};
use serde::{Deserialize, Serialize};
use std::{path::Path, time::Duration};

/// 实际音轨分析结果，时间单位为毫秒。
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Rhythm {
    /// 被分析的素材，不因恢复而串用另一首音乐。
    pub asset_id: String,
    /// 分析的音轨长度。
    pub duration_ms: i64,
    /// 估计 BPM；节奏不可靠时不提供。
    pub bpm: Option<f64>,
    /// 自相关强度，0～1。
    pub confidence: f64,
    /// 实测起音拟合出的拍点；无规则节奏时保存实际强起音。
    pub beats_ms: Vec<i64>,
    /// 可显示的方法说明。
    pub method: String,
}

/// 从单声道采样的短时能量变化识别起音、速度与相位。
fn detect(samples: &[f32], rate: usize) -> Rhythm {
    let hop = rate / 50;
    let energy: Vec<f64> = samples
        .chunks(hop)
        .map(|s| {
            (s.iter().map(|x| (*x as f64).powi(2)).sum::<f64>() / s.len().max(1) as f64).sqrt()
        })
        .collect();
    let flux: Vec<f64> = energy
        .iter()
        .enumerate()
        .map(|(i, e)| (e - energy.get(i.wrapping_sub(1)).copied().unwrap_or(0.)).max(0.))
        .collect();
    let scale = flux.iter().copied().fold(0., f64::max);
    let mut result = Rhythm {
        duration_ms: samples.len() as i64 * 1000 / rate as i64,
        method: "起音能量与自相关估计".into(),
        ..Default::default()
    };
    if scale < 0.002 {
        result.method = "音轨节奏信号不足，保留阅读节奏".into();
        return result;
    }
    let peaks: Vec<usize> = (1..flux.len().saturating_sub(1))
        .filter(|i| flux[*i] > scale * 0.2 && flux[*i] >= flux[*i - 1] && flux[*i] > flux[*i + 1])
        .collect();
    let norm: f64 = flux.iter().map(|x| x * x).sum();
    let (lag, strength) = (17..=50)
        .map(|lag| {
            let sum: f64 = (lag..flux.len()).map(|i| flux[i] * flux[i - lag]).sum();
            (lag, sum / norm.max(1e-9))
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap_or((25, 0.));
    result.confidence = strength.clamp(0., 1.);
    if strength > 0.12 && peaks.len() >= 4 {
        result.bpm = Some(3000. / lag as f64);
        let phase = (0..lag)
            .max_by(|a, b| {
                let score = |phase: usize| {
                    peaks
                        .iter()
                        .filter(|p| {
                            let d = **p % lag;
                            d.abs_diff(phase).min(lag - d.abs_diff(phase)) <= 1
                        })
                        .map(|p| flux[*p])
                        .sum::<f64>()
                };
                score(*a).total_cmp(&score(*b))
            })
            .unwrap_or(0);
        result.beats_ms = (phase..flux.len())
            .step_by(lag)
            .map(|i| i as i64 * 20)
            .collect();
    } else {
        result.beats_ms = peaks.into_iter().map(|i| i as i64 * 20).collect();
        result.method = "检测到起音，规则拍速不可靠；仅使用实际起音点".into();
    }
    result
}

/// 有界解码实际文件；不依赖 Python 音频包，不读取用户任意目录。
pub async fn analyze(path: &Path, asset_id: &str) -> ProviderResult<Rhythm> {
    let mut command = tokio::process::Command::new(
        std::env::var("FFMPEG_PATH").unwrap_or_else(|_| "ffmpeg".into()),
    );
    command
        .args(["-v", "error", "-nostdin", "-i"])
        .arg(path)
        .args([
            "-t", "300", "-vn", "-ac", "1", "-ar", "8000", "-f", "f32le", "-",
        ])
        .kill_on_drop(true);
    let out = tokio::time::timeout(Duration::from_secs(45), command.output())
        .await
        .map_err(|_| ProviderError::render_failed("音乐节奏分析超时，请更换音轨。"))?
        .map_err(|_| ProviderError::render_failed("无法分析音乐，请检查 FFmpeg。"))?;
    if !out.status.success() {
        return Err(ProviderError::invalid("这份音频无法解码，请更换音乐文件。"));
    }
    let samples: Vec<f32> = out
        .stdout
        .chunks_exact(4)
        .map(|x| f32::from_le_bytes(x.try_into().unwrap()))
        .collect();
    let mut result = detect(&samples, 8000);
    result.asset_id = asset_id.into();
    Ok(result)
}

impl Rhythm {
    /// 背景音乐循环时，把已分析节奏按真实音轨长度延展到成片时长。
    pub fn positions(&self, total_ms: i64) -> Vec<i64> {
        if self.duration_ms <= 0 {
            return vec![];
        }
        let mut beats = vec![];
        let mut offset = 0;
        while offset < total_ms {
            for beat in &self.beats_ms {
                let t = offset + beat;
                if t < total_ms {
                    beats.push(t);
                }
            }
            offset += self.duration_ms;
        }
        beats
    }
    /// 在原时长附近寻找拍点，保持所有镜头与文字，最后一镜保持总时长。
    pub fn align(&self, shots: &mut [crate::models::Shot], reading_rate: f64) {
        let total: i64 = shots.iter().map(|s| s.duration_ms).sum();
        let beats = self.positions(total);
        let minimum: Vec<i64> = shots
            .iter()
            .map(|s| {
                ((s.narration.chars().count() as f64 / reading_rate * 1000.) as i64 + 500).max(800)
            })
            .collect();
        let tolerance = self
            .bpm
            .map(|b| (33000. / b) as i64)
            .unwrap_or(320)
            .clamp(180, 500);
        let mut original = 0;
        let mut actual = 0;
        let count = shots.len();
        for (i, shot) in shots.iter_mut().enumerate() {
            original += shot.duration_ms;
            let target = if i + 1 == count {
                total
            } else {
                beats
                    .iter()
                    .copied()
                    .filter(|b| {
                        (*b - original).abs() <= tolerance
                            && *b >= actual + minimum[i]
                            && *b <= total - minimum[i + 1..].iter().sum::<i64>()
                    })
                    .min_by_key(|b| (*b - original).abs())
                    .unwrap_or(original)
            };
            shot.duration_ms = target - actual;
            actual = target;
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    /// 合成已知 120BPM 音轨，必须从实际波形恢复速度，静音不得伪造拍点。
    #[test]
    fn detects_real_pulse_and_rejects_silence() {
        let mut s = vec![0f32; 8000 * 12];
        for at in (0..s.len()).step_by(4000) {
            for j in 0..160 {
                s[at + j] = (j as f32 * 0.3).sin() * (1. - j as f32 / 160.);
            }
        }
        let r = detect(&s, 8000);
        assert!((r.bpm.unwrap() - 120.).abs() < 6., "{r:?}");
        assert!(r.beats_ms.len() > 15);
        assert!(detect(&vec![0.; 16000], 8000).bpm.is_none());
    }
}
