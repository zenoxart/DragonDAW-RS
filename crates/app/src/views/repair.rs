//! Audio Repair: one audio clip opened in a tab of its own, with the iZotope RX tool set (spectral de-noise,
//! de-hum, de-click, de-clip, distortion repair, de-reverb, spectral attenuate, gain) on a spectrogram you
//! can select in (time × frequency), and Melodyne-style note editing (detected notes as blobs: drag them
//! up / down, flatten their drift). Every repair renders to a new file next to the recordings and points
//! the clip at it – Ctrl+Z points it back.

use crate::state::State;
use crate::theme;
use dragon_engine::model::{ClipKind, Id};
use dragon_engine::repair::{self, NoiseProfile, NoteBlob, PitchEdit};
use dragon_engine::samples::AudioData;
use egui::{Color32, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2};
use redhive_contracts::ui::Severity;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

const ROWS: usize = 192;
const MAX_COLS: usize = 1600;

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
    Pitch,
}

const TOOLS: [(Tool, &str, &str); 9] = [
    (Tool::Denoise, "De-noise", "Spectral noise reduction from a learned noise profile"),
    (Tool::Dehum, "De-hum", "Mains hum (50 / 60 Hz) and its harmonics"),
    (Tool::Declick, "De-click", "Clicks, pops and mouth noise"),
    (Tool::Declip, "De-clip", "Peaks cut flat by clipping"),
    (Tool::Distortion, "Distortion repair", "Peaks squashed by soft clipping or overdriven preamps"),
    (Tool::Dereverb, "De-reverb", "Room reverb"),
    (Tool::Spectral, "Spectral attenuate", "Turn a time × frequency selection down"),
    (Tool::Gain, "Gain", "Level of the selection (or the clip)"),
    (Tool::Pitch, "Notes (pitch)", "Melodyne-style: move notes, flatten drift"),
];

/// A selection: seconds, and a frequency span in Hz (`None` = all frequencies).
type Selection = (f64, f64, Option<(f32, f32)>);
/// A render's outcome: the new file and the repair's name.
type RenderSlot = Arc<Mutex<Option<Result<(PathBuf, String), String>>>>;
/// A repair to render.
type Process = Box<dyn FnOnce(&AudioData) -> AudioData + Send>;

/// Everything the tab keeps between frames.
#[derive(Default)]
pub struct RepairUi {
    pub clip: Option<Id>,
    tool: Tool,
    /// Selection: seconds and Hz (frequency span `None` = all frequencies).
    sel: Option<Selection>,
    drag_from: Option<(f64, f32)>,
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
    drift: f32,
    profile: Option<NoiseProfile>,
    // The clip's audio and its pictures (for one source file).
    source: Option<PathBuf>,
    data: Option<Arc<AudioData>>,
    spectro: Option<egui::TextureHandle>,
    wave: Vec<(f32, f32)>,
    loading: Option<Arc<Mutex<Option<Loaded>>>>,
    // Notes.
    notes: Vec<NoteBlob>,
    shifts: Vec<f32>,
    sel_notes: Vec<usize>,
    note_drag: Option<(usize, f32)>,
    // A repair being rendered.
    job: Option<RenderSlot>,
    job_label: String,
    /// The file before the last repair (A/B against it).
    original: Option<PathBuf>,
}

struct Loaded {
    data: Arc<AudioData>,
    spectro: Vec<Vec<f32>>,
    notes: Vec<NoteBlob>,
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
            self.drift = 0.5;
        }
    }
}

/// Opens `clip` in the Audio Repair tab.
pub fn open(st: &mut State, clip: Id) {
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

/// Loads the clip's audio, its spectrogram and its notes (in the background) when its file changed.
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
                    let (a, b) = (c * d.frames() / n, ((c + 1) * d.frames() / n).max(c * d.frames() / n + 1).min(d.frames()));
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
            r.notes = l.notes;
            r.sel_notes.clear();
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
            let cols = (data.frames() / 512).clamp(1, MAX_COLS);
            let spectro = repair::spectrogram(&data, cols, ROWS);
            let notes = repair::detect_notes(&data);
            if let Ok(mut g) = out.lock() {
                *g = Some(Loaded { data, spectro, notes });
            }
            redhive_client_framework::runtime::request_repaint();
        }
    });
    r.source = Some(path.clone());
    r.loading = Some(slot);
    r.spectro = None;
    r.data = None;
    r.profile = None;
}

