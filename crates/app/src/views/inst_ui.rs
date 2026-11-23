//! The stock instruments drawn as rack units, like the stock effects (the modulation units above all):
//! a faceplate photographed for Dragon DAW (Higgsfield), rack ears and screws, the engraved nameplate
//! with pilot lamp, an animated emblem and VU meters, a phosphor display in a bezel and the designed
//! control panel (`fx_panel`) with the same knobs, gestures and hints as the effects.
//!
//! - **Cinnabar** (the subtractive synth): polished cinnabar plate, cream knobs. The display shows the
//!   three oscillators and their sum, the filter curve (drag it: left / right = cutoff, up / down =
//!   resonance; the LFO moves a dot along it) and both envelopes.
//! - **Wyrmdrum** (the drum pads): dragon-scale plate. Sixteen hide pads (drag samples onto them from the
//!   browser or the file explorer, click to play and select, right-click for more) that light up when
//!   hit, a screen with the selected pad's sample, and its level, pan, pitch, choke group and note.

use super::analog::{self, Finish};
use super::fx_panel;
use crate::state::State;
use crate::theme;
use dragon_engine::effects::{EffectDef, ParamInfo, ParamKind};
use dragon_engine::model::*;
use egui::{Color32, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::OnceLock;

const WAVES: &[&str] = &["Saw", "Square", "Triangle", "Sine", "Noise"];
const MODES: &[&str] = &["Low-pass", "High-pass", "Band-pass"];
const VOICES_1_8: &[&str] = &["1", "2", "3", "4", "5", "6", "7", "8"];
const CHOKE: &[&str] = &["Off", "1", "2", "3", "4", "5", "6", "7", "8"];

fn numbers(n: usize) -> &'static [&'static str] {
    static N: OnceLock<Vec<&'static str>> = OnceLock::new();
    let v = N.get_or_init(|| (1..=32).map(|k| &*Box::leak(k.to_string().into_boxed_str())).collect());
    &v[..n.min(v.len())]
}

fn note_names() -> &'static [&'static str] {
    static N: OnceLock<Vec<&'static str>> = OnceLock::new();
    N.get_or_init(|| (0..128u8).map(|k| &*Box::leak(dragon_engine::instruments::note_name(k).into_boxed_str())).collect())
}

/// An instrument is not an effect: its "effect" factory is never called (the definition only describes
/// the controls), but the type needs one.
fn no_effect(rate: f32) -> Box<dyn dragon_engine::effects::Effect> {
    (dragon_engine::effects::find("utility").expect("the utility effect exists").create)(rate)
}

/// The controls of Cinnabar, as effect parameters (so the designed panel draws them).
pub fn cinnabar_def() -> &'static EffectDef {
    static D: OnceLock<EffectDef> = OnceLock::new();
    D.get_or_init(|| {
        let mut v = Vec::new();
        for (wave, level, coarse, fine, pw) in
            [("o1_wave", "o1_level", "o1_coarse", "o1_fine", "o1_pw"), ("o2_wave", "o2_level", "o2_coarse", "o2_fine", "o2_pw"), ("o3_wave", "o3_level", "o3_coarse", "o3_fine", "o3_pw")]
        {
            v.push(ParamInfo::choice(wave, "Wave", WAVES, 0.0));
            v.push(ParamInfo::lin(level, "Level", 0.0, 1.0, 0.8, ""));
            v.push(ParamInfo::lin(coarse, "Coarse", -36.0, 36.0, 0.0, "st"));
            v.push(ParamInfo::lin(fine, "Fine", -100.0, 100.0, 0.0, "ct"));
            v.push(ParamInfo::lin(pw, "Pulse width", 0.05, 0.95, 0.5, ""));
        }
        v.extend([
            ParamInfo::choice("filter", "Filter", MODES, 0.0),
            ParamInfo::log("cutoff", "Cutoff", 20.0, 20000.0, 4000.0, "Hz"),
            ParamInfo::lin("resonance", "Resonance", 0.0, 1.0, 0.2, ""),
            ParamInfo::lin("filter_env", "Env amount", -6.0, 6.0, 2.0, "oct"),
            ParamInfo::log("fa", "Attack", 0.001, 10.0, 0.005, "s"),
            ParamInfo::log("fd", "Decay", 0.001, 10.0, 0.4, "s"),
            ParamInfo::lin("fs", "Sustain", 0.0, 1.0, 0.3, ""),
            ParamInfo::log("fr", "Release", 0.001, 10.0, 0.3, "s"),
            ParamInfo::log("aa", "Attack", 0.001, 10.0, 0.005, "s"),
            ParamInfo::log("ad", "Decay", 0.001, 10.0, 0.2, "s"),
            ParamInfo::lin("as", "Sustain", 0.0, 1.0, 0.8, ""),
            ParamInfo::log("ar", "Release", 0.001, 10.0, 0.25, "s"),
            ParamInfo::choice("unison", "Voices", VOICES_1_8, 0.0),
            ParamInfo::lin("detune", "Detune", 0.0, 100.0, 10.0, "ct"),
            ParamInfo::log("lfo_rate", "LFO rate", 0.05, 20.0, 5.0, "Hz"),
            ParamInfo::lin("lfo_pitch", "To pitch", 0.0, 12.0, 0.0, "st"),
            ParamInfo::lin("lfo_filter", "To filter", 0.0, 4.0, 0.0, "oct"),
            ParamInfo::lin("glide", "Glide", 0.0, 1.0, 0.08, "s"),
            ParamInfo::choice("polyphony", "Polyphony", numbers(32), 15.0),
            ParamInfo::lin("gain", "Gain", 0.0, 2.0, 0.5, ""),
        ]);
        EffectDef {
            id: "cinnabar",
            name: "Cinnabar (subtractive synth)",
            category: "Instrument",
            description: "Three oscillators, a resonant filter with its own envelope, an amp envelope, unison, an LFO and glide.",
            params: Box::leak(v.into_boxed_slice()),
            uses_sidechain: false,
            create: no_effect,
        }
    })
}

