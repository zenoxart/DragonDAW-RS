//! Designed control panels for the stock effects. Each effect gets its own layout – titled sections in
//! the order you work, a skin (panel colours and a knob style that fits the device: modern rings, brushed
//! metal, chicken-head pointers, cream vintage caps), a large knob for its main control, faders where a
//! fader is what the original has, LED switches for on/off and segmented buttons for short choices –
//! instead of a generic grid of identical knobs. Every control is a fixed-size cell, so labels never
//! collide; they shrink and elide to fit.
//!
//! Gestures are the same everywhere: drag (Shift = fine), wheel, double-click or Ctrl+click resets;
//! the editor adds the right-click menu (type a value, MIDI learn, automation).

use crate::theme;
use dragon_engine::effects::{self, EffectDef, ParamInfo, ParamKind};
use egui::{Color32, Pos2, Rect, Response, Sense, Shape, Stroke, Ui, Vec2};
use std::collections::BTreeMap;
use std::f32::consts::PI;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum KnobStyle {
    /// Dark body, coloured value arc (modern / school).
    Ring,
    /// Brushed aluminium with a black line (studio hardware).
    Metal,
    /// Black chicken-head pointer over a numbered scale (vintage British consoles).
    Chicken,
    /// Cream cap with a red line (valve gear).
    Cream,
    /// Engraved brass (the gnostic units).
    Brass,
    /// Black knurled aluminium with a white line (Jadefang).
    Black,
}

#[derive(Clone, Copy)]
pub struct Skin {
    /// The sections.
    pub section: Color32,
    pub knob: KnobStyle,
    /// The printing on the face (labels, values).
    pub ink: Color32,
    pub ink_dim: Color32,
}

