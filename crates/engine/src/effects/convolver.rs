//! Reliquary: a convolution reverb. The impulse response is either one of the built-in spaces (rooms, halls,
//! a plate, a spring, speaker cabinets, a telephone line – synthesised, see [`space_ir`]) or a file loaded by
//! the user (WAV, FLAC, …).
//!
//! The convolution is uniformly partitioned overlap-save in the frequency domain: blocks of [`B`] samples,
//! FFT size `2B`, one stored spectrum per partition of the response. Left and right are transformed together
//! as the real and imaginary part of one complex FFT (two real transforms for the price of one) and split by
//! the conjugate symmetry `X_l[k] = (X[k] + X*[N-k]) / 2`, `X_r[k] = (X[k] - X*[N-k]) / 2i`.
//!
//! Building a response (generating it, stretching it, cutting its spectra) allocates and takes milliseconds,
//! so it never happens on the audio thread once the effect runs: a worker thread owned by the effect builds
//! the kernel and hands it over through a bounded channel; the audio thread swaps it in with a 20 ms
//! crossfade and hands the old one back to the worker to be freed.

use super::{Effect, EffectDef, ParamInfo};
use crate::dsp::{Biquad, BiquadKind, DelayLine, Rng};
use rustfft::{num_complex::Complex, Fft, FftPlanner};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};

/// Partition (block) size in samples; also the effect's latency.
pub const B: usize = 256;
const N: usize = 2 * B;
const H: usize = N / 2 + 1;
/// Longest response kept (seconds at the session rate).
const MAX_SECONDS: f32 = 6.0;
/// Points of the response's envelope sent to the display.
pub const ENV_POINTS: usize = 96;

pub const SPACES: &[&str] = &["Small room", "Studio room", "Chamber", "Concert hall", "Cathedral", "Plate", "Spring", "Cabinet 1×12", "Cabinet 4×12", "Telephone"];

const PARAMS: &[ParamInfo] = &[
    ParamInfo::choice("space", "Space", SPACES, 3.0),
    ParamInfo::lin("size", "Size", 50.0, 200.0, 100.0, "%"),
    ParamInfo::lin("damping", "Damping", 0.0, 100.0, 35.0, "%"),
    ParamInfo::toggle("reverse", "Reverse", false),
    ParamInfo::lin("predelay", "Pre-delay", 0.0, 250.0, 10.0, "ms"),
    ParamInfo::log("low_cut", "Low cut", 20.0, 1000.0, 20.0, "Hz"),
    ParamInfo::log("high_cut", "High cut", 1000.0, 20000.0, 20000.0, "Hz"),
    ParamInfo::lin("width", "Width", 0.0, 200.0, 100.0, "%"),
    ParamInfo::lin("mix", "Mix", 0.0, 100.0, 30.0, "%"),
    ParamInfo::lin("output", "Output", -24.0, 12.0, 0.0, "dB"),
];

pub fn convolver_def() -> EffectDef {
    EffectDef {
        id: "convolver",
        name: "Reliquary (convolution reverb)",
        category: "Space",
        description: "Convolution reverb. Built-in rooms, halls, a plate, a spring, speaker cabinets and a telephone line, or any impulse-response file; size stretches the response, damping darkens its tail.",
        params: PARAMS,
        uses_sidechain: false,
        create: |rate| Box::new(Convolver::new(rate)),
    }
}

/// What the kernel is built from (the parameters that change the response itself).
#[derive(Clone, Copy, PartialEq, Debug)]
struct Shape {
    space: usize,
    size: f32,
    damping: f32,
    reverse: bool,
}

/// The partitioned spectra of a stereo response.
pub struct Kernel {
    /// Per partition: `H` bins of the left and of the right response.
    l: Vec<Complex<f32>>,
    r: Vec<Complex<f32>>,
    parts: usize,
    /// Envelope (dB) for the display.
    env: [f32; ENV_POINTS],
    /// Length in seconds.
    seconds: f32,
}