/// The selected pad's controls.
pub fn wyrmdrum_def() -> &'static EffectDef {
    static D: OnceLock<EffectDef> = OnceLock::new();
    D.get_or_init(|| {
        let v = vec![
            ParamInfo::lin("gain", "Level", 0.0, 2.0, 1.0, ""),
            ParamInfo::lin("pan", "Pan", -1.0, 1.0, 0.0, ""),
            ParamInfo::lin("pitch", "Pitch", -24.0, 24.0, 0.0, "st"),
            ParamInfo::choice("choke", "Choke group", CHOKE, 0.0),
            ParamInfo::choice("note", "Note", note_names(), 36.0),
        ];
        EffectDef {
            id: "wyrmdrum",
            name: "Wyrmdrum (drum pads)",
            category: "Instrument",
            description: "Sixteen pads with velocity layers, choke groups and their own level, pan and pitch.",
            params: Box::leak(v.into_boxed_slice()),
            uses_sidechain: false,
            create: no_effect,
        }
    })
}

fn wave_index(w: Waveform) -> f32 {
    match w {
        Waveform::Saw => 0.0,
        Waveform::Square => 1.0,
        Waveform::Triangle => 2.0,
        Waveform::Sine => 3.0,
        Waveform::Noise => 4.0,
    }
}

fn wave_of(v: f32) -> Waveform {
    match v.round() as i32 {
        1 => Waveform::Square,
        2 => Waveform::Triangle,
        3 => Waveform::Sine,
        4 => Waveform::Noise,
        _ => Waveform::Saw,
    }
}

/// Cinnabar's settings as panel values.
pub fn synth_values(s: &SynthParams) -> BTreeMap<String, f32> {
    let mut m = BTreeMap::new();
    for (k, o) in s.osc.iter().enumerate() {
        let n = k + 1;
        m.insert(format!("o{n}_wave"), wave_index(o.wave));
        m.insert(format!("o{n}_level"), o.level);
        m.insert(format!("o{n}_coarse"), o.coarse as f32);
        m.insert(format!("o{n}_fine"), o.fine);
        m.insert(format!("o{n}_pw"), o.pulse_width);
    }
    let mode = match s.filter {
        FilterMode::LowPass => 0.0,
        FilterMode::HighPass => 1.0,
        FilterMode::BandPass => 2.0,
    };
    for (k, v) in [
        ("filter", mode),
        ("cutoff", s.cutoff),
        ("resonance", s.resonance),
        ("filter_env", s.filter_env),
        ("fa", s.filter_adsr.attack),
        ("fd", s.filter_adsr.decay),
        ("fs", s.filter_adsr.sustain),
        ("fr", s.filter_adsr.release),
        ("aa", s.amp_adsr.attack),
        ("ad", s.amp_adsr.decay),
        ("as", s.amp_adsr.sustain),
        ("ar", s.amp_adsr.release),
        ("unison", s.unison.max(1) as f32 - 1.0),
        ("detune", s.detune),
        ("lfo_rate", s.lfo_rate),
        ("lfo_pitch", s.lfo_pitch),
        ("lfo_filter", s.lfo_filter),
        ("glide", s.glide),
        ("polyphony", s.polyphony.max(1) as f32 - 1.0),
        ("gain", s.gain),
    ] {
        m.insert(k.to_string(), v);
    }
    m
}

