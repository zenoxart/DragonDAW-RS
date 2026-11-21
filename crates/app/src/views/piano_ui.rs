//! The piano editor: a gold title over a painted stage (high damp and pedal on the left, the velocity
//! range on the right, the arpeggiator plaque), a black lacquer case with gold knobs, and the keys.
//! Pages: Performance, Arpeggiator, FX, Settings; the preset browser for the 2,000-preset library.
//! Same top bar, browser and widgets as the guitar so the stock instruments feel like one family.
//! Artwork (stage, lacquer, ornament) was generated with Higgsfield.

use super::guitar_ui::{arrow_button, group_label, pill, star, texture, GOLD, GOLD_DIM, LIT, PANEL_BG};
use super::widgets::{self, knob_colored};
use crate::state::State;
use crate::theme;
use dragon_engine::device::Command;
use dragon_engine::model::{Id, Instrument, Project};
use dragon_engine::pno::*;
use dragon_engine::pno_library::{self, Category};
use dragon_engine::render::LiveEvent;
use dragon_engine::wt::FxSlot;
use egui::{Color32, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2};
use std::collections::BTreeSet;
use std::path::PathBuf;

const W: f32 = 1180.0;
/// The name shown on the instrument.
pub const NAME: &str = "Emberkeys";
const AMBER: Color32 = Color32::from_rgb(0xe8, 0x8a, 0x3a);

const SCENE_JPG: &[u8] = include_bytes!("../../assets/piano/scene.jpg");
const LACQUER_JPG: &[u8] = include_bytes!("../../assets/piano/lacquer.jpg");
const ORNAMENT_PNG: &[u8] = include_bytes!("../../assets/piano/ornament.png");

fn set(p: &mut Project, cid: Id, f: impl FnOnce(&mut PianoParams)) {
    if let Some(Instrument::Piano(g)) = p.channel_mut(cid).map(|c| &mut c.instrument) {
        f(g);
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
enum Page {
    #[default]
    Performance,
    Arp,
    Fx,
    Settings,
}

impl Page {
    const ALL: [Page; 4] = [Page::Performance, Page::Arp, Page::Fx, Page::Settings];
    fn name(self) -> &'static str {
        match self {
            Page::Performance => "Performance",
            Page::Arp => "Arpeggiator",
            Page::Fx => "FX Rack",
            Page::Settings => "Settings",
        }
    }
}

// ------------------------------------------------------------------------------------------------ browser state

#[derive(Default)]
pub struct Browser {
    pub open: bool,
    pub filter: Option<String>,
    pub search: String,
    pub save_name: String,
    favs: Option<BTreeSet<String>>,
    user: Option<Vec<(String, PianoParams)>>,
}

const FAV_FILE: &str = "emberkeys-favourites.json";

fn user_dir(st: &State) -> PathBuf {
    st.ctx.paths.config.join("Emberkeys Presets")
}

fn favs(st: &mut State) -> &mut BTreeSet<String> {
    if st.ui.piano.favs.is_none() {
        let f: Vec<String> = st.ctx.paths.load_json(FAV_FILE).unwrap_or_default();
        st.ui.piano.favs = Some(f.into_iter().collect());
    }
    st.ui.piano.favs.get_or_insert_with(Default::default)
}

fn toggle_fav(st: &mut State, name: &str) {
    let f = favs(st);
    if !f.remove(name) {
        f.insert(name.to_string());
    }
    let list: Vec<String> = f.iter().cloned().collect();
    st.ctx.paths.save_json(FAV_FILE, &list);
}

fn user_presets(st: &mut State) -> &Vec<(String, PianoParams)> {
    if st.ui.piano.user.is_none() {
        let dir = user_dir(st);
        let mut v: Vec<(String, PianoParams)> = std::fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
            .filter_map(|e| Some((e.path().file_stem()?.to_string_lossy().to_string(), st.ctx.paths.load_from(&e.path())?)))
            .collect();
        v.sort_by_key(|a| a.0.to_lowercase());
        st.ui.piano.user = Some(v);
    }
    st.ui.piano.user.get_or_insert_with(Default::default)
}

fn save_user_preset(st: &mut State, name: &str, p: &PianoParams) {
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
    st.ui.piano.user = None;
    st.toast(format!("Saved preset \"{clean}\""), redhive_contracts::ui::Severity::Success);
}

#[derive(Clone)]
struct Entry {
    name: String,
    tag: &'static str,
    kind: String,
    lib: Option<usize>,
    user: Option<usize>,
}

fn entries(st: &mut State) -> Vec<Entry> {
    let filter = st.ui.piano.filter.clone();
    let q = st.ui.piano.search.to_lowercase();
    let words: Vec<&str> = q.split_whitespace().collect();
    let matches = |hay: String| words.iter().all(|w| hay.contains(w));
    let fav = favs(st).clone();
    let mut out = Vec::new();
    if matches!(filter.as_deref(), None | Some("User") | Some("Favourites")) {
        for (i, (name, p)) in user_presets(st).iter().enumerate() {
            let ok = filter.as_deref() != Some("Favourites") || fav.contains(name);
            if ok && matches(format!("{} user {}", name.to_lowercase(), p.model.name().to_lowercase())) {
                out.push(Entry { name: name.clone(), tag: "USR", kind: "User".into(), lib: None, user: Some(i) });
            }
        }
    }
    if filter.as_deref() != Some("User") {
        for (i, p) in pno_library::library().iter().enumerate() {
            let ok = match filter.as_deref() {
                None => true,
                Some("Favourites") => fav.contains(&p.name),
                Some(c) => p.category.name() == c,
            };
            if ok && matches(format!("{} {} {} {}", p.name.to_lowercase(), p.kind.to_lowercase(), p.category.name().to_lowercase(), p.params.model.name().to_lowercase())) {
                out.push(Entry { name: p.name.clone(), tag: p.category.code(), kind: p.kind.to_string(), lib: Some(i), user: None });
            }
        }
    }
    out
}

fn load_entry(st: &mut State, cid: Id, e: &Entry) {
    let params = if let Some(i) = e.lib {
        pno_library::library()[i].params.clone()
    } else if let Some(i) = e.user {
        let Some((_, p)) = user_presets(st).get(i).cloned() else { return };
        p
    } else {
        return;
    };
    let name = e.name.clone();
    st.session.edit(&format!("Preset: {name}"), |pr| {
        set(pr, cid, |g| {
            *g = params;
            g.preset = name;
        })
    });
}

fn step_preset(st: &mut State, cid: Id, cur: &str, dir: i32) {
    let list = entries(st);
    if list.is_empty() {
        return;
    }
    let n = list.len() as i64;
    let next = if dir == 0 {
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(5);
        (dragon_engine::dsp::Rng::new(seed).next_u64() % n as u64) as usize
    } else {
        match list.iter().position(|e| e.name == cur) {
            Some(i) => (i as i64 + dir as i64).rem_euclid(n) as usize,
            None => 0,
        }
    };
    let e = list[next].clone();
    load_entry(st, cid, &e);
}

// ------------------------------------------------------------------------------------------------ widgets

/// A gold knob with an amber label (the case's knobs).
#[allow(clippy::too_many_arguments)]
fn kn(ui: &mut Ui, st: &mut State, cid: Id, v: f32, min: f32, max: f32, default: f32, label: &str, fmt: impl Fn(f32) -> String, write: impl FnOnce(&mut PianoParams, f32)) {
    let mut x = v;
    let r = knob_colored(ui, &mut x, min, max, default, label, fmt, GOLD);
    if r.hovered() || r.dragged() {
        st.hint(NAME, label, format!("{x:.2}"));
    }
    widgets::apply(st, &r, &format!("{NAME}: {label}"), |pr| set(pr, cid, |g| write(g, x)));
}

/// A label in the case's amber, letter-spaced.
fn case_label(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(12.5).strong().color(AMBER));
}