pub enum Ctl {
    Knob(String, &'static str),
    /// The effect's main control, larger.
    Big(String, &'static str),
    Fader(String, &'static str),
    /// A toggle (LED switch) or a choice (segmented buttons when short, otherwise a dropdown).
    Param(String, &'static str),
    /// Drawn by the editor (e.g. the reverb part's EQ curve).
    Custom(&'static str),
    /// The band picker (effects with bands), placed in the section it switches.
    Bands,
}

pub struct Section {
    pub title: String,
    pub ctls: Vec<Ctl>,
}

pub struct Panel {
    pub skin: Skin,
    /// Effects with bands: (number of bands, label); the picker is the layout's `Ctl::Bands` cell.
    pub bands: Option<(usize, &'static str)>,
    pub sections: Vec<Section>,
}

fn k(id: impl Into<String>, l: &'static str) -> Ctl {
    Ctl::Knob(id.into(), l)
}
fn big(id: impl Into<String>, l: &'static str) -> Ctl {
    Ctl::Big(id.into(), l)
}
fn fad(id: impl Into<String>, l: &'static str) -> Ctl {
    Ctl::Fader(id.into(), l)
}
fn p(id: impl Into<String>, l: &'static str) -> Ctl {
    Ctl::Param(id.into(), l)
}
fn sec(title: impl Into<String>, ctls: Vec<Ctl>) -> Section {
    Section { title: title.into(), ctls }
}

/// The multiband panel's per-band meters (custom cells the editor draws from the effect's telemetry).
pub const MB_METERS: [&str; 4] = ["mb.gr1", "mb.gr2", "mb.gr3", "mb.gr4"];

/// The colours of bands 1 … 4 (multiband display and panel columns).
pub const BAND_COLORS: [Color32; 4] = [Color32::from_rgb(0xff, 0x7a, 0x4a), Color32::from_rgb(0xf2, 0xc9, 0x4c), Color32::from_rgb(0x52, 0xd2, 0x7f), Color32::from_rgb(0x4c, 0xa8, 0xff)];

/// Panels laid out as side-by-side band columns (every band visible at once); the last section is the
/// output column.
fn columns(id: &str) -> bool {
    id == "multiband"
}

thread_local! {
    /// The inner width of the column being drawn (column layouts draw compact cells).
    static COLUMN: std::cell::Cell<Option<f32>> = const { std::cell::Cell::new(None) };
}

thread_local! {
    /// The ink of the face being drawn (labels and values printed on it).
    static INK: std::cell::Cell<(Color32, Color32)> = const { std::cell::Cell::new((theme::TEXT, theme::TEXT_DIM)) };
}

/// Sets the ink knob scales are printed in (faces drawn outside `show`).
pub fn set_ink(ink: Color32, dim: Color32) {
    INK.with(|c| c.set((ink, dim)));
}

fn ink() -> Color32 {
    INK.with(|c| c.get().0)
}

fn ink_dim() -> Color32 {
    INK.with(|c| c.get().1)
}

fn column() -> Option<f32> {
    COLUMN.with(|c| c.get())
}

/// Brushed gold (the Soft Clipper).
const GOLD: Skin = Skin { section: Color32::from_rgb(0x5e, 0x4a, 0x24), knob: KnobStyle::Metal, ink: theme::TEXT, ink_dim: theme::TEXT_DIM };
const MODERN: Skin = Skin { section: Color32::from_rgb(0x1d, 0x21, 0x29), knob: KnobStyle::Ring, ink: theme::TEXT, ink_dim: theme::TEXT_DIM };
const STUDIO: Skin = Skin { section: Color32::from_rgb(0x26, 0x29, 0x2f), knob: KnobStyle::Metal, ink: theme::TEXT, ink_dim: theme::TEXT_DIM };
const CONSOLE: Skin = Skin { section: Color32::from_rgb(0x3a, 0x44, 0x52), knob: KnobStyle::Chicken, ink: theme::TEXT, ink_dim: theme::TEXT_DIM };
const VALVE: Skin = Skin { section: Color32::from_rgb(0x33, 0x2a, 0x24), knob: KnobStyle::Cream, ink: theme::TEXT, ink_dim: theme::TEXT_DIM };

/// The panel of a stock effect, showing band `band` (1-based) where the effect has bands.
pub fn panel(id: &str, band: usize) -> Option<Panel> {
    let b = band.max(1);
    let bp = |s: &str| format!("b{b}_{s}");
    let panel = match id {
        "eq" => Panel {
            skin: MODERN,
            bands: Some((dragon_engine::effects::eq::EQ_BANDS, "Band")),
            sections: vec![
                sec(format!("Band {b}"), vec![Ctl::Bands, p(bp("on"), "On"), p(bp("type"), "Shape"), p(bp("slope"), "Slope"), p(bp("stereo"), "Stereo")]),
                sec("Curve", vec![big(bp("freq"), "Frequency"), k(bp("gain"), "Gain"), k(bp("q"), "Q")]),
                sec("Dynamic", vec![k(bp("dyn"), "Range"), k(bp("thr"), "Threshold")]),
                sec("Output", vec![k("output", "Output"), p("auto_gain", "Auto gain"), p("phase", "Phase"), p("solo", "Listen")]),
            ],
        },
        "compressor" => Panel {
            skin: MODERN,
            bands: None,
            sections: vec![
                sec("Style", vec![p("style", "Style")]),
                sec("Compression", vec![big("threshold", "Threshold"), k("ratio", "Ratio"), k("knee", "Knee")]),
                sec("Timing", vec![k("attack", "Attack"), k("release", "Release"), k("hold", "Hold"), k("lookahead", "Lookahead")]),
                sec("Sidechain", vec![p("sidechain", "External"), k("sc_hp", "High-pass"), k("sc_lp", "Low-pass")]),
                sec("Output", vec![k("makeup", "Make-up"), p("auto_makeup", "Auto"), fad("mix", "Mix")]),
            ],
        },
        "multiband" => Panel {
            skin: MODERN,
            bands: None,
            sections: {
                // Every band as a column of its own, the output column last.
                let mut s: Vec<Section> = (1..=4)
                    .map(|n| {
                        let b = |x: &str| format!("b{n}_{x}");
                        sec(
                            format!("Band {n}"),
                            vec![
                                Ctl::Custom(MB_METERS[n - 1]),
                                p(b("mode"), "Mode"),
                                p(b("solo"), "Solo"),
                                p(b("bypass"), "Byp"),
                                p(b("sc"), "SC"),
                                big(b("thr"), "Threshold"),
                                k(b("ratio"), "Ratio"),
                                k(b("range"), "Range"),
                                k(b("knee"), "Knee"),
                                k(b("attack"), "Attack"),
                                k(b("release"), "Release"),
                                k(b("gain"), "Gain"),
                            ],
                        )
                    })
                    .collect();
                s.push(sec("Output", vec![p("bands", "Bands"), p("phase", "Processing"), k("x1", "X-over 1"), k("x2", "X-over 2"), k("x3", "X-over 3"), fad("mix", "Mix"), k("output", "Output")]));
                s
            },
        },
        "clipper" => Panel {
            skin: GOLD,
            bands: None,
            sections: vec![
                sec("Clip", vec![big("input", "Input"), k("threshold", "Threshold"), k("ceiling", "Ceiling")]),
                sec("Character", vec![p("shape", "Shape"), p("oversampling", "Oversampling")]),
                sec("Output", vec![k("post", "Post gain"), p("listen", "Clip only")]),
            ],
        },
        "gate" => Panel {
            skin: MODERN,
            bands: None,
            sections: vec![
                sec("Style", vec![p("style", "Style"), p("flip", "Duck")]),
                sec("Threshold", vec![big("threshold", "Threshold"), k("hysteresis", "Hysteresis"), k("range", "Range"), k("ratio", "Ratio"), k("knee", "Knee")]),
                sec("Timing", vec![k("attack", "Attack"), k("hold", "Hold"), k("release", "Release"), k("lookahead", "Lookahead")]),
                sec("Sidechain", vec![p("sidechain", "External"), k("sc_hp", "High-pass"), k("sc_lp", "Low-pass"), p("listen", "Listen")]),
                sec("Output", vec![fad("mix", "Mix"), k("output", "Output")]),
            ],
        },
        "limiter" => Panel {
            skin: MODERN,
            bands: None,
            sections: vec![
                sec("Style", vec![p("style", "Style"), p("oversampling", "Oversampling")]),
                sec("Limit", vec![big("gain", "Gain"), k("ceiling", "Ceiling"), k("release", "Release")]),
                sec("Options", vec![p("true_peak", "True peak"), p("unity", "Unity gain")]),
            ],
        },
        "deesser" => Panel {
            skin: MODERN,
            bands: None,
            sections: vec![
                sec("Detection", vec![p("mode", "Mode"), p("processing", "Processing"), big("freq", "Frequency"), k("freq_hi", "Upper")]),
                sec("Reduction", vec![k("threshold", "Threshold"), k("range", "Range"), k("lookahead", "Lookahead"), k("link", "Stereo link")]),
                sec("Monitor", vec![p("listen", "Listen")]),
            ],
        },
        "transient" => Panel {
            skin: MODERN,
            bands: None,
            sections: vec![
                sec("Attack", vec![big("attack", "Attack"), p("att_shape", "Shape"), k("att_sens", "Sensitivity"), k("att_dur", "Duration")]),
                sec("Sustain", vec![big("sustain", "Sustain"), p("sus_shape", "Shape"), k("sus_sens", "Sensitivity"), k("sus_dur", "Duration")]),
                sec("Output", vec![p("guard", "Guard"), p("listen", "Listen"), fad("mix", "Mix"), k("output", "Output")]),
            ],
        },
        "reverb" => Panel {
            skin: MODERN,
            bands: Some((6, "Decay band")),
            sections: vec![
                sec("Space", vec![p("mode", "Type"), big("size", "Size"), big("decay", "Decay"), k("predelay", "Pre-delay"), k("width", "Width")]),
                sec("Tone", vec![k("damping", "Damping"), k("lowcut", "Low cut"), k("mod", "Modulation")]),
                sec(
                    "Early reflections",
                    vec![Ctl::Custom("reverb.er"), k("early", "Level"), k("er_size", "Size"), k("er_feed", "Feed"), k("er_lo", "Low cut"), k("er_hi", "High cut"), k("er_freq", "Bell"), k("er_gain", "Bell gain")],
                ),
                sec("Late reverb", vec![Ctl::Custom("reverb.late"), k("late", "Level"), k("late_lo", "Low cut"), k("late_hi", "High cut"), k("late_freq", "Bell"), k("late_gain", "Bell gain")]),
                sec(format!("Decay band {b}"), vec![Ctl::Bands, p(format!("dr_t{b}"), "Shape"), k(format!("dr_f{b}"), "Frequency"), k(format!("dr_q{b}"), "Q"), k(format!("dr_{b}"), "Time ×")]),
                sec("Mix", vec![fad("mix", "Mix")]),
            ],
        },
        "delay" => Panel {
            skin: MODERN,
            bands: Some((4, "Slot")),
            sections: vec![
                sec("Time", vec![p("sync", "Sync"), big("time", "Time"), k("offset", "L/R offset"), p("mode", "Mode")]),
                sec("Feedback", vec![big("feedback", "Feedback"), k("cross", "Cross-feed"), k("diffusion", "Diffusion"), p("freeze", "Freeze")]),
                sec("Colour", vec![k("lowcut", "Low cut"), k("highcut", "High cut"), k("reso", "Resonance"), k("drive", "Drive"), k("pitch", "Pitch")]),
                sec("Motion", vec![k("mod", "Modulation"), k("duck", "Ducking"), k("width", "Width")]),
                sec("Mix", vec![fad("mix", "Mix")]),
                sec("LFO 1", vec![p("lfo1_sync", "Sync"), k("lfo1_rate", "Rate"), p("lfo1_shape", "Shape")]),
                sec("LFO 2", vec![p("lfo2_sync", "Sync"), k("lfo2_rate", "Rate"), p("lfo2_shape", "Shape")]),
                sec(format!("Modulation slot {b}"), vec![Ctl::Bands, p(format!("m{b}_src"), "Source"), p(format!("m{b}_dst"), "Target"), k(format!("m{b}_amt"), "Amount")]),
            ],
        },
        "cinnabar" => Panel {
            skin: MODERN,
            bands: None,
            sections: vec![
                sec("Osc 1", vec![p("o1_wave", "Wave"), k("o1_level", "Level"), k("o1_coarse", "Coarse"), k("o1_fine", "Fine"), k("o1_pw", "Width")]),
                sec("Osc 2", vec![p("o2_wave", "Wave"), k("o2_level", "Level"), k("o2_coarse", "Coarse"), k("o2_fine", "Fine"), k("o2_pw", "Width")]),
                sec("Osc 3", vec![p("o3_wave", "Wave"), k("o3_level", "Level"), k("o3_coarse", "Coarse"), k("o3_fine", "Fine"), k("o3_pw", "Width")]),
                sec("Filter", vec![p("filter", "Mode"), big("cutoff", "Cutoff"), k("resonance", "Resonance"), k("filter_env", "Env amount")]),
                sec("Filter envelope", vec![k("fa", "Attack"), k("fd", "Decay"), k("fs", "Sustain"), k("fr", "Release")]),
                sec("Amp envelope", vec![k("aa", "Attack"), k("ad", "Decay"), k("as", "Sustain"), k("ar", "Release")]),
                sec("Unison", vec![p("unison", "Voices"), k("detune", "Detune")]),
                sec("LFO", vec![k("lfo_rate", "Rate"), k("lfo_pitch", "To pitch"), k("lfo_filter", "To filter")]),
                sec("Voice", vec![k("glide", "Glide"), p("polyphony", "Polyphony"), big("gain", "Gain")]),
            ],
        },
        "wyrmdrum" => {
            Panel { skin: MODERN, bands: None, sections: vec![sec("Selected pad", vec![big("gain", "Level"), k("pan", "Pan"), k("pitch", "Pitch")]), sec("Playing", vec![p("choke", "Choke group"), p("note", "Note")])] }
        }
        "chorus" => Panel { skin: MODERN, bands: None, sections: vec![sec("Chorus", vec![big("rate", "Rate"), big("depth", "Depth"), k("delay", "Delay")]), sec("Mix", vec![fad("mix", "Mix")])] },
        "flanger" => {
            Panel { skin: MODERN, bands: None, sections: vec![sec("Flanger", vec![big("rate", "Rate"), big("depth", "Depth"), k("feedback", "Feedback"), k("delay", "Delay")]), sec("Mix", vec![fad("mix", "Mix")])] }
        }
        "phaser" => Panel {
            skin: MODERN,
            bands: None,
            sections: vec![sec("Phaser", vec![big("rate", "Rate"), big("depth", "Depth"), p("stages", "Stages"), k("feedback", "Feedback"), k("center", "Centre")]), sec("Mix", vec![fad("mix", "Mix")])],
        },
        "tremolo" => {
            Panel { skin: MODERN, bands: None, sections: vec![sec("Rate", vec![p("sync", "Sync"), k("rate", "Free rate")]), sec("Motion", vec![big("depth", "Depth"), p("shape", "Shape"), p("pan", "Auto-pan")])] }
        }
        "orbit" => Panel {
            skin: MODERN,
            bands: None,
            sections: vec![
                sec("Motion", vec![p("mode", "Mode"), p("shape", "Shape"), p("sync", "Sync rate"), k("rate", "Free rate"), k("phase", "Phase"), k("smooth", "Smooth")]),
                sec("Field", vec![big("width", "Width"), big("center", "Centre")]),
                sec("Trigger", vec![k("threshold", "Threshold"), k("glide", "Glide")]),
                sec("Output", vec![p("law", "Pan law"), p("binaural", "Binaural")]),
            ],
        },
        "saturator" => Panel {
            skin: VALVE,
            bands: None,
            sections: vec![
                sec("Ingredients", vec![k("ing_warm", "Warm tube"), k("ing_hot", "Hot triode"), k("ing_crisp", "Crisp pentode")]),
                sec("Character", vec![p("mode", "Mode"), p("tube", "Tube")]),
                sec("Drive", vec![big("drive", "Drive"), k("bias", "Bias"), k("tone", "Tone")]),
                sec("Output", vec![k("output", "Output"), big("mix", "Mix (phial)")]),
            ],
        },
        "multisat" => Panel {
            skin: VALVE,
            bands: Some((4, "Band")),
            sections: vec![
                sec("Crossovers", vec![p("bands", "Bands"), k("x1", "X-over 1"), k("x2", "X-over 2"), k("x3", "X-over 3")]),
                sec(format!("Band {b}"), vec![Ctl::Bands, p(bp("style"), "Style"), big(bp("drive"), "Drive"), k(bp("dyn"), "Dynamics"), k(bp("tone"), "Tone")]),
                sec("Feedback", vec![k(bp("fb"), "Amount"), k(bp("fb_freq"), "Frequency")]),
                sec("Band output", vec![k(bp("level"), "Level"), k(bp("mix"), "Mix")]),
                sec("Output", vec![fad("mix", "Mix"), k("output", "Output")]),
            ],
        },
        "bitcrusher" => Panel { skin: MODERN, bands: None, sections: vec![sec("Crush", vec![big("bits", "Bits"), big("downsample", "Downsample")]), sec("Mix", vec![fad("mix", "Mix")])] },
        "filter" => Panel {
            skin: MODERN,
            bands: None,
            sections: vec![
                sec("Filter A", vec![p("type", "Type"), p("slope", "Slope"), p("style", "Style"), big("cutoff", "Cutoff"), k("resonance", "Resonance"), k("drive", "Drive")]),
                sec("Modulation", vec![k("lfo_rate", "LFO rate"), k("lfo_depth", "LFO depth"), k("env_depth", "Envelope")]),
                sec("Filter B", vec![p("b_on", "On"), p("b_type", "Type"), p("b_slope", "Slope"), k("b_cutoff", "Cutoff"), k("b_resonance", "Resonance"), k("b_follow", "Follow A")]),
                sec("Output", vec![p("routing", "Routing"), k("gain", "Gain"), k("pan", "Pan"), fad("mix", "Mix")]),
            ],
        },
        "utility" => Panel {
            skin: STUDIO,
            bands: None,
            sections: vec![
                sec("Level", vec![big("gain", "Gain"), k("pan", "Pan"), k("width", "Width")]),
                sec("Polarity & routing", vec![p("invert_l", "Invert L"), p("invert_r", "Invert R"), p("swap", "Swap L/R"), p("mono", "Mono"), p("dc", "DC filter")]),
            ],
        },
        "imager" => Panel { skin: STUDIO, bands: None, sections: vec![sec("Stereo", vec![big("width", "Width"), k("mono_below", "Mono below"), k("output", "Output")])] },
        "dragon_particle" => Panel {
            skin: STUDIO,
            bands: None,
            sections: vec![
                sec("Drive", vec![p("character", "Character"), big("amount", "Amount"), k("input", "Input"), k("punch", "Punch"), k("clip", "Clip")]),
                sec("Tone", vec![k("low", "Low"), k("mid", "Mid"), k("high", "High")]),
                sec("Output", vec![p("auto_gain", "Auto gain"), k("width", "Width"), k("ceiling", "Ceiling")]),
            ],
        },
        "tune" => Panel {
            skin: MODERN,
            bands: None,
            sections: vec![
                sec("Key", vec![p("key", "Key"), p("scale", "Scale")]),
                sec("Correction", vec![big("speed", "Retune speed"), k("amount", "Amount"), k("humanize", "Humanize"), k("flex", "Flex-tune"), k("stabilizer", "Stabilizer")]),
                sec("Colour", vec![k("vibrato", "Vibrato"), k("doubler", "Doubler")]),
                sec("Mix", vec![fad("mix", "Mix")]),
            ],
        },
        "clean_voice" => Panel {
            skin: STUDIO,
            bands: None,
            sections: vec![
                sec("Leveling", vec![p("tube", "Tube"), big("comp", "Comp"), k("varimu", "Vari-mu")]),
                sec("De-esser", vec![p("ds_detect", "Detection"), p("ds_band", "Band"), big("ds_thresh", "Threshold"), k("ds_range", "Range")]),
                sec("Multiband I", vec![k("mb1_low", "Low mids"), k("mb1_high", "High mids")]),
                sec("Multiband II", vec![k("mb2_low", "Low mids"), k("mb2_high", "High mids")]),
                sec("Finish", vec![big("presence", "Presence"), k("saturation", "Saturation"), k("soothe", "Tame")]),
            ],
        },
        "class_a_strip" => Panel {
            skin: CONSOLE,
            bands: None,
            sections: vec![
                sec("Gate", vec![fad("ns", "Suppress")]),
                sec("Preamp", vec![big("drive", "Drive")]),
                sec("EQ", vec![p("eq_on", "EQ in"), p("hpf", "High-pass"), k("low_gain", "Low"), k("mid_gain", "Mid"), k("high_gain", "High 12k"), p("low_freq", "Low freq"), p("mid_freq", "Mid freq")]),
                sec("Opto comp", vec![p("comp_on", "Comp in"), p("comp_mode", "Timing"), k("threshold", "Threshold"), k("ratio", "Ratio"), k("attack", "Attack"), k("release", "Release"), k("comp_gain", "Gain")]),
                sec("Output", vec![p("order", "Order"), big("output", "Output")]),
            ],
        },
        "drum_bus" => Panel {
            skin: CONSOLE,
            bands: None,
            sections: vec![
                sec("Drive", vec![p("colour", "Colour"), big("drive", "Drive"), k("input", "Input"), k("drive_hp", "High-pass")]),
                sec("Harmonics", vec![p("focus", "Focus"), big("harmonics", "Amount")]),
                sec("Compression", vec![p("comp_mode", "Mode"), big("comp_mix", "Mix"), k("lift", "Lift")]),
                sec("Harmonic EQ", vec![fad("heq1", "60"), fad("heq2", "180"), fad("heq3", "900"), fad("heq4", "3.5k"), fad("heq5", "10k")]),
                sec("Parallels", vec![k("sub", "Sub"), k("low", "Low"), k("loud", "Loud"), k("grit", "Grit")]),
                sec("Output", vec![big("output", "Output")]),
            ],
        },
        "alter" => Panel {
            skin: VALVE,
            bands: None,
            sections: vec![
                sec("Voice", vec![big("pitch", "Pitch"), big("formant", "Formant"), p("link", "Link")]),
                sec("Character", vec![p("mode", "Mode"), k("drive", "Drive")]),
                sec("Output", vec![fad("mix", "Mix"), k("output", "Output")]),
            ],
        },
        _ => return None,
    };
    // Every stock effect is an analog unit in its own finish.
    let mut panel = panel;
    panel.skin = super::analog::skin(super::analog::finish_of(id));
    Some(panel)
}

// ------------------------------------------------------------------------------------------- drawing

/// Draws `text` centred at `at` within `width`: smaller, then elided, if it does not fit.
fn fit_text(p: &egui::Painter, at: Pos2, align: egui::Align2, text: &str, width: f32, size: f32, color: Color32) {
    let mut s = size;
    loop {
        let g = p.layout_no_wrap(text.to_string(), egui::FontId::proportional(s), color);
        if g.size().x <= width || s <= 8.0 {
            break;
        }
        s -= 0.5;
    }
    let mut chars: Vec<char> = text.chars().collect();
    let mut t = text.to_string();
    while chars.len() > 2 && p.layout_no_wrap(t.clone(), egui::FontId::proportional(s), color).size().x > width {
        chars.pop();
        t = chars.iter().collect::<String>() + "…";
    }
    p.text(at, align, t, egui::FontId::proportional(s), color);
}

/// Knob drag / wheel / reset handling on a normalized value. Returns the new value.
pub fn knob_input(ui: &Ui, r: &mut Response, n: f32, default: f32, px: f32) -> f32 {
    let mut n = n;
    if r.dragged() {
        let fine = if ui.input(|i| i.modifiers.shift) { 0.1 } else { 1.0 };
        n = (n - r.drag_delta().y / px * fine).clamp(0.0, 1.0);
        r.mark_changed();
    }
    if r.hovered() {
        let s = ui.input(|i| i.smooth_scroll_delta.y);
        if s != 0.0 {
            n = (n + s / 600.0).clamp(0.0, 1.0);
            r.mark_changed();
        }
    }
    if r.double_clicked() || (r.clicked() && ui.input(|i| i.modifiers.command)) {
        n = default;
        r.mark_changed();
    }
    n
}

pub fn draw_knob(p: &egui::Painter, c: Pos2, rad: f32, n: f32, style: KnobStyle, accent: Color32, active: bool) {
    let a0 = PI * 0.75;
    let sweep = PI * 1.5;
    let ang = a0 + sweep * n;
    let pt = |a: f32, r: f32| Pos2::new(c.x + r * a.cos(), c.y + r * a.sin());
    let arc = |from: f32, to: f32, r: f32| -> Vec<Pos2> { (0..=32).map(|i| from + (to - from) * i as f32 / 32.0).map(|a| pt(a, r)).collect() };
    if active {
        p.circle_filled(c, rad + 5.0, accent.gamma_multiply(0.10));
    }
    // The photographed cap of the style, turned to the value, over a printed scale and the value arc.
    if let Some(cap) = super::analog::knob_cap(p.ctx(), style) {
        let ink = ink();
        for t in 0..=10 {
            let a = a0 + sweep * t as f32 / 10.0;
            let long = t % 5 == 0;
            p.line_segment([pt(a, rad + 1.5), pt(a, rad + if long { 6.0 } else { 4.0 })], Stroke::new(if long { 1.4_f32 } else { 1.0_f32 }, ink));
            if style == KnobStyle::Chicken && rad >= 20.0 && t % 2 == 0 {
                p.text(pt(a, rad + 12.0), egui::Align2::CENTER_CENTER, format!("{t}"), egui::FontId::proportional(8.0), ink);
            }
        }
        p.add(Shape::line(arc(a0, ang, rad - 0.5), Stroke::new(2.0_f32, accent)));
        let size = match style {
            KnobStyle::Chicken => rad * 1.02,
            _ => rad * 0.92,
        };
        p.circle_filled(c + Vec2::new(1.0, size * 0.18), size * 0.9, Color32::from_black_alpha(70));
        p.circle_filled(c + Vec2::new(0.5, size * 0.08), size * 0.95, Color32::from_black_alpha(60));
        super::analog::rotated(p, &cap, c, size, ang + PI / 2.0, if style == KnobStyle::Brass { Color32::from_rgb(0xf0, 0xcc, 0x84) } else { Color32::WHITE });
        return;
    }
    match style {
        KnobStyle::Ring => {
            p.add(Shape::line(arc(a0, a0 + sweep, rad), Stroke::new(3.5_f32, Color32::from_white_alpha(16))));
            p.add(Shape::line(arc(a0, ang, rad), Stroke::new(6.0_f32, accent.gamma_multiply(0.18))));
            p.add(Shape::line(arc(a0, ang, rad), Stroke::new(3.0_f32, accent)));
            p.circle_filled(c, rad - 6.0, Color32::from_rgb(0x2c, 0x31, 0x3b));
            p.circle_filled(c - Vec2::new(0.0, 1.5), rad - 8.0, Color32::from_rgb(0x35, 0x3b, 0x47));
            p.circle_stroke(c, rad - 6.0, Stroke::new(1.0_f32, Color32::from_white_alpha(22)));
            p.line_segment([pt(ang, rad * 0.25), pt(ang, rad - 9.0)], Stroke::new(2.0_f32, Color32::WHITE));
        }
        KnobStyle::Metal | KnobStyle::Brass | KnobStyle::Black => {
            for t in 0..=10 {
                let a = a0 + sweep * t as f32 / 10.0;
                p.line_segment([pt(a, rad + 1.0), pt(a, rad + if t % 5 == 0 { 5.0 } else { 3.0 })], Stroke::new(1.0_f32, Color32::from_white_alpha(110)));
            }
            p.add(Shape::line(arc(a0, ang, rad - 1.5), Stroke::new(2.0_f32, accent)));
            p.circle_filled(c + Vec2::new(0.0, 2.0), rad - 3.0, Color32::from_black_alpha(90));
            p.circle_filled(c, rad - 3.0, Color32::from_gray(150));
            // Brushed rings.
            for j in 1..6 {
                let rr = (rad - 3.0) * j as f32 / 6.0;
                p.circle_stroke(c, rr, Stroke::new(1.0_f32, Color32::from_gray(if j % 2 == 0 { 170 } else { 135 })));
            }
            p.circle_stroke(c, rad - 3.0, Stroke::new(1.5_f32, Color32::from_gray(95)));
            p.line_segment([pt(ang, rad * 0.2), pt(ang, rad - 5.0)], Stroke::new(2.5_f32, Color32::from_gray(20)));
        }
        KnobStyle::Chicken => {
            for t in 0..=10 {
                let a = a0 + sweep * t as f32 / 10.0;
                p.line_segment([pt(a, rad + 1.0), pt(a, rad + 4.0)], Stroke::new(1.0_f32, Color32::from_white_alpha(150)));
                if rad >= 20.0 && t % 2 == 0 {
                    p.text(pt(a, rad + 10.0), egui::Align2::CENTER_CENTER, format!("{t}"), egui::FontId::proportional(8.0), Color32::from_white_alpha(140));
                }
            }
            p.circle_filled(c + Vec2::new(0.0, 2.0), rad * 0.62, Color32::from_black_alpha(100));
            p.circle_filled(c, rad * 0.62, Color32::from_gray(28));
            p.circle_stroke(c, rad * 0.62, Stroke::new(1.0_f32, Color32::from_gray(70)));
            // The pointer: a tapered head reaching to the scale, and a short tail.
            let perp = ang + PI / 2.0;
            let w = rad * 0.22;
            let head = vec![pt(ang, rad - 1.0), Pos2::new(c.x + w * perp.cos(), c.y + w * perp.sin()), pt(ang + PI, rad * 0.5), Pos2::new(c.x - w * perp.cos(), c.y - w * perp.sin())];
            p.add(Shape::convex_polygon(head, Color32::from_gray(18), Stroke::new(1.0_f32, Color32::from_gray(80))));
            p.line_segment([pt(ang, rad * 0.35), pt(ang, rad - 3.0)], Stroke::new(1.5_f32, Color32::WHITE));
        }
        KnobStyle::Cream => {
            for t in 0..=10 {
                let a = a0 + sweep * t as f32 / 10.0;
                p.line_segment([pt(a, rad + 1.0), pt(a, rad + 4.0)], Stroke::new(1.0_f32, Color32::from_rgb(0xd8, 0xc8, 0xa8).gamma_multiply(0.7)));
            }
            p.add(Shape::line(arc(a0, ang, rad - 1.0), Stroke::new(2.0_f32, accent.gamma_multiply(0.8))));
            p.circle_filled(c + Vec2::new(0.0, 2.0), rad - 3.0, Color32::from_black_alpha(110));
            p.circle_filled(c, rad - 3.0, Color32::from_rgb(0xe9, 0xdc, 0xbf));
            p.circle_filled(c - Vec2::new(rad * 0.15, rad * 0.2), rad * 0.45, Color32::from_rgb(0xf6, 0xee, 0xdc));
            p.circle_stroke(c, rad - 3.0, Stroke::new(1.0_f32, Color32::from_rgb(0x8a, 0x78, 0x5a)));
            p.line_segment([pt(ang, rad * 0.15), pt(ang, rad - 5.0)], Stroke::new(2.5_f32, Color32::from_rgb(0xb8, 0x2a, 0x22)));
        }
    }
}

/// What a control did: the parameter, its response and its (new) value.
pub struct Touch<'a> {
    pub prm: &'a ParamInfo,
    pub resp: Response,
    pub value: f32,
}

const CELL_W: f32 = 70.0;

fn knob_cell(ui: &mut Ui, prm: &ParamInfo, v: f32, label: &str, skin: Skin, accent: Color32, big: bool) -> (Response, f32) {
    let (w, rad) = match (big, column()) {
        (true, Some(cw)) => (cw.max(104.0), 30.0),
        (false, Some(_)) => (48.0, 14.0),
        (true, None) => (104.0, 34.0),
        (false, None) => (CELL_W, 19.0),
    };
    let h = rad * 2.0 + 44.0;
    let (rect, mut r) = ui.allocate_exact_size(Vec2::new(w, h), Sense::click_and_drag());
    let n = knob_input(ui, &mut r, prm.normalize(v), prm.normalize(prm.default), if big { 240.0 } else { 180.0 });
    let value = prm.denormalize(n);
    let p = ui.painter_at(rect.expand(4.0));
    let active = r.hovered() || r.dragged();
    fit_text(&p, Pos2::new(rect.center().x, rect.top() + 1.0), egui::Align2::CENTER_TOP, &label.to_uppercase(), w - 4.0, if big { 10.5 } else { 9.5 }, ink());
    let c = Pos2::new(rect.center().x, rect.top() + 18.0 + rad + 4.0);
    draw_knob(&p, c, rad, n, skin.knob, accent, active);
    fit_text(&p, Pos2::new(rect.center().x, rect.bottom() - 1.0), egui::Align2::CENTER_BOTTOM, &shown(prm, value), w - 2.0, 10.0, if active { accent } else { ink_dim() });
    (r, value)
}

fn fader_cell(ui: &mut Ui, prm: &ParamInfo, v: f32, label: &str, accent: Color32) -> (Response, f32) {
    let (w, h) = (48.0, 150.0);
    let (rect, mut r) = ui.allocate_exact_size(Vec2::new(w, h), Sense::click_and_drag());
    let track = Rect::from_center_size(Pos2::new(rect.center().x, rect.top() + 18.0 + (h - 38.0) / 2.0), Vec2::new(6.0, h - 52.0));
    let mut n = prm.normalize(v);
    if r.dragged() {
        let fine = if ui.input(|i| i.modifiers.shift) { 0.1 } else { 1.0 };
        n = (n - r.drag_delta().y / track.height() * fine).clamp(0.0, 1.0);
        r.mark_changed();
    }
    if r.clicked() && !ui.input(|i| i.modifiers.command) {
        if let Some(pos) = r.interact_pointer_pos() {
            n = ((track.bottom() - pos.y) / track.height()).clamp(0.0, 1.0);
            r.mark_changed();
        }
    }
    n = knob_input(ui, &mut r, n, prm.normalize(prm.default), f32::INFINITY);
    let value = prm.denormalize(n);
    let p = ui.painter_at(rect.expand(4.0));
    let active = r.hovered() || r.dragged();
    fit_text(&p, Pos2::new(rect.center().x, rect.top() + 1.0), egui::Align2::CENTER_TOP, &label.to_uppercase(), w + 6.0, 9.5, ink());
    p.rect_filled(track, 3.0, Color32::from_rgb(0x0e, 0x10, 0x14));
    // Bipolar ranges fill from the centre.
    let zero = if prm.min < 0.0 && prm.max > 0.0 { prm.normalize(0.0) } else { 0.0 };
    let (a, b) = (track.bottom() - track.height() * zero, track.bottom() - track.height() * n);
    p.rect_filled(Rect::from_x_y_ranges(track.x_range(), a.min(b)..=a.max(b)), 3.0, accent.gamma_multiply(if active { 0.9 } else { 0.65 }));
    for t in 0..=10 {
        let y = track.bottom() - track.height() * t as f32 / 10.0;
        p.line_segment([Pos2::new(track.right() + 3.0, y), Pos2::new(track.right() + if t % 5 == 0 { 10.0 } else { 6.0 }, y)], Stroke::new(1.0_f32, Color32::from_white_alpha(60)));
    }
    let cap = Rect::from_center_size(Pos2::new(track.center().x, track.bottom() - track.height() * n), Vec2::new(28.0, 12.0));
    p.rect_filled(cap.translate(Vec2::new(0.0, 2.0)), 3.0, Color32::from_black_alpha(100));
    p.rect_filled(cap, 3.0, if active { Color32::from_gray(230) } else { Color32::from_gray(200) });
    p.line_segment([Pos2::new(cap.left() + 3.0, cap.center().y), Pos2::new(cap.right() - 3.0, cap.center().y)], Stroke::new(1.5_f32, Color32::from_gray(40)));
    fit_text(&p, Pos2::new(rect.center().x, rect.bottom() - 1.0), egui::Align2::CENTER_BOTTOM, &shown(prm, value), w + 6.0, 10.0, if active { accent } else { ink_dim() });
    (r, value)
}

fn switch_cell(ui: &mut Ui, v: f32, label: &str, accent: Color32) -> (Response, f32) {
    let on = v > 0.5;
    let text = ui.painter().layout_no_wrap(label.to_uppercase(), egui::FontId::proportional(9.5), theme::TEXT).size().x;
    let w = if column().is_some() { (text + 24.0).clamp(40.0, 130.0) } else { (text + 30.0).clamp(CELL_W, 130.0) };
    let (rect, mut r) = ui.allocate_exact_size(Vec2::new(w, 26.0), Sense::click());
    let mut v = v;
    if r.clicked() {
        v = if on { 0.0 } else { 1.0 };
        r.mark_changed();
    }
    let p = ui.painter();
    // A dark button on any finish, lit by its LED colour when on.
    p.rect_filled(rect, 5.0, Color32::from_rgb(0x14, 0x17, 0x1c));
    if on {
        p.rect_filled(rect, 5.0, accent.gamma_multiply(0.22));
    }
    p.rect_stroke(rect, 5.0, Stroke::new(1.0_f32, if on { accent } else { Color32::from_white_alpha(r.hovered() as u8 * 30 + 25) }), egui::StrokeKind::Inside);
    let led = Pos2::new(rect.left() + 11.0, rect.center().y);
    if on {
        p.circle_filled(led, 6.5, accent.gamma_multiply(0.25));
    }
    p.circle_filled(led, 3.5, if on { accent } else { Color32::from_gray(55) });
    fit_text(p, Pos2::new(rect.left() + 20.0, rect.center().y), egui::Align2::LEFT_CENTER, &label.to_uppercase(), w - 24.0, 9.5, if on { theme::TEXT } else { theme::TEXT_DIM });
    (r, v)
}

/// A choice: segmented buttons when the names are short, a dropdown otherwise.
fn choice_cell(ui: &mut Ui, prm: &ParamInfo, names: &[&str], v: f32, label: &str, accent: Color32, salt: egui::Id) -> (Response, f32) {
    let cur = (v.round() as usize).min(names.len().saturating_sub(1));
    let font = egui::FontId::proportional(10.0);
    let widths: Vec<f32> = names.iter().map(|n| ui.painter().layout_no_wrap(n.to_string(), font.clone(), theme::TEXT).size().x + 14.0).collect();
    let total: f32 = widths.iter().sum();
    let mut out = cur;
    let r = ui
        .vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 3.0;
            let limit = column().unwrap_or(300.0);
            let (lr, _) = ui.allocate_exact_size(Vec2::new(total.clamp(CELL_W, 300.0).min(if names.len() <= 5 { 300.0 } else { 170.0 }).min(limit), 12.0), Sense::hover());
            fit_text(ui.painter(), lr.left_top(), egui::Align2::LEFT_TOP, &label.to_uppercase(), lr.width(), 9.5, ink());
            if names.len() <= 5 && total <= limit {
                let (rect, mut resp) = ui.allocate_exact_size(Vec2::new(total, 24.0), Sense::click());
                let p = ui.painter();
                p.rect_filled(rect, 5.0, Color32::from_rgb(0x14, 0x17, 0x1c));
                let mut x = rect.left();
                for (k, n) in names.iter().enumerate() {
                    let b = Rect::from_min_size(Pos2::new(x, rect.top()), Vec2::new(widths[k], rect.height()));
                    x += widths[k];
                    let hov = resp.hover_pos().is_some_and(|h| b.contains(h));
                    if k == cur {
                        p.rect_filled(b.shrink(2.0), 4.0, accent.gamma_multiply(0.35));
                        p.rect_stroke(b.shrink(2.0), 4.0, Stroke::new(1.0_f32, accent), egui::StrokeKind::Inside);
                    } else if hov {
                        p.rect_filled(b.shrink(2.0), 4.0, Color32::from_white_alpha(12));
                    }
                    p.text(b.center(), egui::Align2::CENTER_CENTER, *n, font.clone(), if k == cur { Color32::WHITE } else { theme::TEXT_DIM });
                    if resp.clicked() && resp.interact_pointer_pos().is_some_and(|h| b.contains(h)) && k != cur {
                        out = k;
                        resp.mark_changed();
                    }
                }
                resp
            } else {
                let mut idx = cur;
                let resp = egui::ComboBox::from_id_salt(salt.with(prm.id)).width(column().map(|w| (w - 24.0).min(150.0)).unwrap_or(150.0)).selected_text(names.get(cur).copied().unwrap_or("?")).show_ui(ui, |ui| {
                    for (k, n) in names.iter().enumerate() {
                        ui.selectable_value(&mut idx, k, *n);
                    }
                });
                let mut resp = resp.response;
                if idx != cur {
                    out = idx;
                    resp.mark_changed();
                }
                resp
            }
        })
        .inner;
    (r, out as f32)
}

/// The width a control's cell takes (the same numbers the cells allocate).
fn cell_width(ui: &Ui, def: &EffectDef, c: &Ctl) -> f32 {
    let (id, label) = match c {
        Ctl::Custom(_) => return 264.0,
        Ctl::Bands => return panel_bands(def.id).map(|(n, _)| bands_width(n)).unwrap_or(0.0),
        Ctl::Knob(id, l) | Ctl::Big(id, l) | Ctl::Fader(id, l) | Ctl::Param(id, l) => (id.as_str(), *l),
    };
    let Some(prm) = def.params.iter().find(|x| x.id == id) else { return 0.0 };
    let text = |t: &str, size: f32| ui.painter().layout_no_wrap(t.to_string(), egui::FontId::proportional(size), theme::TEXT).size().x;
    match (c, prm.kind) {
        (_, ParamKind::Toggle) => (text(&label.to_uppercase(), 9.5) + 30.0).clamp(CELL_W, 130.0),
        (_, ParamKind::Choice(names)) => {
            let total: f32 = names.iter().map(|n| text(n, 10.0) + 14.0).sum();
            if names.len() <= 5 && total <= 300.0 {
                total.max(CELL_W)
            } else {
                // The dropdown (150) plus egui's arrow and margins.
                170.0
            }
        }
        (Ctl::Fader(..), _) => 48.0,
        (Ctl::Big(..), _) => 104.0,
        _ => CELL_W,
    }
}

/// A cell's width inside a column `cw` wide (the numbers the compact cells allocate).
fn column_cell_width(ui: &Ui, def: &EffectDef, c: &Ctl, cw: f32) -> f32 {
    let (id, label) = match c {
        Ctl::Custom(_) => return cw,
        Ctl::Bands => return panel_bands(def.id).map(|(n, _)| bands_width(n)).unwrap_or(0.0),
        Ctl::Big(..) => return cw.max(104.0),
        Ctl::Knob(id, l) | Ctl::Fader(id, l) | Ctl::Param(id, l) => (id.as_str(), *l),
    };
    let Some(prm) = def.params.iter().find(|x| x.id == id) else { return 0.0 };
    let text = |t: &str, size: f32| ui.painter().layout_no_wrap(t.to_string(), egui::FontId::proportional(size), theme::TEXT).size().x;
    match (c, prm.kind) {
        (_, ParamKind::Toggle) => (text(&label.to_uppercase(), 9.5) + 24.0).clamp(40.0, 130.0),
        (_, ParamKind::Choice(names)) => {
            let total: f32 = names.iter().map(|n| text(n, 10.0) + 14.0).sum();
            if names.len() <= 5 && total <= cw {
                total.max(text(&label.to_uppercase(), 9.5))
            } else {
                cw
            }
        }
        _ => 48.0,
    }
}

fn panel_bands(id: &str) -> Option<(usize, &'static str)> {
    panel(id, 1).and_then(|p| p.bands)
}

fn bands_width(n: usize) -> f32 {
    n as f32 * 28.0 - 2.0
}

/// Numbered buttons picking the band the section shows.
fn band_picker(ui: &mut Ui, n: usize, label: &str, band: &mut usize, accent: Color32) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 3.0;
        let (lr, _) = ui.allocate_exact_size(Vec2::new(bands_width(n), 12.0), Sense::hover());
        fit_text(ui.painter(), lr.left_top(), egui::Align2::LEFT_TOP, &label.to_uppercase(), lr.width(), 9.5, ink());
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for b in 1..=n {
                let sel = *band == b;
                let (r, resp) = ui.allocate_exact_size(Vec2::new(26.0, 24.0), Sense::click());
                let p = ui.painter();
                p.rect_filled(r, 4.0, if sel { accent.gamma_multiply(0.35) } else { Color32::from_rgb(0x14, 0x17, 0x1c) });
                p.rect_stroke(r, 4.0, Stroke::new(1.0_f32, if sel { accent } else { Color32::from_white_alpha(if resp.hovered() { 50 } else { 20 }) }), egui::StrokeKind::Inside);
                p.text(r.center(), egui::Align2::CENTER_CENTER, format!("{b}"), egui::FontId::proportional(11.0), if sel { Color32::WHITE } else { theme::TEXT_DIM });
                if resp.clicked() {
                    *band = b;
                }
            }
        });
    });
}

