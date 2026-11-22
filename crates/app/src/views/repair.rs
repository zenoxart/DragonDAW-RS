//! Audio Repair: one audio clip in a tab of its own, in two modes.
//!
//! **Audio repair** – a spectrogram you can zoom and select in (drag: a time range, Shift+drag: a time ×
//! frequency box, Ctrl+A: everything), "Check the recording" (finds hum, clicks, clipping and background
//! noise and fixes each with one click), and the iZotope RX tools (de-noise, de-hum, de-click, de-clip,
//! distortion repair, de-reverb, spectral attenuate, gain), each with Preview (listen first) and Apply.
//!
//! **Pitch correction** – Melodyne-style notes ("blobs") with their pitch curves: Move (drag up / down,
//! semitone snap, Alt = free), Vibrato (drag to scale a note's vibrato and drift), Split; rubber-band
//! and Ctrl+A selection, arrow keys (semitone), Ctrl+arrows (octave), Alt+arrows (10 cents), Delete
//! resets, its own undo (Ctrl+Z), the "correct pitch" macro, zoom in time and pitch.
//!
//! Every Apply renders to a new file and points the clip at it – Ctrl+Z points it back.

use crate::state::State;
use crate::theme;
use dragon_engine::model::{ClipKind, Id};
use dragon_engine::repair::{self, Diagnosis, NoiseProfile, NoteBlob, PitchEdit};
use dragon_engine::samples::AudioData;
use egui::{Color32, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2};
use redhive_contracts::ui::Severity;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

const ROWS: usize = 192;
const MAX_COLS: usize = 2400;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Tool {
    #[default]
    Denoise,
    Dehum,
    Declick,
    Declip,
    Distortion,
    Dereverb,
    Spectral,
    Gain,
    /// The note editor (pitch correction).
    Pitch,
}

const TOOLS: [(Tool, &str, &str); 8] = [
    (Tool::Denoise, "De-noise", "Hiss, rumble, air conditioning – any steady background noise"),
    (Tool::Dehum, "De-hum", "Mains hum (50 / 60 Hz) and its overtones"),
    (Tool::Declick, "De-click", "Clicks, pops, crackle and mouth noise"),
    (Tool::Declip, "De-clip", "Peaks cut flat because the level was too high"),
    (Tool::Distortion, "Distortion repair", "Peaks squashed by soft clipping or an overdriven preamp"),
    (Tool::Dereverb, "De-reverb", "Too much room sound"),
    (Tool::Spectral, "Spectral attenuate", "Take out one sound: draw a box around it (Shift+drag)"),
    (Tool::Gain, "Gain", "Louder or quieter (the selection, or everything)"),
];

/// The note editor's tools (Melodyne's main, pitch modulation and note separation tools).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum NoteTool {
    #[default]
    Move,
    Modulation,
    Split,
}

/// A selection: seconds, and a frequency span in Hz (`None` = all frequencies).
type Selection = (f64, f64, Option<(f32, f32)>);
/// A render's outcome: the new file, the repair's name, and whether it is only a preview.
type RenderSlot = Arc<Mutex<Option<Result<(PathBuf, String, bool), String>>>>;
/// A repair to render.
type Process = Box<dyn FnOnce(&AudioData) -> AudioData + Send>;
/// Makes a repair (Preview and Apply each need one).
type MakeProcess = Box<dyn Fn() -> Process>;
/// A note's pitch curve: (seconds, MIDI pitch).
type Curve = Vec<(f64, f32)>;
/// Note edits that can be undone: the notes (split changes them), their moves and their modulation.
type NoteState = (Vec<NoteBlob>, Vec<f32>, Vec<f32>);

/// Everything the tab keeps between frames.
#[derive(Default)]
pub struct RepairUi {
    pub clip: Option<Id>,
    tool: Tool,
    sel: Option<Selection>,
    drag_from: Option<(f64, f32)>,
    /// The visible time range (seconds); `None` = the whole clip.
    pub(crate) view: Option<(f64, f64)>,
    /// The visible pitch range of the note editor (MIDI).
    pitch_view: Option<(f32, f32)>,
    // Settings.
    reduction: f32,
    sensitivity: f32,
    hum_base: f32,
    hum_harmonics: usize,
    hum_q: f32,
    click_sens: f32,
    clip_threshold: f32,
    dist_amount: f32,
    reverb_amount: f32,
    reverb_time: f32,
    spectral_db: f32,
    gain_db: f32,
    centre_amount: f32,
    drift_amount: f32,
    profile: Option<NoiseProfile>,
    /// Play the original file instead of the repaired one.
    compare: bool,
    // The clip's audio and its pictures (for one source file).
    source: Option<PathBuf>,
    pub(crate) data: Option<Arc<AudioData>>,
    spectro: Option<egui::TextureHandle>,
    wave: Vec<(f32, f32)>,
    loading: Option<Arc<Mutex<Option<Loaded>>>>,
    diagnosis: Option<Diagnosis>,
    // Notes.
    notes: Vec<NoteBlob>,
    shifts: Vec<f32>,
    /// Vibrato / drift scale per note (1 = as recorded, 0 = flat, 2 = double).
    mods: Vec<f32>,
    sel_notes: Vec<usize>,
    note_tool: NoteTool,
    no_snap: bool,
    /// A drag on notes: where it started and the moves / modulation then.
    note_drag: Option<(Pos2, Vec<f32>, Vec<f32>)>,
    rubber: Option<Pos2>,
    history: Vec<NoteState>,
    // A render running.
    job: Option<RenderSlot>,
    job_label: String,
    /// The file before the first repair (A / B against it).
    original: Option<PathBuf>,
}

struct Loaded {
    data: Arc<AudioData>,
    spectro: Vec<Vec<f32>>,
    notes: Vec<NoteBlob>,
    diagnosis: Diagnosis,
}

impl RepairUi {
    fn defaults(&mut self) {
        if self.reduction == 0.0 {
            self.reduction = 12.0;
            self.sensitivity = 1.0;
            self.hum_harmonics = 8;
            self.hum_q = 30.0;
            self.click_sens = 5.0;
            self.clip_threshold = 0.99;
            self.dist_amount = 0.8;
            self.reverb_amount = 0.6;
            self.reverb_time = 0.8;
            self.spectral_db = -30.0;
            self.gain_db = -6.0;
            self.centre_amount = 1.0;
            self.drift_amount = 0.5;
        }
    }

    /// Whether `tool` is the one shown.
    #[cfg(test)]
    pub fn tool_is(&self, tool: Tool) -> bool {
        self.tool == tool
    }

    fn snapshot(&mut self) {
        self.history.push((self.notes.clone(), self.shifts.clone(), self.mods.clone()));
        if self.history.len() > 100 {
            self.history.remove(0);
        }
    }

    /// The pitch (MIDI) of note `k` at `t`, with its move and modulation.
    fn pitch_at(&self, k: usize, t: f64) -> f32 {
        let n = &self.notes[k];
        let raw = curve_at(n, t);
        n.midi + self.shifts[k] + self.mods[k] * (raw - n.midi)
    }

    /// Splits note `k` at `t` (seconds); both halves keep its move and modulation.
    fn split(&mut self, k: usize, t: f64) {
        let Some(n) = self.notes.get(k).cloned() else { return };
        if t <= n.start + 0.03 || t >= n.end - 0.03 {
            return;
        }
        self.snapshot();
        let centre = |c: &[(f64, f32)]| {
            let mut v: Vec<f32> = c.iter().map(|x| x.1).collect();
            v.sort_by(f32::total_cmp);
            v.get(v.len() / 2).copied().unwrap_or(n.midi)
        };
        let (a, b): (Curve, Curve) = n.curve.iter().partition(|x| x.0 < t);
        let first = NoteBlob { start: n.start, end: t, midi: centre(&a), curve: a, level: n.level };
        let second = NoteBlob { start: t, end: n.end, midi: centre(&b), curve: b, level: n.level };
        self.notes[k] = first;
        self.notes.insert(k + 1, second);
        let (s, m) = (self.shifts[k], self.mods[k]);
        self.shifts.insert(k + 1, s);
        self.mods.insert(k + 1, m);
        self.sel_notes = vec![k, k + 1];
    }

    /// The selected notes, or all of them when none is selected.
    fn targets(&self) -> Vec<usize> {
        if self.sel_notes.is_empty() {
            (0..self.notes.len()).collect()
        } else {
            self.sel_notes.clone()
        }
    }

    fn reset_targets(&mut self) {
        self.snapshot();
        for k in self.targets() {
            self.shifts[k] = 0.0;
            self.mods[k] = 1.0;
        }
    }

    fn edited(&self, k: usize) -> bool {
        self.shifts[k].abs() > 0.005 || (self.mods[k] - 1.0).abs() > 0.005
    }
}

/// The MIDI pitch of `note` at time `t` (from its curve).
fn curve_at(note: &NoteBlob, t: f64) -> f32 {
    match note.curve.iter().position(|(ct, _)| *ct >= t) {
        Some(0) => note.curve[0].1,
        Some(i) => {
            let (a, b) = (note.curve[i - 1], note.curve[i]);
            let f = ((t - a.0) / (b.0 - a.0).max(1e-9)) as f32;
            a.1 + (b.1 - a.1) * f
        }
        None => note.curve.last().map(|c| c.1).unwrap_or(note.midi),
    }
}

