//! Export dialog in three steps – what (full mix or stems, range), where (name and folder), quality (presets
//! for mastering, streaming, sharing, archiving) – with a summary of format, length and file size, and
//! every technical setting under "Advanced". Renders in the background with progress and time left, then
//! shows the loudness and the files.

use crate::state::State;
use crate::theme;
use dragon_engine::export::{ExportFormat, ExportOptions, ExportRange, ExportReport};
use dragon_engine::model::Project;
use dragon_engine::render::Pool;
use dragon_engine::samples::BitDepth;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{mpsc, Arc};

pub struct ExportDialog {
    opts: ExportOptions,
    /// File name without extension (stems: the folder's name).
    name: String,
    dir: PathBuf,
    norm: Norm,
    peak: f32,
    lufs: f32,
    preset: usize,
    progress: Arc<AtomicU32>,
    rx: Option<mpsc::Receiver<Result<ExportReport, String>>>,
    result: Option<Result<ExportReport, String>>,
    started: Option<std::time::Instant>,
    with_video: bool,
    surround: bool,
    adm: bool,
}

#[derive(PartialEq, Clone, Copy)]
enum Norm {
    None,
    Peak,
    Loudness,
}

/// Quality presets: (title, what it is for, format, loudness normalisation).
const PRESETS: &[(&str, &str, ExportFormat, Option<f32>)] = &[
    ("Best quality", "WAV 24-bit · for mastering or an engineer", ExportFormat::Wav(BitDepth::Int24), None),
    ("Ready for streaming", "WAV 24-bit at -14 LUFS · Spotify, YouTube", ExportFormat::Wav(BitDepth::Int24), Some(-14.0)),
    ("Share & send", "MP3 320 · messages, e-mail, phones", ExportFormat::Mp3(320), None),
    ("Archive (lossless)", "FLAC 24-bit · lossless, about half of WAV", ExportFormat::Flac24, None),
    ("Studio master", "WAV 32-bit float · never clips, for processing", ExportFormat::Wav(BitDepth::Float32), None),
    ("Small file", "MP3 192 · quick previews", ExportFormat::Mp3(192), None),
];

fn format_name(f: ExportFormat) -> String {
    match f {
        ExportFormat::Wav(BitDepth::Int16) => "WAV 16-bit".into(),
        ExportFormat::Wav(BitDepth::Int24) => "WAV 24-bit".into(),
        ExportFormat::Wav(_) => "WAV 32-bit float".into(),
        ExportFormat::Flac16 => "FLAC 16-bit".into(),
        ExportFormat::Flac24 => "FLAC 24-bit".into(),
        ExportFormat::Mp3(k) => format!("MP3 {k} kbps"),
        ExportFormat::Ogg(_) => "OGG Vorbis".into(),
    }
}

/// Bytes per second of audio in a format (stereo or mono).
fn bytes_per_second(f: ExportFormat, rate: u32, mono: bool) -> f64 {
    let ch = if mono { 1.0 } else { 2.0 };
    match f {
        ExportFormat::Wav(BitDepth::Int16) => rate as f64 * ch * 2.0,
        ExportFormat::Wav(BitDepth::Int24) => rate as f64 * ch * 3.0,
        ExportFormat::Wav(_) => rate as f64 * ch * 4.0,
        ExportFormat::Flac16 => rate as f64 * ch * 2.0 * 0.6,
        ExportFormat::Flac24 => rate as f64 * ch * 3.0 * 0.6,
        ExportFormat::Mp3(k) => k as f64 * 1000.0 / 8.0,
        ExportFormat::Ogg(_) => 160_000.0 / 8.0,
    }
}

fn size_text(bytes: f64) -> String {
    if bytes >= 1.0e9 {
        format!("{:.1} GB", bytes / 1.0e9)
    } else if bytes >= 1.0e6 {
        format!("{:.0} MB", bytes / 1.0e6)
    } else {
        format!("{:.0} kB", (bytes / 1.0e3).max(1.0))
    }
}