/// A value as the panel shows it: 0 … 1 amounts without a unit as percent.
fn shown(prm: &ParamInfo, v: f32) -> String {
    if prm.unit.is_empty() && prm.min == 0.0 && prm.max == 1.0 {
        format!("{:.0} %", v * 100.0)
    } else {
        prm.format(v)
    }
}

const GAP: f32 = 6.0;
/// A group's padding: 8 px margin and its 1 px engraved edge on each side.
const SECTION_PAD: f32 = 18.0;
/// Where `show` leaves the narrowest width its panel needs (read by the editor window for its minimum size).
pub const NEED_KEY: &str = "fx.panel.need";
/// The output column's width in a column layout.
const COLUMN_OUT: f32 = 192.0;

/// What a panel shows: the effect, its parameter values, the accent colour and an id salt.
pub struct View<'a> {
    pub def: &'a EffectDef,
    pub params: &'a BTreeMap<String, f32>,
    pub accent: Color32,
    pub salt: egui::Id,
}

/// Draws the effect's panel. `band` is the band shown (picked here when the effect has bands); `touch`
/// gets every control's response with its new value (apply it when `resp.changed()`); `custom` draws the
/// layout's custom cells.
pub fn show(ui: &mut Ui, view: View, band: &mut usize, touch: &mut dyn FnMut(&mut Ui, Touch), custom: &mut dyn FnMut(&mut Ui, &str)) -> bool {
    let View { def, params, accent, salt } = view;
    let mut rects: Vec<(String, Rect)> = Vec::new();
    let Some(panel) = panel(def.id, *band) else { return false };
    let skin = panel.skin;
    INK.with(|c| c.set((skin.ink, skin.ink_dim)));
    if flow(def.id) {
        flow_layout(ui, &panel, def, params, accent, salt, touch, custom, &mut rects);
        ui.data_mut(|d| d.insert_temp(salt.with("rects"), rects));
        return true;
    }
    // The editor draws the faceplate (skin.face) around the display and this panel: here only the groups,
    // engraved into it.
    egui::Frame::NONE.show(ui, |ui| {
        ui.set_width(ui.available_width());
        // Pack the sections into rows that fit the editor's width; a section wider than that wraps its
        // own controls.
        let avail = ui.available_width().max(200.0);
        // The narrowest the panel can go without cutting a control: its widest cell (or a column row).
        let mut need = panel.sections.iter().flat_map(|s| s.ctls.iter()).map(|c| cell_width(ui, def, c) + SECTION_PAD).fold(0.0f32, f32::max);
        if columns(def.id) {
            let n = panel.sections.len().saturating_sub(1) as f32;
            need = need.max(130.0 * n + 8.0 * (n - 1.0).max(0.0));
        }
        ui.ctx().data_mut(|d| d.insert_temp(NEED_KEY.into(), need + 16.0));
        let widths: Vec<f32> = panel
            .sections
            .iter()
            .map(|s| {
                let cells: f32 = s.ctls.iter().map(|c| cell_width(ui, def, c)).sum::<f32>() + GAP * (s.ctls.len().saturating_sub(1)) as f32;
                let title = ui.painter().layout_no_wrap(s.title.to_uppercase(), egui::FontId::proportional(10.0), theme::TEXT).size().x;
                (cells.max(title) + SECTION_PAD).min(avail)
            })
            .collect();
        // First fit: a section goes into the first row it fits in, so small sections fill the gaps.
        let mut rows: Vec<Vec<usize>> = Vec::new();
        let mut used: Vec<f32> = Vec::new();
        for (k, w) in widths.iter().enumerate() {
            match used.iter().position(|u| u + 8.0 + w <= avail) {
                Some(r) => {
                    rows[r].push(k);
                    used[r] += 8.0 + w;
                }
                None => {
                    rows.push(vec![k]);
                    used.push(*w);
                }
            }
        }
        let main_accent = accent;
        let mut draw = |ui: &mut Ui, k: usize, inner: f32, accent: Color32, dim: bool| {
            let s = &panel.sections[k];
            ui.scope(|ui| {
                if dim {
                    ui.multiply_opacity(0.4);
                }
                let edge = if skin.knob == KnobStyle::Brass { Color32::from_rgba_unmultiplied(0xe2, 0xc0, 0x76, 70) } else { Color32::from_black_alpha(90) };
                egui::Frame::NONE.fill(skin.section).corner_radius(6.0).inner_margin(egui::Margin::symmetric(8, 6)).stroke(Stroke::new(1.0_f32, edge)).show(ui, |ui| {
                    ui.set_width(inner);
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new(s.title.to_uppercase()).size(10.0).strong().color(if accent == main_accent { skin.ink } else { accent }));
                        // A lone control named like its section needs no label of its own.
                        let lone = s.ctls.len() == 1;
                        let mut cell = |ui: &mut Ui, c: &Ctl| {
                            let (id, label) = match c {
                                Ctl::Custom(name) => {
                                    custom(ui, name);
                                    return;
                                }
                                Ctl::Bands => {
                                    if let Some((n, label)) = panel.bands {
                                        band_picker(ui, n, label, band, accent);
                                    }
                                    return;
                                }
                                Ctl::Knob(id, l) | Ctl::Big(id, l) | Ctl::Fader(id, l) | Ctl::Param(id, l) => (id.as_str(), *l),
                            };
                            let label = if lone && label.eq_ignore_ascii_case(&s.title) { "" } else { label };
                            let Some(prm) = def.params.iter().find(|x| x.id == id) else { return };
                            let v = effects::value(def, params, id);
                            let (resp, value) = match (c, prm.kind) {
                                (_, ParamKind::Toggle) => switch_cell(ui, v, label, accent),
                                (_, ParamKind::Choice(names)) => choice_cell(ui, prm, names, v, label, accent, salt),
                                (Ctl::Fader(..), _) => fader_cell(ui, prm, v, label, accent),
                                (c, _) => knob_cell(ui, prm, v, label, skin, accent, matches!(c, Ctl::Big(..))),
                            };
                            rects.push((prm.id.to_string(), resp.rect));
                            touch(ui, Touch { prm, resp, value });
                        };
                        if let Some(cw) = column() {
                            // Columns: pack the cells into rows that fit the column (compact cells).
                            let mut rows: Vec<Vec<&Ctl>> = vec![Vec::new()];
                            let mut used = 0.0;
                            for c in &s.ctls {
                                let w = column_cell_width(ui, def, c, cw);
                                let need = if rows.last().is_some_and(|r| r.is_empty()) { w } else { used + GAP + w };
                                if need > cw + 0.5 && !rows.last().is_some_and(|r| r.is_empty()) {
                                    rows.push(vec![c]);
                                    used = w;
                                } else {
                                    rows.last_mut().unwrap().push(c);
                                    used = need;
                                }
                            }
                            ui.spacing_mut().item_spacing.y = 6.0;
                            for row in rows {
                                ui.horizontal_top(|ui| {
                                    ui.spacing_mut().item_spacing.x = GAP;
                                    for c in row {
                                        cell(ui, c);
                                    }
                                });
                            }
                        } else {
                            ui.with_layout(egui::Layout::left_to_right(egui::Align::TOP).with_main_wrap(true), |ui| {
                                ui.spacing_mut().item_spacing = Vec2::new(GAP, 6.0);
                                ui.set_max_width(inner);
                                for c in &s.ctls {
                                    cell(ui, c);
                                }
                            });
                        }
                    });
                });
            });
        };
        let n = panel.sections.len().saturating_sub(1);
        let one_row = (avail - COLUMN_OUT - 8.0 * n as f32) / n.max(1) as f32;
        let two_rows = (avail - 8.0 * n.saturating_sub(1) as f32) / n.max(1) as f32;
        if columns(def.id) && n > 0 && two_rows >= 130.0 {
            let count = effects::value(def, params, "bands").round() as usize + 1;
            let bands_row = |ui: &mut Ui, col_w: f32, draw: &mut dyn FnMut(&mut Ui, usize, f32, Color32, bool)| {
                for k in 0..n {
                    let dim = k >= count || effects::value(def, params, &format!("b{}_bypass", k + 1)) > 0.5;
                    COLUMN.with(|c| c.set(Some(col_w - SECTION_PAD)));
                    draw(ui, k, col_w - SECTION_PAD, BAND_COLORS[k % BAND_COLORS.len()], dim);
                }
                COLUMN.with(|c| c.set(None));
            };
            if one_row >= 150.0 {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);
                    bands_row(ui, one_row, &mut draw);
                    COLUMN.with(|c| c.set(Some(COLUMN_OUT - SECTION_PAD)));
                    draw(ui, n, COLUMN_OUT - SECTION_PAD, accent, false);
                    COLUMN.with(|c| c.set(None));
                });
            } else {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);
                    bands_row(ui, two_rows, &mut draw);
                });
                ui.add_space(8.0);
                draw(ui, n, widths[n] - SECTION_PAD, accent, false);
            }
        } else {
            for row in rows {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);
                    for k in row {
                        draw(ui, k, widths[k] - SECTION_PAD, accent, false);
                    }
                });
            }
        }
    });
    // Where each control was drawn (for tests and tooling).
    ui.data_mut(|d| d.insert_temp(salt.with("rects"), rects));
    true
}

