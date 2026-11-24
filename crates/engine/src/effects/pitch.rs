//! Pitch: YIN pitch detection (shared with audio-to-MIDI), a low-latency granular pitch shifter and the
//! "Aurum Tune" pitch-correction effect (snap a monophonic voice to a key).

use super::{Effect, EffectDef, ParamInfo};

/// YIN fundamental-frequency estimate of `buf` (mono). Returns `(hz, clarity 0 … 1)` when a period between
/// `min_hz` and `max_hz` is found with an aperiodicity below 0.2.
pub fn detect_pitch(buf: &[f32], rate: f32, min_hz: f32, max_hz: f32) -> Option<(f32, f32)> {
    detect_pitch_with(buf, rate, min_hz, max_hz, &mut Vec::new())
}

/// Sum of squared differences of `a` and `b` (equal length), in 8 independent lanes so the compiler
/// vectorises it (a single running sum cannot be reordered, so it would stay scalar).
#[inline]
fn sq_diff(a: &[f32], b: &[f32]) -> f32 {
    let mut acc = [0.0f32; 8];
    let ((ca, ra), (cb, rb)) = (a.as_chunks::<8>(), b.as_chunks::<8>());
    for (x, y) in ca.iter().zip(cb) {
        for k in 0..8 {
            let d = x[k] - y[k];
            acc[k] += d * d;
        }
    }
    let mut s: f32 = acc.iter().sum();
    for (x, y) in ra.iter().zip(rb) {
        let d = x - y;
        s += d * d;
    }
    s
}

/// [`detect_pitch`] with a reusable scratch buffer: no allocation once the scratch has grown (for the
/// real-time effects, which call it every few milliseconds).
pub fn detect_pitch_with(buf: &[f32], rate: f32, min_hz: f32, max_hz: f32, scratch: &mut Vec<f32>) -> Option<(f32, f32)> {
    let max_tau = ((rate / min_hz) as usize).min(buf.len() / 2);
    let min_tau = ((rate / max_hz) as usize).max(2);
    if max_tau <= min_tau + 2 {
        return None;
    }
    let w = buf.len() - max_tau;
    // Skip silence.
    let energy: f32 = buf[..w].iter().map(|x| x * x).sum::<f32>() / w as f32;
    if energy < 1e-6 {
        return None;
    }
    // Difference function, then (in place) the cumulative mean normalised difference.
    scratch.clear();
    scratch.resize(max_tau + 1, 1.0);
    let cmnd = &mut scratch[..];
    for tau in 1..=max_tau {
        cmnd[tau] = sq_diff(&buf[..w], &buf[tau..tau + w]);
    }
    let mut run = 0.0;
    for tau in 1..=max_tau {
        let d = cmnd[tau];
        run += d;
        cmnd[tau] = if run > 0.0 { d * tau as f32 / run } else { 1.0 };
    }
    cmnd[0] = 1.0;
    let cmnd = &*cmnd;
    let threshold = 0.15;
    let mut tau = min_tau;
    let mut found = None;
    while tau < max_tau {
        if cmnd[tau] < threshold {
            while tau + 1 < max_tau && cmnd[tau + 1] < cmnd[tau] {
                tau += 1;
            }
            found = Some(tau);
            break;
        }
        tau += 1;
    }
    let tau = match found {
        Some(t) => t,
        None => {
            // Fall back to the global minimum when it is clear enough.
            let (t, v) = (min_tau..max_tau).map(|t| (t, cmnd[t])).fold((0, f32::MAX), |a, b| if b.1 < a.1 { b } else { a });
            if v > 0.2 {
                return None;
            }
            t
        }
    };
    // Parabolic interpolation for a sub-sample period.
    let better = if tau > 1 && tau < max_tau {
        let (a, b, c) = (cmnd[tau - 1], cmnd[tau], cmnd[tau + 1]);
        let den = a + c - 2.0 * b;
        if den.abs() > 1e-9 {
            tau as f32 + 0.5 * (a - c) / den
        } else {
            tau as f32
        }
    } else {
        tau as f32
    };
    Some((rate / better, (1.0 - cmnd[tau]).clamp(0.0, 1.0)))
}

pub fn hz_to_midi(hz: f32) -> f32 {
    69.0 + 12.0 * (hz / 440.0).log2()
}

