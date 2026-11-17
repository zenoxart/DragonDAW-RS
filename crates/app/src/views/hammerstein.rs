//! The effect editor's "Hammerstein" tab: the effect's Hammerstein kernels with its current settings,
//! measured in the background on a fresh instance (stock effects and third-party plugins alike) and redrawn
//! whenever a setting changes.

use crate::theme;
use dragon_engine::effects;
use dragon_engine::hammerstein::{self, Kernels, MAX_ORDER};
use dragon_engine::model::Insert;
use egui::{Color32, Pos2, Rect, Stroke, Ui, Vec2};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

const RATE: f32 = 48_000.0;
/// Sweep levels to pick from (dBFS peak).
const LEVELS: [f32; 4] = [-18.0, -12.0, -6.0, 0.0];
/// Waits this long after the last change before measuring again.
const SETTLE: f64 = 0.35;

pub const ORDER_COLORS: [Color32; MAX_ORDER] =
    [Color32::from_rgb(0xe8, 0xec, 0xf2), Color32::from_rgb(0xff, 0x8a, 0x4c), Color32::from_rgb(0xf2, 0xc9, 0x4c), Color32::from_rgb(0x52, 0xd2, 0x7f), Color32::from_rgb(0x4c, 0xa8, 0xff)];

#[derive(Default)]
struct Job {
    /// Settings the shown result (or the running measurement) belongs to.
    sig: u64,
    /// Settings seen last frame, and since when.
    seen: u64,
    seen_at: f64,
    running: bool,
    failed: bool,
    result: Option<Kernels>,
}

type Shared = Arc<Mutex<Job>>;

/// The tab bar under the editor's header. Returns whether the Hammerstein tab is open.
pub fn tabs(ui: &mut Ui, key: egui::Id, accent: Color32) -> bool {
    let mut open: bool = ui.data(|d| d.get_temp(key)).unwrap_or(false);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for (k, name, tip) in [(false, "Controls", "The effect's controls"), (true, "Hammerstein", "The effect's nonlinearity and filtering with the current settings, measured with a sine sweep")] {
            let sel = open == k;
            let text = egui::RichText::new(name).size(12.0).color(if sel { Color32::WHITE } else { theme::TEXT_DIM });
            let b = egui::Button::new(text)
                .fill(if sel { accent.gamma_multiply(0.35) } else { Color32::TRANSPARENT })
                .stroke(Stroke::new(1.0_f32, if sel { accent } else { Color32::from_white_alpha(25) }))
                .corner_radius(5.0);
            if ui.add(b).on_hover_text(tip).clicked() {
                open = k;
            }
        }
    });
    ui.data_mut(|d| d.insert_temp(key, open));
    open
}

fn signature(ins: &Insert, level: f32, bpm: f64) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    ins.effect.hash(&mut h);
    for (k, v) in &ins.params {
        k.hash(&mut h);
        v.to_bits().hash(&mut h);
    }
    if let Some(p) = &ins.plugin {
        p.path.hash(&mut h);
        p.id.hash(&mut h);
        p.state.hash(&mut h);
    }
    level.to_bits().hash(&mut h);
    bpm.to_bits().hash(&mut h);
    h.finish()
}

fn start(job: Shared, ins: Insert, level: f32, bpm: f64, sig: u64) {
    std::thread::Builder::new()
        .name("hammerstein".into())
        .spawn(move || {
            let fx: Option<Box<dyn effects::Effect>> = match &ins.plugin {
                Some(p) => {
                    use dragon_engine::render::PluginFactory;
                    dragon_engine::plugins::Host.create_effect(p, RATE, 4096)
                }
                None => effects::create(&ins.effect, RATE, &ins.params),
            };
            let result = fx.and_then(|mut fx| {
                fx.set_tempo(bpm);
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| hammerstein::measure(fx.as_mut(), RATE, level))).ok()
            });
            let mut j = job.lock().unwrap_or_else(|e| e.into_inner());
            if j.sig == sig {
                j.running = false;
                j.failed = result.is_none();
                if result.is_some() {
                    j.result = result;
                }
            }
        })
        .ok();
}

