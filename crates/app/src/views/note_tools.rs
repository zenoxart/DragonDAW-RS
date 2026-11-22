//! the piano-roll tool windows in one window with a tab per tool: Arpeggiator (Alt+A), Riff
//! machine (Alt+E), Limit (Alt+K), Articulate (Alt+L), LFO (Alt+O), Randomize (Alt+R), Strum (Alt+S),
//! Chop (Alt+U), Claw machine (Alt+W), Scale levels (Alt+X) and Score flipper (Alt+Y). Each works on the
//! selected notes (all notes of the channel when nothing is selected) and applies as one undo step.

use crate::state::State;
use dragon_engine::edit::{self, ArpMode, NoteProp, SCALES};
use dragon_engine::model::{Note, PPQ};
use egui::{Context, Slider, Ui};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tool {
    #[default]
    Arpeggiator,
    Riff,
    Limit,
    Articulate,
    Lfo,
    Randomize,
    Strum,
    Chop,
    Claw,
    ScaleLevels,
    Flip,
}

impl Tool {
    pub const ALL: [(Tool, &'static str, &'static str); 11] = [
        (Tool::Arpeggiator, "Arpeggiator", "Alt+A"),
        (Tool::Riff, "Riff machine", "Alt+E"),
        (Tool::Limit, "Limit", "Alt+K"),
        (Tool::Articulate, "Articulate", "Alt+L"),
        (Tool::Lfo, "LFO", "Alt+O"),
        (Tool::Randomize, "Randomize", "Alt+R"),
        (Tool::Strum, "Strum", "Alt+S"),
        (Tool::Chop, "Chop", "Alt+U"),
        (Tool::Claw, "Claw machine", "Alt+W"),
        (Tool::ScaleLevels, "Scale levels", "Alt+X"),
        (Tool::Flip, "Score flipper", "Alt+Y"),
    ];
}

/// The window's settings (kept between openings).
#[derive(Debug, Clone)]
pub struct NoteTools {
    pub open: bool,
    pub tool: Tool,
    arp_mode: usize,
    arp_step: f32,
    arp_gate: f32,
    arp_oct: u32,
    riff_bars: u32,
    riff_density: f32,
    riff_seed: u64,
    limit: (u8, u8),
    art_scale: f32,
    art_gap: bool,
    lfo_prop: usize,
    lfo_rate: f32,
    lfo_center: f32,
    lfo_amount: f32,
    rnd_ticks: u32,
    rnd_vel: f32,
    rnd_pitch: u8,
    rnd_seed: u64,
    strum_ticks: u32,
    chop_step: f32,
    claw: (usize, usize),
    lv_prop: usize,
    lv_scale: f32,
    lv_offset: f32,
    flip_v: bool,
    flip_h: bool,
}

impl Default for NoteTools {
    fn default() -> Self {
        NoteTools {
            open: false,
            tool: Tool::default(),
            arp_mode: 0,
            arp_step: 0.25,
            arp_gate: 0.9,
            arp_oct: 1,
            riff_bars: 2,
            riff_density: 0.5,
            riff_seed: 1,
            limit: (48, 72),
            art_scale: 0.5,
            art_gap: false,
            lfo_prop: 0,
            lfo_rate: 1.0,
            lfo_center: 0.6,
            lfo_amount: 0.3,
            rnd_ticks: 6,
            rnd_vel: 0.1,
            rnd_pitch: 0,
            rnd_seed: 7,
            strum_ticks: 8,
            chop_step: 0.25,
            claw: (1, 1),
            lv_prop: 0,
            lv_scale: 1.0,
            lv_offset: 0.0,
            flip_v: true,
            flip_h: false,
        }
    }
}

const PROPS: [(NoteProp, &str); 3] = [(NoteProp::Velocity, "Velocity"), (NoteProp::Pan, "Pan"), (NoteProp::Release, "Release")];
const ARP: [(ArpMode, &str); 4] = [(ArpMode::Up, "Up"), (ArpMode::Down, "Down"), (ArpMode::UpDown, "Up-down"), (ArpMode::Random, "Random")];

/// Opens the window on a tool (the Alt+key commands).
pub fn open(st: &mut State, tool: Tool) {
    st.ui.note_tools.open = true;
    st.ui.note_tools.tool = tool;
}