/// Granular pitch shifter: two crossfaded read taps moving through a delay line.
pub struct PitchShifter {
    buf: Vec<f32>,
    w: usize,
    phase: f32,
    window: f32,
    /// Window the shifter glides towards (set from the detected period for pitch-synchronous, low-latency
    /// shifting: two periods of the voice instead of a fixed 30 ms grain).
    target_window: f32,
    min_window: f32,
    max_window: f32,
}

impl PitchShifter {
    pub fn new(rate: f32) -> Self {
        let max_window = (rate * 0.03).max(64.0);
        let min_window = (rate * 0.004).max(16.0);
        PitchShifter { buf: vec![0.0; (max_window as usize) * 2 + 8], w: 0, phase: 0.0, window: max_window, target_window: max_window, min_window, max_window }
    }

    /// Sets the grain length from the period (in samples) of the input; `None` returns to the default.
    pub fn set_period(&mut self, period: Option<f32>) {
        self.target_window = match period {
            Some(p) => (p * 2.0).clamp(self.min_window, self.max_window),
            None => self.max_window * 0.6,
        };
    }

    /// The average delay through the shifter in samples (half a grain).
    pub fn delay(&self) -> f32 {
        self.window * 0.5
    }

    pub fn reset(&mut self) {
        self.buf.fill(0.0);
        self.phase = 0.0;
    }

    fn read(&self, delay: f32) -> f32 {
        let n = self.buf.len();
        let pos = self.w as f32 - delay;
        let pos = pos.rem_euclid(n as f32);
        let i = pos.floor() as usize;
        let f = pos - i as f32;
        self.buf[i % n] * (1.0 - f) + self.buf[(i + 1) % n] * f
    }

    /// Processes one sample at the pitch `ratio` (2.0 = an octave up).
    #[inline]
    pub fn process(&mut self, x: f32, ratio: f32) -> f32 {
        let n = self.buf.len();
        self.buf[self.w] = x;
        self.window += (self.target_window - self.window) * 0.002;
        self.phase = (self.phase + (1.0 - ratio) / self.window).rem_euclid(1.0);
        let p2 = (self.phase + 0.5).rem_euclid(1.0);
        let g1 = (std::f32::consts::PI * self.phase).sin();
        let g2 = (std::f32::consts::PI * p2).sin();
        let y = self.read(self.phase * self.window + 1.0) * g1 + self.read(p2 * self.window + 1.0) * g2;
        self.w = (self.w + 1) % n;
        // sin² + cos² crossfade keeps the level close to constant.
        y * 0.8
    }
}

/// Pitch-synchronous overlap-add (TD-PSOLA) shifter – the clean,
/// formant-preserving sound (the "hard tune" effect at retune speed 0). Grains two periods long are
/// cut around pitch marks one period apart (aligned to the waveform's peaks) and laid down at the new
/// period; the voice's formants (the grain shapes) stay where they are, only the pitch moves.
/// The delay is about one period plus 1 ms (≈ 5 ms on a 220 Hz voice), so it can be monitored.
pub struct Psola {
    buf: Vec<f32>,
    acc: Vec<f32>,
    /// Samples written (absolute time of the next input sample).
    w: u64,
    /// Absolute time of the next output sample.
    e: u64,
    /// Analysis marks (absolute time, grain half length).
    marks: std::collections::VecDeque<(u64, f32)>,
    last_mark: u64,
    period: f32,
    voiced: bool,
    /// Absolute time of the next synthesis mark.
    next_syn: f64,
    delay: u64,
    started: bool,
    /// The output's recent peak (≈ 7 ms decay at 48 kHz): "near silence" is judged on it, not on one sample
    /// (a zero crossing of a tone that is still sounding is not silence).
    lvl: f32,
}

const PS_N: usize = 1 << 14;
const PS_UNVOICED: f32 = 256.0;
const PS_MARGIN: f32 = 48.0;

impl Psola {
    pub fn new() -> Self {
        Psola {
            buf: vec![0.0; PS_N],
            acc: vec![0.0; PS_N],
            w: 0,
            e: 0,
            marks: std::collections::VecDeque::with_capacity(64),
            last_mark: 0,
            period: PS_UNVOICED,
            voiced: false,
            next_syn: 0.0,
            delay: (PS_UNVOICED + PS_MARGIN) as u64,
            started: false,
            lvl: 0.0,
        }
    }