/// The Hammerstein tab.
pub fn view(ui: &mut Ui, ins: &Insert, bpm: f64, key: egui::Id, accent: Color32) {
    let level_key = key.with("level");
    let mut level: f32 = ui.data(|d| d.get_temp(level_key)).unwrap_or(-6.0);
    let job: Shared = ui.data_mut(|d| d.get_temp_mut_or_default::<Shared>(key.with("job")).clone());
    let now = ui.input(|i| i.time);
    let sig = signature(ins, level, bpm);
    {
        let mut j = job.lock().unwrap_or_else(|e| e.into_inner());
        if j.seen != sig {
            j.seen = sig;
            j.seen_at = now;
        }
        if j.sig != sig && now - j.seen_at >= SETTLE && !j.running {
            j.sig = sig;
            j.running = true;
            j.failed = false;
            start(job.clone(), ins.clone(), level, bpm, sig);
        }
    }
    let (result, busy, failed) = {
        let j = job.lock().unwrap_or_else(|e| e.into_inner());
        (j.result.clone(), j.running || j.sig != sig, j.failed)
    };
    if busy {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(60));
    }

    // Controls: the drive level, the legend, the THD at 1 kHz.
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("DRIVE").size(10.0).strong().color(accent));
        for l in LEVELS {
            let sel = (level - l).abs() < 0.01;
            let b = egui::Button::new(egui::RichText::new(format!("{l:.0} dBFS")).size(11.0).color(if sel { Color32::WHITE } else { theme::TEXT_DIM }))
                .fill(if sel { accent.gamma_multiply(0.35) } else { Color32::TRANSPARENT })
                .stroke(Stroke::new(1.0_f32, if sel { accent } else { Color32::from_white_alpha(25) }));
            if ui.add(b).on_hover_text("How hard the measuring sweep drives the effect").clicked() {
                level = l;
            }
        }
        ui.add_space(12.0);
        if busy {
            ui.spinner();
            ui.label(egui::RichText::new("measuring…").size(11.0).color(theme::TEXT_DIM));
        } else if failed {
            ui.label(egui::RichText::new("could not measure this effect").size(11.0).color(theme::WARN));
        }
        if let Some(k) = &result {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new(format!("THD @ 1 kHz  {:.3} %", k.thd_at(1000.0))).monospace().color(theme::TEXT));
            });
        }
    });
    ui.data_mut(|d| d.insert_temp(level_key, level));

    let w = ui.available_width().max(300.0);
    let (r, resp) = ui.allocate_exact_size(Vec2::new(w, 280.0_f32.max(w * 0.32).min(420.0)), egui::Sense::hover());
    let p = ui.painter_at(r);
    p.rect_filled(r, 6.0, Color32::from_rgb(0x0f, 0x11, 0x15));
    let plot = Rect::from_min_max(Pos2::new(r.left() + 40.0, r.top() + 26.0), Pos2::new(r.right() - 10.0, r.bottom() - 22.0));
    let (top, bottom) = (12.0f32, -120.0f32);
    let x_of = |f: f32| plot.left() + plot.width() * ((f.max(20.0) / 20.0).log10() / 3.0).clamp(0.0, 1.0);
    let y_of = |db: f32| plot.top() + ((top - db) / (top - bottom)).clamp(0.0, 1.0) * plot.height();
    let small = egui::FontId::proportional(9.5);
    let grid = Stroke::new(1.0_f32, Color32::from_white_alpha(12));
    let mut db = 0.0;
    while db >= bottom {
        let y = y_of(db);
        p.line_segment([Pos2::new(plot.left(), y), Pos2::new(plot.right(), y)], if db == 0.0 { Stroke::new(1.0_f32, Color32::from_white_alpha(30)) } else { grid });
        p.text(Pos2::new(plot.left() - 5.0, y), egui::Align2::RIGHT_CENTER, format!("{db:.0}"), small.clone(), theme::TEXT_DIM);
        db -= 20.0;
    }
    for (f, l) in [(50.0, "50"), (100.0, "100"), (200.0, "200"), (500.0, "500"), (1000.0, "1k"), (2000.0, "2k"), (5000.0, "5k"), (10000.0, "10k")] {
        let x = x_of(f);
        p.line_segment([Pos2::new(x, plot.top()), Pos2::new(x, plot.bottom())], grid);
        p.text(Pos2::new(x, plot.bottom() + 3.0), egui::Align2::CENTER_TOP, l, small.clone(), theme::TEXT_DIM);
    }
    // Legend.
    let mut lx = plot.left();
    for (n, c) in ORDER_COLORS.iter().enumerate() {
        let name = match n {
            0 => "G1 linear".to_string(),
            1 => "G2 2nd order".to_string(),
            2 => "G3 3rd order".to_string(),
            k => format!("G{} {}th order", k + 1, k + 1),
        };
        p.line_segment([Pos2::new(lx, r.top() + 13.0), Pos2::new(lx + 14.0, r.top() + 13.0)], Stroke::new(2.5_f32, *c));
        let g = p.layout_no_wrap(name, small.clone(), theme::TEXT);
        let gw = g.size().x;
        p.galley(Pos2::new(lx + 18.0, r.top() + 13.0 - g.size().y / 2.0), g, theme::TEXT);
        lx += gw + 34.0;
    }
    match &result {
        Some(k) => {
            for (n, curve) in k.kernels.iter().enumerate().rev() {
                let pts: Vec<Pos2> = k.freqs.iter().zip(curve).map(|(f, v)| Pos2::new(x_of(*f), y_of(*v))).collect();
                let c = ORDER_COLORS[n];
                p.add(egui::Shape::line(pts, Stroke::new(if n == 0 { 2.2_f32 } else { 1.6 }, if busy { c.gamma_multiply(0.45) } else { c })));
            }
            // The memoryless curve the kernels amount to at 1 kHz, in the corner.
            let inset = Rect::from_min_size(Pos2::new(plot.right() - 118.0, plot.bottom() - 118.0), Vec2::splat(110.0));
            p.rect_filled(inset, 4.0, Color32::from_black_alpha(170));
            p.rect_stroke(inset, 4.0, Stroke::new(1.0_f32, Color32::from_white_alpha(30)), egui::StrokeKind::Inside);
            let span = k.curve.iter().map(|(_, y)| y.abs()).fold(1.0f32, f32::max);
            let map = |x: f32, y: f32| Pos2::new(inset.center().x + x * inset.width() * 0.42, inset.center().y - y / span * inset.height() * 0.42);
            p.line_segment([map(-1.0, -1.0), map(1.0, 1.0)], Stroke::new(1.0_f32, Color32::from_white_alpha(40)));
            let pts: Vec<Pos2> = k.curve.iter().map(|(x, y)| map(*x, *y)).collect();
            p.add(egui::Shape::line(pts, Stroke::new(1.8_f32, accent)));
            p.text(Pos2::new(inset.left() + 5.0, inset.top() + 4.0), egui::Align2::LEFT_TOP, "curve @ 1 kHz", egui::FontId::proportional(9.0), theme::TEXT_DIM);
            // Hover: the values at the pointer's frequency.
            if let Some(h) = resp.hover_pos().filter(|h| plot.contains(*h)) {
                let f = 20.0 * 1000f32.powf(((h.x - plot.left()) / plot.width()).clamp(0.0, 1.0));
                let i = k.freqs.iter().enumerate().min_by(|a, b| (a.1.ln() - f.ln()).abs().total_cmp(&(b.1.ln() - f.ln()).abs())).map(|x| x.0).unwrap_or(0);
                p.line_segment([Pos2::new(h.x, plot.top()), Pos2::new(h.x, plot.bottom())], Stroke::new(1.0_f32, Color32::from_white_alpha(60)));
                let mut lines = vec![format!("{:.0} Hz   THD {:.3} %", k.freqs[i], k.thd[i])];
                for (n, curve) in k.kernels.iter().enumerate() {
                    lines.push(format!("G{}  {:>7.1} dB", n + 1, curve[i]));
                }
                let text = lines.join("\n");
                let g = p.layout_no_wrap(text, egui::FontId::monospace(10.0), theme::TEXT);
                let at = if h.x + g.size().x + 20.0 > plot.right() { h.x - g.size().x - 14.0 } else { h.x + 10.0 };
                let bg = Rect::from_min_size(Pos2::new(at - 4.0, plot.top() + 4.0), g.size() + Vec2::splat(8.0));
                p.rect_filled(bg, 4.0, Color32::from_black_alpha(200));
                p.galley(Pos2::new(at, plot.top() + 8.0), g, theme::TEXT);
            }
        }
        None => {
            p.text(plot.center(), egui::Align2::CENTER_CENTER, if failed { "This effect could not be measured" } else { "Measuring the effect with a sine sweep…" }, egui::FontId::proportional(13.0), theme::TEXT_DIM);
        }
    }
    ui.label(
        egui::RichText::new("A Hammerstein model splits the effect into branches: the input raised to the n-th power, then a filter G_n. G1 is its frequency response, G2 / G3 … how much even / odd distortion it adds at each frequency. Measured on a fresh copy of the effect with an exponential sine sweep.")
            .size(10.5)
            .color(theme::TEXT_DIM),
    );
}