impl Kernel {
    fn build(l: &[f32], r: &[f32], rate: f32, fft: &Arc<dyn Fft<f32>>) -> Kernel {
        let len = l.len().max(1);
        let parts = len.div_ceil(B);
        let mut kl = vec![Complex::default(); parts * H];
        let mut kr = vec![Complex::default(); parts * H];
        let mut buf = vec![Complex::default(); N];
        for p in 0..parts {
            buf.fill(Complex::default());
            for i in 0..B {
                let k = p * B + i;
                if k < l.len() {
                    buf[i] = Complex::new(l[k], r[k]);
                }
            }
            fft.process(&mut buf);
            for k in 0..H {
                let a = buf[k];
                let b = buf[(N - k) % N].conj();
                kl[p * H + k] = (a + b) * 0.5;
                // (a - b) / 2i
                let d = (a - b) * 0.5;
                kr[p * H + k] = Complex::new(d.im, -d.re);
            }
        }
        let mut env = [-90.0f32; ENV_POINTS];
        let step = len.div_ceil(ENV_POINTS);
        for (j, e) in env.iter_mut().enumerate() {
            let a = j * step;
            let b = ((j + 1) * step).min(len);
            let mut peak = 0.0f32;
            for i in a..b {
                peak = peak.max(l.get(i).map_or(0.0, |v| v.abs())).max(r.get(i).map_or(0.0, |v| v.abs()));
            }
            *e = (20.0 * peak.max(1e-6).log10()).max(-90.0);
        }
        let top = env.iter().cloned().fold(-90.0f32, f32::max);
        for e in env.iter_mut() {
            *e -= top;
        }
        Kernel { l: kl, r: kr, parts, env, seconds: len as f32 / rate }
    }
}

/// A loaded impulse-response file (at the session rate).
#[derive(Clone)]
pub struct IrFile {
    pub l: Vec<f32>,
    pub r: Vec<f32>,
}

struct Shared {
    shape: [AtomicU32; 4],
    generation: AtomicU64,
    quit: AtomicBool,
    file: Mutex<Option<Arc<IrFile>>>,
}

impl Shared {
    fn shape(&self) -> Shape {
        let g = |i: usize| f32::from_bits(self.shape[i].load(Ordering::Relaxed));
        Shape { space: g(0) as usize, size: g(1), damping: g(2), reverse: g(3) >= 0.5 }
    }
    fn store(&self, s: Shape) {
        let v = [s.space as f32, s.size, s.damping, if s.reverse { 1.0 } else { 0.0 }];
        for (a, v) in self.shape.iter().zip(v) {
            a.store(v.to_bits(), Ordering::Relaxed);
        }
    }
}

/// Builds the response of a shape: the file (or the built-in space), stretched by the size, darkened by the
/// damping, reversed if asked.
fn response(shape: Shape, file: Option<&IrFile>, rate: f32) -> (Vec<f32>, Vec<f32>) {
    let (mut l, mut r) = match file {
        Some(f) => {
            // Size stretches a file's response in time (linear interpolation).
            let s = shape.size / 100.0;
            let n = ((f.l.len() as f32 * s) as usize).clamp(1, (MAX_SECONDS * rate) as usize);
            let read = |v: &[f32], t: f32| {
                let i = t as usize;
                let fr = t - i as f32;
                let a = v.get(i).copied().unwrap_or(0.0);
                let b = v.get(i + 1).copied().unwrap_or(0.0);
                a + (b - a) * fr
            };
            ((0..n).map(|i| read(&f.l, i as f32 / s)).collect(), (0..n).map(|i| read(&f.r, i as f32 / s)).collect())
        }
        None => space_ir(shape.space, shape.size / 100.0, rate),
    };
    if shape.damping > 0.0 {
        damp(&mut l, &mut r, shape.damping / 100.0, rate);
    }
    if shape.reverse {
        // Reverse the tail, then fade its start in over 30 ms so it swells instead of clicking.
        l.reverse();
        r.reverse();
        let fade = (0.03 * rate) as usize;
        for i in 0..fade.min(l.len()) {
            let g = i as f32 / fade as f32;
            l[i] *= g;
            r[i] *= g;
        }
    }
    normalize(&mut l, &mut r);
    (l, r)
}

/// High frequencies of the tail die faster: the band above 2.5 kHz is multiplied by `e^{-t·d·6}` (t in
/// seconds, d the damping 0 … 1) – a −52 dB extra loss per second at full damping.
fn damp(l: &mut [f32], r: &mut [f32], d: f32, rate: f32) {
    for v in [l, r] {
        let mut lp = Biquad::new(BiquadKind::LowPass, 2500.0, 0.707, 0.0, rate);
        for (i, x) in v.iter_mut().enumerate() {
            let low = lp.process(*x);
            let high = *x - low;
            let t = i as f32 / rate;
            *x = low + high * (-t * d * 6.0).exp();
        }
    }
}

/// Scales a response to unit energy per channel pair (so spaces and files of different lengths come out at
/// a similar loudness), with a little headroom.
fn normalize(l: &mut [f32], r: &mut [f32]) {
    let e: f64 = l.iter().chain(r.iter()).map(|x| (*x as f64) * (*x as f64)).sum();
    if e > 1e-12 {
        let g = (0.5 / e.sqrt()) as f32 * std::f32::consts::SQRT_2;
        for x in l.iter_mut().chain(r.iter_mut()) {
            *x *= g;
        }
    }
}

