//! Reference-track comparison for mastering: analyse a region of a reference song and of the mix, and set
//! them side by side – loudness (integrated, short-term max, loudness range), RMS, peak / true peak, crest
//! factor, PLR, stereo width and correlation, and the tonal balance in third-octave bands.
//!
//! The tonal balance is measured relative to each signal's own loudness, so a quieter mix does not look
//! "darker" than a loud reference: the curves show the *shape* of the spectrum, the loudness rows the level.

use crate::dsp::gain_to_db;
use crate::dsp::loudness::{LoudnessMeter, TruePeak};
use crate::samples::AudioData;
use rustfft::{num_complex::Complex, FftPlanner};

/// Centre frequencies of the third-octave bands (ISO 266, 25 Hz … 16 kHz).
pub const BANDS: [f32; 29] =
    [25.0, 31.5, 40.0, 50.0, 63.0, 80.0, 100.0, 125.0, 160.0, 200.0, 250.0, 315.0, 400.0, 500.0, 630.0, 800.0, 1000.0, 1250.0, 1600.0, 2000.0, 2500.0, 3150.0, 4000.0, 5000.0, 6300.0, 8000.0, 10000.0, 12500.0, 16000.0];

/// Lowest side / mid ratio reported (dB); at this value the signal is treated as mono.
pub const SIDE_FLOOR: f32 = -40.0;

/// Broad ranges for the summary (and the suggestions).
pub const RANGES: [(&str, f32, f32); 6] = [("Sub", 20.0, 60.0), ("Bass", 60.0, 250.0), ("Low mids", 250.0, 1000.0), ("Mids", 1000.0, 4000.0), ("Presence", 4000.0, 8000.0), ("Air", 8000.0, 20000.0)];

#[derive(Debug, Clone, PartialEq, Default)]
pub struct RegionAnalysis {
    /// Seconds analysed.
    pub seconds: f64,
    pub integrated: f32,
    pub short_term_max: f32,
    /// Loudness range (EBU Tech 3342 style: 10th–95th percentile of gated short-term loudness), LU.
    pub lra: f32,
    pub rms_db: f32,
    pub peak_db: f32,
    pub true_peak_db: f32,
    /// Peak − RMS.
    pub crest_db: f32,
    /// Peak to loudness ratio: true peak − integrated.
    pub plr: f32,
    /// Side energy relative to mid energy (dB); −∞ for mono.
    pub side_db: f32,
    /// L/R correlation −1 … 1.
    pub correlation: f32,
    /// Third-octave band levels in dB relative to the integrated loudness (same order as [`BANDS`]).
    pub tone: Vec<f32>,
    /// Broad range levels (same order as [`RANGES`]), dB relative to loudness.
    pub ranges: Vec<f32>,
}

