//! The Transmuter editor. One window, laid out like a hardware synth's front panel:
//! a top bar (pages, the preset with browse / step / save, master), the sound sources as columns
//! (sub, three wavetable oscillators drawn in 3D, noise, the filters), the modulation deck
//! (macros, envelopes, LFOs, velocity / note, voicing) and a playable keyboard.
//! Modulation is drag and drop: drag the handle of an envelope, LFO, macro, velocity or note onto
//! any knob with a ring; right-click the knob to change or remove it.

use super::widgets::{self, knob_colored};
use crate::state::State;
use crate::theme;
use dragon_engine::device::Command;
use dragon_engine::effects::{self, ParamKind};
use dragon_engine::model::{Id, Instrument, Project};
use dragon_engine::render::LiveEvent;
use dragon_engine::wt::{self, *};
use dragon_engine::wt_library::{self, Category};
use egui::{Color32, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2};
use std::collections::BTreeSet;
use std::path::PathBuf;

const OSC_COL: [Color32; 3] = [Color32::from_rgb(0xff, 0x8a, 0x3c), Color32::from_rgb(0x4d, 0xc3, 0xff), Color32::from_rgb(0xb3, 0x88, 0xff)];
const MOD_COL: Color32 = Color32::from_rgb(0x5d, 0xe0, 0xa0);
const SUB_COL: Color32 = Color32::from_rgb(0xe0, 0xc2, 0x4a);
const NOISE_COL: Color32 = Color32::from_rgb(0xa0, 0xa8, 0xb8);
const FILTER_COL: Color32 = Color32::from_rgb(0xff, 0x5a, 0x6e);
const SCREEN: Color32 = Color32::from_rgb(0x0c, 0x0d, 0x12);
const BAR: Color32 = Color32::from_rgb(0x12, 0x14, 0x18);
/// The editor's width (fixed: laying out to the window's width would grow the window every frame).
const W: f32 = 1270.0;

fn set(p: &mut Project, cid: Id, f: impl FnOnce(&mut WavetableParams)) {
    if let Some(Instrument::Wavetable(w)) = p.channel_mut(cid).map(|c| &mut c.instrument) {
        f(w);
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
enum Page {
    #[default]
    Osc,
    Mix,
    Fx,
    Matrix,
    Global,
}

impl Page {
    const ALL: [Page; 5] = [Page::Osc, Page::Mix, Page::Fx, Page::Matrix, Page::Global];
    fn name(self) -> &'static str {
        match self {
            Page::Osc => "OSC",
            Page::Mix => "MIX",
            Page::Fx => "FX",
            Page::Matrix => "MATRIX",
            Page::Global => "GLOBAL",
        }
    }
}

// ------------------------------------------------------------------------------------------------ preset browser state

/// The preset browser (kept in the app state: favourites and user presets are loaded once).
#[derive(Default)]
pub struct Browser {
    pub open: bool,
    /// `None` = all; `Some("Favourites")`, `Some("User")` or a category name.
    pub filter: Option<String>,
    pub search: String,
    pub save_name: String,
    favs: Option<BTreeSet<String>>,
    user: Option<Vec<(String, WavetableParams)>>,
}

fn user_dir(st: &State) -> PathBuf {
    st.ctx.paths.config.join("Transmuter Presets")
}

fn favs(st: &mut State) -> &mut BTreeSet<String> {
    if st.ui.wave.favs.is_none() {
        let f: Vec<String> = st.ctx.paths.load_json("dragon-wave-favourites.json").unwrap_or_default();
        st.ui.wave.favs = Some(f.into_iter().collect());
    }
    st.ui.wave.favs.get_or_insert_with(Default::default)
}

fn toggle_fav(st: &mut State, name: &str) {
    let f = favs(st);
    if !f.remove(name) {
        f.insert(name.to_string());
    }
    let list: Vec<String> = f.iter().cloned().collect();
    st.ctx.paths.save_json("dragon-wave-favourites.json", &list);
}

fn user_presets(st: &mut State) -> &Vec<(String, WavetableParams)> {
    if st.ui.wave.user.is_none() {
        let dir = user_dir(st);
        let mut v: Vec<(String, WavetableParams)> = std::fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
            .filter_map(|e| {
                let p: WavetableParams = st.ctx.paths.load_from(&e.path())?;
                Some((e.path().file_stem()?.to_string_lossy().to_string(), p))
            })
            .collect();
        v.sort_by_key(|a| a.0.to_lowercase());
        st.ui.wave.user = Some(v);
    }
    st.ui.wave.user.get_or_insert_with(Default::default)
}

fn save_user_preset(st: &mut State, name: &str, p: &WavetableParams) {
    let dir = user_dir(st);
    let _ = std::fs::create_dir_all(&dir);
    let clean: String = name.chars().map(|c| if c.is_alphanumeric() || " -_()".contains(c) { c } else { '_' }).collect();
    let clean = clean.trim().to_string();
    if clean.is_empty() {
        return;
    }
    let mut p = p.clone();
    p.preset = clean.clone();
    st.ctx.paths.save_to(&dir.join(format!("{clean}.json")), &p);
    st.ui.wave.user = None;
    st.toast(format!("Saved preset \"{clean}\""), redhive_contracts::ui::Severity::Success);
}

/// One entry of the browser list.
#[derive(Clone)]
struct Entry {
    name: String,
    tag: &'static str,
    kind: String,
    params: Option<usize>,
    user: Option<usize>,
}

/// The presets matching the browser's filter and search, in order.
fn entries(st: &mut State) -> Vec<Entry> {
    let filter = st.ui.wave.filter.clone();
    let q = st.ui.wave.search.to_lowercase();
    let words: Vec<&str> = q.split_whitespace().collect();
    let matches = |name: &str, kind: &str, cat: &str| {
        let hay = format!("{} {} {}", name.to_lowercase(), kind.to_lowercase(), cat.to_lowercase());
        words.iter().all(|w| hay.contains(w))
    };
    let fav = favs(st).clone();
    let mut out = Vec::new();
    if matches!(filter.as_deref(), None | Some("User") | Some("Favourites")) {
        for (i, (name, _)) in user_presets(st).iter().enumerate() {
            let ok = match filter.as_deref() {
                Some("Favourites") => fav.contains(name),
                _ => true,
            };
            if ok && matches(name, "user", "user") {
                out.push(Entry { name: name.clone(), tag: "USR", kind: "User".into(), params: None, user: Some(i) });
            }
        }
    }
    if filter.as_deref() != Some("User") {
        for (i, p) in wt_library::library().iter().enumerate() {
            let ok = match filter.as_deref() {
                None => true,
                Some("Favourites") => fav.contains(&p.name),
                Some(c) => p.category.name() == c,
            };
            if ok && matches(&p.name, p.kind, p.category.name()) {
                out.push(Entry { name: p.name.clone(), tag: p.category.code(), kind: p.kind.to_string(), params: Some(i), user: None });
            }
        }
    }
    out
}

fn load_entry(st: &mut State, cid: Id, e: &Entry) {
    let params = if let Some(i) = e.params {
        wt_library::library()[i].params.clone()
    } else if let Some(i) = e.user {
        let Some((_, p)) = user_presets(st).get(i).cloned() else { return };
        p
    } else {
        return;
    };
    let name = e.name.clone();
    st.session.edit(&format!("Preset: {name}"), |pr| {
        set(pr, cid, |w| {
            *w = params;
            w.preset = name;
        })
    });
}

/// Steps through the browser's list (`dir` = ±1) or jumps to a random entry (`dir` = 0).
fn step_preset(st: &mut State, cid: Id, cur: &str, dir: i32) {
    let list = entries(st);
    if list.is_empty() {
        return;
    }
    let i = list.iter().position(|e| e.name == cur);
    let n = list.len() as i64;
    let next = if dir == 0 {
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(7);
        let mut r = dragon_engine::dsp::Rng::new(seed);
        (r.next_u64() % n as u64) as usize
    } else {
        match i {
            Some(i) => (i as i64 + dir as i64).rem_euclid(n) as usize,
            None => 0,
        }
    };
    let e = list[next].clone();
    load_entry(st, cid, &e);
}

// ------------------------------------------------------------------------------------------------ small widgets

/// A knob for one value of the synth. With a destination it shows a ring for its modulation,
/// takes modulation sources dropped on it and lists them on right-click.
#[allow(clippy::too_many_arguments)]
fn km(
    ui: &mut Ui,
    st: &mut State,
    cid: Id,
    p: &WavetableParams,
    dest: Option<ModDest>,
    v: f32,
    min: f32,
    max: f32,
    default: f32,
    label: &str,
    fmt: impl Fn(f32) -> String,
    color: Color32,
    write: impl FnOnce(&mut WavetableParams, f32),
) {
    let mut x = v;
    let r = knob_colored(ui, &mut x, min, max, default, label, fmt, color);
    if r.hovered() || r.dragged() {
        st.hint("Transmuter", label, format!("{x:.2}"));
    }
    widgets::apply(st, &r, &format!("Transmuter: {label}"), |pr| set(pr, cid, |w| write(w, x)));
    if let Some(dest) = dest {
        mod_target(ui, st, cid, p, &r, dest, Pos2::new(r.rect.center().x, r.rect.top() + 19.0), 18.5, ((v - min) / (max - min)).clamp(0.0, 1.0));
    }
}

/// Modulation ring, drop target and right-click menu for `dest` on the control `r`.
#[allow(clippy::too_many_arguments)]
fn mod_target(ui: &mut Ui, st: &mut State, cid: Id, p: &WavetableParams, r: &egui::Response, dest: ModDest, c: Pos2, rad: f32, n: f32) {
    let slots: Vec<(usize, ModSlot)> = p.matrix.iter().cloned().enumerate().filter(|(_, m)| m.dest == dest).collect();
    let amt: f32 = slots.iter().filter(|(_, m)| m.on).map(|(_, m)| m.amount).sum();
    let pa = ui.painter();
    if !slots.is_empty() {
        let a0 = std::f32::consts::PI * 0.75;
        let sweep = std::f32::consts::PI * 1.5;
        let (from, to) = (n, (n + amt).clamp(0.0, 1.0));
        let (lo, hi) = if from < to { (from, to) } else { (to, from) };
        let pts: Vec<Pos2> = (0..=24).map(|i| a0 + sweep * (lo + (hi - lo) * i as f32 / 24.0)).map(|a| Pos2::new(c.x + rad * a.cos(), c.y + rad * a.sin())).collect();
        pa.add(egui::Shape::line(pts, Stroke::new(2.0_f32, MOD_COL)));
        pa.circle_filled(Pos2::new(c.x + rad * 0.72, c.y - rad * 0.72), 2.5, MOD_COL);
    }
    let pointer_over = ui.ctx().pointer_latest_pos().is_some_and(|q| r.rect.contains(q));
    if egui::DragAndDrop::has_payload_of_type::<ModSource>(ui.ctx()) && pointer_over {
        pa.circle_stroke(c, rad + 2.0, Stroke::new(2.0_f32, MOD_COL));
    }
    if let Some(src) = widgets::dnd_release::<ModSource>(r) {
        let src = *src;
        st.session.edit(&format!("Modulate {} with {}", dest.name(), src.name()), |pr| {
            set(pr, cid, |w| {
                if !w.matrix.iter().any(|m| m.source == src && m.dest == dest) {
                    w.matrix.push(ModSlot { source: src, dest, amount: 0.3, on: true });
                }
            })
        });
    }
    if !slots.is_empty() {
        r.context_menu(|ui| {
            ui.label(RichText::new(format!("Modulating {}", dest.name())).small().strong().color(MOD_COL));
            for (i, m) in &slots {
                let i = *i;
                ui.horizontal(|ui| {
                    ui.label(m.source.name());
                    let mut a = m.amount;
                    let rr = ui.add(egui::Slider::new(&mut a, -1.0..=1.0).custom_formatter(|v, _| format!("{:+.0}%", v * 100.0)));
                    widgets::apply(st, &rr, "Modulation amount", |pr| {
                        set(pr, cid, |w| {
                            if let Some(s) = w.matrix.get_mut(i) {
                                s.amount = a
                            }
                        })
                    });
                    if ui.small_button("×").on_hover_text("Remove").clicked() {
                        st.session.edit("Remove modulation", |pr| {
                            set(pr, cid, |w| {
                                if i < w.matrix.len() {
                                    w.matrix.remove(i);
                                }
                            })
                        });
                        ui.close_menu();
                    }
                });
            }
        });
    }
}

/// The handle a modulation source is dragged from.
fn mod_handle(ui: &mut Ui, src: ModSource) -> egui::Response {
    let (rect, r) = ui.allocate_exact_size(Vec2::new(16.0, 16.0), Sense::drag());
    let c = rect.center();
    let col = if r.hovered() || r.dragged() { Color32::WHITE } else { MOD_COL };
    let pa = ui.painter();
    pa.circle_stroke(c, 6.0, Stroke::new(1.4_f32, col));
    pa.line_segment([c - Vec2::new(3.5, 0.0), c + Vec2::new(3.5, 0.0)], Stroke::new(1.4_f32, col));
    pa.line_segment([c - Vec2::new(0.0, 3.5), c + Vec2::new(0.0, 3.5)], Stroke::new(1.4_f32, col));
    r.dnd_set_drag_payload(src);
    if r.dragged() {
        if let Some(q) = ui.ctx().pointer_latest_pos() {
            let pa = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("wave.dragmod")));
            pa.circle_filled(q, 7.0, MOD_COL.gamma_multiply(0.7));
            pa.text(q + Vec2::new(10.0, -8.0), egui::Align2::LEFT_TOP, src.name(), egui::FontId::proportional(11.0), MOD_COL);
        }
    }
    r.on_hover_text(format!("Drag onto a knob to modulate it with {}", src.name()))
}