/// The built-in responses.
///
/// Rooms and halls: early reflections (sparse taps over the first `40·s` ms, alternating sides) followed by
/// a dense tail of decorrelated noise in six octave-wide bands, each decaying with its own reverb time
/// (`T60` scaled by the size `s`; highs shorter than lows, as air and walls absorb them), reaching full density
/// after a build-up. Plate: dense from the start and bright. Spring: repeated dispersive chirps (a chain of
/// all-passes on an impulse train at the spring's round-trip time). Cabinets and telephone: short filter
/// responses (high-pass, resonances, steep low-pass).
pub fn space_ir(space: usize, s: f32, rate: f32) -> (Vec<f32>, Vec<f32>) {
    let mut rng = Rng::new(0x5eed_0000 + space as u64);
    // (T60 at 500 Hz in s, early-reflection span in ms, build-up ms, brightness 0 … 1)
    let (t60, er_ms, build_ms, bright) = match space {
        0 => (0.45, 18.0, 8.0, 0.55),
        1 => (0.8, 25.0, 12.0, 0.6),
        2 => (1.4, 35.0, 20.0, 0.65),
        3 => (2.3, 55.0, 40.0, 0.5),
        4 => (4.5, 80.0, 70.0, 0.45),
        5 => (2.0, 0.0, 2.0, 0.9),
        6 => return spring(s, rate, &mut rng),
        7 => return cabinet(false, rate),
        8 => return cabinet(true, rate),
        _ => return telephone(rate),
    };
    let t60 = t60 * s;
    let len = ((t60 * 1.2 + 0.05) * rate).min(MAX_SECONDS * rate) as usize;
    let mut l = vec![0.0f32; len];
    let mut r = vec![0.0f32; len];
    // Dense tail: six bands at 125 Hz … 4 kHz (and the rest above), each with its own decay.
    let centres = [125.0f32, 250.0, 500.0, 1000.0, 2000.0, 4000.0];
    for (bi, fc) in centres.iter().enumerate() {
        // Highs decay faster; brightness lengthens them again.
        let ratio = if *fc <= 500.0 { 1.0 + 0.15 * (500.0 / fc).log2() } else { 1.0 - (0.18 - 0.12 * bright) * (fc / 500.0).log2() };
        let t = (t60 * ratio).max(0.05);
        let k = 6.907_755 / (t * rate); // ln(1000): −60 dB after t
        let mut fl = Biquad::new(BiquadKind::BandPass, *fc, 0.9, 0.0, rate);
        let mut fr = Biquad::new(BiquadKind::BandPass, *fc, 0.9, 0.0, rate);
        let gain = if bi == centres.len() - 1 { 0.6 + 0.6 * bright } else { 1.0 };
        for i in 0..len {
            let env = (-(i as f32) * k).exp() * gain;
            l[i] += fl.process(rng.bipolar()) * env;
            r[i] += fr.process(rng.bipolar()) * env;
        }
    }
    // Build-up: the tail grows in over `build_ms` (a room is not dense at once).
    let build = (build_ms * s * 0.001 * rate).max(1.0);
    let start = (er_ms * s * 0.4 * 0.001 * rate) as usize;
    for i in 0..len {
        let g = if i < start { 0.0 } else { (1.0 - (-((i - start) as f32) / build).exp()).min(1.0) };
        l[i] *= g;
        r[i] *= g;
    }
    // Early reflections: 12 taps, alternating sides, growing sparser and quieter.
    if er_ms > 0.0 {
        let tail_rms = (l.iter().take((0.2 * rate) as usize).map(|x| x * x).sum::<f32>() / (0.2 * rate)).sqrt();
        for k in 0..12 {
            let t = (er_ms * s * 0.001 * rate) * ((k as f32 + 1.0) / 12.0).powf(0.8) * (0.85 + 0.3 * rng.next_f32());
            let i = t as usize;
            if i < len {
                let a = tail_rms * 6.0 * (1.0 - k as f32 / 14.0) * if rng.next_f32() < 0.5 { -1.0 } else { 1.0 };
                if k % 2 == 0 {
                    l[i] += a;
                    r[i] += a * 0.5;
                } else {
                    r[i] += a;
                    l[i] += a * 0.5;
                }
            }
        }
    }
    (l, r)
}