/// Opens `clip` in the Audio Repair tab on `tool` (`Tool::Pitch` = the note editor).
pub fn open_with(st: &mut State, clip: Id, tool: Tool) {
    open(st, clip);
    st.ui.repair.tool = tool;
}

/// Opens `clip` in the Audio Repair tab.
pub fn open(st: &mut State, clip: Id) {
    if st.ui.repair.clip != Some(clip) {
        st.ui.repair.view = None;
        st.ui.repair.pitch_view = None;
        st.ui.repair.original = None;
        st.ui.repair.compare = false;
    }
    st.ui.repair.clip = Some(clip);
    st.ui.repair.sel = None;
    st.ctx.open("dragon.repair", None);
}

fn clip_source(st: &State, cid: Id) -> Option<(Id, String, PathBuf)> {
    st.session.project.tracks.iter().find_map(|t| t.clips.iter().find(|c| c.id == cid).and_then(|c| c.audio().map(|a| (t.id, c.name.clone(), a.source.clone()))))
}

fn heat(db: f32) -> Color32 {
    let t = ((db + 100.0) / 100.0).clamp(0.0, 1.0);
    const STOPS: [(f32, [u8; 3]); 5] = [(0.0, [6, 6, 14]), (0.35, [40, 20, 90]), (0.6, [170, 30, 70]), (0.82, [250, 140, 40]), (1.0, [255, 245, 200])];
    let k = STOPS.iter().position(|s| s.0 >= t).unwrap_or(STOPS.len() - 1).max(1);
    let (a, b) = (STOPS[k - 1], STOPS[k]);
    let q = ((t - a.0) / (b.0 - a.0)).clamp(0.0, 1.0);
    let m = |i: usize| (a.1[i] as f32 + (b.1[i] as f32 - a.1[i] as f32) * q) as u8;
    Color32::from_rgb(m(0), m(1), m(2))
}

const NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

fn note_name(m: f32) -> String {
    let r = m.round() as i32;
    format!("{}{}", NAMES[r.rem_euclid(12) as usize], r.div_euclid(12) - 1)
}

fn cents(m: f32) -> i32 {
    ((m - m.round()) * 100.0).round() as i32
}

/// Loads the clip's audio, its spectrogram, its notes and its diagnosis (in the background) when its
/// file changed.
fn ensure_loaded(ui: &Ui, st: &mut State, path: &PathBuf) {
    let r = &mut st.ui.repair;
    if r.source.as_ref() == Some(path) || r.loading.is_some() {
        if let Some(l) = r.loading.as_ref().and_then(|l| l.lock().ok().and_then(|mut g| g.take())) {
            let cols = l.spectro.len();
            let mut img = egui::ColorImage::new([cols.max(1), ROWS], Color32::BLACK);
            for (c, col) in l.spectro.iter().enumerate() {
                for (k, db) in col.iter().enumerate() {
                    img.pixels[(ROWS - 1 - k) * cols + c] = heat(*db);
                }
            }
            r.spectro = Some(ui.ctx().load_texture("repair.spectro", img, egui::TextureOptions::LINEAR));
            let d = &l.data;
            let n = MAX_COLS.min(d.frames().max(1));
            r.wave = (0..n)
                .map(|c| {
                    let a = c * d.frames() / n;
                    let b = ((c + 1) * d.frames() / n).max(a + 1).min(d.frames());
                    let (mut lo, mut hi) = (0.0f32, 0.0f32);
                    for i in a..b {
                        let v = 0.5 * (d.l[i] + d.r[i]);
                        lo = lo.min(v);
                        hi = hi.max(v);
                    }
                    (lo, hi)
                })
                .collect();
            r.shifts = vec![0.0; l.notes.len()];
            r.mods = vec![1.0; l.notes.len()];
            r.history.clear();
            r.notes = l.notes;
            r.sel_notes.clear();
            r.pitch_view = None;
            r.diagnosis = Some(l.diagnosis);
            r.data = Some(l.data);
            r.loading = None;
        }
        return;
    }
    let cache = st.session.cache.clone();
    let rate = st.audio.rate();
    let slot = Arc::new(Mutex::new(None));
    let out = slot.clone();
    let p = path.clone();
    std::thread::spawn(move || {
        if let Ok(data) = cache.get(&p, rate) {
            let cols = (data.frames() / 256).clamp(1, MAX_COLS);
            let spectro = repair::spectrogram(&data, cols, ROWS);
            let notes = repair::detect_notes(&data);
            let diagnosis = repair::diagnose(&data);
            if let Ok(mut g) = out.lock() {
                *g = Some(Loaded { data, spectro, notes, diagnosis });
            }
            redhive_client_framework::runtime::request_repaint();
        }
    });
    r.source = Some(path.clone());
    r.loading = Some(slot);
    r.spectro = None;
    r.data = None;
    r.profile = None;
    r.diagnosis = None;
}

/// Renders `label` with `f` in the background; the result becomes the clip's new file.
#[cfg(test)]
fn render(st: &mut State, label: &str, f: impl FnOnce(&AudioData) -> AudioData + Send + 'static) {
    start_render(st, label, Box::new(f), false);
}

/// Renders `f` (only inside the selected time range when there is one) – to a new file for the clip,
/// or, as a preview, to a scratch file that is played at once.
fn start_render(st: &mut State, label: &str, f: Process, preview: bool) {
    let dir = st.recordings_dir();
    let r = &mut st.ui.repair;
    let (Some(data), Some(src)) = (r.data.clone(), r.source.clone()) else { return };
    let sel = r.sel;
    let slot = Arc::new(Mutex::new(None));
    let out = slot.clone();
    let label_s = label.to_string();
    std::thread::spawn(move || {
        let mut y = f(&data);
        if let Some((t0, t1, _)) = sel {
            let rate = data.rate as f64;
            let (a, b) = ((t0.min(t1) * rate) as usize, (t0.max(t1) * rate) as usize);
            let fade = ((0.01 * rate) as usize).max(1);
            for i in 0..data.frames().min(y.frames()) {
                let w = if i + fade < a || i > b + fade {
                    0.0
                } else if i < a {
                    1.0 - (a - i) as f32 / fade as f32
                } else if i > b {
                    1.0 - (i - b) as f32 / fade as f32
                } else {
                    1.0
                };
                y.l[i] = data.l[i] + (y.l[i] - data.l[i]) * w;
                y.r[i] = data.r[i] + (y.r[i] - data.r[i]) * w;
            }
        }
        let path = if preview {
            std::env::temp_dir().join(format!("dragon-repair-preview-{}.wav", std::process::id()))
        } else {
            let stem = src.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "clip".into());
            let stem = stem.split(" (repaired").next().unwrap_or(&stem).to_string();
            let _ = std::fs::create_dir_all(&dir);
            let mut k = 1;
            loop {
                let p = dir.join(format!("{stem} (repaired {k}).wav"));
                if !p.exists() {
                    break p;
                }
                k += 1;
            }
        };
        let res = repair::write_wav(&path, &y).map(|_| (path, label_s, preview)).map_err(|e| e.to_string());
        if let Ok(mut g) = out.lock() {
            *g = Some(res);
        }
        redhive_client_framework::runtime::request_repaint();
    });
    r.job = Some(slot);
    r.job_label = if preview { format!("Preparing the preview of {label}") } else { format!("Applying {label}") };
}

/// Whether a preview is still playing inside the clip (a finished one leaves `preview_started` set).
fn playing(st: &State) -> Option<f64> {
    let started = st.ui.preview_started?;
    let t = st.ui.preview_offset + started.elapsed().as_secs_f64();
    let dur = st.ui.repair.data.as_ref().map(|d| d.duration())?;
    (t <= dur).then_some(t)
}

/// Where playback starts: the selection, else the left edge of the view.
fn play_from(st: &State) -> f64 {
    st.ui.repair.sel.map(|s| s.0.min(s.1)).or(st.ui.repair.view.map(|v| v.0)).unwrap_or(0.0)
}

/// Applies a finished render (every frame).
pub fn poll(st: &mut State) {
    let Some(job) = &st.ui.repair.job else { return };
    let Some(res) = job.lock().ok().and_then(|mut g| g.take()) else { return };
    st.ui.repair.job = None;
    let Some(cid) = st.ui.repair.clip else { return };
    match res {
        Ok((path, label, true)) => {
            // A preview: the cache must not keep an older one under the same name.
            st.session.cache.forget(&path);
            let from = play_from(st);
            st.preview_from(&path, from);
            st.toast(format!("Preview: {label} (not applied yet)"), Severity::Info);
        }
        Ok((path, label, false)) => {
            let old = clip_source(st, cid).map(|x| x.2);
            st.session.edit(&format!("Audio repair: {label}"), |p| {
                for t in &mut p.tracks {
                    if let Some(c) = t.clips.iter_mut().find(|c| c.id == cid) {
                        if let ClipKind::Audio(a) = &mut c.kind {
                            a.source = path.clone();
                        }
                    }
                }
            });
            if st.ui.repair.original.is_none() {
                st.ui.repair.original = old;
            }
            st.toast(format!("{label} applied – Ctrl+Z to undo"), Severity::Success);
            let e = if label == "Pitch correction" { crate::views::quests::Event::NotesEdited } else { crate::views::quests::Event::Repaired };
            crate::views::quests::event(st, e);
        }
        Err(e) => st.toast(format!("Repair failed: {e}"), Severity::Error),
    }
}