fn time_text(secs: f64) -> String {
    let s = secs.max(0.0).round() as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

/// A selectable card: an icon, a title and a line under it.
fn card(ui: &mut egui::Ui, selected: bool, icon: &str, title: &str, sub: &str, width: f32) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(egui::vec2(width, 52.0), egui::Sense::click());
    let p = ui.painter();
    let fill = if selected {
        theme::ACCENT.gamma_multiply(0.22)
    } else if resp.hovered() {
        theme::PANEL_2
    } else {
        theme::PANEL
    };
    p.rect_filled(r, 7.0, fill);
    p.rect_stroke(r, 7.0, egui::Stroke::new(if selected { 1.5_f32 } else { 1.0 }, if selected { theme::ACCENT } else { theme::GRID }), egui::StrokeKind::Inside);
    let icon_r = egui::Rect::from_min_size(r.min + egui::vec2(10.0, 14.0), egui::vec2(24.0, 24.0));
    super::track_icons::draw(p, icon_r, icon, if selected { egui::Color32::WHITE } else { theme::TEXT_DIM });
    p.text(r.min + egui::vec2(44.0, 9.0), egui::Align2::LEFT_TOP, title, egui::FontId::proportional(13.0), if selected { egui::Color32::WHITE } else { theme::TEXT });
    let sub_g = p.layout(sub.to_string(), egui::FontId::proportional(10.0), theme::TEXT_DIM, width - 52.0);
    p.galley(r.min + egui::vec2(44.0, 27.0), sub_g, theme::TEXT_DIM);
    resp
}

/// A step heading: a numbered circle and a title.
fn step(ui: &mut egui::Ui, n: u32, title: &str) {
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::hover());
        ui.painter().circle_filled(r.center(), 10.0, theme::ACCENT);
        ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, n.to_string(), egui::FontId::proportional(12.0), egui::Color32::WHITE);
        ui.label(egui::RichText::new(title).size(14.0).strong());
    });
    ui.add_space(2.0);
}

impl ExportDialog {
    /// `project_path`: the saved project file; exports then go to its folder's `Renders`.
    pub fn new(p: &Project, stems: bool, project_path: Option<&std::path::Path>) -> Self {
        let name = if p.meta.name.is_empty() { "Untitled".to_string() } else { p.meta.name.clone() };
        let dir = match project_path.and_then(|f| f.parent()) {
            Some(d) => d.join(dragon_engine::project_io::RENDERS_DIR),
            None => dirs_documents().join("DragonDAW Exports"),
        };
        let mut d = ExportDialog {
            opts: ExportOptions { stems, ..Default::default() },
            name,
            dir,
            norm: Norm::None,
            peak: -0.3,
            lufs: p.loudness_target,
            preset: 0,
            progress: Arc::new(AtomicU32::new(0)),
            rx: None,
            result: None,
            started: None,
            with_video: p.video.is_some() && !stems,
            surround: false,
            adm: false,
        };
        d.apply_preset(0);
        d
    }

    /// Presets the dialog for MP3 (FL's Ctrl+Shift+R).
    pub fn set_mp3(&mut self) {
        self.apply_preset(2);
    }

    fn apply_preset(&mut self, k: usize) {
        let (_, _, f, lufs) = PRESETS[k];
        self.preset = k;
        self.opts.format = f;
        match lufs {
            Some(v) => {
                self.norm = Norm::Loudness;
                self.lufs = v;
            }
            None => self.norm = Norm::None,
        }
    }

    /// Where the export goes: the file (or the stems folder).
    fn path(&self) -> PathBuf {
        let name = if self.name.trim().is_empty() { "Untitled" } else { self.name.trim() };
        if self.opts.stems {
            self.dir.join(format!("{name} stems"))
        } else {
            self.dir.join(format!("{name}.{}", self.opts.format.extension()))
        }
    }

    /// Length of what will be exported (seconds), and how many files.
    fn extent(&self, st: &State) -> (f64, usize) {
        let p = &st.session.project;
        let tm = dragon_engine::time::TempoMap::from_project(p);
        let beats = match self.opts.range {
            ExportRange::Song => (0.0, p.song_length()),
            ExportRange::Loop => p.loop_range.map(|r| (r.start, r.end)).unwrap_or((0.0, p.song_length())),
            ExportRange::Pattern(id) => (0.0, p.pattern(id).map(|x| x.length_beats()).unwrap_or(4.0)),
            ExportRange::Range(r) => (r.start, r.end),
        };
        let secs = (tm.beat_to_seconds(beats.1) - tm.beat_to_seconds(beats.0)).max(0.0) + self.opts.tail;
        let files = if self.opts.stems { p.mixer.channels.iter().filter(|c| c.kind == dragon_engine::model::ChannelKind::Audio).count().max(1) } else { 1 };
        (secs, files)
    }