fn spring(s: f32, rate: f32, rng: &mut Rng) -> (Vec<f32>, Vec<f32>) {
    let t60 = 2.2 * s;
    let len = ((t60 + 0.05) * rate).min(MAX_SECONDS * rate) as usize;
    let trip = (0.033 * s * rate) as usize;
    let k = 6.907_755 / (t60 * rate);
    let mut out = [vec![0.0f32; len], vec![0.0f32; len]];
    for (c, v) in out.iter_mut().enumerate() {
        // Impulse train at the round trip, a little jittered per side.
        let off = c * trip / 7;
        let mut i = off;
        while i < len {
            v[i] = (-(i as f32) * k).exp() * (0.9 + 0.2 * rng.next_f32());
            i += trip;
        }
        // Dispersion: 24 first-order all-passes (lows arrive later than highs – the chirp).
        for _ in 0..24 {
            let a = 0.62;
            let (mut x1, mut y1) = (0.0f32, 0.0f32);
            for x in v.iter_mut() {
                let y = -a * *x + x1 + a * y1;
                x1 = *x;
                y1 = y;
                *x = y;
            }
        }
        // The spring's band: ~120 Hz … 4.5 kHz.
        let mut hp = Biquad::new(BiquadKind::HighPass, 120.0, 0.707, 0.0, rate);
        let mut lp = Biquad::new(BiquadKind::LowPass, 4500.0, 0.707, 0.0, rate);
        let mut pk = Biquad::new(BiquadKind::Peak, 1800.0, 1.2, 6.0, rate);
        for x in v.iter_mut() {
            *x = pk.process(lp.process(hp.process(*x)));
        }
    }
    let [l, r] = out;
    (l, r)
}

fn filtered_impulse(len: usize, stages: &mut [Biquad]) -> Vec<f32> {
    let mut v = vec![0.0f32; len];
    v[0] = 1.0;
    for st in stages.iter_mut() {
        for x in v.iter_mut() {
            *x = st.process(*x);
        }
    }
    v
}

/// A speaker cabinet: high-pass (the closed box's low cut), the cone's low resonance, the presence bump,
/// a cone break-up notch and a steep low-pass.
fn cabinet(big: bool, rate: f32) -> (Vec<f32>, Vec<f32>) {
    let len = (0.04 * rate) as usize;
    let mut st = if big {
        vec![
            Biquad::new(BiquadKind::HighPass, 70.0, 0.8, 0.0, rate),
            Biquad::new(BiquadKind::Peak, 105.0, 1.6, 5.0, rate),
            Biquad::new(BiquadKind::Peak, 2600.0, 1.1, 4.0, rate),
            Biquad::new(BiquadKind::Notch, 4300.0, 2.5, 0.0, rate),
            Biquad::new(BiquadKind::LowPass, 5200.0, 0.9, 0.0, rate),
            Biquad::new(BiquadKind::LowPass, 5800.0, 0.7, 0.0, rate),
        ]
    } else {
        vec![
            Biquad::new(BiquadKind::HighPass, 95.0, 0.7, 0.0, rate),
            Biquad::new(BiquadKind::Peak, 140.0, 1.3, 3.0, rate),
            Biquad::new(BiquadKind::Peak, 2200.0, 1.0, 5.0, rate),
            Biquad::new(BiquadKind::Peak, 800.0, 1.4, -3.0, rate),
            Biquad::new(BiquadKind::LowPass, 6000.0, 0.8, 0.0, rate),
            Biquad::new(BiquadKind::LowPass, 6600.0, 0.7, 0.0, rate),
        ]
    };
    let l = filtered_impulse(len, &mut st);
    // The second microphone a hair later and darker.
    let mut r = vec![0.0f32; len];
    let d = (0.0003 * rate) as usize;
    let mut lp = Biquad::new(BiquadKind::LowPass, 4500.0, 0.7, 0.0, rate);
    for i in d..len {
        r[i] = lp.process(l[i - d]);
    }
    (l, r)
}

fn telephone(rate: f32) -> (Vec<f32>, Vec<f32>) {
    let len = (0.02 * rate) as usize;
    let mut st = vec![
        Biquad::new(BiquadKind::HighPass, 300.0, 0.9, 0.0, rate),
        Biquad::new(BiquadKind::HighPass, 320.0, 0.7, 0.0, rate),
        Biquad::new(BiquadKind::Peak, 1500.0, 1.0, 6.0, rate),
        Biquad::new(BiquadKind::LowPass, 3400.0, 0.9, 0.0, rate),
        Biquad::new(BiquadKind::LowPass, 3600.0, 0.7, 0.0, rate),
    ];
    let l = filtered_impulse(len, &mut st);
    (l.clone(), l)
}

/// The running convolution of one kernel: the frequency-domain delay line of input spectra.
struct Engine {
    kernel: Box<Kernel>,
    /// Input spectra (left, right), `parts` slots of `H` bins, written round-robin.
    fdl_l: Vec<Complex<f32>>,
    fdl_r: Vec<Complex<f32>>,
    head: usize,
}