// ------------------------------------------------------------------------------------------------ keys

/// Ctrl+A selects everything: all notes (pitch correction) or the whole clip (audio repair).
pub fn select_all(st: &mut State) {
    let r = &mut st.ui.repair;
    if r.tool == Tool::Pitch {
        r.sel_notes = (0..r.notes.len()).collect();
    } else if let Some(d) = &r.data {
        r.sel = Some((0.0, d.duration(), None));
    }
}

/// Moves the selected notes by `semis` (one undo step).
fn nudge(st: &mut State, semis: f32) {
    let r = &mut st.ui.repair;
    if r.sel_notes.is_empty() {
        return;
    }
    r.snapshot();
    for k in r.sel_notes.clone() {
        if let Some(s) = r.shifts.get_mut(k) {
            *s += semis;
        }
    }
}

/// The app's commands while Audio Repair has the focus; `true` = handled here.
pub fn key(st: &mut State, id: &str) -> bool {
    let notes = st.ui.repair.tool == Tool::Pitch;
    match id {
        "edit.select_all" => select_all(st),
        "edit.deselect" => {
            st.ui.repair.sel = None;
            st.ui.repair.sel_notes.clear();
        }
        // Back to how the selected notes were recorded.
        "edit.delete" if notes => st.ui.repair.reset_targets(),
        "edit.delete" => st.ui.repair.sel = None,
        "edit.undo_toggle" if notes => match st.ui.repair.history.pop() {
            Some((n, s, m)) => {
                let r = &mut st.ui.repair;
                r.notes = n;
                r.shifts = s;
                r.mods = m;
                let len = r.notes.len();
                r.sel_notes.retain(|k| *k < len);
            }
            // Nothing of its own left: the project's undo (an applied repair) takes over.
            None => return false,
        },
        "rack.up" | "edit.move_up" if notes => nudge(st, 1.0),
        "rack.down" | "edit.move_down" if notes => nudge(st, -1.0),
        "edit.transpose_up" if notes => nudge(st, 12.0),
        "edit.transpose_down" if notes => nudge(st, -12.0),
        "ctx.alt_up" if notes => nudge(st, 0.1),
        "ctx.alt_down" if notes => nudge(st, -0.1),
        "zoom.in" => zoom_time(st, 0.5, None),
        "zoom.out" => zoom_time(st, 2.0, None),
        _ => return false,
    }
    true
}

/// Zooms the time axis by `factor` (< 1 = in) around `around` (seconds; the middle when `None`).
fn zoom_time(st: &mut State, factor: f64, around: Option<f64>) {
    let r = &mut st.ui.repair;
    let Some(d) = &r.data else { return };
    let dur = d.duration().max(1e-3);
    let (t0, t1) = r.view.unwrap_or((0.0, dur));
    let c = around.unwrap_or((t0 + t1) / 2.0);
    let w = ((t1 - t0) * factor).clamp(0.05_f64.min(dur), dur);
    let a = (c - (c - t0) * w / (t1 - t0)).clamp(0.0, dur - w);
    r.view = if w >= dur { None } else { Some((a, a + w)) };
}

/// Zooms the pitch axis by `factor` (< 1 = in).
fn zoom_pitch(st: &mut State, factor: f32) {
    if let Some((lo, hi)) = st.ui.repair.pitch_view {
        let c = (lo + hi) / 2.0;
        let half = ((hi - lo) / 2.0 * factor).clamp(3.0, 48.0);
        st.ui.repair.pitch_view = Some((c - half, c + half));
    }
}

// ------------------------------------------------------------------------------------------------ view

pub fn show(ui: &mut Ui, st: &mut State) {
    st.ui.repair.defaults();
    if ui.ui_contains_pointer() && ui.input(|i| i.pointer.any_pressed()) {
        st.ui.editor = crate::state::Editor::Repair;
    }
    let Some(cid) = st.ui.repair.clip else {
        ui.add_space(40.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new("Audio Repair").size(22.0));
            ui.label(RichText::new("Double-click an audio clip in the playlist and choose Pitch correction or Audio repair.").color(theme::TEXT_DIM));
        });
        return;
    };
    let Some((_tid, name, path)) = clip_source(st, cid) else {
        st.ui.repair.clip = None;
        return;
    };
    ensure_loaded(ui, st, &path);
    if st.ui.editor == crate::state::Editor::Repair && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        st.ui.repair.sel = None;
        st.ui.repair.sel_notes.clear();
    }
    toolbar(ui, st, &name, &path);
    let Some(data) = st.ui.repair.data.clone() else {
        ui.add_space(30.0);
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label("Looking at the clip: spectrum, notes, problems …");
        });
        return;
    };
    let side = 330.0;
    let hint_h = 22.0;
    ui.horizontal_top(|ui| {
        let w = (ui.available_width() - side - 10.0).max(320.0);
        let h = (ui.available_height() - hint_h - 6.0).max(260.0);
        ui.vertical(|ui| {
            ui.set_width(w);
            let ruler_h = ruler(ui, st, &data, w);
            let body = Vec2::new(w, h - ruler_h - 20.0);
            if st.ui.repair.tool == Tool::Pitch {
                notes_view(ui, st, &data, body);
            } else {
                spectrogram_view(ui, st, &data, body);
            }
            scrollbar(ui, st, &data, w);
        });
        ui.vertical(|ui| {
            ui.set_width(side);
            egui::ScrollArea::vertical().id_salt("repair.side").max_height(h).show(ui, |ui| {
                if st.ui.repair.tool == Tool::Pitch {
                    pitch_panel(ui, st);
                } else {
                    repair_panel(ui, st);
                }
            });
        });
    });
    hints(ui, st);
}

fn toolbar(ui: &mut Ui, st: &mut State, name: &str, path: &std::path::Path) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(name).size(16.0).strong());
        ui.add_space(10.0);
        // The two modes.
        let pitch = st.ui.repair.tool == Tool::Pitch;
        for (on, label, tip) in [(false, "Audio repair", "Noise, hum, clicks, clipping, distortion, reverb"), (true, "Pitch correction", "Notes like in Melodyne: move them, centre them, calm the vibrato")] {
            let sel = pitch == on;
            let b = egui::Button::new(RichText::new(label).size(13.0).strong().color(if sel { Color32::BLACK } else { theme::TEXT }))
                .fill(if sel { theme::ACCENT } else { Color32::from_white_alpha(10) })
                .min_size(Vec2::new(130.0, 26.0));
            if ui.add(b).on_hover_text(tip).clicked() && !sel {
                st.ui.repair.tool = if on { Tool::Pitch } else { Tool::Denoise };
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Close").clicked() {
                st.ui.repair.clip = None;
            }
            ui.separator();
            if ui.button("Fit").on_hover_text("Show the whole clip").clicked() {
                st.ui.repair.view = None;
                st.ui.repair.pitch_view = None;
            }
            let tip = "Like the playlist: Ctrl+wheel zooms at the mouse, Ctrl+right-drag zooms into a box, Ctrl+right-click zooms out, middle-drag pans, Shift+wheel scrolls";
            if ui.button(" + ").on_hover_text(format!("Zoom in time · {tip}")).clicked() {
                zoom_time(st, 0.5, None);
            }
            if ui.button(" − ").on_hover_text(format!("Zoom out time · {tip}")).clicked() {
                zoom_time(st, 2.0, None);
            }
            ui.label(RichText::new("Zoom").small().color(theme::TEXT_DIM));
            if st.ui.repair.tool == Tool::Pitch {
                ui.separator();
                if ui.button(" + ").on_hover_text("Zoom in pitch (Alt+wheel)").clicked() {
                    zoom_pitch(st, 0.6);
                }
                if ui.button(" − ").on_hover_text("Zoom out pitch (Alt+wheel)").clicked() {
                    zoom_pitch(st, 1.6);
                }
                ui.label(RichText::new("Pitch").small().color(theme::TEXT_DIM));
            }
            ui.separator();
            if st.ui.repair.original.as_ref().is_some_and(|o| o != path) {
                ui.toggle_value(&mut st.ui.repair.compare, "Hear the original").on_hover_text("A / B: play the clip as it was before the repairs");
            }
            let playing = playing(st).is_some();
            let label = if playing { "■ Stop" } else { "▶ Play" };
            if ui.add(egui::Button::new(RichText::new(label).strong()).min_size(Vec2::new(70.0, 24.0))).on_hover_text("Click the time ruler to play from a point").clicked() {
                if playing {
                    st.stop_preview();
                } else {
                    let file = if st.ui.repair.compare { st.ui.repair.original.clone().unwrap_or(path.to_path_buf()) } else { path.to_path_buf() };
                    let from = play_from(st);
                    st.preview_from(&file, from);
                }
            }
        });
    });
    ui.add_space(4.0);
}

/// The visible time range (seconds).
fn view_range(st: &State, data: &AudioData) -> (f64, f64) {
    st.ui.repair.view.unwrap_or((0.0, data.duration().max(1e-3)))
}