// ---------------------------------------------------------------------------------------- flow layout

/// Units laid out as a signal chain: one row of numbered stages, left to right, with arrows between
/// them. Each stage has the same anatomy: its switches and choices on top, its main control large in
/// the middle, the other knobs on an even grid below, faders side by side. Stages share the width and
/// the height of their row.
fn flow(id: &str) -> bool {
    matches!(id, "class_a_strip" | "drum_bus" | "clean_voice" | "dragon_particle")
}

/// The gap between two stages (with the arrow in it).
const ARROW: f32 = 22.0;

#[allow(clippy::too_many_arguments)]
fn flow_ctl(
    ui: &mut Ui,
    c: &Ctl,
    def: &EffectDef,
    params: &BTreeMap<String, f32>,
    skin: Skin,
    accent: Color32,
    salt: egui::Id,
    touch: &mut dyn FnMut(&mut Ui, Touch),
    custom: &mut dyn FnMut(&mut Ui, &str),
    rects: &mut Vec<(String, Rect)>,
    title: &str,
) {
    let (id, label) = match c {
        Ctl::Custom(name) => {
            custom(ui, name);
            return;
        }
        Ctl::Bands => return,
        Ctl::Knob(id, l) | Ctl::Big(id, l) | Ctl::Fader(id, l) | Ctl::Param(id, l) => (id.as_str(), *l),
    };
    let label = if label.eq_ignore_ascii_case(title) { "" } else { label };
    let Some(prm) = def.params.iter().find(|x| x.id == id) else { return };
    let v = effects::value(def, params, id);
    let (resp, value) = match (c, prm.kind) {
        (_, ParamKind::Toggle) => switch_cell(ui, v, label, accent),
        (_, ParamKind::Choice(names)) => choice_cell(ui, prm, names, v, label, accent, salt),
        (Ctl::Fader(..), _) => fader_cell(ui, prm, v, label, accent),
        (c, _) => knob_cell(ui, prm, v, label, skin, accent, matches!(c, Ctl::Big(..))),
    };
    rects.push((prm.id.to_string(), resp.rect));
    touch(ui, Touch { prm, resp, value });
}

