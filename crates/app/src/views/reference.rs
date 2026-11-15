//! Mastering › Reference: load a commercial track, select a region of it on its waveform, and compare it with
//! the mix (whole song or loop range) – loudness, RMS, peaks, crest factor, dynamics, stereo and tonal
//! balance – with suggestions, and loudness-matched A/B listening of the selected region.

use crate::state::State;
use crate::theme;
use dragon_engine::device::Command as Cmd;
use dragon_engine::reference::{self as refa, RegionAnalysis};
use dragon_engine::render::{LiveEvent, Pool};
use dragon_engine::samples::AudioData;
use egui::{Color32, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;

/// Which part of the song the mix analysis covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MixRange {
    #[default]
    Song,
    Loop,
}

/// A decoded reference file with its waveform overview.
type Loaded = (Arc<AudioData>, Vec<(f32, f32)>);
/// What a reference analysis was computed for: the file and the region in seconds.
type RegionKey = (PathBuf, (f64, f64));

enum Job {
    Loaded(PathBuf, Result<Loaded, String>),
    Reference(RegionKey, RegionAnalysis),
    Mix(u64, RegionAnalysis),
}

#[derive(Default)]
pub struct ReferenceState {
    audio: Option<(PathBuf, Arc<AudioData>)>,
    overview: Vec<(f32, f32)>,
    loading: Option<PathBuf>,
    error: Option<String>,
    ref_analysis: Option<(RegionKey, RegionAnalysis)>,
    ref_pending: Option<RegionKey>,
    mix_analysis: Option<(u64, RegionAnalysis)>,
    mix_running: Option<u64>,
    mix_progress: Arc<AtomicU32>,
    pub mix_range: MixRange,
    pub matched: bool,
    jobs: Option<(std::sync::mpsc::Sender<Job>, Receiver<Job>)>,
    drag_from: Option<f64>,
}

impl ReferenceState {
    /// The current reference and mix analyses, if any.
    #[cfg(test)]
    pub fn results(&self) -> (Option<&RegionAnalysis>, Option<&RegionAnalysis>) {
        (self.ref_analysis.as_ref().map(|(_, a)| a), self.mix_analysis.as_ref().map(|(_, a)| a))
    }

    fn tx(&mut self) -> std::sync::mpsc::Sender<Job> {
        self.jobs.get_or_insert_with(channel).0.clone()
    }

    fn poll(&mut self) {
        let Some((_, rx)) = &self.jobs else { return };
        let got: Vec<Job> = rx.try_iter().collect();
        for j in got {
            match j {
                Job::Loaded(path, Ok((d, ov))) => {
                    self.audio = Some((path, d));
                    self.overview = ov;
                    self.loading = None;
                    self.error = None;
                }
                Job::Loaded(path, Err(e)) => {
                    self.loading = None;
                    self.error = Some(format!("{}: {e}", path.display()));
                }
                Job::Reference(key, a) => {
                    if self.ref_pending.as_ref() == Some(&key) {
                        self.ref_pending = None;
                    }
                    self.ref_analysis = Some((key, a));
                }
                Job::Mix(rev, a) => {
                    self.mix_running = None;
                    self.mix_analysis = Some((rev, a));
                }
            }
        }
    }
}