/// The time ruler: ticks, the playhead, click to play from there. Returns its height.
fn ruler(ui: &mut Ui, st: &mut State, data: &AudioData, w: f32) -> f32 {
    let h = 20.0;
    let (r, resp) = ui.allocate_exact_size(Vec2::new(w, h), Sense::click());
    let p = ui.painter_at(r);
    p.rect_filled(r, 3.0, Color32::from_white_alpha(8));
    let (t0, t1) = view_range(st, data);
    let left = r.left() + 44.0;
    let x_of = |t: f64| left + ((t - t0) / (t1 - t0)) as f32 * (r.right() - left);
    let t_of = |x: f32| t0 + ((x - left) / (r.right() - left)).clamp(0.0, 1.0) as f64 * (t1 - t0);
    let span = t1 - t0;
    let step = [0.01, 0.02, 0.05, 0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0, 60.0].into_iter().find(|s| span / s < 14.0).unwrap_or(120.0);
    let mut t = (t0 / step).ceil() * step;
    while t <= t1 {
        let x = x_of(t);
        p.line_segment([Pos2::new(x, r.bottom() - 5.0), Pos2::new(x, r.bottom())], Stroke::new(1.0_f32, theme::TEXT_DIM));
        let text = if step < 1.0 { format!("{t:.2}") } else { format!("{}:{:02}", (t / 60.0) as u32, (t % 60.0) as u32) };
        p.text(Pos2::new(x + 2.0, r.top() + 2.0), egui::Align2::LEFT_TOP, text, egui::FontId::proportional(9.5), theme::TEXT_DIM);
        t += step;
    }
    p.text(Pos2::new(r.left() + 4.0, r.center().y), egui::Align2::LEFT_CENTER, "s", egui::FontId::proportional(9.5), theme::TEXT_DIM);
    if let Some(t) = playing(st) {
        if t >= t0 && t <= t1 {
            let x = x_of(t);
            p.line_segment([Pos2::new(x, r.top()), Pos2::new(x, r.bottom())], Stroke::new(2.0_f32, theme::PLAYHEAD));
        }
        ui.ctx().request_repaint();
    }
    if resp.clicked() {
        if let Some(pos) = resp.interact_pointer_pos() {
            let file = if st.ui.repair.compare { st.ui.repair.original.clone() } else { None }.or(st.ui.repair.source.clone());
            if let Some(f) = file {
                st.preview_from(&f, t_of(pos.x));
            }
        }
    }
    resp.on_hover_text("Click to play from here");
    h
}

/// The scrollbar under the view.
fn scrollbar(ui: &mut Ui, st: &mut State, data: &AudioData, w: f32) {
    let (r, resp) = ui.allocate_exact_size(Vec2::new(w, 12.0), Sense::click_and_drag());
    let dur = data.duration().max(1e-3);
    let (t0, t1) = view_range(st, data);
    let p = ui.painter_at(r);
    p.rect_filled(r, 6.0, Color32::from_white_alpha(8));
    let thumb = Rect::from_min_max(Pos2::new(r.left() + (t0 / dur) as f32 * r.width(), r.top() + 2.0), Pos2::new(r.left() + (t1 / dur) as f32 * r.width(), r.bottom() - 2.0));
    p.rect_filled(thumb, 4.0, if resp.hovered() || resp.dragged() { theme::ACCENT } else { Color32::from_white_alpha(60) });
    if resp.dragged() && t1 - t0 < dur {
        let dt = resp.drag_delta().x as f64 / r.width() as f64 * dur;
        let a = (t0 + dt).clamp(0.0, dur - (t1 - t0));
        st.ui.repair.view = Some((a, a + (t1 - t0)));
    }
}

/// Wheel: Ctrl = zoom time at the pointer, Shift = scroll time, Alt = zoom pitch, plain = scroll pitch
/// (notes) or time (spectrogram).
fn wheel(ui: &Ui, st: &mut State, data: &AudioData, r: Rect, left: f32, notes: bool) {
    if !ui.rect_contains_pointer(r) {
        return;
    }
    let (scroll, zoom, m) = ui.input(|i| (i.raw_scroll_delta, i.zoom_delta(), i.modifiers));
    let Some(h) = ui.ctx().pointer_hover_pos() else { return };
    let (t0, t1) = view_range(st, data);
    let t_at = t0 + (((h.x - left) / (r.right() - left)).clamp(0.0, 1.0) as f64) * (t1 - t0);
    if zoom != 1.0 {
        // Ctrl+wheel and pinch arrive as zoom.
        zoom_time(st, 1.0 / zoom as f64, Some(t_at));
        return;
    }
    if scroll == Vec2::ZERO {
        return;
    }
    let dy = if scroll.y != 0.0 { scroll.y } else { scroll.x };
    if m.command {
        zoom_time(st, (-dy as f64 * 0.004).exp(), Some(t_at));
    } else if m.alt && notes {
        zoom_pitch(st, (-dy * 0.004).exp());
    } else if m.shift || !notes || scroll.x != 0.0 {
        let dur = data.duration().max(1e-3);
        if t1 - t0 < dur {
            let dt = -dy as f64 / r.width() as f64 * (t1 - t0);
            let a = (t0 + dt).clamp(0.0, dur - (t1 - t0));
            st.ui.repair.view = Some((a, a + (t1 - t0)));
        }
    } else if let Some((lo, hi)) = st.ui.repair.pitch_view {
        let d = dy / 40.0;
        st.ui.repair.pitch_view = Some((lo + d, hi + d));
    }
}

/// Playlist navigation in the repair views: Ctrl + right-drag zooms into a box (time; pitch too in
/// the notes view), Ctrl + right-click zooms out, middle-drag pans.
fn nav_gestures(ui: &Ui, st: &mut State, data: &AudioData, plot: Rect, notes: bool, key: &str) {
    ui.data_mut(|d| d.insert_temp(egui::Id::new(key), plot));
    let dur = data.duration().max(1e-3);
    let (t0, t1) = view_range(st, data);
    let t_of = |x: f32| t0 + (((x - plot.left()) / plot.width()).clamp(0.0, 1.0) as f64) * (t1 - t0);
    let pv = st.ui.repair.pitch_view;
    let m_of = |y: f32| pv.map(|(lo, hi)| lo + (plot.bottom() - y) / plot.height() * (hi - lo));
    match super::nav::area_zoom(ui, plot, key) {
        Some(super::nav::AreaZoom::To(b)) => {
            let (a, z) = (t_of(b.left()), t_of(b.right()));
            let w = (z - a).max(0.05_f64.min(dur));
            st.ui.repair.view = if w >= dur { None } else { Some((a.clamp(0.0, dur - w), a.clamp(0.0, dur - w) + w)) };
            if notes {
                if let (Some(top), Some(bot)) = (m_of(b.top()), m_of(b.bottom())) {
                    let (lo, hi) = (bot.min(top), bot.max(top));
                    let c = (lo + hi) / 2.0;
                    let half = ((hi - lo) / 2.0).max(1.5);
                    st.ui.repair.pitch_view = Some((c - half, c + half));
                }
            }
        }
        Some(super::nav::AreaZoom::Out(at)) => {
            zoom_time(st, 2.0, Some(t_of(at.x)));
            if notes {
                zoom_pitch(st, 2.0);
            }
        }
        None => {}
    }
    let pan = super::nav::pan_drag(ui, plot, false);
    if pan != Vec2::ZERO {
        let (t0, t1) = view_range(st, data);
        if t1 - t0 < dur {
            let a = (t0 - pan.x as f64 / plot.width() as f64 * (t1 - t0)).clamp(0.0, dur - (t1 - t0));
            st.ui.repair.view = Some((a, a + (t1 - t0)));
        }
        if notes {
            if let Some((lo, hi)) = st.ui.repair.pitch_view {
                let d = pan.y / plot.height() * (hi - lo);
                st.ui.repair.pitch_view = Some((lo + d, hi + d));
            }
        }
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    }
}