/// Analyses `[from, to)` seconds of `d` (clamped to the file).
pub fn analyze(d: &AudioData, from: f64, to: f64) -> RegionAnalysis {
    let rate = d.rate as f32;
    let a = ((from.max(0.0)) * d.rate as f64) as usize;
    let b = ((to.max(from)) * d.rate as f64).min(d.frames() as f64) as usize;
    let (l, r) = (&d.l[a.min(b)..b], &d.r[a.min(b)..b]);
    let n = l.len();
    if n < (rate * 0.4) as usize {
        return RegionAnalysis {
            seconds: n as f64 / rate as f64,
            integrated: -70.0,
            short_term_max: -70.0,
            rms_db: -120.0,
            peak_db: -120.0,
            true_peak_db: -120.0,
            side_db: SIDE_FLOOR,
            tone: vec![-60.0; BANDS.len()],
            ranges: vec![-60.0; RANGES.len()],
            ..Default::default()
        };
    }
    // Loudness: integrated plus the short-term (3 s) history every 100 ms.
    let mut meter = LoudnessMeter::new(rate);
    let hop = (rate * 0.1) as usize;
    let mut short = Vec::new();
    let mut i = 0;
    while i < n {
        let e = (i + hop).min(n);
        meter.process(&l[i..e], &r[i..e]);
        if i as f32 >= rate * 3.0 {
            short.push(meter.short_term());
        }
        i = e;
    }
    let integrated = meter.integrated();
    if short.is_empty() {
        short.push(meter.short_term());
    }
    let short_term_max = short.iter().copied().fold(-70.0f32, f32::max);
    // LRA: short-term values gated at −70 LUFS absolute and −20 LU relative.
    let mut gated: Vec<f32> = short.iter().copied().filter(|v| *v > -70.0 && *v > integrated - 20.0).collect();
    gated.sort_by(f32::total_cmp);
    let pct = |p: f32| gated.get(((gated.len() as f32 - 1.0) * p).round() as usize).copied().unwrap_or(0.0);
    let lra = if gated.len() > 1 { pct(0.95) - pct(0.10) } else { 0.0 };
    // Levels and stereo.
    let (mut sum2, mut peak, mut mid2, mut side2, mut lr, mut ll, mut rr) = (0.0f64, 0.0f32, 0.0f64, 0.0f64, 0.0f64, 0.0f64, 0.0f64);
    let mut tp = TruePeak::default();
    let mut tpeak = 0.0f32;
    for k in 0..n {
        let (x, y) = (l[k], r[k]);
        sum2 += (x as f64 * x as f64 + y as f64 * y as f64) * 0.5;
        peak = peak.max(x.abs()).max(y.abs());
        tpeak = tpeak.max(tp.process(x, y));
        let (m, s) = ((x + y) as f64 * 0.5, (x - y) as f64 * 0.5);
        mid2 += m * m;
        side2 += s * s;
        lr += x as f64 * y as f64;
        ll += x as f64 * x as f64;
        rr += y as f64 * y as f64;
    }
    let rms_db = gain_to_db(((sum2 / n as f64).sqrt()) as f32);
    let peak_db = gain_to_db(peak);
    let true_peak_db = gain_to_db(tpeak.max(peak));
    // Floored at −40 dB: below that the signal is effectively mono and the exact figure means nothing.
    let side_db = if mid2 > 1e-12 { (10.0 * (side2 / mid2).max(1e-12).log10() as f32).max(SIDE_FLOOR) } else { SIDE_FLOOR };
    let correlation = if ll > 1e-12 && rr > 1e-12 { (lr / (ll * rr).sqrt()) as f32 } else { 1.0 };
    let (tone, ranges) = tonal_balance(l, r, rate, integrated);
    RegionAnalysis { seconds: n as f64 / rate as f64, integrated, short_term_max, lra, rms_db, peak_db, true_peak_db, crest_db: peak_db - rms_db, plr: true_peak_db - integrated, side_db, correlation, tone, ranges }
}

/// Average power spectrum (Welch, 8192-point Hann, 50 % overlap) of the mid signal, summed into bands.
fn tonal_balance(l: &[f32], r: &[f32], rate: f32, loudness: f32) -> (Vec<f32>, Vec<f32>) {
    const N: usize = 8192;
    let fft = FftPlanner::<f32>::new().plan_fft_forward(N);
    let win: Vec<f32> = (0..N).map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / N as f32).cos()).collect();
    let mut power = vec![0.0f64; N / 2];
    let mut frames = 0usize;
    let mut buf = vec![Complex::new(0.0f32, 0.0); N];
    let mut start = 0;
    // Long regions: at most ~400 frames, spread evenly.
    let hop = (N / 2).max((l.len().saturating_sub(N)) / 400);
    while start + N <= l.len() || (frames == 0 && start < l.len()) {
        for (k, b) in buf.iter_mut().enumerate() {
            let j = start + k;
            let m = if j < l.len() { (l[j] + r[j]) * 0.5 } else { 0.0 };
            *b = Complex::new(m * win[k], 0.0);
        }
        fft.process(&mut buf);
        for (k, p) in power.iter_mut().enumerate() {
            *p += buf[k].norm_sqr() as f64;
        }
        frames += 1;
        start += hop;
    }
    let norm = 1.0 / (frames.max(1) as f64 * (N as f64 * 0.375) * N as f64 * 0.5);
    let bin_hz = rate / N as f32;
    let band_power = |lo: f32, hi: f32| -> f32 {
        let (a, b) = (((lo / bin_hz).floor() as usize).max(1), ((hi / bin_hz).ceil() as usize).min(N / 2 - 1));
        let s: f64 = if b > a { power[a..b].iter().sum() } else { power[a.min(N / 2 - 1)] };
        10.0 * (s * norm).max(1e-14).log10() as f32 - loudness
    };
    let k = 2f32.powf(1.0 / 6.0);
    let tone = BANDS.iter().map(|f| band_power(f / k, f * k)).collect();
    let ranges = RANGES.iter().map(|(_, lo, hi)| band_power(*lo, *hi)).collect();
    (tone, ranges)
}