/// A section: a header bar (power switch, title, extras on the right) over a dark body.
#[allow(clippy::too_many_arguments)]
fn module(ui: &mut Ui, st: &mut State, title: &str, color: Color32, power: Option<bool>, width: f32, height: f32, header: impl FnOnce(&mut Ui, &mut State), body: impl FnOnce(&mut Ui, &mut State)) -> bool {
    let mut toggled = false;
    egui::Frame::NONE.fill(theme::PANEL).stroke(Stroke::new(1.0_f32, Color32::from_rgb(0x30, 0x35, 0x3d))).corner_radius(4.0).show(ui, |ui| {
        ui.set_width(width);
        ui.set_min_height(height);
        ui.vertical(|ui| {
            egui::Frame::NONE.fill(BAR).inner_margin(egui::Margin::symmetric(6, 3)).show(ui, |ui| {
                ui.set_width(width - 12.0);
                ui.horizontal(|ui| {
                    if let Some(on) = power {
                        let (rect, r) = ui.allocate_exact_size(Vec2::new(12.0, 12.0), Sense::click());
                        ui.painter().rect_filled(rect, 2.0, if on { color } else { theme::GRID });
                        if r.on_hover_text(if on { "On – click to switch off" } else { "Off – click to switch on" }).clicked() {
                            toggled = true;
                        }
                    }
                    ui.label(RichText::new(title).strong().size(12.0).color(if power != Some(false) { theme::TEXT } else { theme::TEXT_DIM }));
                    header(ui, st);
                });
            });
            egui::Frame::NONE.inner_margin(egui::Margin::symmetric(6, 4)).show(ui, |ui| {
                ui.set_width(width - 12.0);
                ui.vertical(|ui| body(ui, st));
            });
        });
    });
    toggled
}

/// A small −/value/+ stepper for whole numbers (octave, semitones).
#[allow(clippy::too_many_arguments)]
fn stepper(ui: &mut Ui, st: &mut State, cid: Id, label: &str, v: i32, min: i32, max: i32, write: impl FnOnce(&mut WavetableParams, i32)) {
    ui.label(RichText::new(label).small().strong().color(theme::TEXT_DIM));
    let mut x = v;
    let r = ui.add(egui::DragValue::new(&mut x).range(min..=max).speed(0.08).custom_formatter(|v, _| format!("{:+}", v as i32)));
    widgets::apply(st, &r, &format!("Transmuter: {label}"), |pr| set(pr, cid, |w| write(w, x)));
}

fn arrow_button(ui: &mut Ui, left: bool) -> egui::Response {
    let (rect, r) = ui.allocate_exact_size(Vec2::new(18.0, 18.0), Sense::click());
    let col = if r.hovered() { Color32::WHITE } else { theme::TEXT_DIM };
    let c = rect.center();
    let s = if left { -1.0 } else { 1.0 };
    ui.painter().add(egui::Shape::convex_polygon(vec![c + Vec2::new(4.0 * s, 0.0), c + Vec2::new(-3.0 * s, -5.0), c + Vec2::new(-3.0 * s, 5.0)], col, Stroke::NONE));
    r
}

fn star(ui: &mut Ui, on: bool) -> egui::Response {
    let (rect, r) = ui.allocate_exact_size(Vec2::new(16.0, 16.0), Sense::click());
    let c = rect.center();
    let pts: Vec<Pos2> = (0..10)
        .map(|i| {
            let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.0;
            let rad = if i % 2 == 0 { 6.5 } else { 2.8 };
            c + Vec2::new(a.cos(), a.sin()) * rad
        })
        .collect();
    let col = if on {
        theme::WARN
    } else if r.hovered() {
        theme::TEXT
    } else {
        theme::GRID_BAR
    };
    ui.painter().add(egui::Shape::closed_line(pts, Stroke::new(1.3_f32, col)));
    if on {
        ui.painter().circle_filled(c, 2.5, col);
    }
    r
}

// ------------------------------------------------------------------------------------------------ the editor

pub fn editor(ui: &mut Ui, st: &mut State, cid: Id, p: WavetableParams) {
    let page_id = egui::Id::new(("wave.page", cid));
    let mut page: Page = ui.data(|d| d.get_temp::<u8>(page_id)).map(|t| Page::ALL[t as usize % 5]).unwrap_or_default();
    top_bar(ui, st, cid, &p, &mut page);
    ui.data_mut(|d| d.insert_temp(page_id, Page::ALL.iter().position(|x| *x == page).unwrap_or(0) as u8));
    ui.add_space(4.0);
    if st.ui.wave.open {
        browser(ui, st, cid, &p);
    } else {
        match page {
            Page::Osc => {
                sources(ui, st, cid, &p);
                ui.add_space(4.0);
                mod_deck(ui, st, cid, &p);
            }
            Page::Mix => mix_page(ui, st, cid, &p),
            Page::Fx => fx_tab(ui, st, cid, &p),
            Page::Matrix => matrix(ui, st, cid, &p),
            Page::Global => global_page(ui, st, cid, &p),
        }
    }
    ui.add_space(4.0);
    keyboard(ui, st, cid, &p);
}

fn top_bar(ui: &mut Ui, st: &mut State, cid: Id, p: &WavetableParams, page: &mut Page) {
    egui::Frame::NONE.fill(BAR).corner_radius(4.0).inner_margin(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
        ui.set_width(W - 16.0);
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new("TRANSMUTER").size(17.0).strong().color(theme::ACCENT));
                ui.label(RichText::new("wavetable synth").size(9.0).color(theme::TEXT_DIM));
            });
            ui.add_space(8.0);
            for pg in Page::ALL {
                let on = *page == pg && !st.ui.wave.open;
                let b = egui::Button::new(RichText::new(pg.name()).strong().size(13.0).color(if on { theme::TEXT } else { theme::TEXT_DIM })).fill(if on { theme::PANEL_2 } else { BAR }).min_size(Vec2::new(70.0, 34.0));
                let r = ui.add(b);
                if on {
                    ui.painter().line_segment([r.rect.left_top() + Vec2::new(8.0, 1.0), r.rect.right_top() + Vec2::new(-8.0, 1.0)], Stroke::new(2.0_f32, theme::ACCENT));
                }
                if r.clicked() {
                    *page = pg;
                    st.ui.wave.open = false;
                }
            }
            ui.add_space(10.0);
            // The preset display.
            let tag = wt_library::find(&p.preset).map(|l| l.category.code()).unwrap_or("USR");
            egui::Frame::NONE.fill(SCREEN).corner_radius(4.0).inner_margin(egui::Margin::symmetric(8, 4)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.set_width(330.0);
                    let fav = favs(st).contains(&p.preset);
                    if star(ui, fav).on_hover_text("Favourite").clicked() {
                        toggle_fav(st, &p.preset);
                    }
                    ui.label(RichText::new(tag).small().strong().color(theme::ACCENT_2));
                    let r = ui.add(egui::Label::new(RichText::new(&p.preset).size(14.0).color(theme::TEXT)).truncate().sense(Sense::click()));
                    if r.on_hover_text("Click to browse 2,000+ presets").clicked() {
                        st.ui.wave.open = !st.ui.wave.open;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if arrow_button(ui, false).on_hover_text("Next preset").clicked() {
                            step_preset(st, cid, &p.preset, 1);
                        }
                        if arrow_button(ui, true).on_hover_text("Previous preset").clicked() {
                            step_preset(st, cid, &p.preset, -1);
                        }
                    });
                });
            });
            if ui.add(egui::Button::new(RichText::new("Browse").strong()).selected(st.ui.wave.open)).on_hover_text("Find presets by category, type or name").clicked() {
                st.ui.wave.open = !st.ui.wave.open;
            }
            if ui.button("Random").on_hover_text("A random preset from the browser's current list").clicked() {
                step_preset(st, cid, &p.preset, 0);
            }
            ui.menu_button("Save", |ui| {
                ui.label(RichText::new("Save as a user preset").small().color(theme::TEXT_DIM));
                if st.ui.wave.save_name.is_empty() {
                    st.ui.wave.save_name = p.preset.clone();
                }
                let mut name = st.ui.wave.save_name.clone();
                let r = ui.add(egui::TextEdit::singleline(&mut name).desired_width(180.0));
                st.ui.wave.save_name = name.clone();
                if ui.button("Save").clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                    save_user_preset(st, &name, p);
                    st.ui.wave.save_name.clear();
                    let n = name.clone();
                    st.session.edit("Name preset", |pr| set(pr, cid, |w| w.preset = n));
                    ui.close_menu();
                }
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                km(ui, st, cid, p, Some(ModDest::Master), p.master, 0.0, 1.5, 0.7, "MAIN", widgets::db_fmt, theme::ACCENT, |w, v| w.master = v);
            });
        });
    });
}