fn spectrogram_view(ui: &mut Ui, st: &mut State, data: &AudioData, size: Vec2) {
    let (r, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let p = ui.painter_at(r);
    p.rect_filled(r, 4.0, Color32::BLACK);
    let plot = Rect::from_min_max(Pos2::new(r.left() + 44.0, r.top() + 2.0), Pos2::new(r.right() - 2.0, r.bottom() - 2.0));
    wheel(ui, st, data, r, plot.left(), false);
    nav_gestures(ui, st, data, plot, false, "repair.spectrum");
    let dur = data.duration().max(1e-6);
    let (t0, t1) = view_range(st, data);
    let nyq = data.rate as f32 / 2.0;
    let x_of = |t: f64| plot.left() + ((t - t0) / (t1 - t0)) as f32 * plot.width();
    let t_of = |x: f32| t0 + (((x - plot.left()) / plot.width()).clamp(0.0, 1.0) as f64) * (t1 - t0);
    let y_of = |f: f32| plot.bottom() - ((f.max(20.0) / 20.0).ln() / (nyq / 20.0).ln()).clamp(0.0, 1.0) * plot.height();
    let f_of = |y: f32| 20.0 * (nyq / 20.0).powf(((plot.bottom() - y) / plot.height()).clamp(0.0, 1.0));
    if let Some(tex) = &st.ui.repair.spectro {
        let uv = Rect::from_min_max(Pos2::new((t0 / dur) as f32, 0.0), Pos2::new((t1 / dur) as f32, 1.0));
        p.image(tex.id(), plot, uv, Color32::WHITE);
    }
    // The waveform's outline.
    let wave = &st.ui.repair.wave;
    if !wave.is_empty() {
        let mid = plot.center().y;
        let amp = plot.height() * 0.14;
        let n = wave.len();
        let (c0, c1) = (((t0 / dur) * n as f64) as usize, (((t1 / dur) * n as f64).ceil() as usize).min(n));
        let pts = |hi: bool| -> Vec<Pos2> {
            (c0..c1)
                .map(|c| {
                    let t = (c as f64 + 0.5) / n as f64 * dur;
                    let v = if hi { wave[c].1 } else { wave[c].0 };
                    Pos2::new(x_of(t), mid - super::playlist::wave_disp(v) * amp)
                })
                .collect()
        };
        p.add(egui::Shape::line(pts(true), Stroke::new(1.0_f32, Color32::from_white_alpha(55))));
        p.add(egui::Shape::line(pts(false), Stroke::new(1.0_f32, Color32::from_white_alpha(55))));
    }
    let small = egui::FontId::proportional(9.5);
    for f in [100.0f32, 1000.0, 10000.0] {
        let y = y_of(f);
        p.line_segment([Pos2::new(plot.left(), y), Pos2::new(plot.right(), y)], Stroke::new(1.0_f32, Color32::from_white_alpha(18)));
        p.text(Pos2::new(plot.left() - 4.0, y), egui::Align2::RIGHT_CENTER, if f >= 1000.0 { format!("{}k", f / 1000.0) } else { format!("{f}") }, small.clone(), theme::TEXT_DIM);
    }
    // Selection: drag = a time range (all frequencies); Shift+drag = a time × frequency box.
    if resp.drag_started() {
        if let Some(pos) = resp.interact_pointer_pos() {
            st.ui.repair.drag_from = Some((t_of(pos.x), f_of(pos.y)));
        }
    }
    if resp.dragged() {
        if let (Some((ta, fa)), Some(pos)) = (st.ui.repair.drag_from, resp.interact_pointer_pos()) {
            let boxed = ui.input(|i| i.modifiers.shift);
            let fb = f_of(pos.y);
            st.ui.repair.sel = Some((ta, t_of(pos.x), boxed.then(|| (fa.min(fb), fa.max(fb)))));
        }
    }
    if resp.clicked() {
        st.ui.repair.sel = None;
    }
    if let Some((a, b, fr)) = st.ui.repair.sel {
        let (ya, yb) = match fr {
            Some((f0, f1)) => (y_of(f1), y_of(f0)),
            None => (plot.top(), plot.bottom()),
        };
        let sr = Rect::from_min_max(Pos2::new(x_of(a.min(b)).max(plot.left()), ya), Pos2::new(x_of(a.max(b)).min(plot.right()), yb));
        if sr.width() > 0.0 {
            p.rect_filled(sr, 0.0, Color32::from_rgba_unmultiplied(255, 255, 255, 28));
            p.rect_stroke(sr, 0.0, Stroke::new(1.5_f32, theme::ACCENT_2), egui::StrokeKind::Inside);
        }
    }
    if let Some(h) = resp.hover_pos().filter(|h| plot.contains(*h)) {
        p.text(Pos2::new(plot.right() - 6.0, plot.top() + 4.0), egui::Align2::RIGHT_TOP, format!("{:.2} s · {:.0} Hz", t_of(h.x), f_of(h.y)), small, Color32::WHITE);
    }
}

fn blob_rect(rp: &RepairUi, k: usize, x_of: &dyn Fn(f64) -> f32, y_of: &dyn Fn(f32) -> f32, row_h: f32) -> Rect {
    let n = &rp.notes[k];
    let centre = n.midi + rp.shifts[k];
    let half = row_h * (0.28 + 0.2 * n.level.min(1.0));
    Rect::from_min_max(Pos2::new(x_of(n.start), y_of(centre) - half), Pos2::new(x_of(n.end), y_of(centre) + half))
}

fn notes_view(ui: &mut Ui, st: &mut State, data: &AudioData, size: Vec2) {
    let (r, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let p = ui.painter_at(r);
    p.rect_filled(r, 4.0, Color32::from_rgb(0x12, 0x13, 0x18));
    if st.ui.repair.notes.is_empty() {
        p.text(r.center(), egui::Align2::CENTER_CENTER, "No notes found – pitch correction works on one voice at a time: a singer, a bass, a solo instrument", egui::FontId::proportional(13.0), theme::TEXT_DIM);
        return;
    }
    let plot = Rect::from_min_max(Pos2::new(r.left() + 44.0, r.top() + 2.0), Pos2::new(r.right() - 2.0, r.bottom() - 2.0));
    if st.ui.repair.pitch_view.is_none() {
        let lo = st.ui.repair.notes.iter().map(|n| n.midi).fold(f32::MAX, f32::min).floor() - 5.0;
        let hi = st.ui.repair.notes.iter().map(|n| n.midi).fold(f32::MIN, f32::max).ceil() + 5.0;
        st.ui.repair.pitch_view = Some((lo, hi.max(lo + 12.0)));
    }
    wheel(ui, st, data, r, plot.left(), true);
    nav_gestures(ui, st, data, plot, true, "repair.notes");
    let (lo, hi) = st.ui.repair.pitch_view.unwrap_or((48.0, 72.0));
    let (t0, t1) = view_range(st, data);
    let x_of = |t: f64| plot.left() + ((t - t0) / (t1 - t0)) as f32 * plot.width();
    let t_of = |x: f32| t0 + (((x - plot.left()) / plot.width()).clamp(0.0, 1.0) as f64) * (t1 - t0);
    let y_of = |m: f32| plot.bottom() - (m - lo) / (hi - lo) * plot.height();
    let row_h = plot.height() / (hi - lo);
    // Piano rows.
    let mut m = lo.floor() as i32;
    while (m as f32) <= hi + 1.0 {
        let y = y_of(m as f32);
        let pc = m.rem_euclid(12);
        if matches!(pc, 1 | 3 | 6 | 8 | 10) {
            let band = Rect::from_min_max(Pos2::new(plot.left(), y - row_h / 2.0), Pos2::new(plot.right(), y + row_h / 2.0));
            p.rect_filled(band, 0.0, Color32::from_rgb(0x0c, 0x0d, 0x11));
        }
        if pc == 0 {
            p.line_segment([Pos2::new(plot.left(), y + row_h / 2.0), Pos2::new(plot.right(), y + row_h / 2.0)], Stroke::new(1.0_f32, Color32::from_white_alpha(25)));
        }
        if pc == 0 || row_h > 10.0 {
            p.text(Pos2::new(plot.left() - 4.0, y), egui::Align2::RIGHT_CENTER, note_name(m as f32), egui::FontId::proportional(9.0), if pc == 0 { theme::TEXT } else { theme::TEXT_DIM });
        }
        m += 1;
    }
    let mut hit: Option<usize> = None;
    {
        let rp = &st.ui.repair;
        for k in 0..rp.notes.len() {
            let n = &rp.notes[k];
            if n.end < t0 || n.start > t1 {
                continue;
            }
            let br = blob_rect(rp, k, &x_of, &y_of, row_h);
            let sel = rp.sel_notes.contains(&k);
            // Where it was recorded, once it moved.
            if rp.shifts[k].abs() > 0.005 {
                let ghost = br.translate(Vec2::new(0.0, rp.shifts[k] * row_h));
                p.rect_stroke(ghost, ghost.height() / 2.0, Stroke::new(1.0_f32, Color32::from_white_alpha(40)), egui::StrokeKind::Inside);
            }
            let col = if rp.edited(k) { theme::ACCENT_2 } else { Color32::from_rgb(0xd8, 0x8a, 0x3c) };
            p.rect_filled(br, br.height() / 2.0, col.gamma_multiply(if sel { 1.0 } else { 0.62 }));
            if sel {
                p.rect_stroke(br, br.height() / 2.0, Stroke::new(1.5_f32, Color32::WHITE), egui::StrokeKind::Outside);
            }
            // Its pitch line as it will sound.
            let pts: Vec<Pos2> = n.curve.iter().filter(|(t, _)| *t >= t0 - 0.1 && *t <= t1 + 0.1).map(|(t, _)| Pos2::new(x_of(*t), y_of(rp.pitch_at(k, *t)))).collect();
            if pts.len() > 1 {
                p.add(egui::Shape::line(pts, Stroke::new(1.3_f32, Color32::from_black_alpha(170))));
            }
            if br.width() > 46.0 && row_h > 7.0 {
                let mm = n.midi + rp.shifts[k];
                let c = cents(mm);
                let label = if c == 0 { note_name(mm) } else { format!("{} {c:+}", note_name(mm)) };
                p.text(Pos2::new(br.left() + 4.0, br.top() - 2.0), egui::Align2::LEFT_BOTTOM, label, egui::FontId::proportional(10.5), if sel { Color32::WHITE } else { theme::TEXT });
            }
            if resp.hover_pos().or(resp.interact_pointer_pos()).is_some_and(|h| br.expand(3.0).contains(h)) {
                hit = Some(k);
            }
        }
    }
    let modifiers = ui.input(|i| i.modifiers);
    let tool = st.ui.repair.note_tool;
    // Double-click a note: split it there (Melodyne's note separation), with any tool.
    if resp.double_clicked() {
        if let (Some(k), Some(pos)) = (hit, resp.interact_pointer_pos()) {
            st.ui.repair.split(k, t_of(pos.x));
            return;
        }
    }
    if resp.drag_started() {
        if let Some(pos) = resp.interact_pointer_pos() {
            let rp = &mut st.ui.repair;
            match hit {
                Some(k) if tool != NoteTool::Split => {
                    if !rp.sel_notes.contains(&k) {
                        if modifiers.shift || modifiers.command {
                            rp.sel_notes.push(k);
                        } else {
                            rp.sel_notes = vec![k];
                        }
                    }
                    rp.snapshot();
                    rp.note_drag = Some((pos, rp.shifts.clone(), rp.mods.clone()));
                }
                _ => rp.rubber = Some(pos),
            }
        }
    }
    if resp.dragged() {
        if let Some(pos) = resp.interact_pointer_pos() {
            let rp = &mut st.ui.repair;
            if let Some((from, s0, m0)) = rp.note_drag.clone() {
                let dy = (from.y - pos.y) / row_h;
                for k in rp.sel_notes.clone() {
                    if tool == NoteTool::Modulation {
                        rp.mods[k] = (m0[k] + (from.y - pos.y) / 120.0).clamp(0.0, 2.0);
                    } else {
                        // The note's centre snaps onto a semitone unless Alt is held (or snapping is off).
                        let target = rp.notes[k].midi + s0[k] + dy;
                        let snapped = if modifiers.alt || rp.no_snap { target } else { target.round() };
                        rp.shifts[k] = snapped - rp.notes[k].midi;
                    }
                }
            }
            if let Some(a) = rp.rubber {
                let band = Rect::from_two_pos(a, pos);
                p.rect_filled(band, 0.0, Color32::from_rgba_unmultiplied(255, 255, 255, 18));
                p.rect_stroke(band, 0.0, Stroke::new(1.0_f32, theme::ACCENT_2), egui::StrokeKind::Inside);
            }
        }
    }
    if resp.drag_stopped() {
        let rp = &mut st.ui.repair;
        if let (Some(a), Some(pos)) = (rp.rubber.take(), resp.interact_pointer_pos()) {
            let band = Rect::from_two_pos(a, pos);
            let inside: Vec<usize> = (0..rp.notes.len()).filter(|k| blob_rect(rp, *k, &x_of, &y_of, row_h).intersects(band)).collect();
            if modifiers.shift || modifiers.command {
                for k in inside {
                    if !rp.sel_notes.contains(&k) {
                        rp.sel_notes.push(k);
                    }
                }
            } else {
                rp.sel_notes = inside;
            }
        }
        // A drag that changed nothing is no undo step.
        if let Some((_, s0, m0)) = rp.note_drag.take() {
            if s0 == rp.shifts && m0 == rp.mods {
                rp.history.pop();
            }
        }
    }
    if resp.clicked() {
        let rp = &mut st.ui.repair;
        match (hit, tool) {
            (Some(k), NoteTool::Split) => {
                if let Some(pos) = resp.interact_pointer_pos() {
                    rp.split(k, t_of(pos.x));
                }
            }
            (Some(k), _) if modifiers.shift || modifiers.command => {
                if let Some(i) = rp.sel_notes.iter().position(|x| *x == k) {
                    rp.sel_notes.remove(i);
                } else {
                    rp.sel_notes.push(k);
                }
            }
            (Some(k), _) => rp.sel_notes = vec![k],
            (None, _) => rp.sel_notes.clear(),
        }
    }
    if resp.hovered() {
        let cursor = match (hit.is_some(), tool) {
            (true, NoteTool::Split) => egui::CursorIcon::Text,
            (true, NoteTool::Modulation) => egui::CursorIcon::ResizeVertical,
            (true, NoteTool::Move) => egui::CursorIcon::Grab,
            _ => egui::CursorIcon::Default,
        };
        ui.ctx().set_cursor_icon(cursor);
    }
    if let Some(k) = hit.filter(|k| *k < st.ui.repair.notes.len()) {
        let rp = &st.ui.repair;
        let n = &rp.notes[k];
        let mm = n.midi + rp.shifts[k];
        let moved = if rp.shifts[k].abs() > 0.005 { format!(" · moved {:+.2} semitones", rp.shifts[k]) } else { String::new() };
        let vib = if (rp.mods[k] - 1.0).abs() > 0.005 { format!(" · vibrato {:.0} %", rp.mods[k] * 100.0) } else { String::new() };
        resp.on_hover_text_at_pointer(format!("{} {:+} cents · {:.2} – {:.2} s{moved}{vib}", note_name(mm), cents(mm), n.start, n.end));
    }
}

fn slider(ui: &mut Ui, v: &mut f32, range: std::ops::RangeInclusive<f32>, text: &str, suffix: &str) {
    ui.add(egui::Slider::new(v, range).text(text).suffix(suffix));
}

/// A titled box in the side panel.
fn card(ui: &mut Ui, title: &str, body: impl FnOnce(&mut Ui)) {
    egui::Frame::NONE.fill(Color32::from_white_alpha(7)).corner_radius(8.0).inner_margin(egui::Margin::symmetric(10, 8)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(title.to_uppercase()).small().strong().color(theme::ACCENT));
        body(ui);
    });
    ui.add_space(6.0);
}

