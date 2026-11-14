//! Surround mixing (5.1 / 7.1): every top-level mixer channel is rendered as a stem and placed in the
//! surround field with 2D vector-base amplitude panning (VBAP) between the two nearest speakers; a
//! low-passed share goes to the LFE. The result is written as one multichannel WAV in WAVE / SMPTE order.
//!
//! The stereo renderer is reused unchanged, so effects, sends and buses behave exactly as in the stereo mix;
//! the master bus inserts are not applied to the surround mix (a stereo master chain has no meaning there).

use crate::dsp::{Biquad, BiquadKind};
use crate::export::{render_mix, ExportOptions};
use crate::model::*;
use crate::render::Pool;
use crate::samples::{AudioData, BitDepth, SampleCache};
use crate::{EngineError, Result};
use std::path::Path;

/// Speaker azimuths in degrees (0 = front, positive = right) per output channel; `None` = LFE.
pub fn layout(f: SurroundFormat) -> &'static [Option<f32>] {
    match f {
        SurroundFormat::S5_1 => &[Some(-30.0), Some(30.0), Some(0.0), None, Some(-110.0), Some(110.0)],
        SurroundFormat::S7_1 => &[Some(-30.0), Some(30.0), Some(0.0), None, Some(-150.0), Some(150.0), Some(-90.0), Some(90.0)],
    }
}

pub fn channel_names(f: SurroundFormat) -> &'static [&'static str] {
    match f {
        SurroundFormat::S5_1 => &["L", "R", "C", "LFE", "Ls", "Rs"],
        SurroundFormat::S7_1 => &["L", "R", "C", "LFE", "Lb", "Rb", "Ls", "Rs"],
    }
}

/// Constant-power gains for a source at `azimuth` degrees (VBAP between the two adjacent speakers).
pub fn vbap(f: SurroundFormat, azimuth: f32) -> Vec<f32> {
    let lay = layout(f);
    let mut speakers: Vec<(usize, f32)> = lay.iter().enumerate().filter_map(|(i, a)| a.map(|a| (i, a))).collect();
    speakers.sort_by(|a, b| a.1.total_cmp(&b.1));
    let mut gains = vec![0.0f32; lay.len()];
    let az = ((azimuth + 180.0).rem_euclid(360.0)) - 180.0;
    let n = speakers.len();
    for k in 0..n {
        let (ia, a) = speakers[k];
        let (ib, mut b) = speakers[(k + 1) % n];
        if b <= a {
            b += 360.0;
        }
        let mut x = az;
        if x < a {
            x += 360.0;
        }
        if x >= a && x <= b {
            let t = (x - a) / (b - a);
            gains[ia] = (t * std::f32::consts::FRAC_PI_2).cos();
            gains[ib] = (t * std::f32::consts::FRAC_PI_2).sin();
            return gains;
        }
    }
    gains
}

/// Azimuth of a pan position (x right, y front).
pub fn azimuth(p: &SurroundPan) -> f32 {
    p.x.atan2(p.y).to_degrees()
}

/// Renders the surround mix: one buffer per output channel.
pub fn render_surround(p: &Project, cache: &SampleCache, format: SurroundFormat, opts: &ExportOptions, make_pool: &mut dyn FnMut() -> Pool, progress: &mut dyn FnMut(f32)) -> (u32, Vec<Vec<f32>>) {
    // Top-level channels: their stems (soloed, with their sources, buses and sends) add up to the mix.
    let top: Vec<MixerChannel> = p.mixer.channels.iter().filter(|c| c.kind == ChannelKind::Audio && c.output.is_none()).cloned().collect();
    let mut base = p.clone();
    base.mixer.master.inserts.clear();
    let opts = ExportOptions { normalize_peak: None, loudness_target: None, ..opts.clone() };
    let mut out: Vec<Vec<f32>> = Vec::new();
    let mut rate = opts.render_rate;
    let count = top.len().max(1);
    for (k, ch) in top.iter().enumerate() {
        let mut q = base.clone();
        for c in q.mixer.channels.iter_mut() {
            c.solo = c.id == ch.id;
        }
        let mut pool = make_pool();
        let stem: AudioData = render_mix(&q, cache, &opts, &mut pool, &mut |x| progress((k as f32 + x) / count as f32));
        rate = stem.rate;
        if out.is_empty() {
            out = vec![vec![0.0; stem.frames()]; layout(format).len()];
        }
        let len = stem.frames().min(out[0].len());
        if stem.peak() < 1e-6 {
            continue;
        }
        let az = azimuth(&ch.surround);
        let half = ch.surround.spread.clamp(0.0, 180.0) / 2.0;
        let gl = vbap(format, az - half);
        let gr = vbap(format, az + half);
        for (o, (a, b)) in out.iter_mut().zip(gl.iter().zip(&gr)) {
            if *a == 0.0 && *b == 0.0 {
                continue;
            }
            for i in 0..len {
                o[i] += stem.l[i] * a + stem.r[i] * b;
            }
        }
        if ch.surround.lfe > 0.0 {
            let lfe = layout(format).iter().position(|a| a.is_none()).unwrap();
            let mut lp = Biquad::new(BiquadKind::LowPass, 120.0, 0.707, 0.0, rate as f32);
            for i in 0..len {
                out[lfe][i] += lp.process((stem.l[i] + stem.r[i]) * 0.5) * ch.surround.lfe;
            }
        }
    }
    (rate, out)
}