    /// The voice's period in samples (`None` = unvoiced: grains at a fixed size, no pitch change).
    pub fn set_period(&mut self, period: Option<f32>) {
        match period {
            Some(p) => {
                self.period = p.clamp(32.0, 1200.0);
                self.voiced = true;
            }
            None => {
                self.period = PS_UNVOICED;
                self.voiced = false;
            }
        }
    }

    /// Current delay through the shifter, in samples.
    pub fn delay(&self) -> f32 {
        self.delay as f32
    }

    pub fn reset(&mut self) {
        let fresh = Psola::new();
        *self = fresh;
    }

    #[inline]
    fn at(&self, t: u64) -> f32 {
        self.buf[(t as usize) & (PS_N - 1)]
    }

    /// One sample in, one out, at pitch `ratio` (2.0 = an octave up).
    pub fn process(&mut self, x: f32, ratio: f32) -> f32 {
        let mask = PS_N - 1;
        self.buf[(self.w as usize) & mask] = x;
        self.w += 1;
        let p = self.period;
        let ip = p.round().max(1.0) as u64;
        // ---- analysis marks, one period apart, snapped to the waveform's peak (consistent grain shapes)
        if self.marks.is_empty() {
            if self.w > ip * 2 {
                self.last_mark = self.w - ip - 1;
                self.marks.push_back((self.last_mark, p));
            }
        } else {
            let search = if self.voiced { ip / 4 } else { 0 };
            while self.last_mark + ip + search + ip < self.w {
                let cand = self.last_mark + ip;
                let mut best = cand;
                if search > 0 {
                    let mut bv = f32::MIN;
                    for t in cand - search..=cand + search {
                        let v = self.at(t);
                        if v > bv {
                            bv = v;
                            best = t;
                        }
                    }
                }
                self.last_mark = best.max(self.last_mark + ip / 2);
                self.marks.push_back((self.last_mark, p));
                while self.marks.len() > 48 {
                    self.marks.pop_front();
                }
            }
        }
        // ---- delay: about one period plus a margin, glided by a sample now and then
        let want = (p + PS_MARGIN) as u64;
        if !self.started {
            if self.w <= want || self.marks.is_empty() {
                return 0.0;
            }
            self.started = true;
            self.delay = want;
            self.e = self.w - want;
            self.next_syn = self.e as f64;
        }
        // ---- synthesis: lay a grain at each output mark (period / ratio apart) before it is played
        let out_p = (p / ratio.clamp(0.25, 4.0)).max(16.0) as f64;
        let lag = 1.25 * p as f64;
        while self.next_syn <= (self.e as f64) + p as f64 + 1.0 {
            let s = self.next_syn;
            let target = s - lag;
            let (m, half) = self.marks.iter().copied().min_by(|a, b| (a.0 as f64 - target).abs().total_cmp(&(b.0 as f64 - target).abs())).unwrap_or((self.last_mark, p));
            let hp = half.max(8.0);
            let hi = hp as i64;
            let norm = (out_p as f32 / hp).min(1.0);
            let sc = s.round() as i64;
            for k in -hi..hi {
                let ot = sc + k;
                if ot < self.e as i64 {
                    continue;
                }
                let it = m as i64 + k;
                if it < 0 || it as u64 >= self.w {
                    continue;
                }
                let wv = 0.5 + 0.5 * (std::f32::consts::PI * k as f32 / hp).cos();
                self.acc[(ot as usize) & mask] += self.buf[(it as usize) & mask] * wv * norm;
            }
            self.next_syn += out_p;
        }
        // ---- emit
        let idx = (self.e as usize) & mask;
        let y = self.acc[idx];
        self.acc[idx] = 0.0;
        self.e += 1;
        // Glide the delay towards the period's: a skipped or repeated sample, every 16 samples at most and
        // only where it cannot be heard – between notes (unvoiced) or near silence – unless the delay is
        // far too short for the voice (it went down a lot), when grains would be cut.
        self.lvl = y.abs().max(self.lvl * 0.9969);
        let quiet = !self.voiced || self.lvl < 3e-4;
        let far = (self.w - self.e) as f32 + 0.5 * p < want as f32;
        if self.w.is_multiple_of(16) && (quiet || far) {
            let cur = self.w - self.e;
            if want + 24 < cur {
                let i2 = (self.e as usize) & mask;
                self.acc[i2] = 0.5 * (self.acc[i2] + y);
                self.e += 1;
            } else if want > cur + 24 && self.e > 0 {
                self.e -= 1;
                self.acc[(self.e as usize) & mask] = y;
            }
            self.delay = self.w - self.e;
        }
        y
    }
}