/// Preview and Apply for the repair `f` makes.
fn actions(ui: &mut Ui, st: &mut State, label: &str, f: MakeProcess) {
    if st.ui.repair.job.is_some() {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(format!("{} …", st.ui.repair.job_label));
        });
        return;
    }
    ui.horizontal(|ui| {
        let w = (ui.available_width() - 8.0) / 2.0;
        if ui.add(egui::Button::new("▶ Preview").min_size(Vec2::new(w, 30.0))).on_hover_text("Listen to the result first – nothing changes yet").clicked() {
            start_render(st, label, f(), true);
        }
        let b = egui::Button::new(RichText::new("Apply").strong().color(Color32::BLACK)).fill(theme::ACCENT).min_size(Vec2::new(w, 30.0));
        if ui.add(b).on_hover_text("Writes a new file for the clip (Ctrl+Z goes back)").clicked() {
            start_render(st, label, f(), false);
        }
    });
}

/// One finding of "Check the recording": a dot, the text, and a Fix button. Returns whether Fix was clicked.
fn finding(ui: &mut Ui, found: bool, text: &str, busy: bool) -> bool {
    ui.horizontal(|ui| {
        ui.label(RichText::new(if found { "•" } else { "✔" }).color(if found { theme::WARN } else { theme::OK }));
        ui.label(if found { RichText::new(text) } else { RichText::new(text).color(theme::TEXT_DIM) });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| found && ui.add_enabled(!busy, egui::Button::new("Fix").small()).clicked()).inner
    })
    .inner
}