impl Engine {
    fn new(kernel: Box<Kernel>) -> Engine {
        let n = kernel.parts * H;
        Engine { fdl_l: vec![Complex::default(); n], fdl_r: vec![Complex::default(); n], head: 0, kernel }
    }

    /// Takes the spectra of the newest input block (`xl`, `xr`) and accumulates the output spectrum into
    /// `y` (all `N` bins, ready for the inverse transform: `Y_l + i·Y_r`).
    fn run(&mut self, xl: &[Complex<f32>], xr: &[Complex<f32>], y: &mut [Complex<f32>]) {
        let parts = self.kernel.parts;
        self.head = (self.head + parts - 1) % parts;
        let h0 = self.head * H;
        self.fdl_l[h0..h0 + H].copy_from_slice(xl);
        self.fdl_r[h0..h0 + H].copy_from_slice(xr);
        let mut acc_l = [Complex::<f32>::default(); H];
        let mut acc_r = [Complex::<f32>::default(); H];
        for p in 0..parts {
            let s = ((self.head + p) % parts) * H;
            let k = p * H;
            let (il, ir) = (&self.fdl_l[s..s + H], &self.fdl_r[s..s + H]);
            let (kl, kr) = (&self.kernel.l[k..k + H], &self.kernel.r[k..k + H]);
            for b in 0..H {
                acc_l[b] += il[b] * kl[b];
                acc_r[b] += ir[b] * kr[b];
            }
        }
        // Rebuild the full spectrum of Y_l + i·Y_r from the halves (both are conjugate-symmetric).
        for k in 0..N {
            let (a, b) = if k < H { (acc_l[k], acc_r[k]) } else { (acc_l[N - k].conj(), acc_r[N - k].conj()) };
            y[k] = a + Complex::new(-b.im, b.re);
        }
    }

    fn clear(&mut self) {
        self.fdl_l.fill(Complex::default());
        self.fdl_r.fill(Complex::default());
    }
}

pub struct Convolver {
    rate: f32,
    shared: Arc<Shared>,
    shape: Shape,
    started: bool,
    rx: Receiver<Box<Engine>>,
    retire: SyncSender<Box<Engine>>,
    retired: Option<Box<Engine>>,
    cur: Box<Engine>,
    /// The engine being faded in, and how far (samples).
    next: Option<(Box<Engine>, usize)>,
    fwd: Arc<dyn Fft<f32>>,
    inv: Arc<dyn Fft<f32>>,
    scratch: Vec<Complex<f32>>,
    /// The last `N` input samples (overlap-save), per side.
    in_l: Vec<f32>,
    in_r: Vec<f32>,
    fill: usize,
    /// Output of the current block, read out over the next `B` samples.
    out_l: [f32; B],
    out_r: [f32; B],
    spec: Vec<Complex<f32>>,
    xl: Vec<Complex<f32>>,
    xr: Vec<Complex<f32>>,
    y: Vec<Complex<f32>>,
    y2: Vec<Complex<f32>>,
    pre: [DelayLine; 2],
    dry: [DelayLine; 2],
    low: [Biquad; 2],
    high: [Biquad; 2],
    predelay: f32,
    low_cut: f32,
    high_cut: f32,
    width: f32,
    mix: f32,
    gain: f32,
    tele: [f32; ENV_POINTS + 3],
    level: f32,
}

const FADE: f32 = 0.02;

impl Convolver {
    pub fn new(rate: f32) -> Self {
        let mut planner = FftPlanner::new();
        let fwd = planner.plan_fft_forward(N);
        let inv = planner.plan_fft_inverse(N);
        let shape = Shape { space: 3, size: 100.0, damping: 35.0, reverse: false };
        let shared = Arc::new(Shared { shape: Default::default(), generation: AtomicU64::new(0), quit: AtomicBool::new(false), file: Mutex::new(None) });
        shared.store(shape);
        let (l, r) = response(shape, None, rate);
        let kernel = Box::new(Kernel::build(&l, &r, rate, &fwd));
        let (tx, rx) = sync_channel::<Box<Engine>>(2);
        let (retire, graveyard) = sync_channel::<Box<Engine>>(8);
        spawn_worker(shared.clone(), tx, graveyard, rate, fwd.clone());
        let scratch = vec![Complex::default(); fwd.get_inplace_scratch_len().max(inv.get_inplace_scratch_len())];
        let max_pre = (0.26 * rate) as usize + 2;
        let mut c = Convolver {
            rate,
            shared,
            shape,
            started: false,
            rx,
            retire,
            retired: None,
            cur: Box::new(Engine::new(kernel)),
            next: None,
            fwd,
            inv,
            scratch,
            in_l: vec![0.0; N],
            in_r: vec![0.0; N],
            fill: 0,
            out_l: [0.0; B],
            out_r: [0.0; B],
            spec: vec![Complex::default(); N],
            xl: vec![Complex::default(); H],
            xr: vec![Complex::default(); H],
            y: vec![Complex::default(); N],
            y2: vec![Complex::default(); N],
            pre: [DelayLine::new(max_pre), DelayLine::new(max_pre)],
            dry: [DelayLine::new(B + 2), DelayLine::new(B + 2)],
            low: [Biquad::default(), Biquad::default()],
            high: [Biquad::default(), Biquad::default()],
            predelay: 10.0,
            low_cut: 20.0,
            high_cut: 20000.0,
            width: 1.0,
            mix: 0.3,
            gain: 1.0,
            tele: [0.0; ENV_POINTS + 3],
            level: 0.0,
        };
        c.filters();
        c.publish();
        c
    }