// ------------------------------------------------------------------------------------------------ sources

fn sources(ui: &mut Ui, st: &mut State, cid: Id, p: &WavetableParams) {
    let total = W;
    let gap = 4.0;
    let side = 112.0;
    let filt_w = 246.0;
    let osc_w = (total - side * 2.0 - filt_w - gap * 5.0 - 6.0 * 2.0) / 3.0;
    let h = 262.0;
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        sub_module(ui, st, cid, p, side, h);
        for o in 0..3 {
            osc_module(ui, st, cid, p, o, osc_w, h);
        }
        noise_module(ui, st, cid, p, side, h);
        filter_module(ui, st, cid, p, filt_w, h);
    });
}

fn sub_module(ui: &mut Ui, st: &mut State, cid: Id, p: &WavetableParams, w: f32, h: f32) {
    let sub = p.sub.clone();
    let t = module(
        ui,
        st,
        "SUB",
        SUB_COL,
        Some(sub.on),
        w,
        h,
        |_, _| {},
        |ui, st| {
            ui.horizontal(|ui| stepper(ui, st, cid, "OCT", sub.octave, -3, 1, |w, v| w.sub.octave = v));
            ui.add_space(4.0);
            // Shape buttons, each drawing its wave.
            egui::Grid::new(("wave.subshape", cid)).spacing([4.0, 4.0]).show(ui, |ui| {
                for (i, shape) in [SubShape::Sine, SubShape::Triangle, SubShape::Saw, SubShape::Square].into_iter().enumerate() {
                    let (rect, r) = ui.allocate_exact_size(Vec2::new(44.0, 30.0), Sense::click());
                    let on = sub.shape == shape;
                    let pa = ui.painter();
                    pa.rect_filled(rect, 3.0, if on { SUB_COL.gamma_multiply(0.25) } else { SCREEN });
                    let pts: Vec<Pos2> = (0..=32)
                        .map(|k| {
                            let ph = k as f32 / 32.0;
                            let v = match shape {
                                SubShape::Sine => (ph * std::f32::consts::TAU).sin(),
                                SubShape::Triangle => LfoShape::Triangle.at(ph),
                                SubShape::Saw => 1.0 - 2.0 * ph,
                                SubShape::Square => LfoShape::Square.at(ph),
                            };
                            Pos2::new(rect.left() + 6.0 + ph * (rect.width() - 12.0), rect.center().y - v * 9.0)
                        })
                        .collect();
                    pa.add(egui::Shape::line(pts, Stroke::new(1.5_f32, if on { SUB_COL } else { theme::TEXT_DIM })));
                    if r.on_hover_text(format!("{shape:?}")).clicked() {
                        st.session.edit("Sub shape", |pr| set(pr, cid, |w| w.sub.shape = shape));
                    }
                    if i % 2 == 1 {
                        ui.end_row();
                    }
                }
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                km(ui, st, cid, p, None, sub.pan, -1.0, 1.0, 0.0, "PAN", widgets::pan_fmt, SUB_COL, |w, v| w.sub.pan = v);
                km(ui, st, cid, p, Some(ModDest::SubLevel), sub.level, 0.0, 1.0, 0.6, "LEVEL", widgets::pct, SUB_COL, |w, v| w.sub.level = v);
            });
            route_combo(ui, st, cid, ("sub", 0), sub.route, |w, r| w.sub.route = r);
        },
    );
    if t {
        st.session.edit("Sub on / off", |pr| set(pr, cid, |w| w.sub.on = !sub.on));
    }
}

fn noise_module(ui: &mut Ui, st: &mut State, cid: Id, p: &WavetableParams, w: f32, h: f32) {
    let noise = p.noise.clone();
    let t = module(
        ui,
        st,
        "NOISE",
        NOISE_COL,
        Some(noise.on),
        w,
        h,
        |_, _| {},
        |ui, st| {
            let name = if noise.color < 0.4 {
                "Dark"
            } else if noise.color > 0.6 {
                "Bright"
            } else {
                "White"
            };
            ui.label(RichText::new(name).color(theme::TEXT));
            // A picture of the noise: steadier when dark, spikier when bright.
            let (rect, _) = ui.allocate_exact_size(Vec2::new(w - 14.0, 70.0), Sense::hover());
            let pa = ui.painter_at(rect);
            pa.rect_filled(rect, 3.0, SCREEN);
            let mut r = dragon_engine::dsp::Rng::new(5);
            let mut y = 0.0f32;
            let smooth = 1.0 - noise.color * 0.95;
            let pts: Vec<Pos2> = (0..80)
                .map(|i| {
                    y = y * smooth + r.bipolar() * (1.0 - smooth);
                    let env = (1.0 - (i as f32 / 40.0 - 1.0).abs()).powf(0.5);
                    Pos2::new(rect.left() + i as f32 / 79.0 * rect.width(), rect.center().y - y * env * rect.height() * 0.9)
                })
                .collect();
            pa.add(egui::Shape::line(pts, Stroke::new(1.0_f32, if noise.on { NOISE_COL } else { theme::GRID_BAR })));
            ui.horizontal(|ui| {
                km(
                    ui,
                    st,
                    cid,
                    p,
                    None,
                    noise.color,
                    0.0,
                    1.0,
                    0.5,
                    "COLOUR",
                    |v| {
                        if v < 0.4 {
                            "dark".into()
                        } else if v > 0.6 {
                            "bright".into()
                        } else {
                            "white".into()
                        }
                    },
                    NOISE_COL,
                    |w, v| w.noise.color = v,
                );
                km(ui, st, cid, p, Some(ModDest::NoiseLevel), noise.level, 0.0, 1.0, 0.3, "LEVEL", widgets::pct, NOISE_COL, |w, v| w.noise.level = v);
            });
            ui.horizontal(|ui| {
                km(ui, st, cid, p, None, noise.pan, -1.0, 1.0, 0.0, "PAN", widgets::pan_fmt, NOISE_COL, |w, v| w.noise.pan = v);
            });
            route_combo(ui, st, cid, ("noise", 0), noise.route, |w, r| w.noise.route = r);
        },
    );
    if t {
        st.session.edit("Noise on / off", |pr| set(pr, cid, |w| w.noise.on = !noise.on));
    }
}

/// The wavetable drawn as stacked frames in perspective, the current position lit.
fn table_view(ui: &mut Ui, src: &TableSource, pos: f32, color: Color32, width: f32, height: f32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click_and_drag());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 4.0, SCREEN);
    let t = wt::table(src);
    let n = t.len();
    let shown = n.min(24);
    let depth = Vec2::new(rect.width() * 0.22, -rect.height() * 0.42);
    let w = rect.width() * 0.72;
    let h = rect.height() * 0.2;
    let base = Pos2::new(rect.left() + 8.0, rect.bottom() - h - 8.0);
    let cur = (pos * (n - 1) as f32).round() as usize;
    let frame_line = |k: usize, z: f32| -> Vec<Pos2> {
        let f = &t.frames[k];
        let off = base + depth * z;
        (0..=64).map(|i| Pos2::new(off.x + i as f32 / 64.0 * w, off.y - f[(i * (f.len() - 1)) / 64] * h)).collect()
    };
    for s in (0..shown).rev() {
        let k = if shown == 1 { 0 } else { s * (n - 1) / (shown - 1) };
        let z = k as f32 / (n - 1).max(1) as f32;
        p.add(egui::Shape::line(frame_line(k, z), Stroke::new(1.0_f32, color.gamma_multiply(0.15 + 0.25 * (1.0 - z)))));
    }
    let z = cur as f32 / (n - 1).max(1) as f32;
    // The lit frame, with a soft glow under it.
    p.add(egui::Shape::line(frame_line(cur, z), Stroke::new(4.0_f32, color.gamma_multiply(0.25))));
    p.add(egui::Shape::line(frame_line(cur, z), Stroke::new(1.8_f32, color)));
    p.text(rect.right_bottom() + Vec2::new(-6.0, -4.0), egui::Align2::RIGHT_BOTTOM, format!("{}/{}", cur + 1, n), egui::FontId::monospace(10.0), theme::TEXT_DIM);
    resp
}