fn repair_panel(ui: &mut Ui, st: &mut State) {
    // What the recording seems to need, each with a one-click fix.
    card(ui, "Check the recording", |ui| {
        let Some(d) = st.ui.repair.diagnosis.clone() else {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Listening …");
            });
            return;
        };
        let busy = st.ui.repair.job.is_some();
        let mut fix: Option<(&str, Tool, Process)> = None;
        match d.hum {
            Some((hz, above)) => {
                if finding(ui, true, &format!("Mains hum at {hz:.0} Hz ({above:.0} dB)"), busy) {
                    fix = Some(("De-hum", Tool::Dehum, Box::new(move |a: &AudioData| repair::dehum(a, hz, 8, 30.0))));
                }
            }
            None => {
                finding(ui, false, "No hum", busy);
            }
        }
        let clicks = d.clicks > 2;
        if finding(ui, clicks, &if clicks { format!("{} clicks or pops", d.clicks) } else { "No clicks".into() }, busy) {
            fix = Some(("De-click", Tool::Declick, Box::new(|a: &AudioData| repair::declick(a, 5.0).0)));
        }
        let clipped = d.clipping > 0;
        if finding(ui, clipped, &if clipped { format!("Clipped in {} places", d.clipping) } else { "No clipping".into() }, busy) {
            fix = Some(("De-clip", Tool::Declip, Box::new(|a: &AudioData| repair::declip(a, 0.99).0)));
        }
        let noisy = d.snr_db < 45.0;
        if finding(ui, noisy, &if noisy { format!("Background noise ({:.0} dB below the sound)", d.snr_db) } else { "Quiet background".into() }, busy) {
            fix = Some(("De-noise", Tool::Denoise, Box::new(|a: &AudioData| repair::denoise(a, &repair::learn_noise(a, None), 12.0, 1.0))));
        }
        if !(d.hum.is_some() || clicks || clipped || noisy) {
            ui.label(RichText::new("No obvious problems – the tools below are there for fine work.").small().color(theme::TEXT_DIM));
        }
        if let Some((label, tool, f)) = fix {
            st.ui.repair.tool = tool;
            start_render(st, label, f, false);
        }
    });
    card(ui, "Tools", |ui| {
        egui::Grid::new("repair.tools").num_columns(2).spacing([6.0, 6.0]).show(ui, |ui| {
            for (k, (t, name, tip)) in TOOLS.iter().enumerate() {
                let sel = st.ui.repair.tool == *t;
                let b =
                    egui::Button::new(RichText::new(*name).color(if sel { Color32::BLACK } else { theme::TEXT })).fill(if sel { theme::ACCENT_2 } else { Color32::from_white_alpha(10) }).min_size(Vec2::new(142.0, 26.0));
                if ui.add(b).on_hover_text(*tip).clicked() {
                    st.ui.repair.tool = *t;
                }
                if k % 2 == 1 {
                    ui.end_row();
                }
            }
        });
    });
    let tool = st.ui.repair.tool;
    let (title, tip) = TOOLS.iter().find(|x| x.0 == tool).map(|x| (x.1, x.2)).unwrap_or(("Repair", ""));
    let scope = match st.ui.repair.sel {
        Some((a, b, Some((f0, f1)))) => format!("Works on the box: {:.2} – {:.2} s, {:.0} – {:.0} Hz", a.min(b), a.max(b), f0, f1),
        Some((a, b, None)) => format!("Works on {:.2} – {:.2} s (Esc: the whole clip)", a.min(b), a.max(b)),
        None => "Works on the whole clip – drag to pick a part, Ctrl+A for all".into(),
    };
    card(ui, title, |ui| {
        ui.label(RichText::new(tip).small().color(theme::TEXT_DIM));
        ui.label(RichText::new(scope).small().color(theme::ACCENT_2));
        ui.add_space(4.0);
        let r = &mut st.ui.repair;
        let sel = r.sel;
        let f: Option<MakeProcess> = match tool {
            Tool::Denoise => {
                let learn = if r.sel.is_some() { "Learn the noise from the selection" } else { "Learn the noise (quietest parts)" };
                if ui.button(learn).on_hover_text("Select a stretch where only the noise can be heard, then learn it").clicked() {
                    if let Some(d) = &r.data {
                        r.profile = Some(repair::learn_noise(d, r.sel.map(|(a, b, _)| (a.min(b), a.max(b)))));
                    }
                }
                ui.label(if r.profile.is_some() { RichText::new("✔ Noise learned").color(theme::OK) } else { RichText::new("Not learned yet: the quietest parts are used").small().color(theme::TEXT_DIM) });
                slider(ui, &mut r.reduction, 3.0..=40.0, "how much", " dB");
                slider(ui, &mut r.sensitivity, 0.5..=3.0, "sensitivity", "");
                let (red, sens, prof) = (r.reduction, r.sensitivity, r.profile.clone());
                Some(Box::new(move || -> Process {
                    let prof = prof.clone();
                    Box::new(move |d: &AudioData| repair::denoise(d, &prof.unwrap_or_else(|| repair::learn_noise(d, None)), red, sens))
                }))
            }
            Tool::Dehum => {
                ui.horizontal(|ui| {
                    ui.label("Mains");
                    for (v, n) in [(0.0, "Find it"), (50.0, "50 Hz"), (60.0, "60 Hz")] {
                        ui.selectable_value(&mut r.hum_base, v, n);
                    }
                });
                let mut h = r.hum_harmonics as f32;
                slider(ui, &mut h, 1.0..=16.0, "overtones", "");
                r.hum_harmonics = h.round() as usize;
                slider(ui, &mut r.hum_q, 5.0..=60.0, "narrowness", "");
                let (b, hh, q) = (r.hum_base, r.hum_harmonics, r.hum_q);
                Some(Box::new(move || -> Process { Box::new(move |d: &AudioData| repair::dehum(d, b, hh, q)) }))
            }
            Tool::Declick => {
                slider(ui, &mut r.click_sens, 1.0..=10.0, "sensitivity", "");
                let s = r.click_sens;
                Some(Box::new(move || -> Process { Box::new(move |d: &AudioData| repair::declick(d, s).0) }))
            }
            Tool::Declip => {
                slider(ui, &mut r.clip_threshold, 0.9..=1.0, "clip level", "");
                let t = r.clip_threshold;
                Some(Box::new(move || -> Process { Box::new(move |d: &AudioData| repair::declip(d, t).0) }))
            }
            Tool::Distortion => {
                slider(ui, &mut r.dist_amount, 0.0..=1.0, "how much", "");
                let a = r.dist_amount;
                Some(Box::new(move || -> Process { Box::new(move |d: &AudioData| repair::dedistort(d, a, 0.0).0) }))
            }
            Tool::Dereverb => {
                slider(ui, &mut r.reverb_amount, 0.0..=1.0, "how much", "");
                slider(ui, &mut r.reverb_time, 0.2..=4.0, "room size (reverb time)", " s");
                let (a, t) = (r.reverb_amount, r.reverb_time);
                Some(Box::new(move || -> Process { Box::new(move |d: &AudioData| repair::dereverb(d, a, t)) }))
            }
            Tool::Spectral => {
                slider(ui, &mut r.spectral_db, -60.0..=12.0, "gain", " dB");
                match sel {
                    Some((t0, t1, Some((f0, f1)))) => {
                        let g = r.spectral_db;
                        Some(Box::new(move || -> Process { Box::new(move |d: &AudioData| repair::attenuate(d, t0.min(t1), t0.max(t1), f0, f1, g)) }))
                    }
                    _ => {
                        ui.label(RichText::new("Draw a box around the sound first: Shift+drag on the spectrogram.").color(theme::WARN));
                        None
                    }
                }
            }
            Tool::Gain => {
                slider(ui, &mut r.gain_db, -24.0..=24.0, "gain", " dB");
                let g = r.gain_db;
                Some(Box::new(move || -> Process { Box::new(move |d: &AudioData| repair::gain(d, 0.0, d.duration(), g)) }))
            }
            Tool::Pitch => None,
        };
        ui.add_space(4.0);
        if let Some(f) = f {
            actions(ui, st, title, f);
        }
    });
}

fn pitch_panel(ui: &mut Ui, st: &mut State) {
    card(ui, "Tool", |ui| {
        ui.horizontal(|ui| {
            for (t, name, tip) in [
                (NoteTool::Move, "Move", "Drag notes up / down (snaps to semitones; Alt = free). Drag on empty space to select several."),
                (NoteTool::Modulation, "Vibrato", "Drag a note up / down: more or less vibrato and drift (100 % = as sung)"),
                (NoteTool::Split, "Split", "Click a note where it should be cut in two (double-click works with any tool)"),
            ] {
                let sel = st.ui.repair.note_tool == t;
                let b =
                    egui::Button::new(RichText::new(name).color(if sel { Color32::BLACK } else { theme::TEXT })).fill(if sel { theme::ACCENT_2 } else { Color32::from_white_alpha(10) }).min_size(Vec2::new(92.0, 26.0));
                if ui.add(b).on_hover_text(tip).clicked() {
                    st.ui.repair.note_tool = t;
                }
            }
        });
        let mut snap = !st.ui.repair.no_snap;
        if ui.checkbox(&mut snap, "Snap to semitones").on_hover_text("Hold Alt while dragging to move freely").changed() {
            st.ui.repair.no_snap = !snap;
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new("Pitch zoom").small().color(theme::TEXT_DIM));
            if ui.small_button(" + ").on_hover_text("Alt+wheel").clicked() {
                zoom_pitch(st, 0.7);
            }
            if ui.small_button(" − ").on_hover_text("Alt+wheel").clicked() {
                zoom_pitch(st, 1.4);
            }
        });
    });
    let what = {
        let r = &st.ui.repair;
        match r.sel_notes.len() {
            0 => format!("all {} notes", r.notes.len()),
            1 => "the selected note".into(),
            n => format!("{n} selected notes"),
        }
    };
    card(ui, "Selection", |ui| {
        {
            let r = &st.ui.repair;
            if r.sel_notes.len() == 1 && r.sel_notes[0] < r.notes.len() {
                let k = r.sel_notes[0];
                let n = &r.notes[k];
                let mm = n.midi + r.shifts[k];
                ui.label(RichText::new(format!("{}  {:+} cents", note_name(mm), cents(mm))).size(15.0).strong());
                ui.label(RichText::new(format!("{:.2} – {:.2} s · moved {:+.2} semitones · vibrato {:.0} %", n.start, n.end, r.shifts[k], r.mods[k] * 100.0)).small().color(theme::TEXT_DIM));
            } else {
                ui.label(format!("Working on {what}"));
                ui.label(RichText::new("Click a note, drag a box around several, or Ctrl+A for all").small().color(theme::TEXT_DIM));
            }
        }
        ui.horizontal(|ui| {
            if ui.button("Reset").on_hover_text("Back to how it was sung (Delete)").clicked() {
                st.ui.repair.reset_targets();
            }
            if ui.add_enabled(!st.ui.repair.history.is_empty(), egui::Button::new("Undo")).on_hover_text("Ctrl+Z").clicked() {
                key(st, "edit.undo_toggle");
            }
            if ui.button("Select all").on_hover_text("Ctrl+A").clicked() {
                select_all(st);
            }
        });
    });
    card(ui, "Correct pitch", |ui| {
        ui.label(RichText::new(format!("For {what}: pull each note onto its semitone, and calm its vibrato and drift.")).small().color(theme::TEXT_DIM));
        let r = &mut st.ui.repair;
        let mut c = r.centre_amount * 100.0;
        slider(ui, &mut c, 0.0..=100.0, "onto the semitone", " %");
        r.centre_amount = c / 100.0;
        let mut d = r.drift_amount * 100.0;
        slider(ui, &mut d, 0.0..=100.0, "calm vibrato / drift", " %");
        r.drift_amount = d / 100.0;
        if ui.add(egui::Button::new(RichText::new("Correct").strong()).min_size(Vec2::new(ui.available_width(), 26.0))).clicked() {
            r.snapshot();
            for k in r.targets() {
                let m = r.notes[k].midi + r.shifts[k];
                r.shifts[k] += (m.round() - m) * r.centre_amount;
                r.mods[k] *= 1.0 - r.drift_amount;
            }
        }
    });
    card(ui, "Result", |ui| {
        let r = &st.ui.repair;
        let edited: Vec<usize> = (0..r.notes.len()).filter(|k| r.edited(*k)).collect();
        ui.label(RichText::new(format!("{} of {} notes changed", edited.len(), r.notes.len())).color(theme::TEXT_DIM));
        if edited.is_empty() {
            ui.label(RichText::new("Move a note or press Correct to hear a result.").small().color(theme::TEXT_DIM));
            return;
        }
        let notes = r.notes.clone();
        let edits: Vec<PitchEdit> = edited.into_iter().map(|k| PitchEdit { note: k, shift: r.shifts[k], drift: 1.0 - r.mods[k] }).collect();
        actions(
            ui,
            st,
            "Pitch correction",
            Box::new(move || -> Process {
                let (n, e) = (notes.clone(), edits.clone());
                Box::new(move |d: &AudioData| repair::apply_pitch(d, &n, &e))
            }),
        );
    });
}