pub fn show(ui: &mut Ui, st: &mut State) {
    st.reference.poll();
    let reference = st.session.project.reference.clone();
    // Load the reference file when the project names one that is not loaded yet.
    if let Some(r) = &reference {
        let loaded = st.reference.audio.as_ref().map(|(p, _)| p == &r.path).unwrap_or(false);
        if !loaded && st.reference.loading.as_ref() != Some(&r.path) && st.reference.error.is_none() {
            start_load(st, r.path.clone());
        }
    }
    ui.horizontal(|ui| {
        ui.heading("Reference");
        if ui.button("Load reference…").on_hover_text("A finished song to compare your master with").clicked() {
            if let Some(f) = rfd::FileDialog::new().add_filter("Audio", &["wav", "flac", "mp3", "ogg", "aif", "aiff", "m4a"]).pick_file() {
                st.reference.error = None;
                st.session.edit("Reference track", |p| p.reference = Some(dragon_engine::model::ReferenceTrack { path: f, region: None }));
            }
        }
        if let Some(r) = &reference {
            ui.label(RichText::new(r.path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()).strong());
            if ui.small_button("x").on_hover_text("Remove the reference").clicked() {
                st.session.edit("Remove reference", |p| p.reference = None);
                st.reference = ReferenceState { mix_analysis: st.reference.mix_analysis.take(), ..Default::default() };
                st.audio.send(Cmd::Live(LiveEvent::StopPreview));
            }
        }
        if st.reference.loading.is_some() {
            ui.spinner();
            ui.label(RichText::new("decoding…").small().color(theme::TEXT_DIM));
        }
    });
    if let Some(e) = &st.reference.error {
        ui.label(RichText::new(e).color(theme::REC));
    }
    let Some(r) = reference else {
        ui.label(RichText::new("Load a reference song, drag over its waveform to pick a section (for example the chorus), and compare it with your mix.").color(theme::TEXT_DIM));
        return;
    };
    let Some((_, data)) = st.reference.audio.clone().filter(|(p, _)| *p == r.path) else { return };
    let dur = data.duration();
    let region = r.region.unwrap_or((0.0, dur)).clamp_to(dur);

    // ---- waveform with the region
    let w = ui.available_width().max(200.0);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 74.0), Sense::click_and_drag());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 4.0, theme::BG);
    let x_of = |t: f64| rect.left() + (t / dur.max(1e-9)) as f32 * rect.width();
    let t_of = |x: f32| (((x - rect.left()) / rect.width()) as f64 * dur).clamp(0.0, dur);
    let sel = Rect::from_x_y_ranges(x_of(region.0)..=x_of(region.1), rect.y_range());
    p.rect_filled(sel, 0.0, theme::PLAYHEAD.gamma_multiply(0.14));
    let n = st.reference.overview.len().max(1);
    for (k, (lo, hi)) in st.reference.overview.iter().enumerate() {
        let x = rect.left() + k as f32 / n as f32 * rect.width();
        let inside = x >= sel.left() && x <= sel.right();
        let c = if inside { theme::PLAYHEAD } else { theme::TEXT_DIM.gamma_multiply(0.6) };
        p.line_segment([Pos2::new(x, rect.center().y - hi * 34.0), Pos2::new(x, rect.center().y - lo * 34.0)], Stroke::new(1.0_f32, c));
    }
    p.rect_stroke(sel, 0.0, Stroke::new(1.0_f32, theme::PLAYHEAD), egui::StrokeKind::Inside);
    p.text(rect.left_top() + Vec2::new(4.0, 2.0), egui::Align2::LEFT_TOP, format!("{} – {}  ({:.1} s)", mmss(region.0), mmss(region.1), region.1 - region.0), egui::FontId::monospace(10.0), Color32::WHITE);
    p.text(rect.right_top() + Vec2::new(-4.0, 2.0), egui::Align2::RIGHT_TOP, "drag: select region · double-click: whole song", egui::FontId::proportional(10.0), theme::TEXT_DIM);
    if resp.drag_started() {
        if let Some(pos) = ui.input(|i| i.pointer.press_origin()) {
            st.reference.drag_from = Some(t_of(pos.x));
        }
    }
    if let (Some(from), Some(pos)) = (st.reference.drag_from, resp.interact_pointer_pos()) {
        let to = t_of(pos.x);
        let (a, b) = (from.min(to), from.max(to));
        p.rect_filled(Rect::from_x_y_ranges(x_of(a)..=x_of(b), rect.y_range()), 0.0, theme::ACCENT_2.gamma_multiply(0.18));
        if resp.drag_stopped() {
            st.reference.drag_from = None;
            if b - a > 0.5 {
                st.session.edit("Reference region", |p| p.reference.iter_mut().for_each(|r| r.region = Some((a, b))));
            }
        }
    }
    if resp.double_clicked() {
        st.session.edit("Reference region", |p| p.reference.iter_mut().for_each(|r| r.region = None));
    }

    // ---- reference analysis (background, whenever the region changes)
    let key = (r.path.clone(), region);
    let have = st.reference.ref_analysis.as_ref().map(|(k, _)| *k == key).unwrap_or(false);
    if !have && st.reference.ref_pending.as_ref() != Some(&key) {
        st.reference.ref_pending = Some(key.clone());
        let tx = st.reference.tx();
        let d = data.clone();
        let k = key.clone();
        std::thread::spawn(move || {
            let a = refa::analyze(&d, region.0, region.1);
            let _ = tx.send(Job::Reference(k, a));
        });
    }

    // ---- mix analysis controls and A/B
    let rev = st.session.history.revision;
    ui.horizontal(|ui| {
        ui.label("Mix:");
        let has_loop = st.session.project.loop_range.is_some();
        ui.selectable_value(&mut st.reference.mix_range, MixRange::Song, "whole song");
        ui.add_enabled_ui(has_loop, |ui| ui.selectable_value(&mut st.reference.mix_range, MixRange::Loop, "loop range"));
        let stale = st.reference.mix_analysis.as_ref().map(|(r, _)| *r != rev).unwrap_or(true);
        if let Some(_r) = st.reference.mix_running {
            ui.spinner();
            ui.label(RichText::new(format!("rendering {:.0} %", st.reference.mix_progress.load(Ordering::Relaxed) as f32 / 10.0)).small());
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
        } else if ui.button(if stale { "Analyse mix" } else { "Re-analyse mix" }).on_hover_text("Renders the mix offline (like an export) and measures it").clicked() {
            analyse_mix_now(st);
        }
        if stale && st.reference.mix_analysis.is_some() {
            ui.label(RichText::new("project changed since the analysis").small().color(theme::WARN));
        }
        ui.separator();
        if ui.button("▶ Play reference").on_hover_text("Plays the selected region through the master output (stop the song first)").clicked() {
            let gain = match (&st.reference.mix_analysis, &st.reference.ref_analysis) {
                (Some((_, m)), Some((_, a))) if st.reference.matched => refa::match_gain_db(m, a),
                _ => 0.0,
            };
            let ex = refa::excerpt(&data, region.0, region.1, gain);
            st.audio.send(Cmd::Live(LiveEvent::Preview(Arc::new(ex))));
        }
        if ui.button("■").on_hover_text("Stop the reference").clicked() {
            st.audio.send(Cmd::Live(LiveEvent::StopPreview));
        }
        ui.checkbox(&mut st.reference.matched, "loudness-matched").on_hover_text("Play the reference at your mix's loudness, so you compare tone and dynamics, not level");
    });
    if st.reference.ref_pending.is_some() || st.reference.mix_running.is_some() {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
    }

    let ra = st.reference.ref_analysis.as_ref().filter(|(k, _)| *k == key).map(|(_, a)| a.clone());
    let ma = st.reference.mix_analysis.as_ref().map(|(_, a)| a.clone());
    let Some(ra) = ra else {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label("Analysing the reference…");
        });
        return;
    };
    ui.add_space(4.0);
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.label(RichText::new("TONAL BALANCE (relative to loudness)").small().color(theme::TEXT_DIM));
            tone_chart(ui, ma.as_ref(), &ra);
            ui.horizontal(|ui| {
                legend(ui, theme::PLAYHEAD, "reference");
                legend(ui, theme::ACCENT_2, "mix");
                legend(ui, theme::OK, "difference (mix − reference)");
            });
        });
        ui.separator();
        ui.vertical(|ui| match &ma {
            Some(m) => {
                let c = refa::compare(m, &ra);
                egui::Grid::new("ref.table").striped(true).num_columns(4).show(ui, |ui| {
                    ui.label(RichText::new("").small());
                    ui.label(RichText::new("Mix").small().color(theme::ACCENT_2));
                    ui.label(RichText::new("Reference").small().color(theme::PLAYHEAD));
                    ui.label(RichText::new("Δ").small());
                    ui.end_row();
                    for row in &c.rows {
                        ui.label(RichText::new(row.name).small());
                        let shown = |v: f32| if row.name == "Side / mid" && v <= refa::SIDE_FLOOR { "mono".to_string() } else { fmt(v, row.unit) };
                        ui.label(RichText::new(shown(row.mix)).monospace());
                        ui.label(RichText::new(shown(row.reference)).monospace());
                        let d = row.delta();
                        let col = if d.abs() < 1.0 || row.unit.is_empty() && d.abs() < 0.1 { theme::OK } else { theme::WARN };
                        ui.label(RichText::new(format!("{d:+.1}")).monospace().color(col));
                        ui.end_row();
                    }
                    ui.label(RichText::new("Ranges (dB rel. loudness)").small().color(theme::TEXT_DIM));
                    ui.end_row();
                    for (k, (name, _, _)) in refa::RANGES.iter().enumerate() {
                        ui.label(RichText::new(*name).small());
                        ui.label(RichText::new(format!("{:.1}", m.ranges[k])).monospace());
                        ui.label(RichText::new(format!("{:.1}", ra.ranges[k])).monospace());
                        let d = m.ranges[k] - ra.ranges[k];
                        ui.label(RichText::new(format!("{d:+.1}")).monospace().color(if d.abs() < 2.0 { theme::OK } else { theme::WARN }));
                        ui.end_row();
                    }
                });
                ui.add_space(4.0);
                ui.label(RichText::new("SUGGESTIONS").small().color(theme::TEXT_DIM));
                for s in &c.suggestions {
                    ui.label(RichText::new(format!("• {s}")).small());
                }
            }
            None => {
                egui::Grid::new("ref.only").striped(true).num_columns(2).show(ui, |ui| {
                    for (n, v, u) in [
                        ("Integrated loudness", ra.integrated, "LUFS"),
                        ("Short-term max", ra.short_term_max, "LUFS"),
                        ("Loudness range", ra.lra, "LU"),
                        ("RMS", ra.rms_db, "dBFS"),
                        ("True peak", ra.true_peak_db, "dBTP"),
                        ("Crest factor", ra.crest_db, "dB"),
                        ("PLR", ra.plr, "dB"),
                        ("Side / mid", ra.side_db, "dB"),
                        ("Correlation", ra.correlation, ""),
                    ] {
                        ui.label(RichText::new(n).small());
                        ui.label(RichText::new(fmt(v, u)).monospace());
                        ui.end_row();
                    }
                });
                ui.label(RichText::new("Analyse the mix to see the differences.").small().color(theme::TEXT_DIM));
            }
        });
    });
}