fn osc_module(ui: &mut Ui, st: &mut State, cid: Id, p: &WavetableParams, o: usize, w: f32, h: f32) {
    let osc = p.osc[o].clone();
    let col = OSC_COL[o];
    let dim = if osc.on { col } else { theme::TEXT_DIM };
    let t = module(
        ui,
        st,
        ["OSC A", "OSC B", "OSC C"][o],
        col,
        Some(osc.on),
        w,
        h,
        |ui, st| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("Import").on_hover_text("Any audio file: wavetables (cycles of 2048 samples) or a sound cut into cycles – or drop one on the table").clicked() {
                    if let Some(f) = rfd::FileDialog::new().add_filter("Audio", &["wav", "flac", "aif", "aiff", "mp3", "ogg"]).pick_file() {
                        st.session.edit("Import wavetable", |pr| set(pr, cid, |w| w.osc[o].table = TableSource::File(f)));
                    }
                }
                let idx = wt::FACTORY.iter().position(|n| TableSource::Factory((*n).into()) == osc.table);
                if arrow_button(ui, false).on_hover_text("Next wavetable").clicked() {
                    let n = wt::FACTORY[idx.map(|i| (i + 1) % wt::FACTORY.len()).unwrap_or(0)];
                    st.session.edit("Wavetable", |pr| set(pr, cid, |w| w.osc[o].table = TableSource::Factory(n.into())));
                }
                if arrow_button(ui, true).on_hover_text("Previous wavetable").clicked() {
                    let n = wt::FACTORY[idx.map(|i| (i + wt::FACTORY.len() - 1) % wt::FACTORY.len()).unwrap_or(0)];
                    st.session.edit("Wavetable", |pr| set(pr, cid, |w| w.osc[o].table = TableSource::Factory(n.into())));
                }
                egui::ComboBox::from_id_salt(("wave.table", cid, o)).selected_text(RichText::new(osc.table.name()).small()).width(ui.available_width() - 4.0).show_ui(ui, |ui| {
                    for name in wt::FACTORY {
                        let src = TableSource::Factory(name.into());
                        if ui.selectable_label(osc.table == src, name).clicked() {
                            st.session.edit("Wavetable", |pr| set(pr, cid, |w| w.osc[o].table = src));
                        }
                    }
                });
            });
        },
        |ui, st| {
            ui.horizontal(|ui| {
                stepper(ui, st, cid, "OCT", osc.octave, -4, 4, move |w, v| w.osc[o].octave = v);
                stepper(ui, st, cid, "SEM", osc.semi, -12, 12, move |w, v| w.osc[o].semi = v);
                ui.label(RichText::new("FIN").small().strong().color(theme::TEXT_DIM));
                let mut f = osc.fine;
                let r = ui.add(egui::DragValue::new(&mut f).range(-100.0..=100.0).speed(0.5).suffix(" ct").max_decimals(0));
                widgets::apply(st, &r, "Transmuter: fine", |pr| set(pr, cid, |w| w.osc[o].fine = f));
            });
            // The table: drag up / down to scan, drop audio to import, drop a mod source to modulate position.
            let r = table_view(ui, &osc.table, osc.pos, dim, w - 14.0, 96.0);
            if r.dragged() {
                let pos = (osc.pos - r.drag_delta().y / 150.0).clamp(0.0, 1.0);
                st.session.gesture("Wavetable position", |pr| set(pr, cid, |w| w.osc[o].pos = pos));
            }
            if r.drag_stopped() {
                st.session.end_gesture();
            }
            if let Some(path) = widgets::dnd_release::<std::path::PathBuf>(&r) {
                let path = (*path).clone();
                st.session.edit("Import wavetable", |pr| set(pr, cid, |w| w.osc[o].table = TableSource::File(path)));
            }
            let r = r.on_hover_text("Drag up / down to move through the table; drop an audio file to use it as a wavetable");
            if egui::DragAndDrop::has_payload_of_type::<ModSource>(ui.ctx()) {
                mod_target(ui, st, cid, p, &r, ModDest::OscPos(o as u8), r.rect.center(), 0.0, osc.pos);
            }
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                km(ui, st, cid, p, Some(ModDest::OscPos(o as u8)), osc.pos, 0.0, 1.0, 0.0, "WT POS", widgets::pct, dim, move |w, v| w.osc[o].pos = v);
                km(ui, st, cid, p, None, osc.unison as f32, 1.0, 16.0, 1.0, "UNISON", |v| format!("{:.0} voices", v.round()), dim, move |w, v| w.osc[o].unison = v.round() as u32);
                km(ui, st, cid, p, Some(ModDest::OscDetune(o as u8)), osc.detune, 0.0, 1.0, 0.25, "DETUNE", widgets::pct, dim, move |w, v| w.osc[o].detune = v);
                km(ui, st, cid, p, None, osc.blend, 0.0, 1.0, 0.75, "BLEND", widgets::pct, dim, move |w, v| w.osc[o].blend = v);
                km(ui, st, cid, p, Some(ModDest::OscPan(o as u8)), osc.pan, -1.0, 1.0, 0.0, "PAN", widgets::pan_fmt, dim, move |w, v| w.osc[o].pan = v);
            });
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                ui.vertical(|ui| {
                    let mut warp = osc.warp;
                    egui::ComboBox::from_id_salt(("wave.warp", cid, o)).selected_text(RichText::new(warp.name()).small()).width(58.0).show_ui(ui, |ui| {
                        for x in Warp::ALL {
                            ui.selectable_value(&mut warp, x, x.name());
                        }
                    });
                    if warp != osc.warp {
                        st.session.edit("Warp", |pr| set(pr, cid, |w| w.osc[o].warp = warp));
                    }
                    route_combo(ui, st, cid, ("osc", o), osc.route, move |w, r| w.osc[o].route = r);
                });
                km(ui, st, cid, p, Some(ModDest::OscWarp(o as u8)), osc.warp_amount, 0.0, 1.0, 0.0, "WARP", widgets::pct, dim, move |w, v| w.osc[o].warp_amount = v);
                km(ui, st, cid, p, None, osc.width, 0.0, 1.0, 1.0, "WIDTH", widgets::pct, dim, move |w, v| w.osc[o].width = v);
                km(ui, st, cid, p, Some(ModDest::OscLevel(o as u8)), osc.level, 0.0, 1.0, 0.75, "LEVEL", widgets::pct, dim, move |w, v| w.osc[o].level = v);
            });
        },
    );
    if t {
        st.session.edit("Oscillator on / off", |pr| set(pr, cid, |w| w.osc[o].on = !osc.on));
    }
}

fn route_combo(ui: &mut Ui, st: &mut State, cid: Id, salt: (&str, usize), cur: Route, write: impl FnOnce(&mut WavetableParams, Route)) {
    let mut r = cur;
    egui::ComboBox::from_id_salt(("wave.route", cid, salt)).selected_text(RichText::new(format!("to {}", r.name())).small()).width(58.0).show_ui(ui, |ui| {
        for x in Route::ALL {
            ui.selectable_value(&mut r, x, x.name());
        }
    });
    if r != cur {
        st.session.edit("Routing", |pr| set(pr, cid, |w| write(w, r)));
    }
}

fn filter_module(ui: &mut Ui, st: &mut State, cid: Id, p: &WavetableParams, w: f32, h: f32) {
    let sel_id = egui::Id::new(("wave.fsel", cid));
    let mut k: usize = ui.data(|d| d.get_temp(sel_id)).unwrap_or(0);
    let f = p.filters[k].clone();
    let mut power = false;
    let mut switch_to = None;
    module(
        ui,
        st,
        "",
        FILTER_COL,
        None,
        w,
        h,
        |ui, _| {
            for (j, fj) in p.filters.iter().enumerate() {
                let (rect, r) = ui.allocate_exact_size(Vec2::new(12.0, 12.0), Sense::click());
                ui.painter().rect_filled(rect, 2.0, if fj.on { FILTER_COL } else { theme::GRID });
                if r.clicked() {
                    switch_to = Some(j);
                    power = true;
                }
                let b = egui::Button::new(RichText::new(format!("FILTER {}", j + 1)).strong().size(12.0).color(if j == k { theme::TEXT } else { theme::TEXT_DIM })).fill(if j == k { theme::PANEL_2 } else { BAR });
                if ui.add(b).clicked() {
                    switch_to = Some(j);
                }
            }
        },
        |ui, st| {
            let mut kind = f.kind;
            egui::ComboBox::from_id_salt(("wave.fkind", cid, k)).selected_text(kind.name()).width(w - 20.0).show_ui(ui, |ui| {
                for x in FilterKind::ALL {
                    ui.selectable_value(&mut kind, x, x.name());
                }
            });
            if kind != f.kind {
                st.session.edit("Filter type", |pr| set(pr, cid, |w| w.filters[k].kind = kind));
            }
            filter_curve(ui, st, cid, &f, k, w - 14.0, 84.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                let c = if f.on { FILTER_COL } else { theme::TEXT_DIM };
                km(
                    ui,
                    st,
                    cid,
                    p,
                    Some(ModDest::FilterCutoff(k as u8)),
                    f.cutoff,
                    20.0,
                    20000.0,
                    2000.0,
                    "CUTOFF",
                    |v| if v >= 1000.0 { format!("{:.1} kHz", v / 1000.0) } else { format!("{v:.0} Hz") },
                    c,
                    move |w, v| w.filters[k].cutoff = v,
                );
                km(ui, st, cid, p, Some(ModDest::FilterReso(k as u8)), f.resonance, 0.0, 0.99, 0.2, "RES", widgets::pct, c, move |w, v| w.filters[k].resonance = v);
                km(ui, st, cid, p, Some(ModDest::FilterDrive(k as u8)), f.drive, 0.0, 1.0, 0.0, "DRIVE", widgets::pct, c, move |w, v| w.filters[k].drive = v);
                km(ui, st, cid, p, Some(ModDest::FilterMix(k as u8)), f.mix, 0.0, 1.0, 1.0, "MIX", widgets::pct, c, move |w, v| w.filters[k].mix = v);
                km(ui, st, cid, p, None, f.keytrack, 0.0, 1.0, 0.0, "KEY", widgets::pct, c, move |w, v| w.filters[k].keytrack = v);
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("Routing").small().color(theme::TEXT_DIM));
                for (r, name, tip) in [(FilterRouting::Serial, "1 ➡ 2", "Filter 1 into filter 2"), (FilterRouting::Parallel, "1 + 2", "Both filters side by side")] {
                    if widgets::toggle(ui, p.filter_routing == r, name, FILTER_COL).on_hover_text(tip).clicked() && p.filter_routing != r {
                        st.session.edit("Filter routing", |pr| set(pr, cid, |w| w.filter_routing = r));
                    }
                }
            });
        },
    );
    if let Some(j) = switch_to {
        if power {
            let on = p.filters[j].on;
            st.session.edit("Filter on / off", |pr| set(pr, cid, |w| w.filters[j].on = !on));
        }
        k = j;
    }
    ui.data_mut(|d| d.insert_temp(sel_id, k));
}

/// The filter's response; drag it: sideways = cutoff, up / down = resonance.
fn filter_curve(ui: &mut Ui, st: &mut State, cid: Id, f: &WtFilter, k: usize, width: f32, height: f32) {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click_and_drag());
    let pa = ui.painter_at(rect);
    pa.rect_filled(rect, 4.0, SCREEN);
    for hz in [100.0f32, 1000.0, 10000.0] {
        let x = rect.left() + (hz / 20.0).log2() / 10.0 * rect.width();
        pa.line_segment([Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())], Stroke::new(1.0_f32, theme::GRID));
    }
    let x_of = |hz: f32| rect.left() + (hz / 20.0).log2() / 10.0 * rect.width();
    let hz_of = |x: f32| 20.0 * 2f32.powf((x - rect.left()) / rect.width() * 10.0);
    let y_of = |db: f32| rect.center().y - db.clamp(-36.0, 24.0) / 36.0 * rect.height() * 0.5;
    let col = if f.on { FILTER_COL } else { theme::TEXT_DIM };
    let pts: Vec<Pos2> = (0..=100)
        .map(|i| {
            let x = rect.left() + i as f32 / 100.0 * rect.width();
            let db = if f.on {
                let wet = filter_db(f.kind, f.cutoff, f.resonance, hz_of(x));
                let g = 10f32.powf(wet / 20.0) * f.mix + (1.0 - f.mix);
                20.0 * g.max(1e-5).log10()
            } else {
                0.0
            };
            Pos2::new(x, y_of(db))
        })
        .collect();
    let mut fill = pts.clone();
    fill.push(rect.right_bottom());
    fill.push(rect.left_bottom());
    pa.add(egui::Shape::Path(egui::epaint::PathShape { points: fill, closed: true, fill: col.gamma_multiply(0.08), stroke: egui::epaint::PathStroke::NONE }));
    pa.add(egui::Shape::line(pts, Stroke::new(1.8_f32, col)));
    pa.circle_filled(Pos2::new(x_of(f.cutoff), y_of(0.0)), 4.0, if f.on { Color32::WHITE } else { theme::TEXT_DIM });
    if resp.dragged() {
        let d = resp.drag_delta();
        let cutoff = (f.cutoff * 2f32.powf(d.x / rect.width() * 10.0)).clamp(20.0, 20000.0);
        let reso = (f.resonance - d.y / 120.0).clamp(0.0, 0.99);
        st.session.gesture("Filter", |pr| {
            set(pr, cid, |w| {
                w.filters[k].cutoff = cutoff;
                w.filters[k].resonance = reso;
            })
        });
    }
    if resp.drag_stopped() {
        st.session.end_gesture();
    }
    resp.on_hover_text("Drag: sideways = cutoff, up / down = resonance");
}