    /// Shows the dialog; returns `false` when it should close.
    pub fn show(&mut self, ctx: &egui::Context, st: &mut State) -> bool {
        let mut open = true;
        let mut close = false;
        egui::Window::new(if self.opts.stems { "Export stems" } else { "Export" }).open(&mut open).collapsible(false).resizable(false).default_width(520.0).show(ctx, |ui| {
            ui.set_width(520.0);
            if self.rx.is_some() {
                self.progress_view(ui, st);
                ctx.request_repaint();
                return;
            }
            if self.result.is_some() {
                close = self.result_view(ui);
                return;
            }
            close = self.form(ui, st);
        });
        open && !close
    }

    fn progress_view(&mut self, ui: &mut egui::Ui, st: &mut State) {
        let pc = self.progress.load(Ordering::Relaxed) as f32 / 1000.0;
        ui.add_space(14.0);
        ui.vertical_centered(|ui| {
            ui.label(egui::RichText::new(if self.opts.stems { "Rendering the stems …" } else { "Rendering your mix …" }).size(16.0).strong());
            ui.add_space(8.0);
            ui.add(egui::ProgressBar::new(pc).desired_height(22.0).text(egui::RichText::new(format!("{:.0} %", pc * 100.0)).strong()).animate(true));
            if let Some(t) = self.started {
                let el = t.elapsed().as_secs_f64();
                let left = if pc > 0.02 { el / pc as f64 * (1.0 - pc as f64) } else { f64::NAN };
                ui.label(egui::RichText::new(if left.is_finite() { format!("{} elapsed · about {} left", time_text(el), time_text(left)) } else { format!("{} elapsed", time_text(el)) }).small().color(theme::TEXT_DIM));
            }
            ui.label(egui::RichText::new(self.path().display().to_string()).small().color(theme::TEXT_DIM));
        });
        ui.add_space(14.0);
        let got = match self.rx.as_ref().map(|rx| rx.try_recv()) {
            Some(Ok(r)) => Some(r),
            // The export thread died (a panic): report it instead of waiting forever.
            Some(Err(mpsc::TryRecvError::Disconnected)) => Some(Err("The export stopped unexpectedly. A crash report was written to the Crash folder in Dragon DAW's local data.".to_string())),
            _ => None,
        };
        if let Some(r) = got {
            crate::crash::clear(&st.ctx.paths.local, "exporting");
            if r.is_ok() {
                crate::views::quests::event(st, crate::views::quests::Event::Exported);
            }
            self.result = Some(r);
            self.rx = None;
        }
    }

