//! The built-in "Dragon Kit": drum one-shots synthesised on first run and written as WAV files, so a
//! fresh offline install can make beats without any sample library.

use crate::dsp::{Biquad, BiquadKind, Rng};
use crate::samples::{write_wav, BitDepth};
use crate::Result;
use std::path::{Path, PathBuf};

const RATE: u32 = 48000;

fn secs(s: f32) -> usize {
    (s * RATE as f32) as usize
}

fn kick() -> Vec<f32> {
    let n = secs(0.6);
    let mut out = Vec::with_capacity(n);
    let mut phase = 0.0f32;
    for i in 0..n {
        let t = i as f32 / RATE as f32;
        let f = 45.0 + 110.0 * (-t * 28.0).exp();
        phase += f / RATE as f32;
        let body = (std::f32::consts::TAU * phase).sin() * (-t * 6.5).exp();
        let click = if i < 96 { (1.0 - i as f32 / 96.0) * 0.5 } else { 0.0 };
        out.push(((body + click) * 1.4).tanh() * 0.9);
    }
    out
}

fn snare(rng: &mut Rng) -> Vec<f32> {
    let n = secs(0.35);
    let mut bp = Biquad::new(BiquadKind::BandPass, 3200.0, 0.8, 0.0, RATE as f32);
    let mut phase = 0.0f32;
    (0..n)
        .map(|i| {
            let t = i as f32 / RATE as f32;
            phase += (180.0 + 60.0 * (-t * 40.0).exp()) / RATE as f32;
            let tone = (std::f32::consts::TAU * phase).sin() * (-t * 22.0).exp() * 0.6;
            let noise = bp.process(rng.bipolar()) * (-t * 13.0).exp() * 1.6;
            ((tone + noise) * 1.2).tanh() * 0.85
        })
        .collect()
}

fn clap(rng: &mut Rng) -> Vec<f32> {
    let n = secs(0.4);
    let mut bp = Biquad::new(BiquadKind::BandPass, 1400.0, 1.2, 0.0, RATE as f32);
    (0..n)
        .map(|i| {
            let t = i as f32 / RATE as f32;
            // Three short bursts, then the tail.
            let bursts = [0.0f32, 0.011, 0.022].iter().map(|s| if t >= *s && t < s + 0.009 { (-(t - s) * 300.0).exp() } else { 0.0 }).sum::<f32>();
            let tail = if t >= 0.03 { (-(t - 0.03) * 16.0).exp() } else { 0.0 };
            (bp.process(rng.bipolar()) * (bursts + tail) * 2.6).tanh() * 0.8
        })
        .collect()
}

fn hat(rng: &mut Rng, decay: f32, len: f32) -> Vec<f32> {
    let n = secs(len);
    let mut hp = Biquad::new(BiquadKind::HighPass, 7000.0, 0.7, 0.0, RATE as f32);
    let mut pk = Biquad::new(BiquadKind::Peak, 10500.0, 1.5, 6.0, RATE as f32);
    (0..n)
        .map(|i| {
            let t = i as f32 / RATE as f32;
            pk.process(hp.process(rng.bipolar())) * (-t * decay).exp() * 0.55
        })
        .collect()
}

fn tom(freq: f32) -> Vec<f32> {
    let n = secs(0.5);
    let mut phase = 0.0f32;
    (0..n)
        .map(|i| {
            let t = i as f32 / RATE as f32;
            phase += freq * (1.0 + 0.5 * (-t * 20.0).exp()) / RATE as f32;
            (std::f32::consts::TAU * phase).sin() * (-t * 7.0).exp() * 0.8
        })
        .collect()
}

fn rim() -> Vec<f32> {
    let n = secs(0.08);
    let mut phase = 0.0f32;
    (0..n)
        .map(|i| {
            let t = i as f32 / RATE as f32;
            phase += 1700.0 / RATE as f32;
            ((std::f32::consts::TAU * phase).sin() + 0.5 * (std::f32::consts::TAU * phase * 2.3).sin()) * (-t * 70.0).exp() * 0.6
        })
        .collect()
}

/// The kit's sounds: (name, GM drum note).
pub const KIT: &[(&str, u8)] = &[("Kick", 36), ("Rim", 37), ("Snare", 38), ("Clap", 39), ("Low tom", 41), ("Closed hat", 42), ("Mid tom", 43), ("Hi tom", 45), ("Open hat", 46)];

/// Writes the kit into `dir` (only missing files are written) and returns `(name, note, path)`.
pub fn ensure_default_kit(dir: &Path) -> Result<Vec<(String, u8, PathBuf)>> {
    std::fs::create_dir_all(dir).map_err(|e| crate::EngineError::Io(e.to_string()))?;
    let mut rng = Rng::new(0x00D4_A60E);
    let mut out = Vec::new();
    for (name, note) in KIT {
        let path = dir.join(format!("Dragon {name}.wav"));
        if !path.exists() {
            let s = match *name {
                "Kick" => kick(),
                "Rim" => rim(),
                "Snare" => snare(&mut rng),
                "Clap" => clap(&mut rng),
                "Low tom" => tom(95.0),
                "Mid tom" => tom(130.0),
                "Hi tom" => tom(175.0),
                "Closed hat" => hat(&mut rng, 38.0, 0.12),
                _ => hat(&mut rng, 7.0, 0.6),
            };
            write_wav(&path, &s, &s, RATE, BitDepth::Int24, true, false)?;
        }
        out.push((name.to_string(), *note, path));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    #[test]
    fn kit_is_written_and_decodes() {
        let dir = tempfile::tempdir().unwrap();
        let kit = super::ensure_default_kit(dir.path()).unwrap();
        assert_eq!(kit.len(), super::KIT.len());
        for (name, _, path) in &kit {
            let d = crate::samples::decode(path).unwrap();
            let pk = d.peak();
            assert!(pk > 0.2 && pk <= 1.0, "{name}: peak {pk}");
            assert!(d.duration() > 0.05, "{name}");
        }
        // Second call keeps the files.
        let again = super::ensure_default_kit(dir.path()).unwrap();
        assert_eq!(again[0].2, kit[0].2);
    }
}