impl Default for Psola {
    fn default() -> Self {
        Self::new()
    }
}

pub const KEYS: &[&str] = &["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
pub const TUNE_SCALES: &[(&str, &[u8])] = &[
    ("Chromatic", &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]),
    ("Major", &[0, 2, 4, 5, 7, 9, 11]),
    ("Minor", &[0, 2, 3, 5, 7, 8, 10]),
    ("Major pentatonic", &[0, 2, 4, 7, 9]),
    ("Minor pentatonic", &[0, 3, 5, 7, 10]),
    ("Harmonic minor", &[0, 2, 3, 5, 7, 8, 11]),
];
const SCALE_NAMES: &[&str] = &["Chromatic", "Major", "Minor", "Major pentatonic", "Minor pentatonic", "Harmonic minor"];

/// Nearest note (fractional MIDI → integer MIDI) allowed by `root` + `scale`.
pub fn nearest_in_scale(midi: f32, root: u8, scale: &[u8]) -> f32 {
    let base = midi.round() as i32;
    let mut best = base as f32;
    let mut dist = f32::MAX;
    for cand in base - 6..=base + 6 {
        let pc = (cand - root as i32).rem_euclid(12) as u8;
        if scale.contains(&pc) {
            let d = (cand as f32 - midi).abs();
            if d < dist {
                dist = d;
                best = cand as f32;
            }
        }
    }
    best
}

const TUNE_PARAMS: &[ParamInfo] = &[
    ParamInfo::choice("key", "Key", KEYS, 0.0),
    ParamInfo::choice("scale", "Scale", SCALE_NAMES, 0.0),
    // 0 … 400 ms: the bottom is the instant, robotic "hard tune".
    ParamInfo::log("speed", "Retune speed", 0.1, 400.0, 20.0, "ms"),
    ParamInfo::lin("humanize", "Humanize", 0.0, 1.0, 0.0, ""),
    ParamInfo::lin("flex", "Flex-tune", 0.0, 50.0, 0.0, "ct"),
    ParamInfo::lin("vibrato", "Natural vibrato", 0.0, 1.0, 0.0, ""),
    ParamInfo::lin("amount", "Amount", 0.0, 1.0, 1.0, ""),
    ParamInfo::lin("mix", "Mix", 0.0, 1.0, 1.0, ""),
    // Note stabilizer: a held note is kept until the voice moves this far past the midpoint to
    // the next note (no flutter between two notes).
    ParamInfo::lin("stabilizer", "Note stabilizer", 0.0, 100.0, 0.0, "ct"),
    // Doubler: two tuned copies, slightly delayed and detuned, panned left and right.
    ParamInfo::lin("doubler", "Doubler", 0.0, 1.0, 0.0, ""),
];

/// Analysis every 128 samples (2.7 ms at 48 kHz) over 1024 samples (enough for voices down to ~90 Hz).
const HOP: usize = 128;
const FRAME: usize = 1024;

pub struct PitchCorrect {
    rate: f32,
    key: u8,
    scale: usize,
    speed_ms: f32,
    amount: f32,
    mix: f32,
    ring: Vec<f32>,
    pos: usize,
    analysis: Vec<f32>,
    /// Scratch for the pitch detector (no allocation in the audio thread).
    yin: Vec<f32>,
    fill: usize,
    since: usize,
    target_ratio: f32,
    ratio: f32,
    sl: Psola,
    sr: Psola,
    humanize: f32,
    flex: f32,
    vibrato: f32,
    /// Note the voice is held on and for how many analysis hops (humanize slows retuning on held notes).
    held: (f32, u32),
    /// Smoothed pitch (slow) to separate vibrato from the note's centre.
    centre: f32,
    /// Last detected pitch (MIDI) for displays and tests.
    pub detected: Option<f32>,
    /// [detected MIDI (−1 = none), target MIDI, output MIDI, clarity] for the display.
    tele: [f32; 4],
    stabilizer: f32,
    doubler: f32,
    /// Doubler: delay lines of the tuned signal and the modulation phase.
    dbl: [Vec<f32>; 2],
    dbl_pos: usize,
    dbl_phase: f32,
}