/// One row of the comparison table.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub name: &'static str,
    pub mix: f32,
    pub reference: f32,
    pub unit: &'static str,
}

impl Row {
    pub fn delta(&self) -> f32 {
        self.mix - self.reference
    }
}

/// The side-by-side numbers and what to do about the differences.
#[derive(Debug, Clone, PartialEq)]
pub struct Comparison {
    pub rows: Vec<Row>,
    /// Mix minus reference per third-octave band (dB, loudness-normalised).
    pub tone_delta: Vec<f32>,
    pub suggestions: Vec<String>,
}

pub fn compare(mix: &RegionAnalysis, reference: &RegionAnalysis) -> Comparison {
    let rows = vec![
        Row { name: "Integrated loudness", mix: mix.integrated, reference: reference.integrated, unit: "LUFS" },
        Row { name: "Short-term max", mix: mix.short_term_max, reference: reference.short_term_max, unit: "LUFS" },
        Row { name: "Loudness range", mix: mix.lra, reference: reference.lra, unit: "LU" },
        Row { name: "RMS", mix: mix.rms_db, reference: reference.rms_db, unit: "dBFS" },
        Row { name: "Sample peak", mix: mix.peak_db, reference: reference.peak_db, unit: "dBFS" },
        Row { name: "True peak", mix: mix.true_peak_db, reference: reference.true_peak_db, unit: "dBTP" },
        Row { name: "Crest factor", mix: mix.crest_db, reference: reference.crest_db, unit: "dB" },
        Row { name: "PLR", mix: mix.plr, reference: reference.plr, unit: "dB" },
        Row { name: "Side / mid", mix: mix.side_db, reference: reference.side_db, unit: "dB" },
        Row { name: "Correlation", mix: mix.correlation, reference: reference.correlation, unit: "" },
    ];
    let tone_delta: Vec<f32> = mix.tone.iter().zip(&reference.tone).map(|(a, b)| a - b).collect();
    let mut s = Vec::new();
    let d = mix.integrated - reference.integrated;
    if d.abs() >= 1.0 {
        s.push(format!(
            "Your mix is {:.1} dB {} than the reference (integrated). {}",
            d.abs(),
            if d < 0.0 { "quieter" } else { "louder" },
            if d < 0.0 { "Raise the limiter input / lower its threshold, or normalise on export." } else { "Back off the limiter, or compare at matched loudness." }
        ));
    }
    let dc = mix.crest_db - reference.crest_db;
    if dc >= 2.0 {
        s.push(format!("Your mix is more dynamic (crest factor +{dc:.1} dB): more compression or limiting gets it closer."));
    } else if dc <= -2.0 {
        s.push(format!("Your mix is more squashed (crest factor {dc:.1} dB): ease off compression / limiting to keep transients."));
    }
    let dl = mix.lra - reference.lra;
    if dl.abs() >= 3.0 {
        s.push(format!("Loudness range differs by {dl:+.1} LU: {}", if dl > 0.0 { "sections vary more in level than in the reference." } else { "the reference has more contrast between sections." }));
    }
    for (k, (name, lo, hi)) in RANGES.iter().enumerate() {
        let delta = mix.ranges[k] - reference.ranges[k];
        if delta.abs() >= 2.0 {
            s.push(format!("{name} ({}–{}): {:+.1} dB against the reference – {} around there.", hz(*lo), hz(*hi), delta, if delta > 0.0 { "cut" } else { "boost" }));
        }
    }
    let dw = mix.side_db - reference.side_db;
    if dw.abs() >= 3.0 {
        s.push(format!("Stereo: your mix is {} (side/mid {dw:+.1} dB).", if dw > 0.0 { "wider – check mono compatibility" } else { "narrower – widen pads / effects, keep the low end mono" }));
    }
    if mix.true_peak_db > -1.0 {
        s.push(format!("True peak {:.1} dBTP: keep it under −1 dBTP for streaming (limiter ceiling).", mix.true_peak_db));
    }
    if s.is_empty() {
        s.push("Close to the reference on loudness, dynamics, stereo and tonal balance.".into());
    }
    Comparison { rows, tone_delta, suggestions: s }
}

fn hz(f: f32) -> String {
    if f >= 1000.0 {
        format!("{:.0} kHz", f / 1000.0)
    } else {
        format!("{f:.0} Hz")
    }
}