#[allow(clippy::too_many_arguments)]
fn flow_layout(
    ui: &mut Ui,
    panel: &Panel,
    def: &EffectDef,
    params: &BTreeMap<String, f32>,
    accent: Color32,
    salt: egui::Id,
    touch: &mut dyn FnMut(&mut Ui, Touch),
    custom: &mut dyn FnMut(&mut Ui, &str),
    rects: &mut Vec<(String, Rect)>,
) {
    let skin = panel.skin;
    let avail = ui.available_width().max(200.0);
    let is_set = |c: &Ctl| match c {
        Ctl::Param(id, _) => def.params.iter().find(|p| p.id == id.as_str()).is_some_and(|p| matches!(p.kind, ParamKind::Toggle | ParamKind::Choice(_))),
        _ => false,
    };
    // Each stage's natural inner width.
    let natural: Vec<f32> = panel
        .sections
        .iter()
        .map(|s| {
            let set = s.ctls.iter().filter(|c| is_set(c)).map(|c| cell_width(ui, def, c)).fold(0.0f32, f32::max);
            let knobs = s.ctls.iter().filter(|c| matches!(c, Ctl::Knob(..))).count();
            let faders = s.ctls.iter().filter(|c| matches!(c, Ctl::Fader(..))).count();
            let big = if s.ctls.iter().any(|c| matches!(c, Ctl::Big(..))) { 104.0 } else { 0.0 };
            let cols = knobs.min(if knobs >= 4 {
                4
            } else if knobs == 3 && big == 0.0 {
                3
            } else {
                2
            }) as f32;
            let grid = if knobs > 0 { cols * CELL_W + (cols - 1.0) * GAP } else { 0.0 };
            let fad = if faders > 0 { faders as f32 * 48.0 + (faders as f32 - 1.0) * GAP } else { 0.0 };
            let title = ui.painter().layout_no_wrap(s.title.to_uppercase(), egui::FontId::proportional(10.5), theme::TEXT).size().x + 26.0;
            set.max(big).max(grid).max(fad).max(title).max(CELL_W)
        })
        .collect();
    // Rows of stages that fit (normally one).
    let mut rows: Vec<Vec<usize>> = vec![Vec::new()];
    let mut used = 0.0;
    for (k, w) in natural.iter().enumerate() {
        let need = w + SECTION_PAD + if rows.last().is_some_and(|r| r.is_empty()) { 0.0 } else { ARROW };
        if used + need > avail && !rows.last().is_some_and(|r| r.is_empty()) {
            rows.push(vec![k]);
            used = w + SECTION_PAD;
        } else {
            rows.last_mut().unwrap().push(k);
            used += need;
        }
    }
    let widest = natural.iter().fold(0.0f32, |a, b| a.max(*b));
    ui.ctx().data_mut(|d| d.insert_temp(NEED_KEY.into(), widest + SECTION_PAD + 16.0));
    let ink = skin.ink;
    let edge = if skin.knob == KnobStyle::Brass { Color32::from_rgba_unmultiplied(0xe2, 0xc0, 0x76, 70) } else { Color32::from_black_alpha(90) };
    ui.spacing_mut().item_spacing.y = 10.0;
    for (ri, row) in rows.iter().enumerate() {
        // Share the spare width out in proportion, so the row fills the unit.
        let base: f32 = row.iter().map(|k| natural[*k] + SECTION_PAD).sum::<f32>() + ARROW * (row.len() - 1) as f32;
        let spare = (avail - base - 2.0).max(0.0).floor();
        let total: f32 = row.iter().map(|k| natural[*k]).sum();
        let hkey = salt.with(("flow.h", ri));
        let prev_h: f32 = ui.ctx().data(|d| d.get_temp(hkey)).unwrap_or(0.0);
        let mut max_h = 0.0f32;
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for (j, k) in row.iter().enumerate() {
                let s = &panel.sections[*k];
                let inner = natural[*k] + spare * natural[*k] / total.max(1.0);
                if j > 0 {
                    // The arrow from the previous stage, level with the titles.
                    let (ar, _) = ui.allocate_exact_size(Vec2::new(ARROW, prev_h.max(30.0)), Sense::hover());
                    let y = ar.top() + 17.0;
                    let x = ar.center().x;
                    ui.painter().line_segment([Pos2::new(x - 7.0, y), Pos2::new(x + 3.0, y)], Stroke::new(1.5_f32, accent.gamma_multiply(0.8)));
                    ui.painter().add(Shape::convex_polygon(vec![Pos2::new(x + 7.0, y), Pos2::new(x + 1.0, y - 4.5), Pos2::new(x + 1.0, y + 4.5)], accent.gamma_multiply(0.8), Stroke::NONE));
                }
                let frame = egui::Frame::NONE.fill(skin.section).corner_radius(7.0).inner_margin(egui::Margin::symmetric(8, 7)).stroke(Stroke::new(1.0_f32, edge)).show(ui, |ui| {
                    ui.set_width(inner);
                    ui.set_max_width(inner);
                    ui.set_min_height((prev_h - 16.0).max(0.0));
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(GAP, 8.0);
                        // Title bar: the stage's number in a ring, its name, a rule.
                        let (tr, _) = ui.allocate_exact_size(Vec2::new(inner, 20.0), Sense::hover());
                        let p = ui.painter();
                        let num = Pos2::new(tr.left() + 8.0, tr.center().y);
                        p.circle_stroke(num, 8.0, Stroke::new(1.2_f32, accent));
                        p.text(num, egui::Align2::CENTER_CENTER, format!("{}", k + 1), egui::FontId::proportional(10.0), accent);
                        p.text(Pos2::new(tr.left() + 22.0, tr.center().y), egui::Align2::LEFT_CENTER, s.title.to_uppercase(), egui::FontId::proportional(10.5), ink);
                        p.line_segment([Pos2::new(tr.left(), tr.bottom() + 2.0), Pos2::new(tr.right(), tr.bottom() + 2.0)], Stroke::new(1.0_f32, ink.gamma_multiply(0.25)));
                        // Switches and choices, side by side where they fit.
                        let mut set_rows: Vec<Vec<&Ctl>> = Vec::new();
                        let mut used = f32::MAX;
                        for c in s.ctls.iter().filter(|c| is_set(c)) {
                            let w = cell_width(ui, def, c);
                            if used + 10.0 + w > inner {
                                set_rows.push(vec![c]);
                                used = w;
                            } else {
                                set_rows.last_mut().unwrap().push(c);
                                used += 10.0 + w;
                            }
                        }
                        for row in set_rows {
                            ui.horizontal_top(|ui| {
                                ui.spacing_mut().item_spacing.x = 10.0;
                                for c in row {
                                    flow_ctl(ui, c, def, params, skin, accent, salt, touch, custom, rects, &s.title);
                                }
                            });
                        }
                        // The main control, centred.
                        for c in s.ctls.iter().filter(|c| matches!(c, Ctl::Big(..))) {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 0.0;
                                ui.add_space(((inner - 104.0) / 2.0).max(0.0));
                                flow_ctl(ui, c, def, params, skin, accent, salt, touch, custom, rects, &s.title);
                            });
                        }
                        // The other knobs on an even grid, centred.
                        let knobs: Vec<&Ctl> = s.ctls.iter().filter(|c| matches!(c, Ctl::Knob(..) | Ctl::Custom(_))).collect();
                        if !knobs.is_empty() {
                            let cols = (((inner + GAP) / (CELL_W + GAP)).floor() as usize).clamp(1, knobs.len());
                            // Balanced rows: 4 knobs in 2 + 2 rather than 3 + 1.
                            let rows_n = knobs.len().div_ceil(cols);
                            let cols = knobs.len().div_ceil(rows_n);
                            for chunk in knobs.chunks(cols) {
                                let w = chunk.len() as f32 * CELL_W + (chunk.len() as f32 - 1.0) * GAP;
                                ui.horizontal_top(|ui| {
                                    ui.spacing_mut().item_spacing.x = GAP;
                                    ui.add_space(((inner - w) / 2.0 - GAP).max(0.0));
                                    for c in chunk {
                                        flow_ctl(ui, c, def, params, skin, accent, salt, touch, custom, rects, &s.title);
                                    }
                                });
                            }
                        }
                        // Faders side by side, centred.
                        let faders: Vec<&Ctl> = s.ctls.iter().filter(|c| matches!(c, Ctl::Fader(..))).collect();
                        if !faders.is_empty() {
                            let w = faders.len() as f32 * 48.0 + (faders.len() as f32 - 1.0) * GAP;
                            ui.horizontal_top(|ui| {
                                ui.spacing_mut().item_spacing.x = GAP;
                                ui.add_space(((inner - w) / 2.0 - GAP).max(0.0));
                                for c in faders {
                                    flow_ctl(ui, c, def, params, skin, accent, salt, touch, custom, rects, &s.title);
                                }
                            });
                        }
                    });
                });
                max_h = max_h.max(frame.response.rect.height());
            }
        });
        if (max_h - prev_h).abs() > 0.5 {
            ui.ctx().data_mut(|d| d.insert_temp(hkey, max_h));
            ui.ctx().request_repaint();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every stock effect has a designed panel, and across its bands the panel shows every parameter,
    /// each once per band view (nothing falls back to the generic knob grid, nothing is shown twice).
    #[test]
    fn every_effect_has_a_complete_panel() {
        for def in effects::registry().iter().filter(|d| !super::super::fx_jade::own_face(d.id) && !super::super::fx_ghost::is_ghost(d.id)) {
            let first = panel(def.id, 1).unwrap_or_else(|| panic!("{} has no panel", def.id));
            let bands = first.bands.map(|b| b.0).unwrap_or(1);
            let mut seen = std::collections::BTreeSet::new();
            for b in 1..=bands {
                let pnl = panel(def.id, b).unwrap();
                let mut here = std::collections::BTreeSet::new();
                for s in &pnl.sections {
                    for c in &s.ctls {
                        let id = match c {
                            Ctl::Knob(id, _) | Ctl::Big(id, _) | Ctl::Fader(id, _) | Ctl::Param(id, _) => id.clone(),
                            Ctl::Custom(_) | Ctl::Bands => continue,
                        };
                        assert!(def.params.iter().any(|p| p.id == id), "{}: unknown parameter {id}", def.id);
                        assert!(here.insert(id.clone()), "{}: {id} shown twice in band {b}", def.id);
                        seen.insert(id);
                    }
                }
            }
            let missing: Vec<&str> = def.params.iter().map(|p| p.id).filter(|id| !seen.contains(*id)).collect();
            assert!(missing.is_empty(), "{}: not on the panel: {missing:?}", def.id);
        }
    }
}