// ------------------------------------------------------------------------------------------------ the editor

pub fn editor(ui: &mut Ui, st: &mut State, cid: Id, p: PianoParams) {
    let page_id = egui::Id::new(("pno.page", cid));
    let mut page: Page = ui.data(|d| d.get_temp::<u8>(page_id)).map(|t| Page::ALL[t as usize % 4]).unwrap_or_default();
    egui::Frame::NONE.fill(PANEL_BG).corner_radius(6.0).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
        ui.set_width(W);
        ui.vertical(|ui| {
            top_bar(ui, st, cid, &p, &mut page);
            ui.add_space(4.0);
            if st.ui.piano.open {
                browser(ui, st, cid, &p);
            } else {
                match page {
                    Page::Performance => {
                        title(ui);
                        stage(ui, st, cid, &p);
                        case(ui, st, cid, &p);
                    }
                    Page::Arp => arp_page(ui, st, cid, &p),
                    Page::Fx => {
                        fn edit(pr: &mut Project, cid: Id, f: &mut dyn FnMut(&mut Vec<FxSlot>)) {
                            set(pr, cid, |g| f(&mut g.fx));
                        }
                        egui::ScrollArea::vertical().max_height(470.0).show(ui, |ui| super::wave_ui::fx_rack(ui, st, cid, &p.fx, W - 24.0, "pno", edit));
                    }
                    Page::Settings => settings_page(ui, st, cid, &p),
                }
            }
            keys(ui, st, cid, &p);
        });
    });
    ui.data_mut(|d| d.insert_temp(page_id, Page::ALL.iter().position(|x| *x == page).unwrap_or(0) as u8));
}

fn top_bar(ui: &mut Ui, st: &mut State, cid: Id, p: &PianoParams, page: &mut Page) {
    egui::Frame::NONE.fill(Color32::from_rgb(0x1a, 0x1a, 0x1d)).corner_radius(4.0).inner_margin(egui::Margin::symmetric(8, 4)).show(ui, |ui| {
        ui.set_width(W - 28.0);
        ui.horizontal(|ui| {
            for pg in Page::ALL {
                let on = *page == pg && !st.ui.piano.open;
                let r = ui.add(egui::Label::new(RichText::new(pg.name()).size(13.0).strong().color(if on { Color32::WHITE } else { GOLD_DIM })).sense(Sense::click()));
                if on {
                    ui.painter().line_segment([r.rect.left_bottom() + Vec2::new(0.0, 2.0), r.rect.right_bottom() + Vec2::new(0.0, 2.0)], Stroke::new(2.0_f32, LIT));
                }
                if r.clicked() {
                    *page = pg;
                    st.ui.piano.open = false;
                }
                ui.add_space(8.0);
            }
            ui.add_space(10.0);
            let tag = pno_library::find(&p.preset).map(|l| l.category.code()).unwrap_or("USR");
            egui::Frame::NONE.fill(Color32::from_rgb(0x08, 0x08, 0x0a)).corner_radius(12.0).inner_margin(egui::Margin::symmetric(10, 3)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.set_width(360.0);
                    let fav = favs(st).contains(&p.preset);
                    if star(ui, fav).on_hover_text("Favourite").clicked() {
                        toggle_fav(st, &p.preset);
                    }
                    ui.label(RichText::new(tag).small().strong().color(LIT));
                    let r = ui.add(egui::Label::new(RichText::new(&p.preset).size(14.0).color(Color32::WHITE)).truncate().sense(Sense::click()));
                    if r.on_hover_text("Click to browse 2,000 presets").clicked() {
                        st.ui.piano.open = !st.ui.piano.open;
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
            if ui.add(egui::Button::new(RichText::new("Browse").strong()).selected(st.ui.piano.open)).clicked() {
                st.ui.piano.open = !st.ui.piano.open;
            }
            if ui.button("Random").clicked() {
                step_preset(st, cid, &p.preset, 0);
            }
            ui.menu_button("Save", |ui| {
                ui.label(RichText::new("Save as a user preset").small().color(theme::TEXT_DIM));
                if st.ui.piano.save_name.is_empty() {
                    st.ui.piano.save_name = p.preset.clone();
                }
                let mut name = st.ui.piano.save_name.clone();
                let r = ui.add(egui::TextEdit::singleline(&mut name).desired_width(180.0));
                st.ui.piano.save_name = name.clone();
                if ui.button("Save").clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                    save_user_preset(st, &name, p);
                    st.ui.piano.save_name.clear();
                    st.session.edit("Name preset", |pr| set(pr, cid, |g| g.preset = name.clone()));
                    ui.close_menu();
                }
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(NAME.to_uppercase()).size(13.0).strong().color(GOLD));
            });
        });
    });
}