/// Writes a multichannel WAV (WAVE_FORMAT_EXTENSIBLE with the standard channel mask).
pub fn write_multichannel(path: &Path, channels: &[Vec<f32>], rate: u32, depth: BitDepth) -> Result<()> {
    let spec = match depth {
        BitDepth::Int16 => hound::WavSpec { channels: channels.len() as u16, sample_rate: rate, bits_per_sample: 16, sample_format: hound::SampleFormat::Int },
        BitDepth::Int24 => hound::WavSpec { channels: channels.len() as u16, sample_rate: rate, bits_per_sample: 24, sample_format: hound::SampleFormat::Int },
        BitDepth::Float32 => hound::WavSpec { channels: channels.len() as u16, sample_rate: rate, bits_per_sample: 32, sample_format: hound::SampleFormat::Float },
    };
    if let Some(d) = path.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let mut w = hound::WavWriter::create(path, spec).map_err(|e| EngineError::Io(e.to_string()))?;
    let n = channels.first().map(|c| c.len()).unwrap_or(0);
    let full = match depth {
        BitDepth::Int16 => 32767.0,
        BitDepth::Int24 => 8_388_607.0,
        BitDepth::Float32 => 1.0,
    };
    for i in 0..n {
        for c in channels {
            let s = c[i];
            match depth {
                BitDepth::Float32 => w.write_sample(s),
                _ => w.write_sample((s as f64 * full).round().clamp(-full - 1.0, full) as i32),
            }
            .map_err(|e| EngineError::Io(e.to_string()))?;
        }
    }
    w.finalize().map_err(|e| EngineError::Io(e.to_string()))
}

/// Renders and writes the project's surround mix.
pub fn export_surround(p: &Project, cache: &SampleCache, path: &Path, opts: &ExportOptions, make_pool: &mut dyn FnMut() -> Pool, progress: &mut dyn FnMut(f32)) -> Result<Vec<f32>> {
    let format = p.surround.ok_or_else(|| EngineError::Project("the project has no surround format".into()))?;
    let (rate, ch) = render_surround(p, cache, format, opts, make_pool, progress);
    if ch.is_empty() {
        return Err(EngineError::Project("nothing to export".into()));
    }
    // Peak-protect: scale the whole mix if any channel clips.
    let peak = ch.iter().flat_map(|c| c.iter()).fold(0.0f32, |m, x| m.max(x.abs()));
    let ch: Vec<Vec<f32>> = if peak > 0.999 { ch.into_iter().map(|c| c.into_iter().map(|x| x * 0.999 / peak).collect()).collect() } else { ch };
    let depth = match opts.format {
        crate::export::ExportFormat::Wav(d) => d,
        _ => BitDepth::Int24,
    };
    write_multichannel(path, &ch, rate, depth)?;
    Ok(ch.iter().map(|c| c.iter().fold(0.0f32, |m, x| m.max(x.abs()))).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vbap_hits_speakers_and_keeps_power() {
        let f = SurroundFormat::S5_1;
        let g = vbap(f, 0.0);
        assert!((g[2] - 1.0).abs() < 1e-5, "centre: {g:?}");
        let g = vbap(f, -110.0);
        assert!((g[4] - 1.0).abs() < 1e-5, "left surround: {g:?}");
        for az in [-170.0f32, -70.0, 15.0, 80.0, 179.0] {
            let p: f32 = vbap(f, az).iter().map(|x| x * x).sum();
            assert!((p - 1.0).abs() < 1e-4, "{az}: power {p}");
        }
        // Rear centre sits between Ls and Rs.
        let g = vbap(f, 180.0);
        assert!(g[4] > 0.6 && g[5] > 0.6 && g[0] == 0.0, "{g:?}");
    }

    #[test]
    fn surround_export_places_channels() {
        let dir = tempfile::tempdir().unwrap();
        let tone = dir.path().join("t.wav");
        let s: Vec<f32> = (0..48000).map(|i| (i as f32 * 0.05).sin() * 0.4).collect();
        crate::samples::write_wav(&tone, &s, &s, 48000, BitDepth::Float32, false, false).unwrap();
        let mut p = Project::new_default();
        p.bpm = 120.0;
        p.surround = Some(SurroundFormat::S5_1);
        // Track 1 in the rear left, track 2 front centre with no spread.
        for (k, pan) in [SurroundPan { x: -0.94, y: -0.34, spread: 0.0, lfe: 0.5 }, SurroundPan { x: 0.0, y: 1.0, spread: 0.0, lfe: 0.0 }].into_iter().enumerate() {
            let id = p.next_id();
            p.tracks[k].clips.push(Clip { id, name: "t".into(), start: 0.0, length: 2.0, muted: false, color: Rgb::default(), kind: ClipKind::Audio(AudioClip { source: tone.clone(), ..Default::default() }) });
            let m = p.tracks[k].mixer;
            p.mixer.get_mut(m).unwrap().surround = pan;
        }
        let out = dir.path().join("mix51.wav");
        let peaks = export_surround(&p, &SampleCache::default(), &out, &ExportOptions { tail: 0.0, ..Default::default() }, &mut Pool::new, &mut |_| {}).unwrap();
        // L R C LFE Ls Rs
        assert!(peaks[4] > 0.2, "rear-left stem in Ls: {peaks:?}");
        assert!(peaks[2] > 0.2, "centre stem in C: {peaks:?}");
        assert!(peaks[3] > 0.01, "LFE share: {peaks:?}");
        assert!(peaks[1] < 1e-3 && peaks[5] < 0.05, "nothing on the right: {peaks:?}");
        let r = hound::WavReader::open(&out).unwrap();
        assert_eq!(r.spec().channels, 6);
    }
}
