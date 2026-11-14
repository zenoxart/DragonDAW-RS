//! Algorithmic reverb: pre-delay → early reflections → 8-line feedback delay network with a Householder
//! matrix, RT60-based feedback, damping and modulated lines; width and dry/wet.

use super::{Effect, EffectDef, ParamInfo};
use crate::dsp::{DelayLine, Lfo, LfoShape, OnePole};

const LINES: usize = 8;
const BASE_MS: [f32; LINES] = [29.7, 37.1, 41.1, 43.7, 53.3, 59.9, 67.1, 73.3];
const EARLY_MS: [(f32, f32); 6] = [(7.0, 0.8), (11.3, 0.65), (17.9, 0.5), (23.1, 0.42), (31.4, 0.33), (41.7, 0.25)];

const PARAMS: &[ParamInfo] = &[
    ParamInfo::choice("mode", "Mode", &["Room", "Chamber", "Hall", "Plate", "Cathedral"], 2.0),
    ParamInfo::lin("size", "Size", 0.1, 1.0, 0.6, ""),
    ParamInfo::log("decay", "Decay", 0.1, 20.0, 2.2, "s"),
    ParamInfo::log("predelay", "Pre-delay", 0.1, 200.0, 12.0, "ms"),
    ParamInfo::log("damping", "Damping", 1000.0, 20000.0, 7000.0, "Hz"),
    ParamInfo::log("lowcut", "Low cut", 20.0, 1000.0, 120.0, "Hz"),
    ParamInfo::lin("early", "Early level", 0.0, 1.0, 0.4, ""),
    ParamInfo::lin("mod", "Modulation", 0.0, 1.0, 0.3, ""),
    ParamInfo::lin("width", "Width", 0.0, 1.5, 1.0, ""),
    ParamInfo::lin("mix", "Mix", 0.0, 1.0, 0.25, ""),
];

pub struct Reverb {
    rate: f32,
    pre: [DelayLine; 2],
    early: [DelayLine; 2],
    lines: Vec<DelayLine>,
    damp: [OnePole; LINES],
    lowcut: [OnePole; 2],
    lfos: Vec<Lfo>,
    lengths: [f32; LINES],
    gains: [f32; LINES],
    size: f32,
    decay: f32,
    predelay: f32,
    damping: f32,
    lowcut_hz: f32,
    early_level: f32,
    modulation: f32,
    width: f32,
    mix: f32,
    mode: usize,
}

impl Reverb {
    fn new(rate: f32) -> Self {
        let max = (rate * 0.2) as usize;
        Reverb {
            rate,
            pre: [DelayLine::new((rate * 0.25) as usize), DelayLine::new((rate * 0.25) as usize)],
            early: [DelayLine::new((rate * 0.06) as usize), DelayLine::new((rate * 0.06) as usize)],
            lines: (0..LINES).map(|_| DelayLine::new(max)).collect(),
            damp: [OnePole::new(7000.0, rate); LINES],
            lowcut: [OnePole::new(120.0, rate); 2],
            lfos: (0..LINES).map(|i| Lfo::with_phase(i as f32 / LINES as f32)).collect(),
            lengths: [0.0; LINES],
            gains: [0.0; LINES],
            size: 0.6,
            decay: 2.2,
            predelay: 12.0,
            damping: 7000.0,
            lowcut_hz: 120.0,
            early_level: 0.4,
            modulation: 0.3,
            width: 1.0,
            mix: 0.25,
            mode: 2,
        }
    }

    fn update(&mut self) {
        // Mode scales size and colour: plate is dense and bright, cathedral huge and dark.
        let (size_mul, bright) = match self.mode {
            0 => (0.45, 1.0),
            1 => (0.7, 0.9),
            3 => (0.6, 1.3),
            4 => (1.6, 0.75),
            _ => (1.0, 1.0),
        };
        for i in 0..LINES {
            let len = BASE_MS[i] * 0.001 * self.rate * (0.3 + self.size * 1.7) * size_mul;
            self.lengths[i] = len.min(self.lines[i].len() as f32 - 8.0);
            // g = 10^(−3·len/(RT60·rate)) gives RT60 decay.
            self.gains[i] = 10f32.powf(-3.0 * self.lengths[i] / (self.decay.max(0.05) * self.rate));
            self.damp[i].set((self.damping * bright).min(self.rate * 0.45), self.rate);
        }
        for f in &mut self.lowcut {
            f.set(self.lowcut_hz, self.rate);
        }
    }
}