/// Gain (dB) that brings `reference` to the loudness of `mix` – for loudness-matched A/B listening.
pub fn match_gain_db(mix: &RegionAnalysis, reference: &RegionAnalysis) -> f32 {
    if mix.integrated <= -69.0 || reference.integrated <= -69.0 {
        0.0
    } else {
        mix.integrated - reference.integrated
    }
}

/// A region of `d` as its own buffer, with a gain in dB (for previews).
pub fn excerpt(d: &AudioData, from: f64, to: f64, gain_db: f32) -> AudioData {
    let a = ((from.max(0.0)) * d.rate as f64) as usize;
    let b = ((to * d.rate as f64) as usize).min(d.frames()).max(a.min(d.frames()));
    let a = a.min(b);
    let g = crate::dsp::db_to_gain(gain_db);
    AudioData::from_planar(d.rate, d.l[a..b].iter().map(|x| x * g).collect(), d.r[a..b].iter().map(|x| x * g).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noise(secs: f32, rate: u32, seed: u64) -> Vec<f32> {
        let mut rng = crate::dsp::Rng::new(seed);
        (0..(secs * rate as f32) as usize).map(|_| rng.bipolar() * 0.3).collect()
    }

    fn lowpass(x: &[f32], f: f32, rate: f32) -> Vec<f32> {
        let mut b = crate::dsp::Biquad::new(crate::dsp::BiquadKind::LowPass, f, 0.707, 0.0, rate);
        let mut b2 = crate::dsp::Biquad::new(crate::dsp::BiquadKind::LowPass, f, 0.707, 0.0, rate);
        x.iter().map(|s| b2.process(b.process(*s))).collect()
    }

    #[test]
    fn darker_and_quieter_mix_is_reported() {
        let rate = 44100;
        let n = noise(8.0, rate, 3);
        let reference = AudioData::from_planar(rate, n.clone(), noise(8.0, rate, 4));
        // The "mix": the same noise low-passed at 2 kHz (darker), 6 dB down and mono.
        let dark: Vec<f32> = lowpass(&n, 2000.0, rate as f32).iter().map(|x| x * 0.5).collect();
        let mix = AudioData::from_planar(rate, dark.clone(), dark);
        let a = analyze(&mix, 0.0, 8.0);
        let b = analyze(&reference, 0.0, 8.0);
        assert!(a.integrated < b.integrated - 3.0, "{} vs {}", a.integrated, b.integrated);
        assert!(a.correlation > 0.99 && b.correlation.abs() < 0.1, "{} {}", a.correlation, b.correlation);
        assert_eq!(a.side_db, SIDE_FLOOR);
        let c = compare(&a, &b);
        // Highs are missing in the mix, relative to loudness.
        let k8 = BANDS.iter().position(|f| *f == 8000.0).unwrap();
        let k100 = BANDS.iter().position(|f| *f == 100.0).unwrap();
        assert!(c.tone_delta[k8] < -15.0, "8 kHz {}", c.tone_delta[k8]);
        assert!(c.tone_delta[k8] < c.tone_delta[k100] - 10.0);
        let text = c.suggestions.join("\n");
        assert!(text.contains("quieter"), "{text}");
        assert!(text.contains("Air") && text.contains("boost"), "{text}");
        assert!(text.contains("narrower"), "{text}");
        // Loudness matching for A/B.
        assert!((match_gain_db(&a, &b) - (a.integrated - b.integrated)).abs() < 1e-6);
        let ex = excerpt(&reference, 1.0, 2.0, -6.0);
        assert_eq!(ex.frames(), rate as usize);
        assert!((ex.l[10] - reference.l[rate as usize + 10] * 0.501).abs() < 1e-3);
    }

    #[test]
    fn identical_signals_match_and_regions_are_respected() {
        let rate = 44100;
        let mut l = noise(4.0, rate, 9);
        // Second half silent: analysing only the first two seconds must ignore it.
        for x in l.iter_mut().skip(2 * rate as usize) {
            *x = 0.0;
        }
        let d = AudioData::from_planar(rate, l.clone(), l);
        let a = analyze(&d, 0.0, 2.0);
        let b = analyze(&d, 0.0, 2.0);
        let c = compare(&a, &b);
        assert!(c.tone_delta.iter().all(|x| x.abs() < 1e-3));
        assert!(c.suggestions[0].starts_with("Close"), "{:?}", c.suggestions);
        let full = analyze(&d, 0.0, 4.0);
        assert!(full.rms_db < a.rms_db - 2.0, "silence lowers the RMS of the longer region");
        assert!((a.seconds - 2.0).abs() < 0.01);
    }
}