/// Renders `label` with `f` in the background; the result becomes the clip's new file.
fn render(st: &mut State, label: &str, f: impl FnOnce(&AudioData) -> AudioData + Send + 'static) {
    let dir = st.recordings_dir();
    let r = &mut st.ui.repair;
    let (Some(data), Some(src)) = (r.data.clone(), r.source.clone()) else { return };
    let sel = r.sel;
    let slot = Arc::new(Mutex::new(None));
    let out = slot.clone();
    let label_s = label.to_string();
    std::thread::spawn(move || {
        let mut y = f(&data);
        // Only inside the selected time range (10 ms crossfades), when there is one.
        if let Some((t0, t1, _)) = sel {
            let rate = data.rate as f64;
            let (a, b) = (((t0.min(t1)) * rate) as usize, ((t0.max(t1)) * rate) as usize);
            let fade = (0.01 * rate) as usize;
            for i in 0..data.frames() {
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
        let stem = src.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "clip".into());
        let stem = stem.split(" (repaired").next().unwrap_or(&stem).to_string();
        let _ = std::fs::create_dir_all(&dir);
        let mut k = 1;
        let path = loop {
            let p = dir.join(format!("{stem} (repaired {k}).wav"));
            if !p.exists() {
                break p;
            }
            k += 1;
        };
        let res = repair::write_wav(&path, &y).map(|_| (path, label_s)).map_err(|e| e.to_string());
        if let Ok(mut g) = out.lock() {
            *g = Some(res);
        }
        redhive_client_framework::runtime::request_repaint();
    });
    r.job = Some(slot);
    r.job_label = label.to_string();
}

/// Applies a finished render (every frame).
pub fn poll(st: &mut State) {
    let Some(job) = &st.ui.repair.job else { return };
    let Some(res) = job.lock().ok().and_then(|mut g| g.take()) else { return };
    st.ui.repair.job = None;
    let Some(cid) = st.ui.repair.clip else { return };
    match res {
        Ok((path, label)) => {
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
            st.toast(format!("{label} done – Ctrl+Z to undo"), Severity::Success);
        }
        Err(e) => st.toast(format!("Repair failed: {e}"), Severity::Error),
    }
}

pub fn show(ui: &mut Ui, st: &mut State) {
    st.ui.repair.defaults();
    let Some(cid) = st.ui.repair.clip else {
        ui.add_space(40.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new("Audio Repair").size(22.0));
            ui.label(RichText::new("Double-click an audio clip in the playlist and choose “Audio repair”.").color(theme::TEXT_DIM));
        });
        return;
    };
    let Some((_tid, name, path)) = clip_source(st, cid) else {
        st.ui.repair.clip = None;
        return;
    };
    ensure_loaded(ui, st, &path);
    // Header.
    ui.horizontal(|ui| {
        ui.label(RichText::new(&name).size(16.0).strong());
        ui.label(RichText::new(path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()).small().color(theme::TEXT_DIM));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if let Some(orig) = st.ui.repair.original.clone().filter(|o| *o != path) {
                if ui.button("▶ Original").on_hover_text("Hear the clip before the repairs").clicked() {
                    st.preview(&orig);
                }
            }
            if ui.button("▶ Play").on_hover_text("Hear the clip as it is now").clicked() {
                st.preview(&path);
            }
            if st.ui.repair.sel.is_some() && ui.button("Clear selection").clicked() {
                st.ui.repair.sel = None;
            }
        });
    });
    let Some(data) = st.ui.repair.data.clone() else {
        ui.add_space(30.0);
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label("Analysing the clip …");
        });
        return;
    };
    let side = 300.0;
    ui.horizontal_top(|ui| {
        let w = (ui.available_width() - side - 10.0).max(300.0);
        let h = (ui.available_height() - 8.0).max(260.0);
        ui.vertical(|ui| {
            ui.set_width(w);
            if st.ui.repair.tool == Tool::Pitch {
                notes_view(ui, st, &data, Vec2::new(w, h));
            } else {
                spectrogram_view(ui, st, &data, Vec2::new(w, h));
            }
        });
        ui.vertical(|ui| {
            ui.set_width(side);
            tools_panel(ui, st);
        });
    });
}