pub fn show(ctx: &Context, st: &mut State) {
    if !st.ui.note_tools.open {
        return;
    }
    let mut open = true;
    egui::Window::new("Piano roll tools").id(egui::Id::new("note_tools")).open(&mut open).resizable(false).default_width(360.0).show(ctx, |ui| body(ui, st));
    st.ui.note_tools.open &= open;
}

fn body(ui: &mut Ui, st: &mut State) {
    ui.horizontal_wrapped(|ui| {
        for (t, name, key) in Tool::ALL {
            ui.selectable_value(&mut st.ui.note_tools.tool, t, name).on_hover_text(key);
        }
    });
    ui.separator();
    let t = &mut st.ui.note_tools;
    match t.tool {
        Tool::Arpeggiator => {
            egui::ComboBox::from_label("Pattern").selected_text(ARP[t.arp_mode].1).show_ui(ui, |ui| {
                for (k, (_, n)) in ARP.iter().enumerate() {
                    ui.selectable_value(&mut t.arp_mode, k, *n);
                }
            });
            ui.add(Slider::new(&mut t.arp_step, 0.0625..=1.0).logarithmic(true).text("step (beats)"));
            ui.add(Slider::new(&mut t.arp_gate, 0.1..=1.0).text("gate"));
            ui.add(Slider::new(&mut t.arp_oct, 1..=4).text("octaves"));
        }
        Tool::Riff => {
            ui.label(format!("In the piano roll's key: {}", SCALES[st.ui.scale.unwrap_or(0)].0));
            ui.add(Slider::new(&mut t.riff_bars, 1..=8).text("bars"));
            ui.add(Slider::new(&mut t.riff_density, 0.05..=1.0).text("density"));
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut t.riff_seed).prefix("seed "));
                if ui.button("🎲").on_hover_text("New riff").clicked() {
                    t.riff_seed = t.riff_seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407) % 100_000;
                }
            });
        }
        Tool::Limit => {
            ui.add(Slider::new(&mut t.limit.0, 0..=127).text("lowest").custom_formatter(|v, _| dragon_engine::instruments::note_name(v as u8)));
            ui.add(Slider::new(&mut t.limit.1, 0..=127).text("highest").custom_formatter(|v, _| dragon_engine::instruments::note_name(v as u8)));
        }
        Tool::Articulate => {
            ui.add(Slider::new(&mut t.art_scale, 0.05..=2.0).text("length ×"));
            ui.checkbox(&mut t.art_gap, "End each note before the next one");
        }
        Tool::Lfo => {
            props(ui, &mut t.lfo_prop);
            ui.add(Slider::new(&mut t.lfo_rate, 0.25..=8.0).logarithmic(true).text("cycles per bar"));
            ui.add(Slider::new(&mut t.lfo_center, 0.0..=1.0).text("center"));
            ui.add(Slider::new(&mut t.lfo_amount, 0.0..=1.0).text("amount"));
        }
        Tool::Randomize => {
            ui.add(Slider::new(&mut t.rnd_ticks, 0..=48).text("timing ± ticks"));
            ui.add(Slider::new(&mut t.rnd_vel, 0.0..=0.5).text("velocity ±"));
            ui.add(Slider::new(&mut t.rnd_pitch, 0..=12).text("pitch ± (in key)"));
            ui.add(egui::DragValue::new(&mut t.rnd_seed).prefix("seed "));
        }
        Tool::Strum => {
            ui.add(Slider::new(&mut t.strum_ticks, 1..=48).text("ticks between notes"));
        }
        Tool::Chop => {
            ui.add(Slider::new(&mut t.chop_step, 0.0625..=1.0).logarithmic(true).text("piece (beats)"));
        }
        Tool::Claw => {
            ui.add(Slider::new(&mut t.claw.0, 1..=8).text("keep"));
            ui.add(Slider::new(&mut t.claw.1, 0..=8).text("then drop"));
        }
        Tool::ScaleLevels => {
            props(ui, &mut t.lv_prop);
            ui.add(Slider::new(&mut t.lv_scale, 0.0..=2.0).text("scale"));
            ui.add(Slider::new(&mut t.lv_offset, -1.0..=1.0).text("offset"));
        }
        Tool::Flip => {
            ui.checkbox(&mut t.flip_v, "Flip vertically (pitches)");
            ui.checkbox(&mut t.flip_h, "Flip horizontally (time)");
        }
    }
    ui.separator();
    let target = if st.ui.notes.is_empty() { "all notes".to_string() } else { format!("{} selected notes", st.ui.notes.len()) };
    ui.horizontal(|ui| {
        let ok = st.ui.piano_channel.is_some();
        if ui.add_enabled(ok, egui::Button::new(format!("Apply to {target}"))).clicked() {
            apply(st);
        }
        if !ok {
            ui.label(egui::RichText::new("open a channel in the piano roll").small());
        }
    });
}