/// Sets one of Cinnabar's panel values.
pub fn set_synth_value(s: &mut SynthParams, key: &str, v: f32) {
    if let Some(rest) = key.strip_prefix('o') {
        if let Some((n, field)) = rest.split_once('_') {
            if let Some(o) = n.parse::<usize>().ok().and_then(|n| s.osc.get_mut(n.wrapping_sub(1))) {
                match field {
                    "wave" => o.wave = wave_of(v),
                    "level" => o.level = v.clamp(0.0, 1.0),
                    "coarse" => o.coarse = v.round() as i32,
                    "fine" => o.fine = v,
                    "pw" => o.pulse_width = v.clamp(0.05, 0.95),
                    _ => {}
                }
                return;
            }
        }
    }
    match key {
        "filter" => {
            s.filter = match v.round() as i32 {
                1 => FilterMode::HighPass,
                2 => FilterMode::BandPass,
                _ => FilterMode::LowPass,
            }
        }
        "cutoff" => s.cutoff = v.clamp(20.0, 20000.0),
        "resonance" => s.resonance = v.clamp(0.0, 1.0),
        "filter_env" => s.filter_env = v,
        "fa" => s.filter_adsr.attack = v,
        "fd" => s.filter_adsr.decay = v,
        "fs" => s.filter_adsr.sustain = v,
        "fr" => s.filter_adsr.release = v,
        "aa" => s.amp_adsr.attack = v,
        "ad" => s.amp_adsr.decay = v,
        "as" => s.amp_adsr.sustain = v,
        "ar" => s.amp_adsr.release = v,
        "unison" => s.unison = v.round().max(0.0) as u32 + 1,
        "detune" => s.detune = v,
        "lfo_rate" => s.lfo_rate = v,
        "lfo_pitch" => s.lfo_pitch = v,
        "lfo_filter" => s.lfo_filter = v,
        "glide" => s.glide = v.max(0.0),
        "polyphony" => s.polyphony = v.round().max(0.0) as u32 + 1,
        "gain" => s.gain = v.clamp(0.0, 2.0),
        _ => {}
    }
}

fn set_synth(p: &mut Project, cid: Id, f: impl FnOnce(&mut SynthParams)) {
    if let Some(Instrument::Synth(s)) = p.channel_mut(cid).map(|c| &mut c.instrument) {
        f(s);
    }
}

fn set_pad(p: &mut Project, cid: Id, k: usize, f: impl FnOnce(&mut Pad)) {
    if let Some(Instrument::DrumPads(v)) = p.channel_mut(cid).map(|c| &mut c.instrument) {
        if let Some(pad) = v.get_mut(k) {
            f(pad);
        }
    }
}

/// The channel's output level (linear peaks, for the nameplate's meters and emblem).
fn level(st: &State, cid: Id) -> (f32, f32) {
    let Some(m) = st.session.project.channel(cid).map(|c| c.mixer) else { return (0.0, 0.0) };
    st.audio.meters().snapshot().strips.get(&m).map(|(l, r)| (l * 0.7, r * 0.7)).unwrap_or((0.0, 0.0))
}

/// A rack unit in finish `finish_of(id)`: the faceplate behind, the nameplate on top, then `body`.
fn unit(ui: &mut Ui, st: &mut State, def: &EffectDef, cid: Id, body: impl FnOnce(&mut Ui, &mut State)) {
    let finish = analog::finish_of(def.id);
    super::fx_mod::set_screen(super::fx_mod::phosphor(finish));
    let plate_at = ui.painter().add(Shape::Noop);
    let ear = analog::EAR as i8;
    let time = if st.settings.reduced_motion { None } else { Some(ui.input(|i| i.time) as f32) };
    let lv = level(st, cid);
    let dev = egui::Frame::NONE.inner_margin(egui::Margin { left: 14 + ear, right: 14 + ear, top: 8, bottom: 14 }).show(ui, |ui| {
        analog::nameplate(ui, &analog::Plate { id: def.id, name: def.name, category: def.category, enabled: true, gr: 0.0, level: lv, time, key: egui::Id::new(("inst.unit", cid)) });
        ui.add_space(6.0);
        body(ui, st);
    });
    ui.painter().set(plate_at, Shape::Vec(analog::faceplate(ui.ctx(), dev.response.rect, finish)));
}

/// The display's bezel (as on the effects).
fn bezel(ui: &mut Ui, finish: Finish, f: impl FnOnce(&mut Ui)) {
    let face = finish.base();
    let rim = Color32::from_rgb((face.r() as f32 * 0.55) as u8, (face.g() as f32 * 0.55) as u8, (face.b() as f32 * 0.55) as u8);
    egui::Frame::NONE.fill(Color32::from_rgb(0x08, 0x09, 0x0b)).corner_radius(7.0).inner_margin(egui::Margin::same(3)).stroke(Stroke::new(2.0_f32, rim)).show(ui, f);
}

/// Draws the designed panel of `def` and routes every touched control to `set` (one undo step per
/// gesture; choices at once).
#[allow(clippy::too_many_arguments)]
fn controls(ui: &mut Ui, st: &mut State, def: &'static EffectDef, values: &BTreeMap<String, f32>, accent: Color32, salt: egui::Id, cutoff_of: Option<Id>, set: &dyn Fn(&mut Project, &str, f32)) {
    let mut band = 1usize;
    let source = def.name.split(" (").next().unwrap_or(def.name);
    let mut touch = |_ui: &mut Ui, t: fx_panel::Touch| {
        let prm = t.prm;
        let v = t.value;
        if t.resp.hovered() || t.resp.dragged() || t.resp.changed() {
            st.hint(source, prm.name, prm.format(v));
        }
        match prm.kind {
            ParamKind::Choice(_) | ParamKind::Toggle => {
                if t.resp.changed() {
                    st.session.edit(prm.name, |p| set(p, prm.id, v));
                }
            }
            _ => {
                let r = t.resp.on_hover_text(format!("{} – drag, Shift = fine, double-click or Ctrl+click resets", prm.name));
                super::widgets::apply(st, &r, prm.name, |p| set(p, prm.id, v));
                if let (Some(cid), "cutoff") = (cutoff_of, prm.id) {
                    r.context_menu(|ui| {
                        if ui.button("Create automation lane").clicked() {
                            crate::views::automation::add_lane(st, Target::SynthCutoff(cid));
                            ui.close_menu();
                        }
                        if ui.button("MIDI learn").clicked() {
                            st.ui.midi_learn = Some(Target::SynthCutoff(cid));
                            ui.close_menu();
                        }
                    });
                }
            }
        }
    };
    fx_panel::show(ui, fx_panel::View { def, params: values, accent, salt }, &mut band, &mut touch, &mut |_, _| {});
}

