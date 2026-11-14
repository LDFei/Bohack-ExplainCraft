//! 本地程序化编曲：确定性合成 PCM WAV，不调用任何云端音乐模型。

use async_trait::async_trait;

use super::*;

/// 本地合成器。
pub struct ProceduralMusic;

/// 和弦、节奏与旋律参数。
#[derive(Clone, Copy)]
struct Composition {
    /// 每秒节拍数。
    bpm: f64,
    /// 主和弦根音频率。
    root: f64,
    /// 是否为小调。
    minor: bool,
    /// 四和弦的根音偏移（半音）。
    progression: [i32; 4],
    /// 琶音的相对音程。
    arpeggio: [i32; 4],
}

/// 按风格生成确定性参数，相同主题可得到一致结果。
fn composition(ctx: &ProductionContext, track_id: Option<&str>) -> Composition {
    let style = ctx
        .brief
        .as_ref()
        .map(|brief| brief.style.as_str())
        .unwrap_or("温暖");
    let subject = ctx
        .brief
        .as_ref()
        .map(|brief| brief.subject.as_str())
        .unwrap_or(ctx.user_text.as_str());
    let seed = subject
        .bytes()
        .chain(track_id.unwrap_or("auto").bytes())
        .fold(0_u32, |hash, byte| {
            hash.wrapping_mul(33).wrapping_add(byte as u32)
        });
    let roots = [130.81, 146.83, 164.81, 174.61, 196.0, 220.0];
    let root = roots[(seed as usize) % roots.len()];
    let minor = seed % 3 != 0;
    let (bpm, progression) = if style.contains("电影") || style.contains("高级") {
        (72.0, [0, 5, 3, 7])
    } else if style.contains("轻快") || style.contains("活力") {
        (108.0, [0, 7, 5, 0])
    } else if style.contains("简约") {
        (82.0, [0, 3, 5, 7])
    } else {
        (88.0, [0, 5, 3, 7])
    };
    Composition {
        bpm,
        root,
        minor,
        progression,
        arpeggio: if minor { [0, 3, 7, 12] } else { [0, 4, 7, 12] },
    }
}

/// 半音转频率。
fn semitone(root: f64, offset: i32) -> f64 {
    root * 2_f64.powf(offset as f64 / 12.0)
}

/// 带指数起音与释放的包络，避免简单蜂鸣声。
fn envelope(position: f64, length: f64) -> f64 {
    if position <= 0.0 || position >= length {
        return 0.0;
    }
    let attack = (position / 0.025).min(1.0);
    let release = ((length - position) / 0.22).clamp(0.0, 1.0);
    attack.min(release)
}

/// 拨弦音色：基频、二次谐波与快速衰减的高频泛音。
fn pluck(phase: f64, envelope: f64) -> f64 {
    let fundamental = (phase).sin() + 0.35 * (phase * 2.0).sin();
    let overtone = 0.2 * (phase * 5.0).sin() * envelope;
    (fundamental + overtone) * envelope
}