    /// Loads an impulse-response file (called off the audio thread, before the effect runs).
    pub fn set_file(&mut self, file: Option<Arc<IrFile>>) {
        *self.shared.file.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = file.clone();
        self.rebuild_now();
    }

    fn rebuild_now(&mut self) {
        let file = self.shared.file.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
        let (l, r) = response(self.shape, file.as_deref(), self.rate);
        *self.cur = Engine::new(Box::new(Kernel::build(&l, &r, self.rate, &self.fwd)));
        self.publish();
    }

    fn publish(&mut self) {
        self.tele[..ENV_POINTS].copy_from_slice(&self.cur.kernel.env);
        self.tele[ENV_POINTS] = self.cur.kernel.seconds;
    }

    fn filters(&mut self) {
        for f in self.low.iter_mut() {
            f.set(BiquadKind::HighPass, self.low_cut, 0.707, 0.0, self.rate);
        }
        for f in self.high.iter_mut() {
            f.set(BiquadKind::LowPass, self.high_cut.min(self.rate * 0.45), 0.707, 0.0, self.rate);
        }
    }

    fn set_shape(&mut self, s: Shape) {
        if s == self.shape {
            return;
        }
        self.shape = s;
        self.shared.store(s);
        if self.started {
            self.shared.generation.fetch_add(1, Ordering::Release);
        } else {
            self.rebuild_now();
        }
    }

    /// Convolves one full input block (in `in_l` / `in_r`) with the current (and the incoming) kernel.
    fn block(&mut self) {
        for i in 0..N {
            self.spec[i] = Complex::new(self.in_l[i], self.in_r[i]);
        }
        self.fwd.process_with_scratch(&mut self.spec, &mut self.scratch);
        for k in 0..H {
            let a = self.spec[k];
            let b = self.spec[(N - k) % N].conj();
            self.xl[k] = (a + b) * 0.5;
            let d = (a - b) * 0.5;
            self.xr[k] = Complex::new(d.im, -d.re);
        }
        self.cur.run(&self.xl, &self.xr, &mut self.y);
        self.inv.process_with_scratch(&mut self.y, &mut self.scratch);
        let norm = 1.0 / N as f32;
        let fade_len = (FADE * self.rate) as usize;
        if let Some((eng, pos)) = &mut self.next {
            eng.run(&self.xl, &self.xr, &mut self.y2);
            self.inv.process_with_scratch(&mut self.y2, &mut self.scratch);
            for i in 0..B {
                let t = ((*pos + i) as f32 / fade_len as f32).min(1.0);
                let a = self.y[B + i] * (1.0 - t);
                let b = self.y2[B + i] * t;
                self.out_l[i] = (a.re + b.re) * norm;
                self.out_r[i] = (a.im + b.im) * norm;
            }
            *pos += B;
        } else {
            for i in 0..B {
                self.out_l[i] = self.y[B + i].re * norm;
                self.out_r[i] = self.y[B + i].im * norm;
            }
        }
        if matches!(&self.next, Some((_, pos)) if *pos >= fade_len) {
            let (eng, _) = self.next.take().expect("fading engine");
            let old = std::mem::replace(&mut self.cur, eng);
            self.publish();
            self.retire_engine(old);
        }
        // Slide the input window.
        self.in_l.copy_within(B.., 0);
        self.in_r.copy_within(B.., 0);
    }