// ------------------------------------------------------------------------------------------- Cinnabar

/// Cinnabar's editor.
pub fn cinnabar(ui: &mut Ui, st: &mut State, cid: Id, s: SynthParams) {
    let def = cinnabar_def();
    let finish = analog::finish_of(def.id);
    let accent = finish.accent().unwrap_or(theme::ACCENT);
    let values = synth_values(&s);
    unit(ui, st, def, cid, |ui, st| {
        let time = if st.settings.reduced_motion { None } else { Some(ui.input(|i| i.time) as f32) };
        let edits = {
            let mut out = Vec::new();
            bezel(ui, finish, |ui| out = cinnabar_screen(ui, &s, time, super::fx_mod::phosphor(finish), egui::Id::new(("cinnabar.screen", cid))));
            out
        };
        if let Some((k, v)) = edits.last() {
            if let Some(prm) = def.param(k) {
                st.hint("Cinnabar", prm.name, prm.format(*v));
            }
        }
        if !edits.is_empty() {
            st.session.gesture("Filter", |p| {
                set_synth(p, cid, |x| {
                    for (k, v) in &edits {
                        set_synth_value(x, k, *v);
                    }
                })
            });
        }
        if time.is_some() {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(33));
        }
        ui.add_space(8.0);
        controls(ui, st, def, &values, accent, egui::Id::new(("cinnabar.panel", cid)), Some(cid), &|p, k, v| set_synth(p, cid, |x| set_synth_value(x, k, v)));
    });
}