/// The gold title with its ornament.
fn title(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(W - 12.0, 74.0), Sense::hover());
    let ctx = ui.ctx().clone();
    let pa = ui.painter_at(rect);
    if let Some(t) = texture(&ctx, "pno-lacquer", LACQUER_JPG) {
        pa.image(t.id(), rect, Rect::from_min_max(Pos2::new(0.1, 0.25), Pos2::new(0.9, 0.45)), Color32::from_gray(160));
    }
    // Letter-spaced capitals, with a soft shadow.
    let text: String = NAME.to_uppercase().chars().map(|c| format!("{c} ")).collect::<String>().trim_end().to_string();
    let font = egui::FontId::proportional(34.0);
    pa.text(rect.center_top() + Vec2::new(1.5, 24.0), egui::Align2::CENTER_CENTER, &text, font.clone(), Color32::from_black_alpha(200));
    pa.text(rect.center_top() + Vec2::new(0.0, 22.5), egui::Align2::CENTER_CENTER, &text, font, Color32::from_rgb(0xe9, 0xcf, 0x93));
    if let Some(t) = texture(&ctx, "pno-ornament", ORNAMENT_PNG) {
        let w = 230.0;
        let h = w * 156.0 / 640.0;
        let r = Rect::from_center_size(Pos2::new(rect.center().x, rect.top() + 59.0), Vec2::new(w, h * 0.5));
        pa.image(t.id(), r, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
    }
}