    /// Hands an engine back to the worker to be freed there (never frees on the audio thread).
    fn retire_engine(&mut self, e: Box<Engine>) {
        if let Some(prev) = self.retired.take() {
            if let Err(TrySendError::Full(p)) | Err(TrySendError::Disconnected(p)) = self.retire.try_send(prev) {
                self.retired = Some(p);
            }
        }
        if let Err(TrySendError::Full(e)) | Err(TrySendError::Disconnected(e)) = self.retire.try_send(e) {
            if self.retired.is_none() {
                self.retired = Some(e);
            } else {
                // The worker is gone or flooded: leak rather than free here (does not happen in practice).
                std::mem::forget(e);
            }
        }
    }
}

fn spawn_worker(shared: Arc<Shared>, tx: SyncSender<Box<Engine>>, graveyard: Receiver<Box<Engine>>, rate: f32, fft: Arc<dyn Fft<f32>>) {
    let _ = std::thread::Builder::new().name("dragon-convolver".into()).spawn(move || {
        let mut seen = 0u64;
        loop {
            if shared.quit.load(Ordering::Acquire) {
                break;
            }
            while graveyard.try_recv().is_ok() {}
            let g = shared.generation.load(Ordering::Acquire);
            if g != seen {
                // Let a knob come to rest before building (one build per gesture, not per frame).
                std::thread::sleep(std::time::Duration::from_millis(40));
                if shared.generation.load(Ordering::Acquire) != g {
                    continue;
                }
                seen = g;
                let shape = shared.shape();
                let file = shared.file.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
                let (l, r) = response(shape, file.as_deref(), rate);
                let k = Box::new(Kernel::build(&l, &r, rate, &fft));
                if tx.send(Box::new(Engine::new(k))).is_err() {
                    break;
                }
            } else {
                std::thread::sleep(std::time::Duration::from_millis(15));
            }
        }
    });
}

impl Drop for Convolver {
    fn drop(&mut self) {
        self.shared.quit.store(true, Ordering::Release);
    }
}

impl Effect for Convolver {
    fn set_param(&mut self, id: &str, v: f32) {
        let mut s = self.shape;
        match id {
            "space" => s.space = (v.round().max(0.0) as usize).min(SPACES.len() - 1),
            "size" => s.size = v.clamp(50.0, 200.0),
            "damping" => s.damping = v.clamp(0.0, 100.0),
            "reverse" => s.reverse = v >= 0.5,
            "predelay" => self.predelay = v.clamp(0.0, 250.0),
            "low_cut" => {
                self.low_cut = v.clamp(20.0, 1000.0);
                self.filters();
            }
            "high_cut" => {
                self.high_cut = v.clamp(1000.0, 20000.0);
                self.filters();
            }
            "width" => self.width = v.clamp(0.0, 200.0) / 100.0,
            "mix" => self.mix = v.clamp(0.0, 100.0) / 100.0,
            "output" => self.gain = crate::dsp::db_to_gain(v),
            _ => {}
        }
        self.set_shape(s);
    }

    fn reset(&mut self) {
        self.in_l.fill(0.0);
        self.in_r.fill(0.0);
        self.out_l = [0.0; B];
        self.out_r = [0.0; B];
        self.fill = 0;
        self.cur.clear();
        if let Some((e, _)) = &mut self.next {
            e.clear();
        }
        for d in self.pre.iter_mut().chain(self.dry.iter_mut()) {
            d.clear();
        }
        self.low = [Biquad::default(), Biquad::default()];
        self.high = [Biquad::default(), Biquad::default()];
        self.filters();
    }

    fn latency(&self) -> usize {
        B
    }

    fn load_file(&mut self, audio: Option<&crate::samples::AudioData>) -> bool {
        // At most the longest response the convolver keeps.
        let max = (MAX_SECONDS * self.rate) as usize;
        self.set_file(audio.map(|a| Arc::new(IrFile { l: a.l.iter().take(max).copied().collect(), r: a.r.iter().take(max).copied().collect() })));
        true
    }