// ------------------------------------------------------------------------------------------------ the modulation deck

fn mod_deck(ui: &mut Ui, st: &mut State, cid: Id, p: &WavetableParams) {
    let total = W - 8.0;
    let macro_w = 176.0;
    let right_w = 196.0;
    let gap = 4.0;
    let env_w = (total - macro_w - right_w - gap * 3.0) * 0.48;
    let lfo_w = total - macro_w - right_w - env_w - gap * 3.0;
    let h = 236.0;
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        macros_module(ui, st, cid, p, macro_w, h);
        env_module(ui, st, cid, p, env_w, h);
        lfo_module(ui, st, cid, p, lfo_w, h);
        voicing_module(ui, st, cid, p, right_w, h);
    });
}

fn macros_module(ui: &mut Ui, st: &mut State, cid: Id, p: &WavetableParams, w: f32, h: f32) {
    module(
        ui,
        st,
        "MACROS",
        MOD_COL,
        None,
        w,
        h,
        |_, _| {},
        |ui, st| {
            egui::Grid::new(("wave.macros", cid)).spacing([6.0, 2.0]).show(ui, |ui| {
                for k in 0..4 {
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            mod_handle(ui, ModSource::Macro(k as u8));
                            let n = p.matrix.iter().filter(|m| m.source == ModSource::Macro(k as u8)).count();
                            ui.label(RichText::new(format!("{}", k + 1)).small().color(theme::TEXT_DIM));
                            if n > 0 {
                                ui.label(RichText::new(format!("{n}")).small().color(MOD_COL)).on_hover_text(format!("Moves {n} control(s)"));
                            }
                        });
                        let name = p.macro_names[k].clone();
                        km(ui, st, cid, p, None, p.macros[k], 0.0, 1.0, 0.0, &name, widgets::pct, MOD_COL, move |w, v| w.macros[k] = v);
                    });
                    if k % 2 == 1 {
                        ui.end_row();
                    }
                }
            });
            ui.label(RichText::new("Drag a handle onto a knob").small().color(theme::TEXT_DIM));
        },
    );
}

fn tab_id(cid: Id) -> egui::Id {
    egui::Id::new(("wave.modtab", cid))
}

fn env_module(ui: &mut Ui, st: &mut State, cid: Id, p: &WavetableParams, w: f32, h: f32) {
    let id = tab_id(cid).with("env");
    let k: usize = ui.data(|d| d.get_temp(id)).unwrap_or(0);
    let mut new_k = k;
    module(
        ui,
        st,
        "",
        MOD_COL,
        None,
        w,
        h,
        |ui, _| {
            for j in 0..4 {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 1.0;
                    let b = egui::Button::new(RichText::new(format!("ENV {}", j + 1)).strong().size(12.0).color(if j == k { theme::TEXT } else { theme::TEXT_DIM })).fill(if j == k { theme::PANEL_2 } else { BAR });
                    if ui.add(b).on_hover_text(if j == 0 { "Env 1 shapes the level of every note" } else { "A free envelope: drag its handle onto a knob" }).clicked() {
                        new_k = j;
                    }
                    mod_handle(ui, ModSource::Env(j as u8));
                });
            }
        },
        |ui, st| {
            let label = if k == 0 { "Env 1 · amp".to_string() } else { format!("Env {}", k + 1) };
            let kk = k;
            super::envelope::editor(ui, st, &label, p.env[k], MOD_COL, move |pr, e| set(pr, cid, |w| w.env[kk] = e));
        },
    );
    ui.data_mut(|d| d.insert_temp(id, new_k));
}

fn lfo_module(ui: &mut Ui, st: &mut State, cid: Id, p: &WavetableParams, w: f32, h: f32) {
    let id = tab_id(cid).with("lfo");
    let k: usize = ui.data(|d| d.get_temp(id)).unwrap_or(0);
    let mut new_k = k;
    module(
        ui,
        st,
        "",
        MOD_COL,
        None,
        w,
        h,
        |ui, _| {
            for j in 0..4 {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 1.0;
                    let n = p.matrix.iter().filter(|m| m.source == ModSource::Lfo(j as u8)).count();
                    let text = if n > 0 { format!("LFO {} ({n})", j + 1) } else { format!("LFO {}", j + 1) };
                    let b = egui::Button::new(RichText::new(text).strong().size(12.0).color(if j == k { theme::TEXT } else { theme::TEXT_DIM })).fill(if j == k { theme::PANEL_2 } else { BAR });
                    if ui.add(b).clicked() {
                        new_k = j;
                    }
                    mod_handle(ui, ModSource::Lfo(j as u8));
                });
            }
        },
        |ui, st| lfo_editor(ui, st, cid, p, k, w - 14.0),
    );
    ui.data_mut(|d| d.insert_temp(id, new_k));
}

fn lfo_editor(ui: &mut Ui, st: &mut State, cid: Id, p: &WavetableParams, k: usize, width: f32) {
    let l = p.lfo[k].clone();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 112.0), Sense::hover());
    let pa = ui.painter_at(rect);
    pa.rect_filled(rect, 4.0, SCREEN);
    for i in 1..4 {
        let x = rect.left() + rect.width() * i as f32 / 4.0;
        pa.line_segment([Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())], Stroke::new(1.0_f32, theme::GRID));
    }
    pa.line_segment([Pos2::new(rect.left(), rect.center().y), Pos2::new(rect.right(), rect.center().y)], Stroke::new(1.0_f32, theme::GRID));
    let cycles = if l.mode == LfoMode::OneShot { 1.0 } else { 2.0 };
    let pts: Vec<Pos2> = (0..=160)
        .map(|i| {
            let ph = i as f32 / 160.0 * cycles;
            let v = l.shape.at(ph);
            let v = if l.unipolar { v * 0.5 + 0.5 } else { v };
            Pos2::new(rect.left() + 4.0 + i as f32 / 160.0 * (rect.width() - 8.0), rect.center().y - v * rect.height() * 0.4)
        })
        .collect();
    let mut fill = pts.clone();
    fill.push(Pos2::new(rect.right() - 4.0, rect.center().y));
    fill.push(Pos2::new(rect.left() + 4.0, rect.center().y));
    pa.add(egui::Shape::line(pts.clone(), Stroke::new(4.0_f32, MOD_COL.gamma_multiply(0.2))));
    pa.add(egui::Shape::line(pts, Stroke::new(1.8_f32, MOD_COL)));
    let rate = if l.sync { LFO_BEATS.iter().find(|b| (b.0 - l.beats).abs() < 1e-3).map(|b| b.1.to_string()).unwrap_or_default() } else { format!("{:.2} Hz", l.rate) };
    pa.text(rect.right_top() + Vec2::new(-6.0, 4.0), egui::Align2::RIGHT_TOP, rate, egui::FontId::monospace(11.0), theme::TEXT);
    ui.horizontal(|ui| {
        let mut shape = l.shape;
        egui::ComboBox::from_id_salt(("wave.lfo.shape", cid, k)).selected_text(shape.name()).width(92.0).show_ui(ui, |ui| {
            for x in LfoShape::ALL {
                ui.selectable_value(&mut shape, x, x.name());
            }
        });
        if shape != l.shape {
            st.session.edit("LFO shape", |pr| set(pr, cid, |w| w.lfo[k].shape = shape));
        }
        for (x, name, tip) in [(LfoMode::Free, "FREE", "Runs on for all notes"), (LfoMode::Retrig, "RETRIG", "Restarts with every note"), (LfoMode::OneShot, "ENVELOPE", "One cycle per note, then holds")] {
            if widgets::toggle(ui, l.mode == x, name, MOD_COL).on_hover_text(tip).clicked() && l.mode != x {
                st.session.edit("LFO mode", |pr| set(pr, cid, |w| w.lfo[k].mode = x));
            }
        }
        if widgets::toggle(ui, l.unipolar, "0…1", MOD_COL).on_hover_text("Unipolar (0 … 1) instead of −1 … 1").clicked() {
            st.session.edit("LFO polarity", |pr| set(pr, cid, |w| w.lfo[k].unipolar = !l.unipolar));
        }
    });
    ui.horizontal(|ui| {
        if widgets::toggle(ui, l.sync, "BPM", MOD_COL).on_hover_text("Lock the rate to the tempo").clicked() {
            st.session.edit("LFO sync", |pr| set(pr, cid, |w| w.lfo[k].sync = !l.sync));
        }
        if l.sync {
            let cur = LFO_BEATS.iter().find(|b| (b.0 - l.beats).abs() < 1e-3).map(|b| b.1).unwrap_or("1/4");
            ui.vertical(|ui| {
                egui::ComboBox::from_id_salt(("wave.lfo.beats", cid, k)).selected_text(cur).width(64.0).show_ui(ui, |ui| {
                    for (b, name) in LFO_BEATS {
                        if ui.selectable_label((b - l.beats).abs() < 1e-3, name).clicked() {
                            st.session.edit("LFO rate", |pr| set(pr, cid, |w| w.lfo[k].beats = b));
                        }
                    }
                });
                ui.label(RichText::new("RATE").small().color(theme::TEXT_DIM));
            });
        } else {
            km(ui, st, cid, p, Some(ModDest::LfoRate(k as u8)), l.rate, 0.01, 40.0, 2.0, "RATE", |v| format!("{v:.2} Hz"), MOD_COL, move |w, v| w.lfo[k].rate = v);
        }
        km(ui, st, cid, p, None, l.rise, 0.0, 4.0, 0.0, "RISE", |v| format!("{v:.2} s"), MOD_COL, move |w, v| w.lfo[k].rise = v);
        km(ui, st, cid, p, None, l.delay, 0.0, 4.0, 0.0, "DELAY", |v| format!("{v:.2} s"), MOD_COL, move |w, v| w.lfo[k].delay = v);
    });
}