fn spectrogram_view(ui: &mut Ui, st: &mut State, data: &AudioData, size: Vec2) {
    let (r, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let p = ui.painter_at(r);
    p.rect_filled(r, 4.0, Color32::BLACK);
    let plot = Rect::from_min_max(Pos2::new(r.left() + 44.0, r.top() + 4.0), Pos2::new(r.right() - 4.0, r.bottom() - 20.0));
    let dur = data.duration().max(1e-6);
    let nyq = data.rate as f32 / 2.0;
    let x_of = |t: f64| plot.left() + (t / dur) as f32 * plot.width();
    let t_of = |x: f32| (((x - plot.left()) / plot.width()) as f64 * dur).clamp(0.0, dur);
    let y_of = |f: f32| plot.bottom() - ((f.max(20.0) / 20.0).ln() / (nyq / 20.0).ln()).clamp(0.0, 1.0) * plot.height();
    let f_of = |y: f32| 20.0 * (nyq / 20.0).powf(((plot.bottom() - y) / plot.height()).clamp(0.0, 1.0));
    if let Some(tex) = &st.ui.repair.spectro {
        p.image(tex.id(), plot, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
    }
    // Waveform overlay.
    let wave = &st.ui.repair.wave;
    if !wave.is_empty() {
        let mid = plot.center().y;
        // A faint outline only, so the spectrogram stays readable.
        let amp = plot.height() * 0.14;
        let top: Vec<Pos2> = wave.iter().enumerate().map(|(c, (_, hi))| Pos2::new(plot.left() + (c as f32 + 0.5) / wave.len() as f32 * plot.width(), mid - hi * amp)).collect();
        let bot: Vec<Pos2> = wave.iter().enumerate().map(|(c, (lo, _))| Pos2::new(plot.left() + (c as f32 + 0.5) / wave.len() as f32 * plot.width(), mid - lo * amp)).collect();
        p.add(egui::Shape::line(top, Stroke::new(1.0_f32, Color32::from_white_alpha(55))));
        p.add(egui::Shape::line(bot, Stroke::new(1.0_f32, Color32::from_white_alpha(55))));
    }
    // Axes.
    let small = egui::FontId::proportional(9.5);
    for f in [100.0f32, 1000.0, 10000.0] {
        let y = y_of(f);
        p.line_segment([Pos2::new(plot.left(), y), Pos2::new(plot.right(), y)], Stroke::new(1.0_f32, Color32::from_white_alpha(18)));
        p.text(Pos2::new(plot.left() - 4.0, y), egui::Align2::RIGHT_CENTER, if f >= 1000.0 { format!("{}k", f / 1000.0) } else { format!("{f}") }, small.clone(), theme::TEXT_DIM);
    }
    let step = [0.5, 1.0, 2.0, 5.0, 10.0, 30.0, 60.0].into_iter().find(|s| dur / s < 12.0).unwrap_or(120.0);
    let mut t = 0.0;
    while t <= dur {
        let x = x_of(t);
        p.text(Pos2::new(x, plot.bottom() + 3.0), egui::Align2::CENTER_TOP, format!("{}:{:04.1}", (t / 60.0) as u32, t % 60.0), small.clone(), theme::TEXT_DIM);
        t += step;
    }
    // Selection: drag = time range (all frequencies); Shift+drag = a time × frequency box.
    if resp.drag_started() {
        if let Some(pos) = resp.interact_pointer_pos() {
            st.ui.repair.drag_from = Some((t_of(pos.x), f_of(pos.y)));
        }
    }
    if resp.dragged() {
        if let (Some((t0, f0)), Some(pos)) = (st.ui.repair.drag_from, resp.interact_pointer_pos()) {
            let boxed = ui.input(|i| i.modifiers.shift);
            st.ui.repair.sel = Some((t0, t_of(pos.x), boxed.then(|| (f0.min(f_of(pos.y)), f0.max(f_of(pos.y))))));
        }
    }
    if resp.clicked() {
        st.ui.repair.sel = None;
    }
    if let Some((t0, t1, fr)) = st.ui.repair.sel {
        let (ya, yb) = match fr {
            Some((f0, f1)) => (y_of(f1), y_of(f0)),
            None => (plot.top(), plot.bottom()),
        };
        let sr = Rect::from_min_max(Pos2::new(x_of(t0.min(t1)), ya), Pos2::new(x_of(t0.max(t1)), yb));
        p.rect_filled(sr, 0.0, Color32::from_rgba_unmultiplied(255, 255, 255, 28));
        p.rect_stroke(sr, 0.0, Stroke::new(1.5_f32, theme::ACCENT_2), egui::StrokeKind::Inside);
        let label = match fr {
            Some((f0, f1)) => format!("{:.2} – {:.2} s · {:.0} – {:.0} Hz", t0.min(t1), t0.max(t1), f0, f1),
            None => format!("{:.2} – {:.2} s", t0.min(t1), t0.max(t1)),
        };
        p.text(sr.left_top() + Vec2::new(4.0, 4.0), egui::Align2::LEFT_TOP, label, small, Color32::WHITE);
    }
    if let Some(h) = resp.hover_pos().filter(|h| plot.contains(*h)) {
        resp.on_hover_text_at_pointer(format!("{:.2} s · {:.0} Hz   (drag: select a time range, Shift+drag: a time × frequency box)", t_of(h.x), f_of(h.y)));
    }
}

fn notes_view(ui: &mut Ui, st: &mut State, data: &AudioData, size: Vec2) {
    let (r, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let p = ui.painter_at(r);
    p.rect_filled(r, 4.0, Color32::from_rgb(0x12, 0x13, 0x18));
    let rp = &st.ui.repair;
    if rp.notes.is_empty() {
        p.text(r.center(), egui::Align2::CENTER_CENTER, "No notes found (Notes works on monophonic audio: a voice, a bass, a solo instrument)", egui::FontId::proportional(13.0), theme::TEXT_DIM);
        return;
    }
    let lo = rp.notes.iter().map(|n| n.midi).fold(f32::MAX, f32::min).floor() - 4.0;
    let hi = rp.notes.iter().map(|n| n.midi).fold(f32::MIN, f32::max).ceil() + 4.0;
    let plot = Rect::from_min_max(Pos2::new(r.left() + 40.0, r.top() + 4.0), Pos2::new(r.right() - 4.0, r.bottom() - 4.0));
    let dur = data.duration().max(1e-6);
    let x_of = |t: f64| plot.left() + (t / dur) as f32 * plot.width();
    let y_of = |m: f32| plot.bottom() - (m - lo) / (hi - lo) * plot.height();
    let row_h = plot.height() / (hi - lo);
    const NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    let mut m = lo as i32;
    while (m as f32) <= hi {
        let y = y_of(m as f32);
        let black = matches!(m.rem_euclid(12), 1 | 3 | 6 | 8 | 10);
        p.rect_filled(Rect::from_min_max(Pos2::new(plot.left(), y - row_h / 2.0), Pos2::new(plot.right(), y + row_h / 2.0)), 0.0, if black { Color32::from_rgb(0x0c, 0x0d, 0x11) } else { Color32::TRANSPARENT });
        if m.rem_euclid(12) == 0 || row_h > 11.0 {
            p.text(Pos2::new(plot.left() - 4.0, y), egui::Align2::RIGHT_CENTER, format!("{}{}", NAMES[m.rem_euclid(12) as usize], m / 12 - 1), egui::FontId::proportional(9.0), theme::TEXT_DIM);
        }
        m += 1;
    }
    let mut hit: Option<usize> = None;
    for (k, n) in rp.notes.iter().enumerate() {
        let shift = rp.shifts.get(k).copied().unwrap_or(0.0);
        let centre = n.midi + shift;
        let br = Rect::from_min_max(Pos2::new(x_of(n.start), y_of(centre) - row_h * (0.2 + 0.25 * n.level.min(1.0))), Pos2::new(x_of(n.end), y_of(centre) + row_h * (0.2 + 0.25 * n.level.min(1.0))));
        let sel = rp.sel_notes.contains(&k);
        let col = if shift.abs() > 0.01 { theme::ACCENT_2 } else { Color32::from_rgb(0xd8, 0x8a, 0x3c) };
        p.rect_filled(br, br.height() / 2.0, col.gamma_multiply(if sel { 0.95 } else { 0.6 }));
        if sel {
            p.rect_stroke(br, br.height() / 2.0, Stroke::new(1.5_f32, Color32::WHITE), egui::StrokeKind::Outside);
        }
        // The pitch curve through it.
        let pts: Vec<Pos2> = n.curve.iter().map(|(t, mm)| Pos2::new(x_of(*t), y_of(mm + shift))).collect();
        p.add(egui::Shape::line(pts, Stroke::new(1.2_f32, Color32::from_black_alpha(160))));
        if resp.hover_pos().is_some_and(|h| br.expand(2.0).contains(h)) {
            hit = Some(k);
        }
    }
    // Click selects (Ctrl adds); drag moves the selected notes (in semitones; Alt = free).
    if resp.drag_started() {
        if let (Some(k), Some(pos)) = (hit, resp.interact_pointer_pos()) {
            let r = &mut st.ui.repair;
            if !r.sel_notes.contains(&k) {
                r.sel_notes = vec![k];
            }
            r.note_drag = Some((k, pos.y));
        }
    }
    if resp.dragged() {
        if let (Some((_, y0)), Some(pos)) = (st.ui.repair.note_drag, resp.interact_pointer_pos()) {
            let free = ui.input(|i| i.modifiers.alt);
            let d = (y0 - pos.y) / row_h;
            let step = if free { d } else { d.round() };
            if step.abs() > 0.0 {
                let r = &mut st.ui.repair;
                for k in r.sel_notes.clone() {
                    if let Some(s) = r.shifts.get_mut(k) {
                        *s += step;
                    }
                }
                r.note_drag = Some((r.note_drag.unwrap().0, y0 - step * row_h));
            }
        }
    }
    if resp.drag_stopped() {
        st.ui.repair.note_drag = None;
    }
    if resp.clicked() {
        let ctrl = ui.input(|i| i.modifiers.command);
        let r = &mut st.ui.repair;
        match hit {
            Some(k) if ctrl => {
                if let Some(i) = r.sel_notes.iter().position(|x| *x == k) {
                    r.sel_notes.remove(i);
                } else {
                    r.sel_notes.push(k);
                }
            }
            Some(k) => r.sel_notes = vec![k],
            None => r.sel_notes.clear(),
        }
    }
    if let Some(k) = hit {
        let n = &st.ui.repair.notes[k];
        let s = st.ui.repair.shifts.get(k).copied().unwrap_or(0.0);
        let m = n.midi + s;
        let cents = ((m - m.round()) * 100.0).round() as i32;
        resp.on_hover_text_at_pointer(format!(
            "{}{} {cents:+} cents · {:.2} – {:.2} s{}",
            NAMES[(m.round() as i32).rem_euclid(12) as usize],
            m.round() as i32 / 12 - 1,
            n.start,
            n.end,
            if s.abs() > 0.01 { format!(" · moved {s:+.2} st") } else { String::new() }
        ));
    }
}

fn slider(ui: &mut Ui, v: &mut f32, range: std::ops::RangeInclusive<f32>, text: &str, suffix: &str) {
    ui.add(egui::Slider::new(v, range).text(text).suffix(suffix));
}

fn tools_panel(ui: &mut Ui, st: &mut State) {
    egui::ScrollArea::vertical().id_salt("repair.tools").show(ui, |ui| {
        for (t, name, tip) in TOOLS {
            let sel = st.ui.repair.tool == t;
            if ui.add_sized([ui.available_width(), 24.0], egui::SelectableLabel::new(sel, RichText::new(name).size(13.0))).on_hover_text(tip).clicked() {
                st.ui.repair.tool = t;
            }
        }
        ui.separator();
        let busy = st.ui.repair.job.is_some();
        let scope = if st.ui.repair.sel.is_some() { "the selection" } else { "the whole clip" };
        let r = &mut st.ui.repair;
        let tool = r.tool;
        let mut go: Option<(&'static str, Process)> = None;
        match tool {
            Tool::Denoise => {
                ui.label(RichText::new("Learn the noise from a stretch of noise alone (select it), or from the quietest parts.").small().color(theme::TEXT_DIM));
                if ui.button(if r.sel.is_some() { "Learn noise from the selection" } else { "Learn noise (quietest parts)" }).clicked() {
                    if let Some(d) = &r.data {
                        r.profile = Some(repair::learn_noise(d, r.sel.map(|(a, b, _)| (a.min(b), a.max(b)))));
                    }
                }
                ui.label(if r.profile.is_some() { RichText::new("✔ Noise profile learned").color(theme::OK) } else { RichText::new("No profile yet (the quietest parts are used)").color(theme::TEXT_DIM) });
                slider(ui, &mut r.reduction, 3.0..=40.0, "reduction", " dB");
                slider(ui, &mut r.sensitivity, 0.5..=3.0, "sensitivity", "");
                let (red, sens, prof) = (r.reduction, r.sensitivity, r.profile.clone());
                go = Some(("De-noise", Box::new(move |d| repair::denoise(d, &prof.unwrap_or_else(|| repair::learn_noise(d, None)), red, sens))));
            }
            Tool::Dehum => {
                ui.horizontal(|ui| {
                    ui.label("Mains");
                    for (v, n) in [(0.0, "Auto"), (50.0, "50 Hz"), (60.0, "60 Hz")] {
                        ui.selectable_value(&mut r.hum_base, v, n);
                    }
                });
                let mut h = r.hum_harmonics as f32;
                slider(ui, &mut h, 1.0..=16.0, "harmonics", "");
                r.hum_harmonics = h.round() as usize;
                slider(ui, &mut r.hum_q, 5.0..=60.0, "narrowness (Q)", "");
                let (b, hh, q) = (r.hum_base, r.hum_harmonics, r.hum_q);
                go = Some(("De-hum", Box::new(move |d| repair::dehum(d, b, hh, q))));
            }
            Tool::Declick => {
                slider(ui, &mut r.click_sens, 1.0..=10.0, "sensitivity", "");
                let s = r.click_sens;
                go = Some(("De-click", Box::new(move |d| repair::declick(d, s).0)));
            }
            Tool::Declip => {
                slider(ui, &mut r.clip_threshold, 0.9..=1.0, "clip level (× peak)", "");
                let t = r.clip_threshold;
                go = Some(("De-clip", Box::new(move |d| repair::declip(d, t).0)));
            }
            Tool::Distortion => {
                ui.label(RichText::new("Rebuilds peaks flattened by soft clipping, an overdriven preamp or a saturated recording.").small().color(theme::TEXT_DIM));
                slider(ui, &mut r.dist_amount, 0.0..=1.0, "amount", "");
                let a = r.dist_amount;
                go = Some(("Distortion repair", Box::new(move |d| repair::dedistort(d, a, 0.0).0)));
            }
            Tool::Dereverb => {
                slider(ui, &mut r.reverb_amount, 0.0..=1.0, "amount", "");
                slider(ui, &mut r.reverb_time, 0.2..=4.0, "room reverb time", " s");
                let (a, t) = (r.reverb_amount, r.reverb_time);
                go = Some(("De-reverb", Box::new(move |d| repair::dereverb(d, a, t))));
            }
            Tool::Spectral => {
                ui.label(RichText::new("Shift+drag a box on the spectrogram around the sound to take out (a cough, a phone, a squeak).").small().color(theme::TEXT_DIM));
                slider(ui, &mut r.spectral_db, -60.0..=12.0, "gain", " dB");
                match r.sel {
                    Some((t0, t1, Some((f0, f1)))) => {
                        let g = r.spectral_db;
                        go = Some(("Spectral attenuate", Box::new(move |d| repair::attenuate(d, t0, t1, f0, f1, g))));
                    }
                    _ => {
                        ui.label(RichText::new("Select a time × frequency box first (Shift+drag).").color(theme::WARN));
                    }
                }
            }
            Tool::Gain => {
                slider(ui, &mut r.gain_db, -24.0..=24.0, "gain", " dB");
                let g = r.gain_db;
                go = Some(("Gain", Box::new(move |d| repair::gain(d, 0.0, d.duration(), g))));
            }
            Tool::Pitch => {
                ui.label(RichText::new("Drag notes up / down (semitones; Alt = free). Click selects, Ctrl+click adds.").small().color(theme::TEXT_DIM));
                let all = r.sel_notes.is_empty();
                let targets: Vec<usize> = if all { (0..r.notes.len()).collect() } else { r.sel_notes.clone() };
                if ui.button(if all { "Pitch centre: all notes onto the nearest semitone" } else { "Pitch centre: selected notes" }).clicked() {
                    for k in &targets {
                        let m = r.notes[*k].midi + r.shifts[*k];
                        r.shifts[*k] += m.round() - m;
                    }
                }
                slider(ui, &mut r.drift, 0.0..=1.0, "flatten drift / vibrato", "");
                if ui.button("Reset moves").clicked() {
                    r.shifts.iter_mut().for_each(|s| *s = 0.0);
                }
                let moved = r.shifts.iter().filter(|s| s.abs() > 0.005).count();
                ui.label(RichText::new(format!("{} notes · {moved} moved", r.notes.len())).color(theme::TEXT_DIM));
                let notes = r.notes.clone();
                let edits: Vec<PitchEdit> =
                    (0..notes.len()).filter(|k| r.shifts[*k].abs() > 0.005 || targets.contains(k)).map(|k| PitchEdit { note: k, shift: r.shifts[k], drift: if targets.contains(&k) { r.drift } else { 0.0 } }).collect();
                go = Some(("Notes", Box::new(move |d| repair::apply_pitch(d, &notes, &edits))));
            }
        }
        ui.add_space(8.0);
        if busy {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(format!("{} …", st.ui.repair.job_label));
            });
        } else if let Some((label, f)) = go {
            let b = egui::Button::new(RichText::new(format!("Render {label} on {scope}")).strong()).fill(theme::ACCENT.gamma_multiply(0.6)).min_size(Vec2::new(ui.available_width(), 30.0));
            if ui.add(b).on_hover_text("Writes a new file and points the clip at it (Ctrl+Z goes back)").clicked() {
                render(st, label, f);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(path: &std::path::Path) {
        // A 220 Hz tone with 60 Hz hum, 1 s.
        let rate = 48_000u32;
        let d = AudioData::from_planar(
            rate,
            (0..rate)
                .map(|i| {
                    let t = i as f32 / rate as f32;
                    0.4 * (std::f32::consts::TAU * 220.0 * t).sin() + 0.2 * (std::f32::consts::TAU * 60.0 * t).sin()
                })
                .collect(),
            (0..rate)
                .map(|i| {
                    let t = i as f32 / rate as f32;
                    0.4 * (std::f32::consts::TAU * 220.0 * t).sin() + 0.2 * (std::f32::consts::TAU * 60.0 * t).sin()
                })
                .collect(),
        );
        repair::write_wav(path, &d).unwrap();
    }

    #[test]
    fn a_repair_renders_to_a_new_file_and_undo_goes_back() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("take.wav");
        wav(&src);
        let mut st = State::new(redhive_client_framework::AppContext::new("en"), crate::state::Settings { show_splash: false, ..Default::default() });
        st.new_project();
        st.add_audio_clip_at(&src, None, 0.0);
        let cid = st.ui.clips[0];
        open(&mut st, cid);
        let ctx = egui::Context::default();
        let mut frame = |st: &mut State| {
            let _ = ctx.run(egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1200.0, 800.0))), ..Default::default() }, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| show(ui, st));
            });
        };
        let wait = |st: &mut State, frame: &mut dyn FnMut(&mut State), done: &dyn Fn(&State) -> bool| {
            for _ in 0..400 {
                frame(st);
                poll(st);
                if done(st) {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            panic!("timed out");
        };
        wait(&mut st, &mut frame, &|st| st.ui.repair.data.is_some());
        assert!(st.ui.repair.spectro.is_some(), "spectrogram drawn");
        assert!(!st.ui.repair.notes.is_empty(), "the tone is a note");
        // De-hum the whole clip.
        render(&mut st, "De-hum", |d| repair::dehum(d, 0.0, 4, 30.0));
        wait(&mut st, &mut frame, &|st| st.ui.repair.job.is_none() && clip_source(st, cid).map(|x| x.2) != Some(src.clone()));
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
}