/// The painted stage with high damp, pedal, velocity range and the arpeggiator plaque.
fn stage(ui: &mut Ui, st: &mut State, cid: Id, p: &PianoParams) {
    let h = 285.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(W - 12.0, h), Sense::hover());
    let ctx = ui.ctx().clone();
    let pa = ui.painter_at(rect);
    if let Some(t) = texture(&ctx, "pno-scene", SCENE_JPG) {
        // The image is 1344 × 576; show a band through the piano and the dragon.
        let vis = rect.height() / rect.width() * 1344.0 / 576.0;
        let top = 0.2;
        pa.image(t.id(), rect, Rect::from_min_max(Pos2::new(0.0, top), Pos2::new(1.0, top + vis)), Color32::WHITE);
    }
    // Soft vignette at the sides for the controls.
    for (x0, x1, a0, a1) in [(rect.left(), rect.left() + 120.0, 200u8, 0u8), (rect.right() - 120.0, rect.right(), 0, 200)] {
        let mut mesh = egui::Mesh::default();
        let c0 = Color32::from_black_alpha(a0);
        let c1 = Color32::from_black_alpha(a1);
        mesh.colored_vertex(Pos2::new(x0, rect.top()), c0);
        mesh.colored_vertex(Pos2::new(x1, rect.top()), c1);
        mesh.colored_vertex(Pos2::new(x1, rect.bottom()), c1);
        mesh.colored_vertex(Pos2::new(x0, rect.bottom()), c0);
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(0, 2, 3);
        pa.add(egui::Shape::mesh(mesh));
    }
    // Left: high damp and the pedal.
    let mut left = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_size(rect.left_top() + Vec2::new(10.0, 8.0), Vec2::new(96.0, h - 16.0))).layout(egui::Layout::top_down(egui::Align::Center)));
    case_label(&mut left, "HIGH DAMP");
    kn(&mut left, st, cid, p.high_damp, 0.0, 1.0, 0.15, "", widgets::pct, |g, v| g.high_damp = v);
    left.add_space(14.0);
    case_label(&mut left, "PEDAL VOL");
    kn(&mut left, st, cid, p.pedal_noise, 0.0, 1.0, 0.3, "", widgets::pct, |g, v| g.pedal_noise = v);
    left.add_space(4.0);
    // The sustain pedal: hold it with the mouse, or latch it down.
    let (prect, pr) = left.allocate_exact_size(Vec2::new(70.0, 26.0), Sense::click_and_drag());
    let held_id = egui::Id::new(("pno.pedal", cid));
    let was: bool = left.data(|d| d.get_temp(held_id)).unwrap_or(false);
    let down = pr.is_pointer_button_down_on();
    if down != was {
        st.audio.send(Command::Live(LiveEvent::Param { channel: cid, param: "__sustain", value: if down { 1.0 } else { 0.0 } }));
        left.data_mut(|d| d.insert_temp(held_id, down));
    }
    let lit = down || p.pedal_latch;
    let lp = left.painter();
    lp.rect_filled(prect, 5.0, if lit { GOLD } else { Color32::from_gray(35) });
    lp.text(prect.center(), egui::Align2::CENTER_CENTER, "PEDAL", egui::FontId::proportional(11.0), if lit { Color32::BLACK } else { GOLD });
    pr.on_hover_text("Hold for the sustain pedal (your MIDI pedal works too)");
    if pill(&mut left, p.pedal_latch, "Latch").on_hover_text("Keep the pedal down").clicked() {
        st.session.edit("Pedal latch", |pr| set(pr, cid, |g| g.pedal_latch = !p.pedal_latch));
    }
    // Right: the velocity range, a vertical slider with two handles.
    let vr = Rect::from_min_size(Pos2::new(rect.right() - 74.0, rect.top() + 8.0), Vec2::new(64.0, h - 16.0));
    pa.text(vr.center_top() + Vec2::new(0.0, 6.0), egui::Align2::CENTER_CENTER, "VELOCITY", egui::FontId::proportional(12.0), AMBER);
    pa.text(vr.center_top() + Vec2::new(0.0, 20.0), egui::Align2::CENTER_CENTER, "RANGE", egui::FontId::proportional(12.0), AMBER);
    let track = Rect::from_min_max(Pos2::new(vr.center().x - 9.0, vr.top() + 40.0), Pos2::new(vr.center().x + 9.0, vr.bottom() - 18.0));
    pa.rect_filled(track.expand(4.0), 9.0, Color32::from_black_alpha(170));
    pa.rect_filled(track, 4.0, Color32::from_gray(25));
    let y_of = |v: f32| track.bottom() - v * track.height();
    let band = Rect::from_min_max(Pos2::new(track.left() + 4.0, y_of(p.velocity_high)), Pos2::new(track.right() - 4.0, y_of(p.velocity_low)));
    pa.rect_filled(band, 3.0, GOLD.gamma_multiply(0.8));
    for (v, label) in [(p.velocity_high, format!("{:.0}", p.velocity_high * 127.0)), (p.velocity_low, format!("{:.0}", p.velocity_low * 127.0))] {
        let y = y_of(v);
        pa.add(egui::Shape::convex_polygon(vec![Pos2::new(track.right() + 2.0, y), Pos2::new(track.right() + 10.0, y - 5.0), Pos2::new(track.right() + 10.0, y + 5.0)], AMBER, Stroke::NONE));
        pa.text(Pos2::new(track.left() - 6.0, y), egui::Align2::RIGHT_CENTER, label, egui::FontId::proportional(10.0), Color32::WHITE);
    }
    let resp = ui.interact(track.expand2(Vec2::new(14.0, 6.0)), egui::Id::new(("pno.vel", cid)), Sense::click_and_drag());
    if let Some(q) = resp.interact_pointer_pos().filter(|_| resp.dragged() || resp.clicked()) {
        let v = ((track.bottom() - q.y) / track.height()).clamp(0.0, 1.0);
        // Move whichever handle is nearer.
        let high = (v - p.velocity_high).abs() <= (v - p.velocity_low).abs();
        st.session.gesture("Velocity range", |pr| {
            set(pr, cid, |g| {
                if high {
                    g.velocity_high = v.max(g.velocity_low + 0.05);
                } else {
                    g.velocity_low = v.min(g.velocity_high - 0.05);
                }
            })
        });
    }
    if resp.drag_stopped() || resp.clicked() {
        st.session.end_gesture();
    }
    resp.on_hover_text("Drag the handles: the keys' softest and loudest strikes");
    // The arpeggiator plaque.
    let plaque = Rect::from_center_size(Pos2::new(rect.center().x + 40.0, rect.bottom() - 24.0), Vec2::new(210.0, 34.0));
    let pr = ui.interact(plaque, egui::Id::new(("pno.arp", cid)), Sense::click());
    let on = p.arp.on;
    pa.rect_filled(plaque, 14.0, if on { Color32::from_rgb(0x3a, 0x22, 0x10) } else { Color32::from_rgba_unmultiplied(0x12, 0x20, 0x2a, 230) });
    pa.rect_stroke(plaque, 14.0, Stroke::new(1.5_f32, if on { LIT } else { GOLD_DIM }), egui::StrokeKind::Inside);
    pa.text(plaque.center(), egui::Align2::CENTER_CENTER, "Arpeggiator", egui::FontId::proportional(20.0), if on { Color32::from_rgb(0xff, 0xd0, 0x90) } else { Color32::from_rgb(0xc8, 0xc0, 0xa8) });
    if pr.on_hover_text("Turn the arpeggiator on or off (settings on the Arpeggiator page)").clicked() {
        st.session.edit("Arpeggiator", |pr| set(pr, cid, |g| g.arp.on = !on));
    }
    // The model, small at the bottom left of the stage.
    pa.text(Pos2::new(rect.left() + 118.0, rect.bottom() - 12.0), egui::Align2::LEFT_CENTER, p.model.name().to_uppercase(), egui::FontId::proportional(12.0), Color32::from_white_alpha(170));
}

/// A knob on the case: label, value, min, max, default, how it writes, tooltip.
type CaseKnob = (&'static str, f32, f32, f32, f32, fn(&mut PianoParams, f32), &'static str);