fn voicing_module(ui: &mut Ui, st: &mut State, cid: Id, p: &WavetableParams, w: f32, h: f32) {
    module(
        ui,
        st,
        "VELO · NOTE · VOICING",
        MOD_COL,
        None,
        w,
        h,
        |_, _| {},
        |ui, st| {
            // Velocity curve (how much quiet notes drop) as a picture, plus the per-note sources.
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(Vec2::new(70.0, 52.0), Sense::hover());
                let pa = ui.painter_at(rect);
                pa.rect_filled(rect, 3.0, SCREEN);
                let pts: Vec<Pos2> = (0..=20)
                    .map(|i| {
                        let v = i as f32 / 20.0;
                        let g = 1.0 - p.velocity * (1.0 - v);
                        Pos2::new(rect.left() + v * rect.width(), rect.bottom() - g * rect.height())
                    })
                    .collect();
                pa.add(egui::Shape::line(pts, Stroke::new(1.5_f32, MOD_COL)));
                ui.vertical(|ui| {
                    for (src, name) in [(ModSource::Velocity, "VELO"), (ModSource::Note, "NOTE"), (ModSource::Random, "RAND")] {
                        ui.horizontal(|ui| {
                            mod_handle(ui, src);
                            ui.label(RichText::new(name).small().strong());
                        });
                    }
                });
            });
            ui.horizontal(|ui| {
                km(ui, st, cid, p, None, p.velocity, 0.0, 1.0, 0.5, "VELOCITY", widgets::pct, MOD_COL, |w, v| w.velocity = v);
                km(ui, st, cid, p, None, p.glide, 0.0, 1.0, 0.0, "PORTA", |v| format!("{:.0} ms", v * 1000.0), MOD_COL, |w, v| w.glide = v);
                km(ui, st, cid, p, None, p.polyphony as f32, 1.0, 32.0, 16.0, "POLY", |v| format!("{:.0}", v.round()), MOD_COL, |w, v| w.polyphony = v.round() as u32);
            });
            ui.horizontal(|ui| {
                if widgets::toggle(ui, p.mono, "MONO", MOD_COL).on_hover_text("One note at a time; the last held note comes back when you let go").clicked() {
                    st.session.edit("Mono", |pr| set(pr, cid, |w| w.mono = !p.mono));
                }
                if widgets::toggle(ui, p.legato, "LEGATO", MOD_COL).on_hover_text("Mono: overlapping notes don't restart the envelopes").clicked() {
                    st.session.edit("Legato", |pr| set(pr, cid, |w| w.legato = !p.legato));
                }
            });
        },
    );
}

// ------------------------------------------------------------------------------------------------ other pages

fn mix_page(ui: &mut Ui, st: &mut State, cid: Id, p: &WavetableParams) {
    ui.label(RichText::new("Every sound source side by side: level, pan and where it goes.").small().color(theme::TEXT_DIM));
    ui.add_space(4.0);
    ui.horizontal_top(|ui| {
        let strip = |ui: &mut Ui, st: &mut State, name: &str, col: Color32, on: bool, level: f32, pan: f32, route: Route, salt: usize, write: fn(&mut WavetableParams, Option<f32>, Option<f32>, Option<Route>, bool)| {
            egui::Frame::NONE.fill(theme::PANEL).corner_radius(4.0).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
                ui.set_width(120.0);
                ui.vertical_centered(|ui| {
                    if widgets::toggle(ui, on, name, col).clicked() {
                        st.session.edit("Source on / off", |pr| set(pr, cid, |w| write(w, None, None, None, true)));
                    }
                    let mut v = level;
                    let r = ui.horizontal(|ui| {
                        ui.add_space(50.0);
                        ui.spacing_mut().slider_width = 140.0;
                        ui.add(egui::Slider::new(&mut v, 0.0..=1.0).vertical().show_value(false))
                    });
                    let r = r.inner;
                    widgets::apply(st, &r, &format!("{name} level"), |pr| set(pr, cid, |w| write(w, Some(v), None, None, false)));
                    ui.label(RichText::new(format!("{:.0}%", level * 100.0)).small());
                    km(ui, st, cid, p, None, pan, -1.0, 1.0, 0.0, "PAN", widgets::pan_fmt, col, move |w, x| write(w, None, Some(x), None, false));
                    let mut r = route;
                    egui::ComboBox::from_id_salt(("wave.mixroute", cid, salt)).selected_text(r.name()).width(90.0).show_ui(ui, |ui| {
                        for x in Route::ALL {
                            ui.selectable_value(&mut r, x, x.name());
                        }
                    });
                    if r != route {
                        st.session.edit("Routing", |pr| set(pr, cid, |w| write(w, None, None, Some(r), false)));
                    }
                });
            });
        };
        fn osc_w<const O: usize>(w: &mut WavetableParams, l: Option<f32>, pn: Option<f32>, r: Option<Route>, t: bool) {
            let o = &mut w.osc[O];
            if let Some(l) = l {
                o.level = l;
            }
            if let Some(x) = pn {
                o.pan = x;
            }
            if let Some(r) = r {
                o.route = r;
            }
            if t {
                o.on = !o.on;
            }
        }
        fn sub_w(w: &mut WavetableParams, l: Option<f32>, pn: Option<f32>, r: Option<Route>, t: bool) {
            if let Some(l) = l {
                w.sub.level = l;
            }
            if let Some(x) = pn {
                w.sub.pan = x;
            }
            if let Some(r) = r {
                w.sub.route = r;
            }
            if t {
                w.sub.on = !w.sub.on;
            }
        }
        fn noise_w(w: &mut WavetableParams, l: Option<f32>, pn: Option<f32>, r: Option<Route>, t: bool) {
            if let Some(l) = l {
                w.noise.level = l;
            }
            if let Some(x) = pn {
                w.noise.pan = x;
            }
            if let Some(r) = r {
                w.noise.route = r;
            }
            if t {
                w.noise.on = !w.noise.on;
            }
        }
        strip(ui, st, "SUB", SUB_COL, p.sub.on, p.sub.level, p.sub.pan, p.sub.route, 10, sub_w);
        strip(ui, st, "OSC A", OSC_COL[0], p.osc[0].on, p.osc[0].level, p.osc[0].pan, p.osc[0].route, 0, osc_w::<0>);
        strip(ui, st, "OSC B", OSC_COL[1], p.osc[1].on, p.osc[1].level, p.osc[1].pan, p.osc[1].route, 1, osc_w::<1>);
        strip(ui, st, "OSC C", OSC_COL[2], p.osc[2].on, p.osc[2].level, p.osc[2].pan, p.osc[2].route, 2, osc_w::<2>);
        strip(ui, st, "NOISE", NOISE_COL, p.noise.on, p.noise.level, p.noise.pan, p.noise.route, 11, noise_w);
    });
}

fn global_page(ui: &mut Ui, st: &mut State, cid: Id, p: &WavetableParams) {
    ui.horizontal_top(|ui| {
        module(
            ui,
            st,
            "VOICING",
            theme::ACCENT_2,
            None,
            360.0,
            160.0,
            |_, _| {},
            |ui, st| {
                ui.horizontal(|ui| {
                    let c = theme::ACCENT_2;
                    km(ui, st, cid, p, None, p.polyphony as f32, 1.0, 32.0, 16.0, "VOICES", |v| format!("{:.0}", v.round()), c, |w, v| w.polyphony = v.round() as u32);
                    km(ui, st, cid, p, None, p.glide, 0.0, 1.0, 0.0, "GLIDE", |v| format!("{:.0} ms", v * 1000.0), c, |w, v| w.glide = v);
                    km(ui, st, cid, p, None, p.velocity, 0.0, 1.0, 0.5, "VELOCITY", widgets::pct, c, |w, v| w.velocity = v);
                    km(ui, st, cid, p, None, p.trim_db, -36.0, 36.0, 0.0, "TRIM", |v| format!("{v:+.1} dB"), c, |w, v| w.trim_db = v);
                });
                ui.horizontal(|ui| {
                    if widgets::toggle(ui, p.mono, "Mono", theme::ACCENT_2).clicked() {
                        st.session.edit("Mono", |pr| set(pr, cid, |w| w.mono = !p.mono));
                    }
                    if widgets::toggle(ui, p.legato, "Legato", theme::ACCENT_2).clicked() {
                        st.session.edit("Legato", |pr| set(pr, cid, |w| w.legato = !p.legato));
                    }
                });
                ui.label(
                    RichText::new("Velocity: how much softer quiet notes play. Glide: pitch slides between notes. Trim: the preset's level (set for every factory preset so they all play equally loud).")
                        .small()
                        .color(theme::TEXT_DIM),
                );
            },
        );
        module(
            ui,
            st,
            "MACRO NAMES",
            MOD_COL,
            None,
            260.0,
            160.0,
            |_, _| {},
            |ui, st| {
                for k in 0..4 {
                    let mut n = p.macro_names[k].clone();
                    if ui.add(egui::TextEdit::singleline(&mut n).desired_width(220.0)).changed() {
                        st.session.gesture("Macro name", |pr| set(pr, cid, |w| w.macro_names[k] = n));
                    }
                }
            },
        );
        module(
            ui,
            st,
            "PRESET",
            theme::ACCENT,
            None,
            300.0,
            160.0,
            |_, _| {},
            |ui, st| {
                ui.label(RichText::new(&p.preset).strong());
                if let Some(l) = wt_library::find(&p.preset) {
                    ui.label(format!("{} · {}", l.category.name(), l.kind));
                } else {
                    ui.label("User preset");
                }
                ui.label(RichText::new(format!("{} factory presets · user presets in {}", wt_library::library().len(), user_dir(st).display())).small().color(theme::TEXT_DIM));
                if ui.button("Open user preset folder").clicked() {
                    let d = user_dir(st);
                    let _ = std::fs::create_dir_all(&d);
                    crate::chains::reveal(&d);
                }
            },
        );
    });
}

// ------------------------------------------------------------------------------------------------ browser