/// 生成一个 48kHz 双声道 16-bit PCM WAV。
fn synthesize(request: &MusicRequest) -> Vec<u8> {
    let sample_rate = 48_000_u32;
    let duration_ms = request.duration_ms.clamp(1000, 300_000) as u32;
    let sample_count = (duration_ms as u64 * sample_rate as u64 / 1000) as usize;
    let composition = composition(&request.ctx, request.track_id.as_deref());
    let beat = 60.0 / composition.bpm;
    let mut samples = vec![0.0_f32; sample_count * 2];
    let mut lowpass = 0.0_f32;
    for index in 0..sample_count {
        let time = index as f64 / sample_rate as f64;
        let beat_index = (time / beat).floor() as i32;
        let chord = composition.progression[(beat_index.div_euclid(4) as usize) % 4];
        let root = semitone(composition.root, chord);
        let chord_third = if composition.minor { 3 } else { 4 };
        let pad = (2.0 * std::f64::consts::PI * root * time).sin()
            + 0.55 * (2.0 * std::f64::consts::PI * semitone(root, chord_third) * time).sin()
            + 0.4 * (2.0 * std::f64::consts::PI * semitone(root, 7) * time).sin();
        let bass = (2.0 * std::f64::consts::PI * root * 0.5 * time).sin() * 0.42;
        let step = beat_index.rem_euclid(4) as usize;
        let note = semitone(root * 2.0, composition.arpeggio[step]);
        let note_position = time % (beat / 2.0);
        let note_length = beat / 2.0;
        let env = envelope(note_position, note_length);
        let phase = 2.0 * std::f64::consts::PI * note * time;
        let melody = pluck(phase, env) * 0.38;
        // 每拍一个柔和底鼓，半拍一个高通感短噪声，形成节奏但不喧宾夺主。
        let beat_phase = time % beat;
        let kick = if beat_phase < 0.16 {
            let frequency = 72.0 - 35.0 * (beat_phase / 0.16);
            (2.0 * std::f64::consts::PI * frequency * beat_phase).sin()
                * (-22.0 * beat_phase).exp()
                * 0.45
        } else {
            0.0
        };
        let hat_phase = (time + beat / 2.0) % beat;
        let hat = if hat_phase < 0.025 {
            let noise = ((index as u32)
                .wrapping_mul(1_664_525)
                .wrapping_add(1_013_904_223)) as f32
                / u32::MAX as f32
                * 2.0
                - 1.0;
            noise * (-180.0 * hat_phase).exp() as f32 * 0.08
        } else {
            0.0
        };
        let mixed = (pad * 0.20 + bass + melody + kick + hat as f64) as f32;
        // 一阶低通和轻混响让声音连贯，左右声道略有时间差。
        lowpass += 0.12 * (mixed - lowpass);
        let delay_index = index.saturating_sub((sample_rate as f64 * 0.18) as usize);
        let delayed = samples[delay_index * 2] * 0.16;
        let fade = ((index as f64 / sample_rate as f64) / 0.35)
            .min(1.0)
            .min(((sample_count - index - 1) as f64 / sample_rate as f64 / 0.45).max(0.0));
        samples[index * 2] = (lowpass + delayed) * fade as f32;
        samples[index * 2 + 1] = (lowpass * 0.96 + delayed * 0.7) * fade as f32;
    }
    let peak = samples
        .iter()
        .map(|sample| sample.abs())
        .fold(0.01_f32, f32::max);
    let gain = 0.82 / peak;
    let mut bytes = Vec::with_capacity(44 + samples.len() * 2);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&((36 + samples.len() * 2) as u32).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&sample_rate.to_le_bytes());
    bytes.extend_from_slice(&(sample_rate * 4).to_le_bytes());
    bytes.extend_from_slice(&4_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&(samples.len() as u32 * 2).to_le_bytes());
    for sample in samples {
        let value = (sample * gain).clamp(-1.0, 1.0);
        bytes.extend_from_slice(&((value * i16::MAX as f32) as i16).to_le_bytes());
    }
    bytes
}

#[async_trait]
impl MusicModel for ProceduralMusic {
    async fn compose(&self, request: MusicRequest) -> ProviderResult<MediaOutput> {
        let duration_ms = request.duration_ms;
        let bytes = tokio::task::spawn_blocking(move || synthesize(&request))
            .await
            .map_err(|_| ProviderError::render_failed("本地音乐合成失败"))?;
        Ok(MediaOutput {
            data: MediaData::Bytes(bytes),
            file_name: "procedural-music.wav".into(),
            mime_type: "audio/wav".into(),
            kind: "audio".into(),
            source: "synthesized".into(),
            width: None,
            height: None,
            duration_ms: Some(duration_ms as i64),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_timed_stereo_wav_without_silence() {
        let request = MusicRequest {
            ctx: ProductionContext {
                brief: None,
                settings: Settings {
                    target_duration_seconds: 2,
                    aspect_ratio: "16:9".into(),
                    language: "zh-CN".into(),
                    style_id: "warm".into(),
                },
                user_text: "温暖的咖啡店".into(),
                answers: vec![],
            },
            mode: "auto".into(),
            track_id: None,
            duration_ms: 2000,
        };
        let bytes = synthesize(&request);
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(bytes.len(), 44 + 2 * 48_000 * 2 * 2);
        assert!(bytes[44..].iter().any(|byte| *byte != 0));
    }
}