fn hints(ui: &mut Ui, st: &State) {
    let text = if st.ui.repair.tool == Tool::Pitch {
        "Ctrl+wheel zoom time · Alt+wheel zoom pitch · wheel scroll · Shift+wheel scroll time   |   Up / Down semitone · Ctrl+Up / Down octave · Alt+Up / Down 10 cents · Ctrl+A all · Delete reset · double-click split · Ctrl+Z undo"
    } else {
        "Ctrl+wheel zoom · wheel scroll · drag: select a time range · Shift+drag: a time × frequency box · Ctrl+A everything · Esc clear · click the ruler to play from there"
    };
    ui.add_space(4.0);
    ui.label(RichText::new(text).small().color(theme::TEXT_DIM));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(path: &std::path::Path) {
        // A 220 Hz tone with 60 Hz hum, 1 s.
        let rate = 48_000u32;
        let sig = || -> Vec<f32> {
            (0..rate)
                .map(|i| {
                    let t = i as f32 / rate as f32;
                    0.4 * (std::f32::consts::TAU * 220.0 * t).sin() + 0.2 * (std::f32::consts::TAU * 60.0 * t).sin()
                })
                .collect()
        };
        repair::write_wav(path, &AudioData::from_planar(rate, sig(), sig())).unwrap();
    }

    fn frame(ctx: &egui::Context, st: &mut State) {
        let _ = ctx.run(egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1200.0, 800.0))), ..Default::default() }, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| show(ui, st));
        });
    }

    fn wait(ctx: &egui::Context, st: &mut State, done: &dyn Fn(&State) -> bool) {
        for _ in 0..400 {
            frame(ctx, st);
            poll(st);
            if done(st) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        panic!("timed out");
    }

    fn loaded() -> (tempfile::TempDir, PathBuf, State, Id, egui::Context) {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("take.wav");
        wav(&src);
        let mut st = State::new(redhive_client_framework::AppContext::new("en"), crate::state::Settings { show_splash: false, ..Default::default() });
        st.new_project();
        st.add_audio_clip_at(&src, None, 0.0);
        let cid = st.ui.clips[0];
        open(&mut st, cid);
        let ctx = egui::Context::default();
        wait(&ctx, &mut st, &|st| st.ui.repair.data.is_some());
        (dir, src, st, cid, ctx)
    }

    #[test]
    fn a_repair_renders_to_a_new_file_and_undo_goes_back() {
        let (_dir, src, mut st, cid, ctx) = loaded();
        assert!(st.ui.repair.spectro.is_some(), "spectrogram drawn");
        assert!(!st.ui.repair.notes.is_empty(), "the tone is a note");
        let hum = st.ui.repair.diagnosis.as_ref().and_then(|d| d.hum);
        assert!(hum.is_some_and(|h| (h.0 - 60.0).abs() < 1.0), "the check finds the hum: {hum:?}");
        // De-hum the whole clip.
        render(&mut st, "De-hum", |d| repair::dehum(d, 0.0, 4, 30.0));
        wait(&ctx, &mut st, &|st| st.ui.repair.job.is_none() && clip_source(st, cid).map(|x| x.2) != Some(src.clone()));
        let new = clip_source(&st, cid).unwrap().2;
        assert!(new.exists() && new.file_name().unwrap().to_string_lossy().contains("repaired"), "{new:?}");
        // The new file has no hum.
        let d = dragon_engine::samples::decode(&new).unwrap();
        let level = |hz: f32| {
            let w = std::f32::consts::TAU * hz / d.rate as f32;
            let (mut re, mut im) = (0.0f32, 0.0f32);
            // The middle half (the narrow notches ring in and out at the ends).
            for (i, v) in d.l.iter().enumerate().skip(12000).take(24000) {
                re += v * (w * i as f32).cos();
                im += v * (w * i as f32).sin();
            }
            2.0 * (re * re + im * im).sqrt() / 24000.0
        };
        assert!(level(60.0) < 0.01, "hum {}", level(60.0));
        assert!((level(220.0) - 0.4).abs() < 0.02);
        st.session.undo();
        assert_eq!(clip_source(&st, cid).unwrap().2, src, "undo points the clip back");
    }

    #[test]
    fn notes_select_all_move_split_and_undo() {
        let (_dir, _src, mut st, _cid, _ctx) = loaded();
        st.ui.repair.tool = Tool::Pitch;
        st.ui.editor = crate::state::Editor::Repair;
        let count = st.ui.repair.notes.len();
        assert!(count > 0);
        // Ctrl+A, then up a semitone, an octave and 10 cents.
        assert!(key(&mut st, "edit.select_all"));
        assert_eq!(st.ui.repair.sel_notes.len(), count);
        key(&mut st, "rack.up");
        key(&mut st, "edit.transpose_up");
        key(&mut st, "ctx.alt_up");
        assert!(st.ui.repair.shifts.iter().all(|s| (s - 13.1).abs() < 1e-4), "{:?}", st.ui.repair.shifts);
        // Split the first note in its middle: one more note, all vectors aligned, both halves keep the move.
        let n = st.ui.repair.notes[0].clone();
        st.ui.repair.split(0, (n.start + n.end) / 2.0);
        let r = &st.ui.repair;
        assert_eq!(r.notes.len(), count + 1);
        assert!(r.shifts.len() == r.notes.len() && r.mods.len() == r.notes.len());
        assert!(r.shifts.iter().all(|s| (s - 13.1).abs() < 1e-4));
        // Undo the split, then the three moves; then nothing of its own is left (the project's undo takes over).
        assert!(key(&mut st, "edit.undo_toggle"));
        assert_eq!(st.ui.repair.notes.len(), count);
        for _ in 0..3 {
            assert!(key(&mut st, "edit.undo_toggle"));
        }
        assert!(st.ui.repair.shifts.iter().all(|s| s.abs() < 1e-6));
        assert!(!key(&mut st, "edit.undo_toggle"), "falls through to the project's undo");
        // Delete resets the selection.
        st.ui.repair.shifts[0] = 0.4;
        st.ui.repair.sel_notes = vec![0];
        assert!(key(&mut st, "edit.delete"));
        assert_eq!(st.ui.repair.shifts[0], 0.0);
    }

    #[test]
    fn ctrl_a_in_repair_selects_the_whole_clip_and_zoom_stays_inside() {
        let (_dir, _src, mut st, _cid, _ctx) = loaded();
        st.ui.repair.tool = Tool::Denoise;
        select_all(&mut st);
        let (a, b, f) = st.ui.repair.sel.unwrap();
        assert!(a == 0.0 && (b - 1.0).abs() < 0.01 && f.is_none());
        zoom_time(&mut st, 0.25, Some(0.9));
        let (t0, t1) = st.ui.repair.view.unwrap();
        assert!((t1 - t0 - 0.25).abs() < 1e-6 && t0 >= 0.0 && t1 <= 1.0 + 1e-6, "{t0} {t1}");
        assert!(key(&mut st, "zoom.out"));
        assert!((st.ui.repair.view.map(|v| v.1 - v.0).unwrap() - 0.5).abs() < 1e-6);
        zoom_time(&mut st, 100.0, None);
        assert_eq!(st.ui.repair.view, None, "zooming out stops at the whole clip");
    }
}