fn browser(ui: &mut Ui, st: &mut State, cid: Id, p: &WavetableParams) {
    let h = 500.0;
    ui.horizontal_top(|ui| {
        // Categories.
        egui::Frame::NONE.fill(theme::PANEL).corner_radius(4.0).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
            ui.set_width(190.0);
            ui.set_min_height(h);
            ui.vertical(|ui| {
                ui.label(RichText::new("PRESETS").strong().color(theme::ACCENT));
                let lib = wt_library::library();
                let nfav = favs(st).len();
                let nuser = user_presets(st).len();
                let mut rows: Vec<(Option<String>, String)> =
                    vec![(None, format!("All  ({})", lib.len() + nuser)), (Some("Favourites".into()), format!("Favourites  ({nfav})")), (Some("User".into()), format!("User  ({nuser})"))];
                for c in Category::ALL {
                    let n = lib.iter().filter(|p| p.category == c).count();
                    rows.push((Some(c.name().to_string()), format!("{}  ({n})", c.name())));
                }
                for (f, label) in rows {
                    let on = st.ui.wave.filter == f;
                    if ui.add_sized([178.0, 20.0], egui::Button::new(RichText::new(label).color(if on { Color32::BLACK } else { theme::TEXT })).fill(if on { theme::ACCENT_2 } else { theme::PANEL })).clicked() {
                        st.ui.wave.filter = f;
                    }
                }
            });
        });
        // The list.
        egui::Frame::NONE.fill(theme::PANEL).corner_radius(4.0).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
            ui.set_width(W - 220.0);
            ui.set_min_height(h);
            ui.vertical(|ui| {
                let list = entries(st);
                ui.horizontal(|ui| {
                    let mut q = st.ui.wave.search.clone();
                    let r = ui.add(egui::TextEdit::singleline(&mut q).hint_text("Search: name, type or category (e.g. \"reese\", \"dark pad\")").desired_width(380.0));
                    if r.changed() {
                        st.ui.wave.search = q;
                    }
                    if !st.ui.wave.search.is_empty() && ui.small_button("×").clicked() {
                        st.ui.wave.search.clear();
                    }
                    ui.label(RichText::new(format!("{} presets", list.len())).small().color(theme::TEXT_DIM));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Close").clicked() {
                            st.ui.wave.open = false;
                        }
                        if ui.button("Random").clicked() {
                            step_preset(st, cid, &p.preset, 0);
                        }
                    });
                });
                ui.label(RichText::new("Click to load and hear it, double-click to load and close. The arrows at the top step through this list.").small().color(theme::TEXT_DIM));
                let fav = favs(st).clone();
                let row_h = 20.0;
                egui::ScrollArea::vertical().max_height(h - 50.0).auto_shrink([false, false]).show_rows(ui, row_h, list.len(), |ui, range| {
                    for e in &list[range] {
                        let cur = e.name == p.preset;
                        ui.horizontal(|ui| {
                            if star(ui, fav.contains(&e.name)).on_hover_text("Favourite").clicked() {
                                toggle_fav(st, &e.name);
                            }
                            let (rect, r) = ui.allocate_exact_size(Vec2::new(W - 290.0, row_h - 2.0), Sense::click());
                            let pa = ui.painter();
                            if cur {
                                pa.rect_filled(rect, 3.0, theme::ACCENT_2.gamma_multiply(0.85));
                            } else if r.hovered() {
                                pa.rect_filled(rect, 3.0, theme::PANEL_2);
                            }
                            let ink = if cur { Color32::BLACK } else { theme::TEXT };
                            let dim = if cur { Color32::from_gray(40) } else { theme::TEXT_DIM };
                            let y = rect.center().y;
                            pa.text(Pos2::new(rect.left() + 6.0, y), egui::Align2::LEFT_CENTER, e.tag, egui::FontId::monospace(11.0), if cur { Color32::BLACK } else { theme::ACCENT_2 });
                            pa.text(Pos2::new(rect.left() + 42.0, y), egui::Align2::LEFT_CENTER, &e.name, egui::FontId::proportional(13.5), ink);
                            pa.text(Pos2::new(rect.left() + 360.0, y), egui::Align2::LEFT_CENTER, &e.kind, egui::FontId::proportional(11.5), dim);
                            if r.clicked() {
                                load_entry(st, cid, e);
                            }
                            if r.double_clicked() {
                                st.ui.wave.open = false;
                            }
                        });
                    }
                });
            });
        });
    });
}

// ------------------------------------------------------------------------------------------------ keyboard

/// A playable keyboard (C1 – C7) that sends notes to this synth.
fn keyboard(ui: &mut Ui, st: &mut State, cid: Id, p: &WavetableParams) {
    let held_id = egui::Id::new(("wave.kbd", cid));
    let held: Option<u8> = ui.data(|d| d.get_temp(held_id)).flatten();
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            km(ui, st, cid, p, None, p.glide, 0.0, 1.0, 0.0, "PORTA", |v| format!("{:.0} ms", v * 1000.0), theme::ACCENT_2, |w, v| w.glide = v);
        });
        let (rect, r) = ui.allocate_exact_size(Vec2::new(W - 60.0, 58.0), Sense::click_and_drag());
        let pa = ui.painter_at(rect);
        let (lo, hi) = (24u8, 96u8);
        let whites: Vec<u8> = (lo..=hi).filter(|n| !is_black(*n)).collect();
        let ww = rect.width() / whites.len() as f32;
        let mut key_rects: Vec<(u8, Rect)> = Vec::new();
        for (i, n) in whites.iter().enumerate() {
            key_rects.push((*n, Rect::from_min_size(Pos2::new(rect.left() + i as f32 * ww, rect.top()), Vec2::new(ww - 1.0, rect.height()))));
        }
        let mut blacks = Vec::new();
        for n in lo..=hi {
            if is_black(n) {
                let left_white = whites.iter().position(|w| *w == n - 1).unwrap_or(0);
                let x = rect.left() + (left_white + 1) as f32 * ww - ww * 0.3;
                blacks.push((n, Rect::from_min_size(Pos2::new(x, rect.top()), Vec2::new(ww * 0.6, rect.height() * 0.6))));
            }
        }
        for (n, kr) in &key_rects {
            let on = held == Some(*n);
            pa.rect_filled(*kr, 2.0, if on { theme::ACCENT_2 } else { Color32::from_rgb(0xd8, 0xdb, 0xe0) });
            if n % 12 == 0 {
                pa.text(kr.center_bottom() - Vec2::new(0.0, 3.0), egui::Align2::CENTER_BOTTOM, format!("C{}", *n as i32 / 12 - 1), egui::FontId::proportional(9.0), Color32::from_gray(90));
            }
        }
        for (n, kr) in &blacks {
            let on = held == Some(*n);
            pa.rect_filled(*kr, 2.0, if on { theme::ACCENT_2 } else { Color32::from_rgb(0x1a, 0x1c, 0x20) });
        }
        // Which key is under the pointer (black keys first, they sit on top).
        let at = |q: Pos2| blacks.iter().chain(key_rects.iter()).find(|(_, kr)| kr.contains(q)).map(|(n, _)| *n);
        let pointer = ui.ctx().pointer_latest_pos();
        let down = r.is_pointer_button_down_on() || r.dragged();
        let want = if down { pointer.and_then(at) } else { None };
        if want != held {
            if let Some(n) = held {
                st.audio.send(Command::Live(LiveEvent::NoteOff { channel: cid, note: n }));
            }
            if let Some(n) = want {
                let vel = pointer.map(|q| (0.4 + 0.6 * (q.y - rect.top()) / rect.height()).clamp(0.1, 1.0)).unwrap_or(0.8);
                st.audio.send(Command::Live(LiveEvent::NoteOn { channel: cid, note: n, velocity: vel }));
            }
            ui.data_mut(|d| d.insert_temp(held_id, want));
        }
        r.on_hover_text("Click to play (lower on the key = louder)");
    });
}

fn is_black(n: u8) -> bool {
    matches!(n % 12, 1 | 3 | 6 | 8 | 10)
}

// ------------------------------------------------------------------------------------------------ filter maths, FX and matrix

/// Approximate magnitude (dB) of a filter at `f` Hz, for drawing.
pub fn filter_db(kind: FilterKind, fc: f32, reso: f32, f: f32) -> f32 {
    let x = f / fc.max(1.0);
    let q = |r: f32| 1.0 / (2.0 - 1.95 * r.clamp(0.0, 1.0));
    let lp = |r: f32| 1.0 / ((1.0 - x * x).powi(2) + (x / q(r)).powi(2)).sqrt();
    let hp = |r: f32| x * x / ((1.0 - x * x).powi(2) + (x / q(r)).powi(2)).sqrt();
    let bp = |r: f32| (x / q(r)) / ((1.0 - x * x).powi(2) + (x / q(r)).powi(2)).sqrt();
    let g = match kind {
        FilterKind::Lp12 => lp(reso),
        FilterKind::Lp24 => lp(reso * 0.6).powi(2),
        FilterKind::Hp12 => hp(reso),
        FilterKind::Hp24 => hp(reso * 0.6).powi(2),
        FilterKind::Bp => bp(reso),
        FilterKind::Notch => ((1.0 - x * x).abs()) / ((1.0 - x * x).powi(2) + (x / q(reso)).powi(2)).sqrt(),
        FilterKind::CombPlus | FilterKind::CombMinus => {
            let fb = 0.2 + reso * 0.75;
            let w = std::f32::consts::TAU * x;
            let s = if kind == FilterKind::CombPlus { -fb } else { fb };
            (1.0 - fb * 0.5) / ((1.0 + s * w.cos()).powi(2) + (s * w.sin()).powi(2)).sqrt()
        }
        FilterKind::Formant => {
            const V: [[f32; 3]; 5] = [[800.0, 1150.0, 2900.0], [400.0, 1600.0, 2700.0], [270.0, 2100.0, 3000.0], [450.0, 800.0, 2830.0], [325.0, 700.0, 2530.0]];
            let k = ((fc.max(20.0) / 20.0).log2() / 10.0).clamp(0.0, 1.0) * 4.0;
            let i = (k as usize).min(3);
            let t = k - i as f32;
            let qq = 1.0 / (2.0 - 1.95 * (0.5 + reso * 0.45));
            (0..3)
                .map(|j| {
                    let c = V[i][j] * (1.0 - t) + V[i + 1][j] * t;
                    let y = f / c;
                    [1.0, 0.7, 0.4][j] * (y / qq) / ((1.0 - y * y).powi(2) + (y / qq).powi(2)).sqrt()
                })
                .sum::<f32>()
                * 1.6
        }
    };
    20.0 * g.max(1e-5).log10()
}

/// How an FX rack reaches its slots in the project (each instrument keeps its own list).
pub type FxEdit = fn(&mut Project, Id, &mut dyn FnMut(&mut Vec<FxSlot>));

fn fx_tab(ui: &mut Ui, st: &mut State, cid: Id, p: &WavetableParams) {
    fn edit(pr: &mut Project, cid: Id, f: &mut dyn FnMut(&mut Vec<FxSlot>)) {
        set(pr, cid, |w| f(&mut w.fx));
    }
    fx_rack(ui, st, cid, &p.fx, W - 20.0, "wave", edit);
}