impl Effect for Reverb {
    fn set_param(&mut self, id: &str, v: f32) {
        match id {
            "mode" => self.mode = v.round().clamp(0.0, 4.0) as usize,
            "size" => self.size = v,
            "decay" => self.decay = v,
            "predelay" => self.predelay = v,
            "damping" => self.damping = v,
            "lowcut" => self.lowcut_hz = v,
            "early" => self.early_level = v,
            "mod" => self.modulation = v,
            "width" => self.width = v,
            "mix" => self.mix = v,
            _ => return,
        }
        self.update();
    }

    fn reset(&mut self) {
        self.pre.iter_mut().chain(self.early.iter_mut()).chain(self.lines.iter_mut()).for_each(DelayLine::clear);
        self.damp.iter_mut().chain(self.lowcut.iter_mut()).for_each(OnePole::reset);
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _sc: Option<(&[f32], &[f32])>) {
        let pre = self.predelay * 0.001 * self.rate;
        let mod_depth = self.modulation * 0.0015 * self.rate;
        for i in 0..l.len() {
            let (dl, dr) = (l[i], r[i]);
            self.pre[0].write(self.lowcut[0].highpass(dl));
            self.pre[1].write(self.lowcut[1].highpass(dr));
            let (pl, pr) = (self.pre[0].read(pre), self.pre[1].read(pre));
            self.early[0].write(pl);
            self.early[1].write(pr);
            let mut el = 0.0;
            let mut er = 0.0;
            for (k, (ms, g)) in EARLY_MS.iter().enumerate() {
                let d = ms * 0.001 * self.rate * (0.5 + self.size);
                if k % 2 == 0 {
                    el += self.early[0].read(d) * g;
                    er += self.early[1].read(d * 1.07) * g;
                } else {
                    el += self.early[1].read(d) * g * 0.7;
                    er += self.early[0].read(d * 0.93) * g * 0.7;
                }
            }
            // Read the network.
            let mut outs = [0.0f32; LINES];
            for k in 0..LINES {
                let m = self.lfos[k].next(0.3 + k as f32 * 0.07, self.rate, LfoShape::Sine) * mod_depth;
                outs[k] = self.damp[k].process(self.lines[k].read(self.lengths[k] + m)) * self.gains[k];
            }
            // Householder mix: y = x − 2/N·Σx.
            let sum: f32 = outs.iter().sum::<f32>() * (2.0 / LINES as f32);
            let input = [pl, pr];
            for k in 0..LINES {
                let fb = outs[k] - sum;
                self.lines[k].write(fb + input[k % 2] * 0.35 + if k % 2 == 0 { el } else { er } * 0.1);
            }
            let mut wl = 0.0;
            let mut wr = 0.0;
            for k in 0..LINES {
                if k % 2 == 0 {
                    wl += outs[k];
                } else {
                    wr += outs[k];
                }
            }
            wl = wl * 0.5 + el * self.early_level;
            wr = wr * 0.5 + er * self.early_level;
            let mid = (wl + wr) * 0.5;
            let side = (wl - wr) * 0.5 * self.width;
            l[i] = dl * (1.0 - self.mix) + (mid + side) * self.mix;
            r[i] = dr * (1.0 - self.mix) + (mid - side) * self.mix;
        }
    }
}

pub fn reverb_def() -> EffectDef {
    EffectDef {
        id: "reverb",
        name: "Reverb",
        category: "Space",
        description: "Room/hall/plate reverb (8-line FDN) with pre-delay, early reflections, damping and width.",
        params: PARAMS,
        uses_sidechain: false,
        create: |rate| {
            let mut r = Reverb::new(rate);
            r.update();
            Box::new(r)
        },
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn reverb_tail_decays() {
        let mut p = std::collections::BTreeMap::new();
        p.insert("mix".into(), 1.0);
        p.insert("decay".into(), 0.5);
        let mut e = crate::effects::create("reverb", 48000.0, &p).unwrap();
        let mut l = vec![0.0f32; 96000];
        let mut r = l.clone();
        l[0] = 1.0;
        r[0] = 1.0;
        e.process(&mut l, &mut r, None);
        let early: f32 = l[1000..10000].iter().map(|x| x * x).sum();
        let late: f32 = l[80000..89000].iter().map(|x| x * x).sum();
        assert!(early > 0.0 && late < early * 1e-3, "{early} {late}");
    }
}