fn props(ui: &mut Ui, k: &mut usize) {
    egui::ComboBox::from_label("Property").selected_text(PROPS[*k].1).show_ui(ui, |ui| {
        for (i, (_, n)) in PROPS.iter().enumerate() {
            ui.selectable_value(k, i, *n);
        }
    });
}

/// Runs the current tool on the selection (or all notes) as one undo step.
pub fn apply(st: &mut State) {
    let (pat, Some(ch)) = (st.ui.pattern, st.ui.piano_channel) else { return };
    let t = st.ui.note_tools.clone();
    let sel = st.ui.notes.clone();
    let scale = SCALES[st.ui.scale.unwrap_or(0)].1;
    let root = st.ui.scale_root;
    let name = Tool::ALL.iter().find(|x| x.0 == t.tool).map(|x| x.1).unwrap_or("Tool");
    st.session.edit(name, |p| {
        let Some(c) = p.pattern_mut(pat).map(|pt| pt.channels.entry(ch).or_default()) else { return };
        let picked: Vec<usize> = if sel.is_empty() { (0..c.notes.len()).collect() } else { sel.iter().copied().filter(|i| *i < c.notes.len()).collect() };
        let mut src: Vec<Note> = picked.iter().map(|i| c.notes[*i]).collect();
        let rest: Vec<Note> = c.notes.iter().enumerate().filter(|(i, _)| !picked.contains(i)).map(|(_, n)| *n).collect();
        let ticks = |beats: f32| ((beats * PPQ as f32) as u32).max(1);
        let out: Vec<Note> = match t.tool {
            Tool::Arpeggiator => edit::arpeggiate(&src, ARP[t.arp_mode].0, ticks(t.arp_step), t.arp_gate, t.arp_oct),
            Tool::Riff => {
                let start = src.iter().map(|n| n.start).min().unwrap_or(0);
                let base = 48 + root;
                let mut r = edit::riff(base, scale, t.riff_bars, PPQ / 4, t.riff_density, t.riff_seed);
                r.iter_mut().for_each(|n| n.start += start);
                // The riff replaces the selection (or is added when there are no notes).
                r
            }
            Tool::Limit => {
                edit::limit(&mut src, t.limit.0, t.limit.1);
                src
            }
            Tool::Articulate => {
                edit::articulate(&mut src, t.art_scale, t.art_gap.then_some(PPQ / 32));
                src
            }
            Tool::Lfo => {
                edit::lfo(&mut src, PROPS[t.lfo_prop].0, PPQ * 4, t.lfo_rate, t.lfo_center, t.lfo_amount);
                src
            }
            Tool::Randomize => {
                edit::randomize(&mut src, t.rnd_ticks, t.rnd_vel, t.rnd_pitch, Some((root, scale)), t.rnd_seed);
                src
            }
            Tool::Strum => {
                edit::strum(&mut src, t.strum_ticks);
                src
            }
            Tool::Chop => {
                let step = ticks(t.chop_step);
                src.iter()
                    .flat_map(|n| {
                        let mut v = Vec::new();
                        let mut s = n.start;
                        while s < n.end() {
                            let len = step.min(n.end() - s);
                            v.push(Note { start: s, length: len, ..*n });
                            s += len;
                        }
                        v
                    })
                    .collect()
            }
            Tool::Claw => edit::claw(&src, t.claw.0, t.claw.1),
            Tool::ScaleLevels => {
                edit::scale_levels(&mut src, PROPS[t.lv_prop].0, t.lv_scale, t.lv_offset);
                src
            }
            Tool::Flip => {
                edit::flip(&mut src, t.flip_v, t.flip_h);
                src
            }
        };
        c.notes = rest;
        c.notes.extend(out);
    });
    st.ui.notes.clear();
}
