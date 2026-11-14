//! Export dialog: WAV 16/24/32-float or FLAC 16/24, sample rate, song / loop / pattern range, tail,
//! loudness or peak normalisation, dither, mono, stems. Renders in the background with progress.

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
    path: PathBuf,
    norm: Norm,
    peak: f32,
    lufs: f32,
    progress: Arc<AtomicU32>,
    rx: Option<mpsc::Receiver<Result<ExportReport, String>>>,
    result: Option<Result<ExportReport, String>>,
}

#[derive(PartialEq, Clone, Copy)]
enum Norm {
    None,
    Peak,
    Loudness,
}

impl ExportDialog {
    pub fn new(p: &Project, stems: bool) -> Self {
        let name = if p.meta.name.is_empty() { "Untitled".to_string() } else { p.meta.name.clone() };
        let dir = dirs_documents().join("DragonDAW Exports");
        let path = if stems { dir.join(format!("{name} stems")) } else { dir.join(format!("{name}.wav")) };
        ExportDialog { opts: ExportOptions { stems, ..Default::default() }, path, norm: Norm::None, peak: -0.3, lufs: p.loudness_target, progress: Arc::new(AtomicU32::new(0)), rx: None, result: None }
    }

    /// Shows the dialog; returns `false` when it should close.
    pub fn show(&mut self, ctx: &egui::Context, st: &mut State) -> bool {
        let mut open = true;
        let mut close = false;
        egui::Window::new(if self.opts.stems { "Export stems" } else { "Export mix" }).open(&mut open).collapsible(false).resizable(false).default_width(420.0).show(ctx, |ui| {
            if let Some(rx) = &self.rx {
                let pc = self.progress.load(Ordering::Relaxed) as f32 / 1000.0;
                ui.add(egui::ProgressBar::new(pc).show_percentage().animate(true));
                if let Ok(r) = rx.try_recv() {
                    self.result = Some(r);
                    self.rx = None;
                }
                ctx.request_repaint();
                return;
            }
            if let Some(r) = &self.result {
                match r {
                    Ok(rep) => {
                        ui.colored_label(theme::OK, format!("Exported {} file(s), {:.1} s", rep.files.len(), rep.seconds));
                        ui.label(format!("Integrated {:.1} LUFS · true peak {:.1} dBTP", rep.loudness.integrated, rep.loudness.true_peak_db));
                        for f in rep.files.iter().take(8) {
                            ui.label(egui::RichText::new(f.display().to_string()).small());
                        }
                    }
                    Err(e) => {
                        ui.colored_label(theme::REC, e);
                    }
                }
                if ui.button("Close").clicked() {
                    close = true;
                }
                return;
            }
            egui::Grid::new("exportgrid").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                ui.label(if self.opts.stems { "Folder" } else { "File" });
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(self.path.display().to_string()).small());
                    if ui.button("…").clicked() {
                        let pick = if self.opts.stems { rfd::FileDialog::new().pick_folder() } else { rfd::FileDialog::new().set_file_name(self.path.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default()).save_file() };
                        if let Some(p) = pick {
                            self.path = p;
                        }
                    }
                });
                ui.end_row();
                ui.label("Format");
                ui.horizontal(|ui| {
                    for (f, n) in [(ExportFormat::Wav(BitDepth::Int16), "WAV 16"), (ExportFormat::Wav(BitDepth::Int24), "WAV 24"), (ExportFormat::Wav(BitDepth::Float32), "WAV 32f"), (ExportFormat::Flac16, "FLAC 16"), (ExportFormat::Flac24, "FLAC 24")] {
                        ui.selectable_value(&mut self.opts.format, f, n);
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
                ui.label("Range");
                ui.horizontal(|ui| {
                    let is = |r: &ExportRange, k: u8| matches!((r, k), (ExportRange::Song, 0) | (ExportRange::Loop, 1) | (ExportRange::Pattern(_), 2));
                    if ui.selectable_label(is(&self.opts.range, 0), "Song").clicked() {
                        self.opts.range = ExportRange::Song;
                    }
                    if ui.add_enabled(st.session.project.loop_range.is_some(), egui::SelectableLabel::new(is(&self.opts.range, 1), "Loop range")).clicked() {
                        self.opts.range = ExportRange::Loop;
                    }
                    if ui.selectable_label(is(&self.opts.range, 2), "Current pattern").clicked() {
                        self.opts.range = ExportRange::Pattern(st.ui.pattern);
                    }
                });
                ui.end_row();
                ui.label("Tail");
                ui.add(egui::Slider::new(&mut self.opts.tail, 0.0..=10.0).suffix(" s"));
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
                    ui.checkbox(&mut self.opts.stems, "stems");
                });
                ui.end_row();
            });
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button(egui::RichText::new("Export").strong()).clicked() {
                    self.start(st);
                }
                if ui.button("Cancel").clicked() {
                    close = true;
                }
            });
        });
        open && !close
    }

    fn start(&mut self, st: &mut State) {
        self.opts.normalize_peak = (self.norm == Norm::Peak).then_some(self.peak);
        self.opts.loudness_target = (self.norm == Norm::Loudness).then_some(self.lufs);
        let ext = self.opts.format.extension();
        let path = if self.opts.stems { self.path.clone() } else { self.path.with_extension(ext) };
        if let Some(d) = if self.opts.stems { Some(path.as_path()) } else { path.parent() } {
            let _ = std::fs::create_dir_all(d);
        }
        st.capture_plugin_states();
        let project = st.session.project.clone();
        let cache = st.session.cache.clone();
        let opts = self.opts.clone();
        let progress = self.progress.clone();
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        std::thread::spawn(move || {
            let r = dragon_engine::export::export_with(&project, &cache, &path, &opts, &mut || Pool::with_plugins(Box::new(dragon_engine::plugins::Host)), &mut |x| progress.store((x * 1000.0) as u32, Ordering::Relaxed));
            let _ = tx.send(r.map_err(|e| e.to_string()));
        });
    }
}

fn dirs_documents() -> PathBuf {
    std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(|h| PathBuf::from(h).join("Documents")).unwrap_or_else(std::env::temp_dir)
}