/// The black lacquer case with the main knobs.
fn case(ui: &mut Ui, st: &mut State, cid: Id, p: &PianoParams) {
    let h = 112.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(W - 12.0, h), Sense::hover());
    let ctx = ui.ctx().clone();
    let pa = ui.painter_at(rect);
    if let Some(t) = texture(&ctx, "pno-lacquer", LACQUER_JPG) {
        pa.image(t.id(), rect, Rect::from_min_max(Pos2::new(0.0, 0.05), Pos2::new(1.0, 0.33)), Color32::WHITE);
    } else {
        pa.rect_filled(rect, 4.0, Color32::from_gray(10));
    }
    let electric = !p.model.strings();
    let knobs: Vec<CaseKnob> = if electric {
        vec![
            ("TONE", p.tone, 0.0, 1.0, 0.5, |g, v| g.tone = v, "Hammer hardness: darker … brighter"),
            ("BARK", p.drive, 0.0, 1.0, 0.25, |g, v| g.drive = v, "How much the pickup growls when hit hard"),
            ("TREMOLO", p.tremolo_depth, 0.0, 1.0, 0.3, |g, v| g.tremolo_depth = v, "Stereo tremolo depth"),
            ("SPEED", p.tremolo_rate, 0.5, 10.0, 4.5, |g, v| g.tremolo_rate = v, "Tremolo speed (Hz)"),
            ("RELEASE", p.release, 0.02, 1.5, 0.12, |g, v| g.release = v, "How quickly a note stops when let go"),
            ("SUSTAIN", p.sustain, 0.2, 2.5, 1.0, |g, v| g.sustain = v, "How long notes ring"),
            ("BODY", p.body, 0.0, 1.0, 0.6, |g, v| g.body = v, "Case resonance"),
            ("WIDTH", p.width, 0.0, 1.0, 0.7, |g, v| g.width = v, "Stereo spread across the keys"),
        ]
    } else {
        vec![
            ("TONE", p.tone, 0.0, 1.0, 0.55, |g, v| g.tone = v, "Hammer hardness: felt … bright"),
            ("BODY", p.body, 0.0, 1.0, 0.6, |g, v| g.body = v, "Soundboard and case resonance"),
            ("HAMMER", p.hammer, 0.0, 1.0, 0.4, |g, v| g.hammer = v, "The hammer's knock and felt noise"),
            ("RELEASE", p.release, 0.02, 1.5, 0.18, |g, v| g.release = v, "How quickly the dampers stop a note"),
            ("REL. VOL", p.release_noise, 0.0, 1.0, 0.3, |g, v| g.release_noise = v, "Damper and key-release noise"),
            ("TIGHTNESS", p.tightness, 0.0, 1.0, 0.8, |g, v| g.tightness = v, "How closely each key's strings agree (low = honky-tonk)"),
            ("RESONANCE", p.resonance, 0.0, 1.0, 0.35, |g, v| g.resonance = v, "Sympathetic resonance of the other strings (more with the pedal down)"),
            ("SUSTAIN", p.sustain, 0.2, 2.5, 1.0, |g, v| g.sustain = v, "How long the strings ring"),
        ]
    };
    let n = knobs.len() as f32;
    let span = rect.width() - 120.0;
    for (i, (label, v, min, max, def, write, tip)) in knobs.into_iter().enumerate() {
        let cx = rect.left() + 60.0 + span * (i as f32 + 0.5) / n;
        let cell = Rect::from_center_size(Pos2::new(cx, rect.top() + 62.0), Vec2::new(96.0, 84.0));
        let mut cu = ui.new_child(egui::UiBuilder::new().max_rect(cell).layout(egui::Layout::top_down(egui::Align::Center)));
        case_label(&mut cu, label);
        let mut x = v;
        let r = knob_colored(&mut cu, &mut x, min, max, def, "", move |v| if max > 2.0 { format!("{v:.2}") } else { format!("{:.0}%", (v - min) / (max - min) * 100.0) }, GOLD);
        if r.hovered() || r.dragged() {
            st.hint(NAME, label, format!("{x:.2}"));
        }
        widgets::apply(st, &r, &format!("{NAME}: {label}"), |pr| set(pr, cid, |g| write(g, x)));
        r.on_hover_text(tip);
    }
}

// ------------------------------------------------------------------------------------------------ pages