/// The FX rack: effects in order, each with its main controls and the rest folded away.
pub fn fx_rack(ui: &mut Ui, st: &mut State, cid: Id, fxs: &[FxSlot], width: f32, salt: &'static str, edit: FxEdit) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("FX RACK").small().strong().color(theme::ACCENT_2));
        ui.label(RichText::new("runs after the voices, top to bottom").small().color(theme::TEXT_DIM));
        ui.menu_button("+ Add effect", |ui| {
            for id in FX_CHOICES {
                if let Some(def) = effects::find(id) {
                    if ui.button(def.name).on_hover_text(def.description).clicked() {
                        st.session.edit("Add effect", |pr| edit(pr, cid, &mut |fx| fx.push(FxSlot { id: id.into(), on: true, params: Default::default() })));
                        ui.close_menu();
                    }
                }
            }
        });
    });
    if fxs.is_empty() {
        ui.label(RichText::new("No effects yet – add distortion, chorus, delay, reverb …").small().color(theme::TEXT_DIM));
    }
    let n = fxs.len();
    for (i, f) in fxs.iter().cloned().enumerate() {
        let Some(def) = effects::find(&f.id) else { continue };
        let col = theme::ACCENT_2;
        egui::Frame::NONE.fill(theme::PANEL_2).corner_radius(6.0).inner_margin(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
            ui.set_width(width);
            ui.horizontal(|ui| {
                if widgets::toggle(ui, f.on, if f.on { "On" } else { "Off" }, col).clicked() {
                    st.session.edit("Effect on / off", |pr| {
                        edit(pr, cid, &mut |fx| {
                            if let Some(x) = fx.get_mut(i) {
                                x.on = !f.on
                            }
                        })
                    });
                }
                ui.label(RichText::new(def.name).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("×").on_hover_text("Remove").clicked() {
                        st.session.edit("Remove effect", |pr| {
                            edit(pr, cid, &mut |fx| {
                                if i < fx.len() {
                                    fx.remove(i);
                                }
                            })
                        });
                    }
                    if i + 1 < n && ui.small_button("down").clicked() {
                        st.session.edit("Move effect", |pr| edit(pr, cid, &mut |fx| fx.swap(i, i + 1)));
                    }
                    if i > 0 && ui.small_button("up").clicked() {
                        st.session.edit("Move effect", |pr| edit(pr, cid, &mut |fx| fx.swap(i, i - 1)));
                    }
                });
            });
            // The main controls first; the rest folded away so long effects stay tidy.
            let (main, more) = def.params.split_at(def.params.len().min(8));
            fx_params(ui, st, cid, i, &f, main, col, salt, edit);
            if !more.is_empty() {
                egui::CollapsingHeader::new(RichText::new(format!("More controls ({})", more.len())).small()).id_salt((salt, "fxmore", cid, i)).show(ui, |ui| fx_params(ui, st, cid, i, &f, more, col, salt, edit));
            }
        });
        ui.add_space(4.0);
    }
}

#[allow(clippy::too_many_arguments)]
fn fx_params(ui: &mut Ui, st: &mut State, cid: Id, i: usize, f: &FxSlot, params: &[effects::ParamInfo], col: Color32, salt: &'static str, edit: FxEdit) {
    ui.horizontal_wrapped(|ui| {
        for prm in params {
            let v = f.params.get(prm.id).copied().unwrap_or(prm.default);
            let key = prm.id.to_string();
            match prm.kind {
                ParamKind::Toggle => {
                    if ui.checkbox(&mut (v > 0.5), prm.name).clicked() {
                        st.session.edit(prm.name, |pr| {
                            edit(pr, cid, &mut |fx| {
                                if let Some(x) = fx.get_mut(i) {
                                    x.params.insert(key.clone(), if v > 0.5 { 0.0 } else { 1.0 });
                                }
                            })
                        });
                    }
                }
                ParamKind::Choice(names) => {
                    let cur = names.get(v.round() as usize).copied().unwrap_or("?");
                    egui::ComboBox::from_id_salt((salt, "fxc", cid, i, prm.id)).selected_text(cur).width(90.0).show_ui(ui, |ui| {
                        for (j, nm) in names.iter().enumerate() {
                            if ui.selectable_label(j == v.round() as usize, *nm).clicked() {
                                let key = key.clone();
                                st.session.edit(prm.name, |pr| {
                                    edit(pr, cid, &mut |fx| {
                                        if let Some(x) = fx.get_mut(i) {
                                            x.params.insert(key.clone(), j as f32);
                                        }
                                    })
                                });
                            }
                        }
                    });
                }
                _ => {
                    let prm = *prm;
                    // Long names get shortened so neighbouring labels don't run together.
                    let short = if prm.name.chars().count() > 9 { format!("{}.", prm.name.chars().take(8).collect::<String>().trim_end()) } else { prm.name.to_string() };
                    ui.add_space(2.0);
                    let mut x = v;
                    let r = knob_colored(ui, &mut x, prm.min, prm.max, prm.default, &short, move |x| prm.format(x), col);
                    if r.hovered() || r.dragged() {
                        st.hint(prm.name, &short, prm.format(x));
                    }
                    widgets::apply(st, &r, prm.name, |pr| {
                        edit(pr, cid, &mut |fx| {
                            if let Some(s) = fx.get_mut(i) {
                                s.params.insert(key.clone(), x);
                            }
                        })
                    });
                }
            }
        }
    });
}

fn matrix(ui: &mut Ui, st: &mut State, cid: Id, p: &WavetableParams) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("MODULATION MATRIX").small().strong().color(MOD_COL));
        ui.label(RichText::new("a source moves a destination by the amount (− reverses it)").small().color(theme::TEXT_DIM));
        if ui.button("+ Add").clicked() {
            st.session.edit("Add modulation", |pr| set(pr, cid, |w| w.matrix.push(ModSlot { source: ModSource::Lfo(0), dest: ModDest::OscPos(0), amount: 0.5, on: true })));
        }
    });
    if p.matrix.is_empty() {
        ui.label(RichText::new("Nothing modulated yet. Try: LFO 1 ➡ Osc A position, or Env 2 ➡ Filter 1 cutoff.").small().color(theme::TEXT_DIM));
    }
    egui::Grid::new(("wave.matrix", cid)).num_columns(6).spacing([8.0, 4.0]).striped(true).show(ui, |ui| {
        for (i, s) in p.matrix.clone().into_iter().enumerate() {
            if widgets::toggle(ui, s.on, if s.on { "on" } else { "off" }, MOD_COL).clicked() {
                st.session.edit("Modulation on / off", |pr| {
                    set(pr, cid, |w| {
                        if let Some(x) = w.matrix.get_mut(i) {
                            x.on = !s.on
                        }
                    })
                });
            }
            egui::ComboBox::from_id_salt(("wave.src", cid, i)).selected_text(s.source.name()).width(110.0).show_ui(ui, |ui| {
                for x in ModSource::all() {
                    if ui.selectable_label(x == s.source, x.name()).clicked() {
                        st.session.edit("Modulation source", |pr| {
                            set(pr, cid, |w| {
                                if let Some(m) = w.matrix.get_mut(i) {
                                    m.source = x
                                }
                            })
                        });
                    }
                }
            });
            ui.label("➡");
            egui::ComboBox::from_id_salt(("wave.dst", cid, i)).selected_text(s.dest.name()).width(150.0).show_ui(ui, |ui| {
                for x in ModDest::all() {
                    if ui.selectable_label(x == s.dest, x.name()).clicked() {
                        st.session.edit("Modulation destination", |pr| {
                            set(pr, cid, |w| {
                                if let Some(m) = w.matrix.get_mut(i) {
                                    m.dest = x
                                }
                            })
                        });
                    }
                }
            });
            let mut a = s.amount;
            let r = ui.add(egui::Slider::new(&mut a, -1.0..=1.0).show_value(true).custom_formatter(|v, _| format!("{:+.0}%", v * 100.0)));
            widgets::apply(st, &r, "Modulation amount", |pr| {
                set(pr, cid, |w| {
                    if let Some(m) = w.matrix.get_mut(i) {
                        m.amount = a
                    }
                })
            });
            if ui.small_button("×").on_hover_text("Remove").clicked() {
                st.session.edit("Remove modulation", |pr| {
                    set(pr, cid, |w| {
                        if i < w.matrix.len() {
                            w.matrix.remove(i);
                        }
                    })
                });
            }
            ui.end_row();
        }
    });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new("Macro names").small().color(theme::TEXT_DIM));
        for k in 0..4 {
            let mut n = p.macro_names[k].clone();
            if ui.add(egui::TextEdit::singleline(&mut n).desired_width(80.0)).changed() {
                st.session.gesture("Macro name", |pr| set(pr, cid, |w| w.macro_names[k] = n));
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::Rect;

    #[test]
    fn filter_curves_have_the_right_shape() {
        assert!(filter_db(FilterKind::Lp24, 1000.0, 0.2, 100.0).abs() < 1.0);
        assert!(filter_db(FilterKind::Lp24, 1000.0, 0.2, 8000.0) < -30.0);
        assert!(filter_db(FilterKind::Hp12, 1000.0, 0.2, 100.0) < -30.0);
        assert!(filter_db(FilterKind::Bp, 1000.0, 0.5, 1000.0) > filter_db(FilterKind::Bp, 1000.0, 0.5, 100.0) + 10.0);
    }

    #[test]
    fn the_editor_draws_every_page_and_the_browser_loads_presets() {
        let mut st = State::new(redhive_client_framework::AppContext::new("en"), crate::state::Settings { show_splash: false, ..Default::default() });
        st.new_project();
        let cid = st.session.edit("x", |p| p.add_channel("Wave", Instrument::Wavetable(Box::new(wt::presets()[1].1.clone()))));
        let ctx = egui::Context::default();
        let draw = |st: &mut State, page: u8| {
            let _ = ctx.run(egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1400.0, 1000.0))), ..Default::default() }, |ctx| {
                ctx.data_mut(|d| d.insert_temp(egui::Id::new(("wave.page", cid)), page));
                egui::CentralPanel::default().show(ctx, |ui| {
                    let Some(Instrument::Wavetable(p)) = st.session.project.channel(cid).map(|c| c.instrument.clone()) else { panic!() };
                    editor(ui, st, cid, *p);
                });
            });
        };
        for page in 0..5u8 {
            draw(&mut st, page);
        }
        st.ui.wave.open = true;
        draw(&mut st, 0);
        // The arrows step through the browser's list; a category narrows it; search finds by type.
        let wave = |st: &State| match st.session.project.channel(cid).map(|c| c.instrument.clone()) {
            Some(Instrument::Wavetable(p)) => *p,
            _ => panic!(),
        };
        st.ui.wave.filter = Some("Bass".into());
        step_preset(&mut st, cid, "nothing", 1);
        let first = wave(&st).preset;
        assert_eq!(wt_library::find(&first).unwrap().category, Category::Bass);
        step_preset(&mut st, cid, &first, 1);
        assert_ne!(wave(&st).preset, first);
        st.ui.wave.search = "reese".into();
        let found = entries(&mut st);
        assert!(found.len() > 20 && found.iter().all(|e| e.kind == "Reese" || e.name.to_lowercase().contains("reese")));
        draw(&mut st, 0);
    }
}