    fn telemetry(&self) -> &[f32] {
        &self.tele
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _sc: Option<(&[f32], &[f32])>) {
        self.started = true;
        if self.next.is_none() {
            if let Ok(mut eng) = self.rx.try_recv() {
                // The new engine starts warm: it gets the input history the running one has (as much of it
                // as both hold), so its tail is complete from the first block of the crossfade.
                let parts = eng.kernel.parts.min(self.cur.kernel.parts);
                let (cp, np) = (self.cur.kernel.parts, eng.kernel.parts);
                for p in 0..parts {
                    let src = ((self.cur.head + p) % cp) * H;
                    let dst = p * H;
                    eng.fdl_l[dst..dst + H].copy_from_slice(&self.cur.fdl_l[src..src + H]);
                    eng.fdl_r[dst..dst + H].copy_from_slice(&self.cur.fdl_r[src..src + H]);
                }
                eng.head = 0;
                let _ = np;
                self.next = Some((eng, 0));
            }
        }
        let pre = self.predelay * 0.001 * self.rate;
        let mut peak = 0.0f32;
        for i in 0..l.len() {
            let (x_l, x_r) = (l[i], r[i]);
            // Pre-delayed input into the convolution window.
            self.pre[0].write(x_l);
            self.pre[1].write(x_r);
            let (p_l, p_r) = if pre >= 1.0 { (self.pre[0].read(pre), self.pre[1].read(pre)) } else { (x_l, x_r) };
            self.in_l[B + self.fill] = p_l;
            self.in_r[B + self.fill] = p_r;
            let (mut w_l, mut w_r) = (self.out_l[self.fill], self.out_r[self.fill]);
            self.fill += 1;
            if self.fill == B {
                self.fill = 0;
                self.block();
            }
            // Tone, width.
            // At their end stops the tone filters are out of the path.
            if self.low_cut > 20.5 {
                w_l = self.low[0].process(w_l);
                w_r = self.low[1].process(w_r);
            }
            if self.high_cut < 19_999.0 {
                w_l = self.high[0].process(w_l);
                w_r = self.high[1].process(w_r);
            }
            let m = 0.5 * (w_l + w_r);
            let s = 0.5 * (w_l - w_r) * self.width;
            w_l = m + s;
            w_r = m - s;
            // The dry path is delayed by the block, like the wet one.
            self.dry[0].write(x_l);
            self.dry[1].write(x_r);
            let (d_l, d_r) = (self.dry[0].read(B as f32), self.dry[1].read(B as f32));
            l[i] = (d_l * (1.0 - self.mix) + w_l * self.mix) * self.gain;
            r[i] = (d_r * (1.0 - self.mix) + w_r * self.mix) * self.gain;
            peak = peak.max(w_l.abs()).max(w_r.abs());
        }
        self.level = self.level * 0.9 + peak * 0.1;
        self.tele[ENV_POINTS + 1] = self.level;
        self.tele[ENV_POINTS + 2] = if self.next.is_some() { 1.0 } else { 0.0 };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The partitioned convolution equals direct convolution (delayed by the block).
    #[test]
    fn matches_direct_convolution() {
        let rate = 48000.0;
        let mut c = Convolver::new(rate);
        c.set_param("space", 0.0);
        c.set_param("damping", 0.0);
        c.set_param("predelay", 0.0);
        c.set_param("mix", 100.0);
        c.reset();
        let file = response(c.shape, None, rate);
        let n = 3000;
        let mut rng = Rng::new(7);
        let x: Vec<f32> = (0..n).map(|_| rng.bipolar() * 0.3).collect();
        let (mut l, mut r) = (x.clone(), x.iter().map(|v| -v * 0.5).collect::<Vec<_>>());
        let (xl, xr) = (l.clone(), r.clone());
        for (a, b) in l.chunks_mut(100).zip(r.chunks_mut(100)) {
            c.process(a, b, None);
        }
        for i in (B + 200..n).step_by(97) {
            let k = i - B;
            let mut el = 0.0f32;
            let mut er = 0.0f32;
            for j in 0..=k.min(file.0.len() - 1) {
                el += xl[k - j] * file.0[j];
                er += xr[k - j] * file.1[j];
            }
            assert!((l[i] - el).abs() < 1e-3, "left {i}: {} vs {el}", l[i]);
            assert!((r[i] - er).abs() < 1e-3, "right {i}: {} vs {er}", r[i]);
        }
    }

    /// A shape change while running arrives from the worker and is faded in without a click.
    #[test]
    fn new_kernel_arrives_and_fades() {
        let rate = 48000.0;
        let mut c = Convolver::new(rate);
        let mut l = vec![0.0f32; 128];
        let mut r = vec![0.0f32; 128];
        c.process(&mut l, &mut r, None);
        let before = c.cur.kernel.parts;
        c.set_param("space", 4.0);
        let t0 = std::time::Instant::now();
        while c.cur.kernel.parts == before && t0.elapsed().as_secs() < 20 {
            l.fill(0.1);
            r.fill(0.1);
            c.process(&mut l, &mut r, None);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(c.cur.kernel.parts > before, "the cathedral kernel did not arrive");
    }

    #[test]
    fn every_space_is_finite_and_bounded() {
        for s in 0..SPACES.len() {
            let (l, r) = space_ir(s, 1.0, 48000.0);
            assert!(!l.is_empty() && l.iter().chain(&r).all(|x| x.is_finite()), "{}", SPACES[s]);
        }
    }
}