fn arp_page(ui: &mut Ui, st: &mut State, cid: Id, p: &PianoParams) {
    ui.set_min_height(440.0);
    let a = p.arp.clone();
    ui.horizontal(|ui| {
        group_label(ui, "ARPEGGIATOR");
        if pill(ui, a.on, if a.on { "On" } else { "Off" }).clicked() {
            st.session.edit("Arpeggiator", |pr| set(pr, cid, |g| g.arp.on = !a.on));
        }
    });
    ui.label(RichText::new("Hold a chord and the arpeggiator plays its notes in turn, in time with the song.").small().color(theme::TEXT_DIM));
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        group_label(ui, "PATTERN");
        for m in ArpMode::ALL {
            if pill(ui, a.mode == m, m.name()).clicked() {
                st.session.edit("Arp pattern", |pr| set(pr, cid, |g| g.arp.mode = m));
            }
        }
    });
    ui.horizontal(|ui| {
        group_label(ui, "RATE");
        for (beats, name) in ARP_RATES {
            if pill(ui, (a.rate - beats).abs() < 1e-3, name).clicked() {
                st.session.edit("Arp rate", |pr| set(pr, cid, |g| g.arp.rate = beats));
            }
        }
    });
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        kn(ui, st, cid, a.octaves as f32, 1.0, 4.0, 2.0, "OCTAVES", |v| format!("{:.0}", v.round()), |g, v| g.arp.octaves = v.round() as u8);
        kn(ui, st, cid, a.gate, 0.05, 1.0, 0.8, "GATE", widgets::pct, |g, v| g.arp.gate = v);
        kn(ui, st, cid, a.swing, 0.0, 0.5, 0.0, "SWING", widgets::pct, |g, v| g.arp.swing = v);
        kn(ui, st, cid, a.accent, 0.0, 1.0, 0.3, "ACCENT", widgets::pct, |g, v| g.arp.accent = v);
    });
    // A picture of the pattern on a C major chord over the octaves.
    ui.add_space(8.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(W - 24.0, 150.0), Sense::hover());
    let pa = ui.painter_at(rect);
    pa.rect_filled(rect, 4.0, Color32::from_gray(14));
    let chord = [60u8, 64, 67];
    let mut notes: Vec<u8> = (0..a.octaves.clamp(1, 4)).flat_map(|o| chord.iter().map(move |n| n + 12 * o)).collect();
    notes.sort();
    let len = notes.len();
    let steps = 16;
    let lo = notes[0] as f32;
    let hi = *notes.last().unwrap_or(&72) as f32 + 1.0;
    for s in 0..steps {
        let picks: Vec<u8> = match a.mode {
            ArpMode::Up | ArpMode::AsPlayed => vec![notes[s % len]],
            ArpMode::Down => vec![notes[len - 1 - s % len]],
            ArpMode::UpDown => {
                let cycle = (2 * len).saturating_sub(2).max(1);
                let k = s % cycle;
                vec![notes[if k < len { k } else { cycle - k }]]
            }
            ArpMode::Random => vec![notes[(s * 7 + 3) % len]],
            ArpMode::Chord => chord.to_vec(),
        };
        let x = rect.left() + 10.0 + s as f32 * (rect.width() - 20.0) / steps as f32;
        let w = (rect.width() - 20.0) / steps as f32 * a.gate.clamp(0.05, 1.0) - 2.0;
        for n in picks {
            let y = rect.bottom() - 10.0 - (n as f32 - lo) / (hi - lo) * (rect.height() - 20.0);
            pa.rect_filled(Rect::from_min_size(Pos2::new(x, y - 5.0), Vec2::new(w.max(3.0), 9.0)), 2.0, if a.on { LIT } else { GOLD_DIM });
        }
    }
}

fn settings_page(ui: &mut Ui, st: &mut State, cid: Id, p: &PianoParams) {
    ui.set_min_height(440.0);
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            group_label(ui, "INSTRUMENT");
            egui::Grid::new(("pno.models", cid)).spacing([6.0, 6.0]).show(ui, |ui| {
                for (k, m) in PianoModel::ALL.into_iter().enumerate() {
                    if pill(ui, p.model == m, m.name()).clicked() && p.model != m {
                        st.session.edit("Instrument", |pr| {
                            set(pr, cid, |g| {
                                let mut n = PianoParams::for_model(m);
                                n.arp = g.arp.clone();
                                n.fx = g.fx.clone();
                                n.preset = format!("{} (edited)", m.name());
                                *g = n;
                            })
                        });
                    }
                    if k % 2 == 1 {
                        ui.end_row();
                    }
                }
            });
            ui.add_space(8.0);
            group_label(ui, "PRESET");
            ui.label(RichText::new(&p.preset).strong());
            if let Some(l) = pno_library::find(&p.preset) {
                ui.label(format!("{} · {}", l.category.name(), l.kind));
            }
            ui.label(RichText::new(format!("{} factory presets · your presets live in {}", pno_library::library().len(), user_dir(st).display())).small().color(theme::TEXT_DIM));
            if ui.button("Open user preset folder").clicked() {
                let d = user_dir(st);
                let _ = std::fs::create_dir_all(&d);
                crate::chains::reveal(&d);
            }
        });
        ui.add_space(24.0);
        ui.vertical(|ui| {
            group_label(ui, "STRINGS & TUNING");
            ui.horizontal(|ui| {
                kn(ui, st, cid, p.stiffness, 0.0, 1.0, 0.5, "STIFFNESS", widgets::pct, |g, v| g.stiffness = v);
                kn(ui, st, cid, p.stretch, 0.0, 1.0, 0.6, "STRETCH", widgets::pct, |g, v| g.stretch = v);
                kn(ui, st, cid, p.tightness, 0.0, 1.0, 0.8, "TIGHTNESS", widgets::pct, |g, v| g.tightness = v);
                kn(ui, st, cid, p.sustain, 0.2, 2.5, 1.0, "SUSTAIN", |v| format!("× {v:.2}"), |g, v| g.sustain = v);
            });
            ui.label(RichText::new("Stiffness stretches the upper partials (a piano's shimmer). Stretch tuning sets the bass a little flat and the treble sharp, as piano tuners do.").small().color(theme::TEXT_DIM));
            ui.add_space(6.0);
            group_label(ui, "PLAYING");
            ui.horizontal(|ui| {
                kn(ui, st, cid, p.dynamics, 0.0, 1.0, 0.75, "DYNAMICS", widgets::pct, |g, v| g.dynamics = v);
                kn(ui, st, cid, p.swell, 0.0, 3.0, 0.0, "SWELL", |v| if v < 0.005 { "off".into() } else { format!("{v:.2} s") }, |g, v| g.swell = v);
                kn(ui, st, cid, p.width, 0.0, 1.0, 0.7, "WIDTH", widgets::pct, |g, v| g.width = v);
                kn(ui, st, cid, p.master, 0.0, 1.5, 0.8, "MAIN", widgets::db_fmt, |g, v| g.master = v);
                kn(ui, st, cid, p.trim_db, -36.0, 36.0, 0.0, "TRIM", |v| format!("{v:+.1} dB"), |g, v| g.trim_db = v);
            });
            ui.label(RichText::new("Pedals: CC64 sustain, CC66 sostenuto, CC67 soft (una corda) from your MIDI keyboard.").small().color(theme::TEXT_DIM));
        });
    });
}