fn osc_sample(w: Waveform, ph: f32, pw: f32, seed: u32) -> f32 {
    let x = ph.rem_euclid(1.0);
    match w {
        Waveform::Saw => 2.0 * x - 1.0,
        Waveform::Square => {
            if x < pw {
                1.0
            } else {
                -1.0
            }
        }
        Waveform::Triangle => 1.0 - 4.0 * (x - 0.5).abs(),
        Waveform::Sine => (x * std::f32::consts::TAU).sin(),
        Waveform::Noise => {
            let h = ((ph * 97.0) as u32).wrapping_mul(2654435761).wrapping_add(seed.wrapping_mul(40503));
            (h >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0
        }
    }
}

/// The filter's response at `f` (dB): a resonant two-pole curve of the mode.
fn filter_db(mode: FilterMode, fc: f32, reso: f32, f: f32) -> f32 {
    let q = 0.7 + reso * 9.0;
    let x = f / fc.max(1.0);
    let den = ((1.0 - x * x).powi(2) + (x / q).powi(2)).sqrt().max(1e-6);
    let mag = match mode {
        FilterMode::LowPass => 1.0 / den,
        FilterMode::HighPass => x * x / den,
        FilterMode::BandPass => (x / q) / den,
    };
    20.0 * mag.max(1e-6).log10()
}

fn env_points(e: &Envelope, r: Rect) -> Vec<Pos2> {
    // Times on a gentle square-root scale, so short and long envelopes both read.
    let t = |s: f32| s.max(0.0).sqrt();
    let hold = 0.6;
    let total = t(e.attack) + t(e.decay) + hold + t(e.release);
    let x = |u: f32| r.left() + u / total * r.width();
    let y = |l: f32| r.bottom() - l * r.height();
    let (a, d) = (t(e.attack), t(e.decay));
    vec![Pos2::new(x(0.0), y(0.0)), Pos2::new(x(a), y(1.0)), Pos2::new(x(a + d), y(e.sustain)), Pos2::new(x(a + d + hold), y(e.sustain)), Pos2::new(x(total), y(0.0))]
}

/// The display: oscillators | filter | envelopes. Returns edits (dragging the filter curve).
fn cinnabar_screen(ui: &mut Ui, s: &SynthParams, time: Option<f32>, ph: Color32, key: egui::Id) -> Vec<(String, f32)> {
    let w = ui.available_width().max(420.0);
    let (r, resp) = ui.allocate_exact_size(Vec2::new(w, 156.0), Sense::click_and_drag());
    let p = ui.painter_at(r);
    super::fx_mod::screen(&p, r, 6.0);
    let t = time.unwrap_or(0.0);
    let inner = r.shrink(10.0);
    let gap = 10.0;
    let col_w = (inner.width() - 2.0 * gap) / 3.0;
    let pane = |k: usize| Rect::from_min_size(Pos2::new(inner.left() + k as f32 * (col_w + gap), inner.top() + 14.0), Vec2::new(col_w, inner.height() - 14.0));
    let label = |k: usize, text: &str| {
        p.text(Pos2::new(pane(k).left() + 2.0, inner.top() + 5.0), egui::Align2::LEFT_CENTER, text, egui::FontId::proportional(9.5), ph.gamma_multiply(0.8));
    };
    let dim = |a: f32| Color32::from_rgba_unmultiplied(ph.r(), ph.g(), ph.b(), (a * 255.0) as u8);

    // Oscillators: each faint, their sum bright – two cycles of oscillator 1.
    let o = pane(0);
    super::fx_mod::inset(&p, o, 4.0);
    label(0, "OSCILLATORS");
    let n = 160;
    let drift = t * 0.15;
    let total: f32 = s.osc.iter().map(|x| x.level).sum::<f32>().max(0.001);
    let mut sum = vec![0.0f32; n + 1];
    for (k, osc) in s.osc.iter().enumerate() {
        if osc.level <= 0.001 {
            continue;
        }
        let ratio = 2f32.powf((osc.coarse as f32 + osc.fine / 100.0) / 12.0);
        let pts: Vec<Pos2> = (0..=n)
            .map(|i| {
                let u = i as f32 / n as f32;
                let v = osc_sample(osc.wave, (u * 2.0 + drift) * ratio, osc.pulse_width, i as u32 + k as u32 * 977) * osc.level;
                sum[i] += v;
                Pos2::new(o.left() + 4.0 + u * (o.width() - 8.0), o.center().y - v * o.height() * 0.32)
            })
            .collect();
        p.add(Shape::line(pts, Stroke::new(1.0_f32, dim(0.28))));
    }
    let pts: Vec<Pos2> = sum.iter().enumerate().map(|(i, v)| Pos2::new(o.left() + 4.0 + i as f32 / n as f32 * (o.width() - 8.0), o.center().y - (v / total).clamp(-1.2, 1.2) * o.height() * 0.38)).collect();
    p.add(Shape::line(pts, Stroke::new(1.8_f32, ph)));

    // Filter: the curve on a log frequency axis, the cutoff handle, the LFO's dot.
    let fpane = pane(1);
    super::fx_mod::inset(&p, fpane, 4.0);
    label(1, "FILTER  ·  drag the curve");
    let fx = |f: f32| fpane.left() + ((f / 20.0).max(1.0).log10() / 3.0) * fpane.width();
    let f_of = |x: f32| 20.0 * 10f32.powf(((x - fpane.left()) / fpane.width()).clamp(0.0, 1.0) * 3.0);
    let fy = |db: f32| fpane.top() + (1.0 - (db.clamp(-36.0, 18.0) + 36.0) / 54.0) * fpane.height();
    for f in [100.0, 1000.0, 10000.0] {
        p.line_segment([Pos2::new(fx(f), fpane.top()), Pos2::new(fx(f), fpane.bottom())], Stroke::new(1.0_f32, dim(0.08)));
    }
    p.line_segment([Pos2::new(fpane.left(), fy(0.0)), Pos2::new(fpane.right(), fy(0.0))], Stroke::new(1.0_f32, dim(0.12)));
    let curve: Vec<Pos2> = (0..=120).map(|i| fpane.left() + i as f32 / 120.0 * fpane.width()).map(|x| Pos2::new(x, fy(filter_db(s.filter, s.cutoff, s.resonance, f_of(x))))).collect();
    let fill = {
        let mut v = curve.clone();
        v.push(fpane.right_bottom());
        v.push(fpane.left_bottom());
        v
    };
    let pf = p.with_clip_rect(fpane);
    pf.add(Shape::convex_polygon(fill.iter().copied().filter(|q| q.y <= fpane.bottom()).collect(), dim(0.06), Stroke::NONE));
    pf.add(Shape::line(curve, Stroke::new(1.8_f32, ph)));
    let handle = Pos2::new(fx(s.cutoff), fy(filter_db(s.filter, s.cutoff, s.resonance, s.cutoff)).max(fpane.top() + 4.0));
    pf.circle_filled(handle, 4.5, ph);
    if s.lfo_filter > 0.001 && time.is_some() {
        let lfo = (t * s.lfo_rate * std::f32::consts::TAU).sin();
        let fm = (s.cutoff * 2f32.powf(lfo * s.lfo_filter)).clamp(20.0, 20000.0);
        pf.circle_stroke(Pos2::new(fx(fm), fy(filter_db(s.filter, s.cutoff, s.resonance, fm))), 3.5, Stroke::new(1.2_f32, ph.gamma_multiply(0.8)));
    }
    p.text(
        fpane.right_top() + Vec2::new(-4.0, 4.0),
        egui::Align2::RIGHT_TOP,
        format!("{} · {:.0} %", if s.cutoff >= 1000.0 { format!("{:.2} kHz", s.cutoff / 1000.0) } else { format!("{:.0} Hz", s.cutoff) }, s.resonance * 100.0),
        egui::FontId::monospace(9.0),
        dim(0.85),
    );

    // Envelopes: amp bright, filter dim.
    let e = pane(2);
    super::fx_mod::inset(&p, e, 4.0);
    label(2, "ENVELOPES  ·  amp / filter");
    let er = e.shrink2(Vec2::new(6.0, 8.0));
    p.add(Shape::line(env_points(&s.filter_adsr, er), Stroke::new(1.2_f32, dim(0.45))));
    p.add(Shape::line(env_points(&s.amp_adsr, er), Stroke::new(1.8_f32, ph)));

    // Dragging on the filter pane: left / right = cutoff, up / down = resonance.
    let mut out = Vec::new();
    if resp.dragged() || resp.drag_started() {
        if let Some(pos) = resp.interact_pointer_pos() {
            let start: Option<(f32, f32, Pos2)> = ui.data(|d| d.get_temp(key));
            if resp.drag_started() && fpane.contains(pos) {
                ui.data_mut(|d| d.insert_temp(key, (s.cutoff, s.resonance, pos)));
            } else if let Some((c0, r0, p0)) = start {
                let fine = if ui.input(|i| i.modifiers.shift) { 0.25 } else { 1.0 };
                let d = (pos - p0) * fine;
                let c = (c0 * 10f32.powf(d.x / fpane.width() * 3.0)).clamp(20.0, 20000.0);
                let r = (r0 - d.y / fpane.height()).clamp(0.0, 1.0);
                out.push(("cutoff".to_string(), c));
                out.push(("resonance".to_string(), r));
            }
        }
    }
    if resp.drag_stopped() {
        ui.data_mut(|d| d.remove::<(f32, f32, Pos2)>(key));
    }
    if resp.double_clicked() && resp.interact_pointer_pos().is_some_and(|q| fpane.contains(q)) {
        out.push(("cutoff".to_string(), 4000.0));
        out.push(("resonance".to_string(), 0.2));
    }
    if resp.hovered() && ui.input(|i| i.pointer.hover_pos()).is_some_and(|q| fpane.contains(q)) {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    out
}

// ------------------------------------------------------------------------------------------- Wyrmdrum

const PAD_ART: &[u8] = include_bytes!("../../assets/fx/drum_pad.jpg");

fn selected_key(cid: Id) -> egui::Id {
    egui::Id::new(("wyrmdrum.sel", cid))
}

/// Wyrmdrum's editor.
pub fn wyrmdrum(ui: &mut Ui, st: &mut State, cid: Id, pads: Vec<Pad>) {
    let def = wyrmdrum_def();
    let finish = analog::finish_of(def.id);
    let accent = finish.accent().unwrap_or(theme::ACCENT_2);
    let ph = super::fx_mod::phosphor(finish);
    let sel: usize = ui.data(|d| d.get_temp(selected_key(cid))).unwrap_or(0).min(pads.len().saturating_sub(1));
    unit(ui, st, def, cid, |ui, st| {
        let pad_w = 92.0;
        let grid_w = 4.0 * pad_w + 3.0 * 8.0 + 20.0;
        ui.horizontal_top(|ui| {
            bezel(ui, finish, |ui| {
                ui.set_width(grid_w);
                pad_grid(ui, st, cid, &pads, sel, pad_w, ph);
            });
            ui.add_space(8.0);
            ui.vertical(|ui| {
                bezel(ui, finish, |ui| pad_screen(ui, st, &pads, sel, ph));
            });
        });
        ui.add_space(8.0);
        if let Some(pad) = pads.get(sel) {
            let mut values = BTreeMap::new();
            values.insert("gain".to_string(), pad.gain);
            values.insert("pan".to_string(), pad.pan);
            values.insert("pitch".to_string(), pad.pitch);
            values.insert("choke".to_string(), pad.choke as f32);
            values.insert("note".to_string(), pad.note as f32);
            controls(ui, st, def, &values, accent, egui::Id::new(("wyrmdrum.panel", cid)), None, &move |p, k, v| {
                set_pad(p, cid, sel, |x| match k {
                    "gain" => x.gain = v,
                    "pan" => x.pan = v,
                    "pitch" => x.pitch = v,
                    "choke" => x.choke = v.round().clamp(0.0, 8.0) as u8,
                    "note" => x.note = v.round().clamp(0.0, 127.0) as u8,
                    _ => {}
                })
            });
        }
    });
}

fn pad_grid(ui: &mut Ui, st: &mut State, cid: Id, pads: &[Pad], sel: usize, pad_w: f32, ph: Color32) {
    let ctx = ui.ctx().clone();
    let art = super::guitar_ui::texture(&ctx, "wyrmdrum-pad", PAD_ART);
    let dragging_file = egui::DragAndDrop::has_payload_of_type::<PathBuf>(&ctx) || ctx.input(|i| !i.raw.hovered_files.is_empty());
    let mut rects: Vec<(Id, usize, Rect)> = Vec::new();
    let mut lit_any = false;
    let pad_h = 74.0;
    ui.add_space(4.0);
    egui::Grid::new(("wyrm.pads", cid)).spacing([8.0, 8.0]).show(ui, |ui| {
        for (k, pad) in pads.iter().enumerate() {
            let (r, resp) = ui.allocate_exact_size(Vec2::new(pad_w, pad_h), Sense::click());
            rects.push((cid, k, r));
            let light = match st.ui.pad_hits.get(&(cid, pad.note)) {
                Some((true, _)) => 1.0,
                Some((false, t)) => (1.0 - t.elapsed().as_secs_f32() / 0.3).max(0.0),
                None => 0.0,
            };
            lit_any |= light > 0.0;
            let has = !pad.layers.is_empty();
            let p = ui.painter();
            // The pad: hide on a dark well, pressed in a little while lit.
            let press = light * 1.5;
            let face = r.shrink(press);
            p.rect_filled(r.expand(1.0), 6.0, Color32::from_black_alpha(150));
            let tint = if has { Color32::from_gray(210) } else { Color32::from_gray(95) };
            match &art {
                Some(t) => {
                    p.image(t.id(), face, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), tint);
                }
                None => {
                    p.rect_filled(face, 5.0, if has { Color32::from_rgb(0xd8, 0xcc, 0xb0) } else { Color32::from_gray(60) });
                }
            }
            if light > 0.0 {
                super::fx_mod::halo(p, face.center(), face.width() * 0.75, Color32::from_rgb(0xff, 0xa8, 0x3c), 0.55 * light);
                p.rect_filled(face, 5.0, Color32::from_rgba_unmultiplied(0xff, 0x9a, 0x3c, (70.0 * light) as u8));
            }
            if k == sel {
                p.rect_stroke(r.expand(2.0), 7.0, Stroke::new(2.0_f32, analog::GNOSTIC_GOLD), egui::StrokeKind::Outside);
            }
            if dragging_file && resp.contains_pointer() {
                p.rect_stroke(r.expand(3.0), 7.0, Stroke::new(2.0_f32, theme::PLAYHEAD), egui::StrokeKind::Outside);
            }
            // Name and note, printed on the hide.
            let ink = if has { Color32::from_rgb(0x2a, 0x1c, 0x10) } else { Color32::from_gray(170) };
            let name = if pad.name.is_empty() { "empty".to_string() } else { pad.name.clone() };
            let name = if name.chars().count() > 13 { format!("{}…", name.chars().take(12).collect::<String>()) } else { name };
            p.text(Pos2::new(face.center().x, face.bottom() - 18.0), egui::Align2::CENTER_CENTER, name, egui::FontId::proportional(11.0), ink);
            p.text(Pos2::new(face.center().x, face.bottom() - 7.0), egui::Align2::CENTER_CENTER, dragon_engine::instruments::note_name(pad.note), egui::FontId::monospace(8.5), ink.gamma_multiply(0.75));
            p.text(face.left_top() + Vec2::new(6.0, 5.0), egui::Align2::LEFT_TOP, (k + 1).to_string(), egui::FontId::monospace(8.5), ink.gamma_multiply(0.6));
            if resp.hovered() {
                ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            let resp = resp.on_hover_text(format!("{} – click to play and select · drop a sample here · right-click for more", if has { pad.name.as_str() } else { "Empty pad" }));
            if resp.clicked() {
                ctx.data_mut(|d| d.insert_temp(selected_key(cid), k));
                st.audio.send(dragon_engine::device::Command::Live(dragon_engine::render::LiveEvent::NoteOn { channel: cid, note: pad.note, velocity: 0.9 }));
                st.audio.send(dragon_engine::device::Command::Live(dragon_engine::render::LiveEvent::NoteOff { channel: cid, note: pad.note }));
                st.ui.pad_hits.insert((cid, pad.note), (false, std::time::Instant::now()));
            }
            if let Some(path) = super::widgets::dnd_release::<PathBuf>(&resp) {
                super::windows::assign_pad(st, cid, k, (*path).clone());
                ctx.data_mut(|d| d.insert_temp(selected_key(cid), k));
            }
            resp.context_menu(|ui| pad_menu(ui, st, cid, k));
            if k % 4 == 3 {
                ui.end_row();
            }
        }
    });
    ui.add_space(4.0);
    let now = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| {
        let e = d.get_temp_mut_or_default::<(u64, Vec<(Id, usize, Rect)>)>(egui::Id::new(super::windows::PAD_RECTS));
        if e.0 != now {
            *e = (now, Vec::new());
        }
        e.1.retain(|x| x.0 != cid);
        e.1.extend(rects);
    });
    if lit_any || dragging_file {
        ctx.request_repaint_after(std::time::Duration::from_millis(16));
    }
    let _ = ph;
}

fn pick_audio() -> Option<PathBuf> {
    rfd::FileDialog::new().add_filter("Audio", &["wav", "flac", "mp3", "ogg", "aif", "aiff"]).pick_file()
}

fn pad_menu(ui: &mut Ui, st: &mut State, cid: Id, k: usize) {
    if ui.button("Load sample…").clicked() {
        if let Some(f) = pick_audio() {
            super::windows::assign_pad(st, cid, k, f);
        }
        ui.close_menu();
    }
    if ui.button("Add velocity layer…").clicked() {
        if let Some(f) = pick_audio() {
            st.session.edit("Add layer", |p| {
                set_pad(p, cid, k, |pad| {
                    let n = pad.layers.len() as f32 + 1.0;
                    pad.layers.push(PadLayer { path: f, velocity_low: 0.0, velocity_high: 1.0 });
                    for (i, l) in pad.layers.iter_mut().enumerate() {
                        l.velocity_low = i as f32 / n;
                        l.velocity_high = (i + 1) as f32 / n;
                    }
                })
            });
        }
        ui.close_menu();
    }
    if ui.button("Clear pad").clicked() {
        st.session.edit("Clear pad", |p| {
            set_pad(p, cid, k, |pad| {
                pad.layers.clear();
                pad.name.clear();
            })
        });
        ui.close_menu();
    }
}

/// The selected pad's screen: its sample's waveform, name, layers and note.
fn pad_screen(ui: &mut Ui, st: &mut State, pads: &[Pad], sel: usize, ph: Color32) {
    let w = ui.available_width().max(200.0);
    let (r, _) = ui.allocate_exact_size(Vec2::new(w, 4.0 * 74.0 + 3.0 * 8.0 + 8.0), Sense::hover());
    let p = ui.painter_at(r);
    super::fx_mod::screen(&p, r, 6.0);
    let dim = |a: f32| Color32::from_rgba_unmultiplied(ph.r(), ph.g(), ph.b(), (a * 255.0) as u8);
    let Some(pad) = pads.get(sel) else { return };
    let inner = r.shrink(12.0);
    p.text(inner.left_top(), egui::Align2::LEFT_TOP, format!("PAD {}  ·  {}", sel + 1, dragon_engine::instruments::note_name(pad.note)), egui::FontId::monospace(10.0), dim(0.8));
    p.text(inner.left_top() + Vec2::new(0.0, 16.0), egui::Align2::LEFT_TOP, if pad.name.is_empty() { "Empty – drop a sample on the pad".to_string() } else { pad.name.clone() }, egui::FontId::proportional(16.0), ph);
    let wave = Rect::from_min_max(inner.left_top() + Vec2::new(0.0, 44.0), inner.right_bottom() - Vec2::new(0.0, 46.0));
    super::fx_mod::inset(&p, wave, 4.0);
    let rate = st.audio.rate();
    if let Some(l) = pad.layers.last() {
        let data = st.session.cache.peek(&l.path, rate).or_else(|| st.session.cache.get(&l.path, rate).ok());
        if let Some(d) = data {
            let n = wave.width().max(10.0) as usize;
            let peaks = d.peaks(0, d.frames(), n);
            let mid = wave.center().y;
            let h = wave.height() * 0.45;
            for (i, (lo, hi)) in peaks.iter().enumerate() {
                let x = wave.left() + i as f32 + 0.5;
                p.line_segment([Pos2::new(x, mid - hi.max(0.0) * h), Pos2::new(x, mid - lo.min(0.0) * h)], Stroke::new(1.0_f32, ph.gamma_multiply(0.9)));
            }
            p.text(wave.right_bottom() + Vec2::new(0.0, 4.0), egui::Align2::RIGHT_TOP, format!("{:.2} s", d.duration()), egui::FontId::monospace(9.0), dim(0.7));
        }
    } else {
        p.text(wave.center(), egui::Align2::CENTER_CENTER, "no sample", egui::FontId::proportional(11.0), dim(0.5));
    }
    let layers = pad.layers.len();
    let choke = if pad.choke == 0 { "no choke".to_string() } else { format!("choke group {}", pad.choke) };
    p.text(Pos2::new(inner.left(), inner.bottom() - 20.0), egui::Align2::LEFT_TOP, format!("{layers} layer{}  ·  {choke}", if layers == 1 { "" } else { "s" }), egui::FontId::monospace(9.5), dim(0.75));
    p.text(Pos2::new(inner.left(), inner.bottom() - 6.0), egui::Align2::LEFT_CENTER, "Pads and keys both play it", egui::FontId::proportional(9.0), dim(0.55));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synth_values_round_trip() {
        let s = SynthParams::default();
        let v = synth_values(&s);
        let mut t = SynthParams { cutoff: 100.0, unison: 5, polyphony: 2, ..SynthParams::default() };
        t.osc[0].wave = Waveform::Noise;
        for (k, x) in &v {
            set_synth_value(&mut t, k, *x);
        }
        assert_eq!(t, s, "every control maps back to the same setting");
        // Every value the panel shows is a parameter of the definition, and in its range.
        let def = cinnabar_def();
        for (k, x) in &v {
            let prm = def.param(k).unwrap_or_else(|| panic!("{k} has no control"));
            assert!(*x >= prm.min - 1e-4 && *x <= prm.max + 1e-4 || matches!(prm.kind, ParamKind::Choice(_)), "{k} = {x} outside {}..{}", prm.min, prm.max);
        }
        assert_eq!(def.params.len(), v.len());
    }

    #[test]
    fn filter_curve_peaks_at_the_cutoff_with_resonance() {
        let lp = |f| filter_db(FilterMode::LowPass, 1000.0, 0.8, f);
        assert!(lp(1000.0) > lp(100.0) + 6.0, "resonant peak");
        assert!(lp(10000.0) < -30.0, "falls off above");
        assert!(filter_db(FilterMode::HighPass, 1000.0, 0.0, 50.0) < -40.0);
    }
}