impl PitchCorrect {
    pub fn new(rate: f32) -> Self {
        PitchCorrect {
            rate,
            key: 0,
            scale: 0,
            speed_ms: 20.0,
            amount: 1.0,
            mix: 1.0,
            ring: vec![0.0; FRAME],
            pos: 0,
            analysis: vec![0.0; FRAME],
            yin: Vec::with_capacity(FRAME),
            fill: 0,
            since: 0,
            target_ratio: 1.0,
            ratio: 1.0,
            sl: Psola::new(),
            sr: Psola::new(),
            humanize: 0.0,
            flex: 0.0,
            vibrato: 0.0,
            held: (0.0, 0),
            centre: 0.0,
            detected: None,
            tele: [-1.0, -1.0, -1.0, 0.0],
            stabilizer: 0.0,
            doubler: 0.0,
            dbl: [vec![0.0; (rate * 0.05) as usize + 4], vec![0.0; (rate * 0.05) as usize + 4]],
            dbl_pos: 0,
            dbl_phase: 0.0,
        }
    }

    fn analyse(&mut self) {
        // Unroll the ring, oldest sample first.
        let (a, b) = self.ring.split_at(self.pos);
        self.analysis[..b.len()].copy_from_slice(b);
        self.analysis[b.len()..].copy_from_slice(a);
        match detect_pitch_with(&self.analysis, self.rate, 85.0, 1100.0, &mut self.yin) {
            Some((hz, clarity)) if clarity > 0.8 => {
                let m = hz_to_midi(hz);
                self.detected = Some(m);
                // The slow centre follows the note; the difference is vibrato, kept when asked to.
                if (m - self.centre).abs() > 1.0 {
                    self.centre = m;
                } else {
                    self.centre += (m - self.centre) * 0.08;
                }
                let mut target = nearest_in_scale(self.centre, self.key, TUNE_SCALES[self.scale].1);
                // Note stabilizer: stay on the held note until the voice is clearly on another.
                if self.stabilizer > 0.0 && self.held.0 > 0.0 && (target - self.held.0).abs() > 0.01 && (self.centre - self.held.0).abs() * 100.0 < 50.0 + self.stabilizer {
                    target = self.held.0;
                }
                let wobble = (m - self.centre) * self.vibrato;
                let off_cents = (target - m).abs() * 100.0;
                // Flex-tune: notes already within the window are left alone.
                let semis = if self.flex > 0.0 && off_cents <= self.flex { 0.0 } else { (target - m + wobble) * self.amount };
                self.target_ratio = 2f32.powf(semis / 12.0);
                if (target - self.held.0).abs() < 0.01 {
                    self.held.1 = self.held.1.saturating_add(1);
                } else {
                    self.held = (target, 0);
                }
                let period = self.rate / hz;
                self.sl.set_period(Some(period));
                self.sr.set_period(Some(period));
                self.tele = [m, target, m + semis, clarity];
            }
            _ => {
                self.detected = None;
                self.target_ratio = 1.0;
                self.held = (0.0, 0);
                self.sl.set_period(None);
                self.sr.set_period(None);
                self.tele = [-1.0, -1.0, -1.0, 0.0];
            }
        }
    }
}