    /// Returns `true` to close.
    fn result_view(&mut self, ui: &mut egui::Ui) -> bool {
        let mut close = false;
        ui.add_space(10.0);
        match self.result.as_ref() {
            Some(Ok(rep)) => {
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(egui::vec2(34.0, 34.0), egui::Sense::hover());
                    ui.painter().circle_filled(r.center(), 16.0, theme::OK);
                    ui.painter().line_segment([r.center() + egui::vec2(-7.0, 0.0), r.center() + egui::vec2(-2.0, 6.0)], egui::Stroke::new(3.0_f32, egui::Color32::WHITE));
                    ui.painter().line_segment([r.center() + egui::vec2(-2.0, 6.0), r.center() + egui::vec2(8.0, -6.0)], egui::Stroke::new(3.0_f32, egui::Color32::WHITE));
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new("Done!").size(18.0).strong());
                        ui.label(format!("{} file{} · {} of audio", rep.files.len(), if rep.files.len() == 1 { "" } else { "s" }, time_text(rep.seconds)));
                    });
                });
                ui.add_space(8.0);
                if rep.loudness.integrated > -69.0 {
                    ui.horizontal(|ui| {
                        for (label, v) in [("Loudness", format!("{:.1} LUFS", rep.loudness.integrated)), ("True peak", format!("{:.1} dBTP", rep.loudness.true_peak_db))] {
                            egui::Frame::NONE.fill(theme::PANEL_2).corner_radius(6.0).inner_margin(egui::Margin::symmetric(10, 5)).show(ui, |ui| {
                                ui.vertical(|ui| {
                                    ui.label(egui::RichText::new(label).small().color(theme::TEXT_DIM));
                                    ui.label(egui::RichText::new(v).strong());
                                });
                            });
                        }
                        if rep.loudness.true_peak_db > -0.1 {
                            ui.colored_label(theme::WARN, "Peaks reach 0 dB – consider a limiter or normalising to -1 dBTP");
                        }
                    });
                }
                ui.add_space(6.0);
                for f in rep.files.iter().take(6) {
                    ui.label(egui::RichText::new(f.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()).monospace().small());
                }
                if rep.files.len() > 6 {
                    ui.label(egui::RichText::new(format!("… and {} more", rep.files.len() - 6)).small().color(theme::TEXT_DIM));
                }
            }
            Some(Err(e)) => {
                ui.label(egui::RichText::new("The export did not finish").size(16.0).strong().color(theme::REC));
                ui.label(e);
            }
            None => {}
        }
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            let folder = if self.opts.stems { self.path() } else { self.dir.clone() };
            if ui.add(egui::Button::new(egui::RichText::new("Open folder").strong()).min_size(egui::vec2(120.0, 30.0))).clicked() {
                crate::chains::reveal(&folder);
            }
            if ui.add(egui::Button::new("Export again").min_size(egui::vec2(110.0, 30.0))).clicked() {
                self.result = None;
            }
            if ui.add(egui::Button::new("Close").min_size(egui::vec2(80.0, 30.0))).clicked() {
                close = true;
            }
        });
        close
    }

    /// The three steps and the export button. Returns `true` to close.
    fn form(&mut self, ui: &mut egui::Ui, st: &mut State) -> bool {
        let mut close = false;
        let w = 520.0;
        // ---- 1. What
        step(ui, 1, "What to export");
        ui.horizontal(|ui| {
            if card(ui, !self.opts.stems, "master", "Full mix", "One file: everything you hear", (w - 8.0) / 2.0).clicked() {
                self.opts.stems = false;
            }
            if card(ui, self.opts.stems, "bus", "Stems", "One file per mixer track, all aligned", (w - 8.0) / 2.0).clicked() {
                self.opts.stems = true;
            }
        });
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Range").small().color(theme::TEXT_DIM));
            let is = |r: &ExportRange, k: u8| matches!((r, k), (ExportRange::Song, 0) | (ExportRange::Loop, 1) | (ExportRange::Pattern(_), 2));
            if ui.selectable_label(is(&self.opts.range, 0), "Whole song").clicked() {
                self.opts.range = ExportRange::Song;
            }
            if ui.add_enabled(st.session.project.loop_range.is_some(), egui::SelectableLabel::new(is(&self.opts.range, 1), "Loop range")).on_disabled_hover_text("Set a loop range in the playlist first").clicked() {
                self.opts.range = ExportRange::Loop;
            }
            if ui.selectable_label(is(&self.opts.range, 2), "Current pattern").clicked() {
                self.opts.range = ExportRange::Pattern(st.ui.pattern);
            }
        });
        // ---- 2. Where
        step(ui, 2, "Name and place");
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.name).desired_width(w - 110.0).hint_text("File name").font(egui::TextStyle::Body));
            ui.label(egui::RichText::new(if self.opts.stems { " (folder)".to_string() } else { format!(".{}", self.opts.format.extension()) }).color(theme::TEXT_DIM));
        });
        ui.horizontal(|ui| {
            let folder = self.dir.display().to_string();
            ui.label(egui::RichText::new(format!("in  {folder}")).small().color(theme::TEXT_DIM)).on_hover_text(folder);
            if ui.small_button("Change…").clicked() {
                if let Some(d) = rfd::FileDialog::new().set_directory(&self.dir).pick_folder() {
                    self.dir = d;
                }
            }
            if ui.small_button("Open").clicked() {
                let _ = std::fs::create_dir_all(&self.dir);
                crate::chains::reveal(&self.dir);
            }
        });
        // ---- 3. Quality
        step(ui, 3, "Quality");
        let cw = (w - 8.0) / 2.0;
        egui::Grid::new("export.presets").spacing(egui::vec2(8.0, 8.0)).show(ui, |ui| {
            for (k, (title, sub, _, _)) in PRESETS.iter().enumerate() {
                let icon = match k {
                    0 | 4 => "master",
                    1 => "sfx",
                    2 | 5 => "fx",
                    _ => "keys",
                };
                if card(ui, self.preset == k, icon, title, sub, cw).clicked() {
                    self.apply_preset(k);
                }
                if k % 2 == 1 {
                    ui.end_row();
                }
            }
        });
        ui.add_space(4.0);
        egui::CollapsingHeader::new(egui::RichText::new("Advanced settings").small()).id_salt("export.advanced").show(ui, |ui| self.advanced(ui, st));
        // ---- summary and export
        ui.add_space(6.0);
        let (secs, files) = self.extent(st);
        let size = bytes_per_second(self.opts.format, self.opts.rate, self.opts.mono) * secs * files as f64;
        let norm = match self.norm {
            Norm::None => String::new(),
            Norm::Peak => format!(" · peak {:.1} dB", self.peak),
            Norm::Loudness => format!(" · {:.0} LUFS", self.lufs),
        };
        egui::Frame::NONE.fill(theme::PANEL_2).corner_radius(6.0).inner_margin(egui::Margin::symmetric(10, 6)).show(ui, |ui| {
            ui.set_width(w - 20.0);
            ui.label(egui::RichText::new(self.path().file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default()).strong());
            ui.label(
                egui::RichText::new(format!(
                    "{}{} · {} · {:.1} kHz{} · {} · about {}",
                    if files > 1 { format!("{files} files · ") } else { String::new() },
                    format_name(self.opts.format),
                    if self.opts.mono { "mono" } else { "stereo" },
                    self.opts.rate as f32 / 1000.0,
                    norm,
                    time_text(secs),
                    size_text(size)
                ))
                .small()
                .color(theme::TEXT_DIM),
            );
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let go = ui
                .add(egui::Button::new(egui::RichText::new(if self.opts.stems { "Export stems" } else { "Export" }).size(15.0).strong().color(egui::Color32::WHITE)).fill(theme::ACCENT).min_size(egui::vec2(160.0, 34.0)));
            if go.clicked() {
                self.start(st);
            }
            if ui.add(egui::Button::new("Cancel").min_size(egui::vec2(90.0, 34.0))).clicked() {
                close = true;
            }
        });
        close
    }

    fn advanced(&mut self, ui: &mut egui::Ui, st: &mut State) {
        egui::Grid::new("exportgrid").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            ui.label("Format");
            ui.horizontal_wrapped(|ui| {
                for f in [
                    ExportFormat::Wav(BitDepth::Int16),
                    ExportFormat::Wav(BitDepth::Int24),
                    ExportFormat::Wav(BitDepth::Float32),
                    ExportFormat::Flac16,
                    ExportFormat::Flac24,
                    ExportFormat::Mp3(320),
                    ExportFormat::Mp3(192),
                    ExportFormat::Ogg(0.6),
                ] {
                    if ui.selectable_label(self.opts.format == f, format_name(f)).clicked() {
                        self.opts.format = f;
                        self.preset = usize::MAX;
                    }
                }
            });
            ui.end_row();
            ui.label("Sample rate");
            ui.horizontal(|ui| {
                for r in [44100u32, 48000, 88200, 96000] {
                    if ui.selectable_label(self.opts.rate == r, format!("{} kHz", r as f32 / 1000.0)).clicked() {
                        self.opts.rate = r;
                        self.opts.render_rate = r;
                    }
                }
            });
            ui.end_row();
            ui.label("Tail");
            ui.add(egui::Slider::new(&mut self.opts.tail, 0.0..=10.0).suffix(" s")).on_hover_text("Extra time after the end, for reverb and delay tails");
            ui.end_row();
            ui.label("Normalise");
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.norm, Norm::None, "off");
                ui.selectable_value(&mut self.norm, Norm::Peak, "peak");
                ui.selectable_value(&mut self.norm, Norm::Loudness, "loudness");
                match self.norm {
                    Norm::Peak => {
                        ui.add(egui::DragValue::new(&mut self.peak).range(-24.0..=0.0).speed(0.1).suffix(" dBFS"));
                    }
                    Norm::Loudness => {
                        ui.add(egui::DragValue::new(&mut self.lufs).range(-36.0..=-5.0).speed(0.1).suffix(" LUFS"));
                        ui.menu_button("presets", |ui| {
                            for (n, v) in [("Streaming −14", -14.0), ("Apple Music −16", -16.0), ("Broadcast EBU R128 −23", -23.0), ("Podcast −16", -16.0), ("Club / CD −9", -9.0)] {
                                if ui.button(n).clicked() {
                                    self.lufs = v;
                                    ui.close_menu();
                                }
                            }
                        });
                    }
                    Norm::None => {}
                }
            });
            ui.end_row();
            ui.label("Options");
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.opts.dither, "dither (16/24-bit)");
                ui.checkbox(&mut self.opts.mono, "mono");
            });
            ui.end_row();
            if let (Some(f), false) = (st.session.project.surround, self.opts.stems) {
                ui.label("Surround");
                if ui.checkbox(&mut self.surround, format!("write the {} mix as a multichannel WAV", dragon_engine::surround::label(f))).changed() && self.surround {
                    self.adm = false;
                }
                ui.end_row();
            }
            if st.session.project.surround.is_some() && !self.opts.stems {
                ui.label("Dolby Atmos");
                if ui.checkbox(&mut self.adm, "write the channels as objects in an ADM BWF file (_adm.wav)").changed() && self.adm {
                    self.surround = false;
                }
                ui.end_row();
            }
            if st.session.project.video.is_some() && !self.opts.stems {
                ui.label("Video");
                ui.checkbox(&mut self.with_video, "also write the video with this mix (.mp4, picture copied)");
                ui.end_row();
            }
        });
    }

    fn start(&mut self, st: &mut State) {
        self.opts.normalize_peak = (self.norm == Norm::Peak).then_some(self.peak);
        self.opts.loudness_target = (self.norm == Norm::Loudness).then_some(self.lufs);
        let path = self.path();
        self.started = Some(std::time::Instant::now());
        self.progress.store(0, Ordering::Relaxed);
        if let Some(d) = if self.opts.stems { Some(path.as_path()) } else { path.parent() } {
            let _ = std::fs::create_dir_all(d);
        }
        st.capture_plugin_states();
        let project = st.session.project.clone();
        let cache = st.session.cache.clone();
        let opts = self.opts.clone();
        let progress = self.progress.clone();
        let video = if self.with_video && !self.opts.stems { project.video.clone() } else { None };
        let surround = self.surround && project.surround.is_some() && !self.opts.stems;
        let adm = self.adm && project.surround.is_some() && !self.opts.stems;
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        crate::crash::mark(&st.ctx.paths.local, "exporting", &path);
        std::thread::spawn(move || {
            let r = if adm {
                let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "mix".into());
                let path = path.with_file_name(format!("{stem}_adm.wav"));
                dragon_engine::surround::export_adm(&project, &cache, &path, &opts, &mut || Pool::with_plugins(Box::new(dragon_engine::plugins::Host)), &mut |x| progress.store((x * 1000.0) as u32, Ordering::Relaxed))
                    .map(|_| ExportReport { files: vec![path], loudness: dragon_engine::dsp::loudness::LoudnessStats { integrated: -70.0, true_peak_db: -180.0, sample_peak_db: -180.0 }, seconds: 0.0 })
            } else if surround {
                let path = path.with_extension("wav");
                dragon_engine::surround::export_surround(&project, &cache, &path, &opts, &mut || Pool::with_plugins(Box::new(dragon_engine::plugins::Host)), &mut |x| {
                    progress.store((x * 1000.0) as u32, Ordering::Relaxed)
                })
                .map(|_| ExportReport { files: vec![path], loudness: dragon_engine::dsp::loudness::LoudnessStats { integrated: -70.0, true_peak_db: -180.0, sample_peak_db: -180.0 }, seconds: 0.0 })
            } else {
                dragon_engine::export::export_with(&project, &cache, &path, &opts, &mut || Pool::with_plugins(Box::new(dragon_engine::plugins::Host)), &mut |x| progress.store((x * 1000.0) as u32, Ordering::Relaxed))
            };
            let r = r.map_err(|e| e.to_string()).and_then(|mut rep| {
                if let Some(v) = video {
                    let out = path.with_extension("mp4");
                    dragon_engine::video::mux(&v, &path, &out).map_err(|e| e.to_string())?;
                    rep.files.push(out);
                }
                Ok(rep)
            });
            let _ = tx.send(r);
        });
    }
}

fn dirs_documents() -> PathBuf {
    std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(|h| PathBuf::from(h).join("Documents")).unwrap_or_else(std::env::temp_dir)
}

#[cfg(test)]
impl ExportDialog {
    pub fn test_set_dir(&mut self, d: &std::path::Path) {
        self.dir = d.to_path_buf();
    }
    pub fn test_preset(&mut self, k: usize) {
        self.apply_preset(k);
    }
    pub fn test_stems(&mut self, on: bool) {
        self.opts.stems = on;
    }
    /// (file name, loudness target if normalising to loudness).
    pub fn test_summary(&self) -> (String, Option<f32>) {
        (self.path().file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default(), (self.norm == Norm::Loudness).then_some(self.lufs))
    }
    pub fn test_start(&mut self, st: &mut State) {
        self.start(st);
    }
    pub fn test_running(&self) -> bool {
        self.rx.is_some()
    }
    pub fn test_files(&self) -> Option<Vec<PathBuf>> {
        self.result.as_ref().and_then(|r| r.as_ref().ok()).map(|r| r.files.clone())
    }
}