fn browser(ui: &mut Ui, st: &mut State, cid: Id, p: &PianoParams) {
    let h = 440.0;
    ui.horizontal_top(|ui| {
        egui::Frame::NONE.fill(Color32::from_rgb(0x18, 0x18, 0x1b)).corner_radius(4.0).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
            ui.set_width(190.0);
            ui.set_min_height(h);
            ui.vertical(|ui| {
                group_label(ui, "PRESETS");
                let lib = pno_library::library();
                let nfav = favs(st).len();
                let nuser = user_presets(st).len();
                let mut rows: Vec<(Option<String>, String)> =
                    vec![(None, format!("All  ({})", lib.len() + nuser)), (Some("Favourites".into()), format!("Favourites  ({nfav})")), (Some("User".into()), format!("User  ({nuser})"))];
                for c in Category::ALL {
                    let n = lib.iter().filter(|p| p.category == c).count();
                    rows.push((Some(c.name().to_string()), format!("{}  ({n})", c.name())));
                }
                for (f, label) in rows {
                    let on = st.ui.piano.filter == f;
                    if ui.add_sized([176.0, 20.0], egui::Button::new(RichText::new(label).color(if on { Color32::BLACK } else { theme::TEXT })).fill(if on { GOLD } else { Color32::from_rgb(0x18, 0x18, 0x1b) })).clicked()
                    {
                        st.ui.piano.filter = f;
                    }
                }
            });
        });
        egui::Frame::NONE.fill(Color32::from_rgb(0x18, 0x18, 0x1b)).corner_radius(4.0).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
            ui.set_width(W - 230.0);
            ui.set_min_height(h);
            ui.vertical(|ui| {
                let list = entries(st);
                ui.horizontal(|ui| {
                    let mut q = st.ui.piano.search.clone();
                    if ui.add(egui::TextEdit::singleline(&mut q).hint_text("Search: name, style or instrument (e.g. \"felt\", \"lo-fi\", \"tine\")").desired_width(380.0)).changed() {
                        st.ui.piano.search = q;
                    }
                    if !st.ui.piano.search.is_empty() && ui.small_button("×").clicked() {
                        st.ui.piano.search.clear();
                    }
                    ui.label(RichText::new(format!("{} presets", list.len())).small().color(theme::TEXT_DIM));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Close").clicked() {
                            st.ui.piano.open = false;
                        }
                        if ui.button("Random").clicked() {
                            step_preset(st, cid, &p.preset, 0);
                        }
                    });
                });
                ui.label(RichText::new("Click to load and play it on the keys below; double-click to load and close.").small().color(theme::TEXT_DIM));
                let fav = favs(st).clone();
                let row_h = 20.0;
                egui::ScrollArea::vertical().id_salt("pno.list").max_height(h - 50.0).auto_shrink([false, false]).show_rows(ui, row_h, list.len(), |ui, range| {
                    for e in &list[range] {
                        let cur = e.name == p.preset;
                        ui.horizontal(|ui| {
                            if star(ui, fav.contains(&e.name)).clicked() {
                                toggle_fav(st, &e.name);
                            }
                            let (rect, r) = ui.allocate_exact_size(Vec2::new(W - 290.0, row_h - 2.0), Sense::click());
                            let pa = ui.painter();
                            if cur {
                                pa.rect_filled(rect, 3.0, GOLD);
                            } else if r.hovered() {
                                pa.rect_filled(rect, 3.0, Color32::from_gray(40));
                            }
                            let y = rect.center().y;
                            pa.text(Pos2::new(rect.left() + 6.0, y), egui::Align2::LEFT_CENTER, e.tag, egui::FontId::monospace(11.0), if cur { Color32::BLACK } else { LIT });
                            pa.text(Pos2::new(rect.left() + 42.0, y), egui::Align2::LEFT_CENTER, &e.name, egui::FontId::proportional(13.5), if cur { Color32::BLACK } else { theme::TEXT });
                            pa.text(Pos2::new(rect.left() + 360.0, y), egui::Align2::LEFT_CENTER, &e.kind, egui::FontId::proportional(11.5), if cur { Color32::from_gray(40) } else { theme::TEXT_DIM });
                            if r.clicked() {
                                load_entry(st, cid, e);
                            }
                            if r.double_clicked() {
                                st.ui.piano.open = false;
                            }
                        });
                    }
                });
            });
        });
    });
}

// ------------------------------------------------------------------------------------------------ keys

fn is_black(n: u8) -> bool {
    matches!(n % 12, 1 | 3 | 6 | 8 | 10)
}