impl Effect for PitchCorrect {
    fn set_param(&mut self, id: &str, v: f32) {
        match id {
            "key" => self.key = (v.round() as u8).min(11),
            "scale" => self.scale = (v.round() as usize).min(TUNE_SCALES.len() - 1),
            "speed" => self.speed_ms = v.max(0.0),
            "amount" => self.amount = v.clamp(0.0, 1.0),
            "mix" => self.mix = v.clamp(0.0, 1.0),
            "humanize" => self.humanize = v.clamp(0.0, 1.0),
            "flex" => self.flex = v.clamp(0.0, 50.0),
            "vibrato" => self.vibrato = v.clamp(0.0, 1.0),
            "stabilizer" => self.stabilizer = v.clamp(0.0, 100.0),
            "doubler" => self.doubler = v.clamp(0.0, 1.0),
            _ => {}
        }
    }
    fn telemetry(&self) -> &[f32] {
        &self.tele
    }
    fn reset(&mut self) {
        self.sl.reset();
        self.sr.reset();
        self.ring.fill(0.0);
        self.fill = 0;
        self.ratio = 1.0;
        self.target_ratio = 1.0;
        self.dbl.iter_mut().for_each(|d| d.fill(0.0));
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _sc: Option<(&[f32], &[f32])>) {
        // Humanize: notes held longer than ~120 ms are retuned more slowly (up to 8× the speed setting).
        let held_s = self.held.1 as f32 * HOP as f32 / self.rate;
        let slow = 1.0 + self.humanize * 7.0 * ((held_s - 0.12) / 0.3).clamp(0.0, 1.0);
        // Retune speed: below ~0.5 ms the pitch snaps at once (hard tune).
        let tau = self.speed_ms * slow * 0.001 * self.rate;
        let coef = if self.speed_ms < 0.5 { 0.0 } else { (-1.0 / tau.max(1e-3)).exp() };
        for i in 0..l.len() {
            // Analysis ring (most recent FRAME samples, oldest first).
            self.ring[self.pos] = (l[i] + r[i]) * 0.5;
            self.pos = (self.pos + 1) % FRAME;
            self.fill = (self.fill + 1).min(FRAME);
            self.since += 1;
            if self.since >= HOP && self.fill == FRAME {
                self.since = 0;
                self.analyse();
            }
            self.ratio = self.target_ratio + (self.ratio - self.target_ratio) * coef;
            let (dl, dr) = (l[i], r[i]);
            let mut wl = self.sl.process(dl, self.ratio);
            let mut wr = self.sr.process(dr, self.ratio);
            if self.doubler > 0.0 {
                // Two copies of the tuned voice: 14 and 23 ms behind, their delay swaying slowly (a few
                // cents of detune), one on each side.
                let n = self.dbl[0].len();
                self.dbl[0][self.dbl_pos] = wl;
                self.dbl[1][self.dbl_pos] = wr;
                self.dbl_phase = (self.dbl_phase + 0.37 / self.rate).fract();
                let sway = (self.dbl_phase * std::f32::consts::TAU).sin();
                let tap = |buf: &[f32], ms: f32| {
                    let d = (ms * 0.001 * self.rate).min(n as f32 - 2.0);
                    let p = (self.dbl_pos as f32 - d).rem_euclid(n as f32);
                    // `rem_euclid` can return exactly `n` for a value just below 0 (rounding): wrap the index.
                    let (k, f) = (p.floor() as usize % n, p.fract());
                    buf[k] * (1.0 - f) + buf[(k + 1) % n] * f
                };
                let a = tap(&self.dbl[0], 14.0 + 1.5 * sway);
                let b = tap(&self.dbl[1], 23.0 - 1.5 * sway);
                self.dbl_pos = (self.dbl_pos + 1) % n;
                let g = self.doubler * 0.7;
                wl += a * g;
                wr += b * g;
            }
            l[i] = dl * (1.0 - self.mix) + wl * self.mix;
            r[i] = dr * (1.0 - self.mix) + wr * self.mix;
        }
    }
}