trait Clamp {
    fn clamp_to(self, dur: f64) -> (f64, f64);
}

impl Clamp for (f64, f64) {
    fn clamp_to(self, dur: f64) -> (f64, f64) {
        let a = self.0.clamp(0.0, dur);
        let b = self.1.clamp(a, dur);
        (a, b)
    }
}

fn start_load(st: &mut State, path: PathBuf) {
    st.reference.loading = Some(path.clone());
    let tx = st.reference.tx();
    std::thread::spawn(move || {
        let r = dragon_engine::samples::decode(&path).map(|d| {
            let ov = d.peaks(0, d.frames(), 900);
            (Arc::new(d), ov)
        });
        let _ = tx.send(Job::Loaded(path, r.map_err(|e| e.to_string())));
    });
}

/// Starts the mix analysis for the current project state.
pub fn analyse_mix_now(st: &mut State) {
    let rev = st.session.history.revision;
    start_mix(st, rev);
}

fn start_mix(st: &mut State, rev: u64) {
    st.capture_plugin_states();
    let project = st.session.project.clone();
    let cache = st.session.cache.clone();
    let range = match st.reference.mix_range {
        MixRange::Loop if project.loop_range.is_some() => dragon_engine::export::ExportRange::Loop,
        _ => dragon_engine::export::ExportRange::Song,
    };
    let opts = dragon_engine::export::ExportOptions { range, tail: 0.0, ..Default::default() };
    let progress = st.reference.mix_progress.clone();
    progress.store(0, Ordering::Relaxed);
    st.reference.mix_running = Some(rev);
    let tx = st.reference.tx();
    std::thread::spawn(move || {
        let mut pool = Pool::with_plugins(Box::new(dragon_engine::plugins::Host));
        let mix = dragon_engine::export::render_mix(&project, &cache, &opts, &mut pool, &mut |x| progress.store((x * 1000.0) as u32, Ordering::Relaxed));
        let a = refa::analyze(&mix, 0.0, mix.duration());
        let _ = tx.send(Job::Mix(rev, a));
    });
}