/// 88 keys (A0 – C8) set in the lacquer case; the model's range bright, the rest dimmed. Click to play.
fn keys(ui: &mut Ui, st: &mut State, cid: Id, p: &PianoParams) {
    let held_id = egui::Id::new(("pno.kbd", cid));
    let held: Option<u8> = ui.data(|d| d.get_temp(held_id)).flatten();
    let (lo, hi) = p.model.range();
    let (rect, r) = ui.allocate_exact_size(Vec2::new(W - 12.0, 92.0), Sense::click_and_drag());
    let ctx = ui.ctx().clone();
    let pa = ui.painter_at(rect);
    if let Some(t) = texture(&ctx, "pno-lacquer", LACQUER_JPG) {
        pa.image(t.id(), rect, Rect::from_min_max(Pos2::new(0.0, 0.75), Pos2::new(1.0, 0.95)), Color32::WHITE);
    } else {
        pa.rect_filled(rect, 4.0, Color32::from_gray(8));
    }
    let (k0, k1) = (21u8, 108u8);
    let whites: Vec<u8> = (k0..=k1).filter(|n| !is_black(*n)).collect();
    let kb = Rect::from_min_max(rect.left_top() + Vec2::new(12.0, 10.0), rect.right_bottom() - Vec2::new(12.0, 4.0));
    let ww = kb.width() / whites.len() as f32;
    let mut whites_r: Vec<(u8, Rect)> = Vec::new();
    let mut blacks_r: Vec<(u8, Rect)> = Vec::new();
    for (i, n) in whites.iter().enumerate() {
        whites_r.push((*n, Rect::from_min_size(Pos2::new(kb.left() + i as f32 * ww, kb.top()), Vec2::new(ww - 1.0, kb.height()))));
    }
    for n in k0..=k1 {
        if is_black(n) {
            let lw = whites.iter().position(|w| *w == n - 1).unwrap_or(0);
            let x = kb.left() + (lw + 1) as f32 * ww - ww * 0.32;
            blacks_r.push((n, Rect::from_min_size(Pos2::new(x, kb.top()), Vec2::new(ww * 0.64, kb.height() * 0.62))));
        }
    }
    // The red felt strip behind the keys.
    pa.rect_filled(Rect::from_min_max(Pos2::new(kb.left(), kb.top() - 4.0), Pos2::new(kb.right(), kb.top())), 0.0, Color32::from_rgb(0x6a, 0x12, 0x14));
    for (n, kr) in &whites_r {
        let inr = *n >= lo && *n <= hi;
        let col = if held == Some(*n) {
            GOLD
        } else if inr {
            Color32::from_rgb(0xf4, 0xf1, 0xe8)
        } else {
            Color32::from_gray(120)
        };
        pa.rect_filled(*kr, 2.0, col);
        // A shade at the bottom of each key.
        pa.rect_filled(Rect::from_min_max(Pos2::new(kr.left(), kr.bottom() - 5.0), kr.max), 2.0, Color32::from_black_alpha(40));
        if n % 12 == 0 {
            pa.text(kr.center_bottom() - Vec2::new(0.0, 8.0), egui::Align2::CENTER_BOTTOM, format!("C{}", *n as i32 / 12 - 1), egui::FontId::proportional(8.5), Color32::from_gray(110));
        }
    }
    for (n, kr) in &blacks_r {
        let col = if held == Some(*n) { GOLD } else { Color32::from_rgb(0x12, 0x12, 0x14) };
        pa.rect_filled(*kr, 2.0, col);
        pa.rect_filled(Rect::from_min_max(kr.min + Vec2::new(1.5, 1.0), Pos2::new(kr.max.x - 1.5, kr.min.y + kr.height() * 0.85)), 1.5, if held == Some(*n) { GOLD } else { Color32::from_rgb(0x2a, 0x2a, 0x2e) });
    }
    let at = |q: Pos2| blacks_r.iter().chain(whites_r.iter()).find(|(_, kr)| kr.contains(q)).map(|(n, _)| *n);
    let pointer = ui.ctx().pointer_latest_pos();
    let down = r.is_pointer_button_down_on() || r.dragged();
    let want = if down { pointer.and_then(at) } else { None };
    if held.is_some() || want.is_some() {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(30));
    }
    if want != held {
        if let Some(n) = held {
            st.audio.send(Command::Live(LiveEvent::NoteOff { channel: cid, note: n }));
        }
        if let Some(n) = want {
            let vel = pointer.map(|q| (0.3 + 0.7 * (q.y - kb.top()) / kb.height()).clamp(0.1, 1.0)).unwrap_or(0.8);
            st.audio.send(Command::Live(LiveEvent::NoteOn { channel: cid, note: n, velocity: vel }));
        }
        ui.data_mut(|d| d.insert_temp(held_id, want));
    }
    r.on_hover_text("Click to play – lower on a key is louder");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_artwork_decodes() {
        for b in [SCENE_JPG, LACQUER_JPG, ORNAMENT_PNG] {
            assert!(image::load_from_memory(b).is_ok());
        }
    }

    #[test]
    fn the_editor_draws_every_page_and_the_browser_steps_presets() {
        let mut st = State::new(redhive_client_framework::AppContext::new("en"), crate::state::Settings { show_splash: false, ..Default::default() });
        st.new_project();
        let cid = st.session.edit("x", |p| p.add_channel("Piano", Instrument::Piano(Box::new(pno_library::library()[0].params.clone()))));
        let ctx = egui::Context::default();
        let draw = |st: &mut State, page: u8| {
            let _ = ctx.run(egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1400.0, 1000.0))), ..Default::default() }, |ctx| {
                ctx.data_mut(|d| d.insert_temp(egui::Id::new(("pno.page", cid)), page));
                egui::CentralPanel::default().show(ctx, |ui| {
                    let Some(Instrument::Piano(p)) = st.session.project.channel(cid).map(|c| c.instrument.clone()) else { panic!() };
                    editor(ui, st, cid, *p);
                });
            });
        };
        for page in 0..4u8 {
            draw(&mut st, page);
        }
        st.ui.piano.open = true;
        draw(&mut st, 0);
        let cur = |st: &State| match st.session.project.channel(cid).map(|c| c.instrument.clone()) {
            Some(Instrument::Piano(p)) => p.preset,
            _ => panic!(),
        };
        st.ui.piano.filter = Some("Felt & Intimate".into());
        step_preset(&mut st, cid, "", 1);
        let first = cur(&st);
        assert_eq!(pno_library::find(&first).unwrap().category, Category::Felt);
        step_preset(&mut st, cid, &first, 1);
        assert_ne!(cur(&st), first);
        st.ui.piano.filter = None;
        st.ui.piano.search = "lo-fi".into();
        assert!(entries(&mut st).len() >= 50);
    }
}