pub fn pitch_correct_def() -> EffectDef {
    EffectDef {
        id: "tune",
        name: "Aurum Tune (pitch correction)",
        category: "Pitch",
        description: "Real-time, zero-latency pitch correction for tracking: snaps a monophonic voice to a key (retune speed, humanize, flex-tune, natural vibrato). Put it on the track's insert and arm the track to sing through it.",
        params: TUNE_PARAMS,
        uses_sidechain: false,
        create: |rate| Box::new(PitchCorrect::new(rate)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(hz: f32, secs: f32) -> Vec<f32> {
        (0..(48000.0 * secs) as usize).map(|i| (std::f32::consts::TAU * hz * i as f32 / 48000.0).sin() * 0.5).collect()
    }

    #[test]
    fn yin_finds_the_fundamental() {
        for hz in [82.4f32, 220.0, 440.0, 659.3] {
            let (f, c) = detect_pitch(&sine(hz, 0.05), 48000.0, 60.0, 1100.0).unwrap();
            assert!((f - hz).abs() / hz < 0.005, "{hz}: {f}");
            assert!(c > 0.9);
        }
        assert!(detect_pitch(&vec![0.0; 2048], 48000.0, 60.0, 1000.0).is_none());
    }

    /// PSOLA moves the pitch by the ratio and keeps a steady level (no grain chorus / tremolo).
    #[test]
    fn psola_shifts_cleanly() {
        for (hz, ratio) in [(220.0f32, 1.0f32), (220.0, 1.12), (300.0, 0.9), (150.0, 1.5)] {
            let mut s = Psola::new();
            s.set_period(Some(48000.0 / hz));
            let x = sine(hz, 1.0);
            let y: Vec<f32> = x.iter().map(|v| s.process(*v, ratio)).collect();
            let (f, c) = detect_pitch(&y[30000..32048], 48000.0, 60.0, 1100.0).unwrap();
            assert!((f - hz * ratio).abs() < hz * ratio * 0.01, "{hz}×{ratio}: {f}");
            assert!(c > 0.9, "clean: {c}");
            // Level steady across 10 ms windows (a granular shifter wobbles here).
            let rms: Vec<f32> = y[24000..48000].chunks(480).map(|w| (w.iter().map(|v| v * v).sum::<f32>() / 480.0).sqrt()).collect();
            let (lo, hi) = rms.iter().fold((f32::MAX, 0.0f32), |(l, h), v| (l.min(*v), h.max(*v)));
            assert!(hi / lo < 1.25, "{hz}×{ratio}: level {lo:.3} … {hi:.3}");
            // A pure tone's "formant" is the tone itself, so big shifts lose level (as PSOLA does on any
            // sine); within the few semitones of tuning the level holds.
            if (ratio - 1.0).abs() < 0.15 {
                assert!((hi - 0.354).abs() < 0.1, "{hz}×{ratio}: level {hi}");
            }
        }
    }

    /// Retune speed 0 makes a gliding voice stair-step onto the notes (the hard-tune effect).
    /// While a note is sung the delay is not touched (no skipped / repeated samples inside it): the
    /// output stays a clean, click-free tone.
    #[test]
    fn delay_changes_wait_for_quiet_moments() {
        let mut s = Psola::new();
        s.set_period(Some(48000.0 / 300.0));
        let x = sine(300.0, 1.0);
        let mut y = Vec::new();
        let mut d_mid = 0.0;
        for (i, v) in x.iter().enumerate() {
            // Mid-note the detector reports a lower voice: the wanted delay grows.
            if i == 12000 {
                s.set_period(Some(48000.0 / 250.0));
                d_mid = s.delay();
            }
            y.push(s.process(*v, 1.0));
        }
        assert_eq!(s.delay(), d_mid, "the delay moved while the note was sung");
        // No sample-to-sample jump beyond what a 300 Hz sine at this level can make.
        let max_step = y[14000..47000].windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
        assert!(max_step < 0.5 * 2.0 * std::f32::consts::PI * 300.0 / 48000.0 * 1.6, "a click: {max_step}");
        // Once the voice stops, the delay follows.
        for _ in 0..4800 {
            s.process(0.0, 1.0);
        }
        assert_ne!(s.delay(), d_mid, "the delay adjusts in the silence");
    }

    #[test]
    fn hard_tune_snaps_a_glide() {
        let mut e = PitchCorrect::new(48000.0);
        e.set_param("speed", 0.1);
        let mut ph = 0.0f32;
        let x: Vec<f32> = (0..96000)
            .map(|i| {
                let m = 57.0 + 2.0 * i as f32 / 96000.0;
                ph += 440.0 * 2f32.powf((m - 69.0) / 12.0) / 48000.0;
                (ph * std::f32::consts::TAU).sin() * 0.5
            })
            .collect();
        let (mut l, mut r) = (x.clone(), x);
        for (a, b) in l.chunks_mut(128).zip(r.chunks_mut(128)) {
            e.process(a, b, None);
        }
        // Inside each note the output pitch sits on it.
        // The glide is at 57.4 around sample 20000 (→ A3) and at 58.85 around 88000 (→ B3).
        for (at, want) in [(20000usize, 220.0f32), (86000, 246.94)] {
            let (f, _) = detect_pitch(&l[at..at + 2048], 48000.0, 60.0, 1100.0).unwrap();
            assert!((f - want).abs() / want < 0.012, "at {at}: {f} (want {want})");
        }
    }

    #[test]
    fn scale_snapping() {
        // 61.4 (between C#4 and D4) snaps to D in C major and to C# chromatically.
        assert_eq!(nearest_in_scale(61.4, 0, TUNE_SCALES[0].1), 61.0);
        assert_eq!(nearest_in_scale(61.4, 0, TUNE_SCALES[1].1), 62.0);
    }

    #[test]
    fn shifter_moves_pitch_by_the_ratio() {
        let mut s = PitchShifter::new(48000.0);
        let x = sine(300.0, 1.0);
        let y: Vec<f32> = x.iter().map(|v| s.process(*v, 1.5)).collect();
        let (f, _) = detect_pitch(&y[30000..32048], 48000.0, 60.0, 1100.0).unwrap();
        assert!((f - 450.0).abs() < 6.0, "{f}");
    }

    /// Aurum Tune reports no latency and its shifter delay stays short (a few ms) on a voice, so it can be
    /// monitored while recording.
    #[test]
    fn tune_is_low_latency() {
        let mut e = PitchCorrect::new(48000.0);
        assert_eq!(e.latency(), 0);
        let x = sine(220.0, 0.5);
        let (mut l, mut r) = (x.clone(), x);
        for (a, b) in l.chunks_mut(64).zip(r.chunks_mut(64)) {
            e.process(a, b, None);
        }
        let ms = e.sl.delay() / 48.0;
        assert!(ms < 6.5, "PSOLA delay {ms:.1} ms");
        let t = e.telemetry();
        assert!((t[0] - 57.0).abs() < 0.1, "detected A3: {t:?}");
    }

    /// The Note Stabilizer holds a note when the voice wavers just past the midpoint; the Doubler
    /// widens a mono voice (the two sides differ).
    #[test]
    fn stabilizer_and_doubler() {
        // C4 held, then the voice drifts to 60.6 (past the C/C# midpoint, chromatic scale).
        let run = |stab: f32| {
            let mut e = PitchCorrect::new(48000.0);
            e.set_param("stabilizer", stab);
            let hz = |m: f32| 440.0 * 2f32.powf((m - 69.0) / 12.0);
            let mut ph = 0.0f32;
            let mut sig = |m: f32, n: usize| -> Vec<f32> {
                (0..n)
                    .map(|_| {
                        ph += hz(m) / 48000.0;
                        (ph * std::f32::consts::TAU).sin() * 0.5
                    })
                    .collect()
            };
            let mut x = sig(60.0, 24000);
            x.extend(sig(60.6, 24000));
            let (mut l, mut r) = (x.clone(), x);
            for (a, b) in l.chunks_mut(64).zip(r.chunks_mut(64)) {
                e.process(a, b, None);
            }
            e.telemetry()[1]
        };
        assert_eq!(run(0.0), 61.0, "without the stabilizer it jumps to C#");
        assert_eq!(run(30.0), 60.0, "the stabilizer keeps C");
        let mut e = PitchCorrect::new(48000.0);
        e.set_param("doubler", 1.0);
        let x = sine(220.0, 1.0);
        let (mut l, mut r) = (x.clone(), x);
        for (a, b) in l.chunks_mut(64).zip(r.chunks_mut(64)) {
            e.process(a, b, None);
        }
        let diff: f32 = l[24000..].iter().zip(&r[24000..]).map(|(a, b)| (a - b).abs()).sum::<f32>() / 24000.0;
        assert!(diff > 0.02, "doubled sides differ: {diff}");
    }

    /// Flex-tune leaves notes that are already close enough alone.
    #[test]
    fn flex_tune_leaves_close_notes() {
        let mut e = PitchCorrect::new(48000.0);
        e.set_param("flex", 30.0);
        e.set_param("speed", 5.0);
        // 446 Hz is ~23 cents sharp of A4: inside the 30-cent window, so it stays.
        let x = sine(446.0, 1.0);
        let (mut l, mut r) = (x.clone(), x);
        for (a, b) in l.chunks_mut(128).zip(r.chunks_mut(128)) {
            e.process(a, b, None);
        }
        let (f, _) = detect_pitch(&l[36000..38048], 48000.0, 60.0, 1100.0).unwrap();
        assert!((f - 446.0).abs() < 2.5, "left at {f} Hz");
    }

    #[test]
    fn tune_pulls_a_sharp_note_to_the_scale() {
        // 452 Hz is ~47 cents sharp of A4 (440 Hz): chromatic correction brings it to 440.
        let mut e = PitchCorrect::new(48000.0);
        e.set_param("speed", 5.0);
        let x = sine(452.0, 1.0);
        let (mut l, mut r) = (x.clone(), x);
        for (a, b) in l.chunks_mut(128).zip(r.chunks_mut(128)) {
            e.process(a, b, None);
        }
        let (f, _) = detect_pitch(&l[36000..38048], 48000.0, 60.0, 1100.0).unwrap();
        assert!((f - 440.0).abs() < 3.0, "corrected to {f} Hz");
    }
}