fn mmss(t: f64) -> String {
    format!("{}:{:04.1}", (t / 60.0).floor() as u32, t % 60.0)
}

fn fmt(v: f32, unit: &str) -> String {
    if v <= -69.0 && unit != "LU" && unit != "dB" {
        "–".into()
    } else if unit.is_empty() {
        format!("{v:.2}")
    } else {
        format!("{v:.1} {unit}")
    }
}

fn legend(ui: &mut Ui, c: Color32, text: &str) {
    let (r, _) = ui.allocate_exact_size(Vec2::new(14.0, 3.0), Sense::hover());
    ui.painter().rect_filled(r, 0.0, c);
    ui.label(RichText::new(text).small().color(theme::TEXT_DIM));
}

fn tone_chart(ui: &mut Ui, mix: Option<&RegionAnalysis>, reference: &RegionAnalysis) {
    let w = (ui.available_width() * 0.55).clamp(320.0, 620.0);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 190.0), Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 4.0, theme::BG);
    let n = refa::BANDS.len();
    let x_of = |k: usize| rect.left() + 18.0 + k as f32 / (n - 1) as f32 * (rect.width() - 26.0);
    // Curves: −50 … 0 dB relative to loudness (top area); differences ±12 dB around the bottom line.
    let curve_area = Rect::from_min_max(rect.min + Vec2::new(0.0, 6.0), Pos2::new(rect.right(), rect.top() + rect.height() * 0.62));
    let diff_area = Rect::from_min_max(Pos2::new(rect.left(), curve_area.bottom() + 8.0), rect.max - Vec2::new(0.0, 14.0));
    let all: Vec<f32> = reference.tone.iter().chain(mix.map(|m| m.tone.iter()).into_iter().flatten()).copied().collect();
    let top = all.iter().copied().fold(-200.0f32, f32::max).ceil() + 2.0;
    let bottom = top - 48.0;
    let y_of = |db: f32| curve_area.top() + ((top - db) / (top - bottom)).clamp(0.0, 1.0) * curve_area.height();
    for (k, f) in refa::BANDS.iter().enumerate() {
        if [31.5, 100.0, 315.0, 1000.0, 3150.0, 10000.0].contains(f) {
            p.line_segment([Pos2::new(x_of(k), rect.top()), Pos2::new(x_of(k), rect.bottom() - 12.0)], Stroke::new(1.0_f32, theme::GRID));
            let label = if *f >= 1000.0 { format!("{}k", f / 1000.0) } else { format!("{f}") };
            p.text(Pos2::new(x_of(k), rect.bottom() - 1.0), egui::Align2::CENTER_BOTTOM, label, egui::FontId::monospace(9.0), theme::TEXT_DIM);
        }
    }
    let line = |vals: &[f32], c: Color32| {
        let pts: Vec<Pos2> = vals.iter().enumerate().map(|(k, v)| Pos2::new(x_of(k), y_of(*v))).collect();
        p.add(egui::Shape::line(pts, Stroke::new(2.0_f32, c)));
    };
    line(&reference.tone, theme::PLAYHEAD);
    let zero = diff_area.center().y;
    p.line_segment([Pos2::new(rect.left(), zero), Pos2::new(rect.right(), zero)], Stroke::new(1.0_f32, theme::GRID_BAR));
    p.text(Pos2::new(rect.left() + 2.0, diff_area.top()), egui::Align2::LEFT_TOP, "+12", egui::FontId::monospace(8.0), theme::TEXT_DIM);
    p.text(Pos2::new(rect.left() + 2.0, diff_area.bottom()), egui::Align2::LEFT_BOTTOM, "−12", egui::FontId::monospace(8.0), theme::TEXT_DIM);
    if let Some(m) = mix {
        line(&m.tone, theme::ACCENT_2);
        let bw = (rect.width() - 26.0) / n as f32 * 0.7;
        for k in 0..n {
            let d = (m.tone[k] - reference.tone[k]).clamp(-12.0, 12.0);
            let y = zero - d / 12.0 * (diff_area.height() / 2.0);
            let r = Rect::from_x_y_ranges(x_of(k) - bw / 2.0..=x_of(k) + bw / 2.0, y.min(zero)..=y.max(zero));
            p.rect_filled(r, 1.0, if d.abs() < 2.0 { theme::OK.gamma_multiply(0.7) } else { theme::WARN.gamma_multiply(0.85) });
        }
        if let Some(pos) = resp.hover_pos() {
            let k = (((pos.x - rect.left() - 18.0) / (rect.width() - 26.0)) * (n - 1) as f32).round().clamp(0.0, (n - 1) as f32) as usize;
            let f = refa::BANDS[k];
            resp.on_hover_text(format!("{} Hz: mix {:.1}, reference {:.1} ({:+.1} dB)", f, m.tone[k], reference.tone[k], m.tone[k] - reference.tone[k]));
        }
    }
}
