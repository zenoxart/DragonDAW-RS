//! The Dragon Strings editor: a guitar laid across the top (headstock, a painted neck with the
//! notes lighting up where they are played, the body), a leather control strip of knobs, and a
//! keyboard showing the key switches and the instrument's range. Pages: Main, Strummer, FX, Settings;
//! a preset browser for the 2,000-preset library.
//! Artwork (body, headstock, fretboard wood, leather) was generated with Higgsfield.

use super::widgets::{self, knob_colored};
use crate::state::State;
use crate::theme;
use dragon_engine::device::Command;
use dragon_engine::gtr::*;
use dragon_engine::gtr_library::{self, Category};
use dragon_engine::model::{Id, Instrument, Project};
use dragon_engine::render::LiveEvent;
use dragon_engine::wt::FxSlot;
use egui::{Color32, Pos2, Rect, RichText, Sense, Stroke, TextureHandle, Ui, Vec2};
use std::collections::BTreeSet;
use std::path::PathBuf;

const W: f32 = 1180.0;
const GOLD: Color32 = Color32::from_rgb(0xd9, 0xb7, 0x6e);
const GOLD_DIM: Color32 = Color32::from_rgb(0x8c, 0x76, 0x4a);
const LIT: Color32 = Color32::from_rgb(0xff, 0x9a, 0x3c);
const PANEL_BG: Color32 = Color32::from_rgb(0x10, 0x10, 0x12);

const BODY_PNG: &[u8] = include_bytes!("../../assets/guitar/body.png");
const EBODY_PNG: &[u8] = include_bytes!("../../assets/guitar/ebody.png");
const HEAD_PNG: &[u8] = include_bytes!("../../assets/guitar/head.png");
const BOARD_JPG: &[u8] = include_bytes!("../../assets/guitar/board.jpg");
const LEATHER_JPG: &[u8] = include_bytes!("../../assets/guitar/leather.jpg");

// Measured on the artwork (pixels of the full images).
/// A body picture: its size, the fretboard stub's top and bottom rows, and the six strings (high e first).
struct BodyArt {
    name: &'static str,
    bytes: &'static [u8],
    size: Vec2,
    board: (f32, f32),
    strings: [f32; 6],
}

const ACOUSTIC_BODY: BodyArt = BodyArt { name: "body", bytes: BODY_PNG, size: Vec2::new(752.0, 861.0), board: (350.0, 491.0), strings: [362.0, 386.0, 409.0, 433.0, 457.0, 481.0] };
const ELECTRIC_BODY: BodyArt = BodyArt { name: "ebody", bytes: EBODY_PNG, size: Vec2::new(752.0, 960.0), board: (419.0, 567.0), strings: [429.0, 454.0, 479.0, 505.0, 530.0, 555.0] };
/// Headstock image: the nut's right edge and its top and bottom.
const HEAD_SIZE: Vec2 = Vec2::new(620.0, 376.0);
const HEAD_NUT: (f32, f32, f32) = (612.0, 107.5, 260.5);

fn set(p: &mut Project, cid: Id, f: impl FnOnce(&mut GuitarParams)) {
    if let Some(Instrument::Guitar(g)) = p.channel_mut(cid).map(|c| &mut c.instrument) {
        f(g);
    }
}

fn texture(ctx: &egui::Context, name: &'static str, bytes: &[u8]) -> Option<TextureHandle> {
    let id = egui::Id::new(("guitar-art", name));
    if let Some(t) = ctx.data(|d| d.get_temp::<TextureHandle>(id)) {
        return Some(t);
    }
    let img = image::load_from_memory(bytes).ok()?.to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    let tex = ctx.load_texture(name, egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw()), egui::TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, tex.clone()));
    Some(tex)
}

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
enum Page {
    #[default]
    Main,
    Strummer,
    Fx,
    Settings,
}

impl Page {
    const ALL: [Page; 4] = [Page::Main, Page::Strummer, Page::Fx, Page::Settings];
    fn name(self) -> &'static str {
        match self {
            Page::Main => "Main",
            Page::Strummer => "Strummer",
            Page::Fx => "FX",
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
    user: Option<Vec<(String, GuitarParams)>>,
}

const FAV_FILE: &str = "dragon-strings-favourites.json";

fn user_dir(st: &State) -> PathBuf {
    st.ctx.paths.config.join("Dragon Strings Presets")
}

fn favs(st: &mut State) -> &mut BTreeSet<String> {
    if st.ui.guitar.favs.is_none() {
        let f: Vec<String> = st.ctx.paths.load_json(FAV_FILE).unwrap_or_default();
        st.ui.guitar.favs = Some(f.into_iter().collect());
    }
    st.ui.guitar.favs.get_or_insert_with(Default::default)
}

fn toggle_fav(st: &mut State, name: &str) {
    let f = favs(st);
    if !f.remove(name) {
        f.insert(name.to_string());
    }
    let list: Vec<String> = f.iter().cloned().collect();
    st.ctx.paths.save_json(FAV_FILE, &list);
}

fn user_presets(st: &mut State) -> &Vec<(String, GuitarParams)> {
    if st.ui.guitar.user.is_none() {
        let dir = user_dir(st);
        let mut v: Vec<(String, GuitarParams)> = std::fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
            .filter_map(|e| Some((e.path().file_stem()?.to_string_lossy().to_string(), st.ctx.paths.load_from(&e.path())?)))
            .collect();
        v.sort_by_key(|a| a.0.to_lowercase());
        st.ui.guitar.user = Some(v);
    }
    st.ui.guitar.user.get_or_insert_with(Default::default)
}

fn save_user_preset(st: &mut State, name: &str, p: &GuitarParams) {
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
    st.ui.guitar.user = None;
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
    let filter = st.ui.guitar.filter.clone();
    let q = st.ui.guitar.search.to_lowercase();
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
        for (i, p) in gtr_library::library().iter().enumerate() {
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
        gtr_library::library()[i].params.clone()
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
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(9);
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

// ------------------------------------------------------------------------------------------------ small widgets

/// A gold knob writing one value.
#[allow(clippy::too_many_arguments)]
fn kn(ui: &mut Ui, st: &mut State, cid: Id, v: f32, min: f32, max: f32, default: f32, label: &str, fmt: impl Fn(f32) -> String, write: impl FnOnce(&mut GuitarParams, f32)) {
    let mut x = v;
    let r = knob_colored(ui, &mut x, min, max, default, label, fmt, GOLD);
    if r.hovered() || r.dragged() {
        st.hint("Dragon Strings", label, format!("{x:.2}"));
    }
    widgets::apply(st, &r, &format!("Dragon Strings: {label}"), |pr| set(pr, cid, |g| write(g, x)));
}

fn group_label(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(10.5).strong().color(GOLD_DIM));
}

/// A small pill that is lit when `on`.
fn pill(ui: &mut Ui, on: bool, text: &str) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(text.to_string(), egui::FontId::proportional(11.0), Color32::WHITE);
    let size = Vec2::new(galley.size().x + 14.0, 18.0);
    let (rect, r) = ui.allocate_exact_size(size, Sense::click());
    let pa = ui.painter();
    pa.rect_filled(
        rect,
        9.0,
        if on {
            LIT.gamma_multiply(0.9)
        } else if r.hovered() {
            Color32::from_gray(48)
        } else {
            Color32::from_gray(30)
        },
    );
    pa.circle_filled(Pos2::new(rect.left() + 7.0, rect.center().y), 2.5, if on { Color32::WHITE } else { Color32::from_gray(80) });
    pa.text(Pos2::new(rect.left() + 12.0, rect.center().y), egui::Align2::LEFT_CENTER, text, egui::FontId::proportional(11.0), if on { Color32::BLACK } else { theme::TEXT });
    r
}

fn arrow_button(ui: &mut Ui, left: bool) -> egui::Response {
    let (rect, r) = ui.allocate_exact_size(Vec2::new(18.0, 18.0), Sense::click());
    let col = if r.hovered() { Color32::WHITE } else { GOLD_DIM };
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
            c + Vec2::new(a.cos(), a.sin()) * if i % 2 == 0 { 6.5 } else { 2.8 }
        })
        .collect();
    let col = if on {
        GOLD
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

pub fn editor(ui: &mut Ui, st: &mut State, cid: Id, p: GuitarParams) {
    let page_id = egui::Id::new(("gtr.page", cid));
    let mut page: Page = ui.data(|d| d.get_temp::<u8>(page_id)).map(|t| Page::ALL[t as usize % 4]).unwrap_or_default();
    egui::Frame::NONE.fill(PANEL_BG).corner_radius(6.0).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
        ui.set_width(W);
        ui.vertical(|ui| {
            top_bar(ui, st, cid, &p, &mut page);
            ui.add_space(4.0);
            if st.ui.guitar.open {
                browser(ui, st, cid, &p);
            } else {
                match page {
                    Page::Main => {
                        instrument_view(ui, st, cid, &p);
                        controls(ui, st, cid, &p);
                    }
                    Page::Strummer => strummer_page(ui, st, cid, &p),
                    Page::Fx => {
                        fn edit(pr: &mut Project, cid: Id, f: &mut dyn FnMut(&mut Vec<FxSlot>)) {
                            set(pr, cid, |g| f(&mut g.fx));
                        }
                        egui::ScrollArea::vertical().max_height(430.0).show(ui, |ui| super::wave_ui::fx_rack(ui, st, cid, &p.fx, W - 24.0, "gtr", edit));
                    }
                    Page::Settings => settings_page(ui, st, cid, &p),
                }
            }
            ui.add_space(4.0);
            keyboard(ui, st, cid, &p);
        });
    });
    ui.data_mut(|d| d.insert_temp(page_id, Page::ALL.iter().position(|x| *x == page).unwrap_or(0) as u8));
}

fn top_bar(ui: &mut Ui, st: &mut State, cid: Id, p: &GuitarParams, page: &mut Page) {
    egui::Frame::NONE.fill(Color32::from_rgb(0x1a, 0x1a, 0x1d)).corner_radius(4.0).inner_margin(egui::Margin::symmetric(8, 4)).show(ui, |ui| {
        ui.set_width(W - 28.0);
        ui.horizontal(|ui| {
            for pg in Page::ALL {
                let on = *page == pg && !st.ui.guitar.open;
                let r = ui.add(egui::Label::new(RichText::new(pg.name()).size(13.0).strong().color(if on { Color32::WHITE } else { GOLD_DIM })).sense(Sense::click()));
                if on {
                    ui.painter().line_segment([r.rect.left_bottom() + Vec2::new(0.0, 2.0), r.rect.right_bottom() + Vec2::new(0.0, 2.0)], Stroke::new(2.0_f32, LIT));
                }
                if r.clicked() {
                    *page = pg;
                    st.ui.guitar.open = false;
                }
                ui.add_space(8.0);
            }
            ui.add_space(30.0);
            // The preset display.
            let tag = gtr_library::find(&p.preset).map(|l| l.category.code()).unwrap_or("USR");
            egui::Frame::NONE.fill(Color32::from_rgb(0x08, 0x08, 0x0a)).corner_radius(12.0).inner_margin(egui::Margin::symmetric(10, 3)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.set_width(380.0);
                    let fav = favs(st).contains(&p.preset);
                    if star(ui, fav).on_hover_text("Favourite").clicked() {
                        toggle_fav(st, &p.preset);
                    }
                    ui.label(RichText::new(tag).small().strong().color(LIT));
                    let r = ui.add(egui::Label::new(RichText::new(&p.preset).size(14.0).color(Color32::WHITE)).truncate().sense(Sense::click()));
                    if r.on_hover_text("Click to browse 2,000 presets").clicked() {
                        st.ui.guitar.open = !st.ui.guitar.open;
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
            if ui.add(egui::Button::new(RichText::new("Browse").strong()).selected(st.ui.guitar.open)).clicked() {
                st.ui.guitar.open = !st.ui.guitar.open;
            }
            if ui.button("Random").clicked() {
                step_preset(st, cid, &p.preset, 0);
            }
            ui.menu_button("Save", |ui| {
                ui.label(RichText::new("Save as a user preset").small().color(theme::TEXT_DIM));
                if st.ui.guitar.save_name.is_empty() {
                    st.ui.guitar.save_name = p.preset.clone();
                }
                let mut name = st.ui.guitar.save_name.clone();
                let r = ui.add(egui::TextEdit::singleline(&mut name).desired_width(180.0));
                st.ui.guitar.save_name = name.clone();
                if ui.button("Save").clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                    save_user_preset(st, &name, p);
                    st.ui.guitar.save_name.clear();
                    st.session.edit("Name preset", |pr| set(pr, cid, |g| g.preset = name.clone()));
                    ui.close_menu();
                }
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new("DRAGON STRINGS").size(13.0).strong().color(GOLD));
            });
        });
    });
}

// ------------------------------------------------------------------------------------------------ the guitar

/// Where fret `f` sits, from the nut (`x0`) with the twelfth fret at half the scale length.
fn fret_x(x0: f32, scale: f32, f: f32) -> f32 {
    x0 + scale * (1.0 - 2f32.powf(-f / 12.0))
}

fn instrument_view(ui: &mut Ui, st: &mut State, cid: Id, p: &GuitarParams) {
    let h = 260.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(W - 12.0, h), Sense::hover());
    let pa = ui.painter_at(rect);
    let ctx = ui.ctx().clone();
    // Leather behind everything, darkened.
    if let Some(t) = texture(&ctx, "leather", LEATHER_JPG) {
        pa.image(t.id(), rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, rect.height() / rect.width() * 1024.0 / 573.0)), Color32::from_gray(150));
    } else {
        pa.rect_filled(rect, 4.0, Color32::from_gray(18));
    }
    let open = p.open_strings();
    let n = open.len().clamp(1, 8);
    // The fret each string plays, from the engine.
    let frets = st.audio.meters().snapshot().frets.get(&cid).copied().unwrap_or([-1; 8]);
    let cy = rect.top() + 132.0;
    // Body: scaled so its fretboard stub is the neck's width where they meet.
    let art = if matches!(p.model, GuitarModel::Electric | GuitarModel::Bass) { &ELECTRIC_BODY } else { &ACOUSTIC_BODY };
    let joint_w = 64.0;
    let bs = joint_w / (art.board.1 - art.board.0);
    let body_size = art.size * bs;
    let board_mid = (art.board.0 + art.board.1) / 2.0 * bs;
    let body_rect = Rect::from_min_size(Pos2::new(rect.right() - body_size.x + 40.0, cy - board_mid), body_size);
    let joint_x = body_rect.left() + 2.0;
    // Headstock: scaled so its nut matches the neck at the nut.
    let nut_w = 54.0;
    let hs = nut_w / (HEAD_NUT.2 - HEAD_NUT.1);
    let head_size = HEAD_SIZE * hs;
    let nut_x = rect.left() + head_size.x - 4.0;
    let head_rect = Rect::from_min_size(Pos2::new(nut_x - HEAD_NUT.0 * hs, cy - (HEAD_NUT.1 + HEAD_NUT.2) / 2.0 * hs), head_size);
    // Title.
    pa.text(Pos2::new(rect.left() + 360.0, rect.top() + 34.0), egui::Align2::LEFT_CENTER, "Dragon Strings", egui::FontId::proportional(34.0), Color32::from_rgba_unmultiplied(0xd9, 0xb7, 0x6e, 150));
    pa.text(Pos2::new(rect.left() + 612.0, rect.top() + 38.0), egui::Align2::LEFT_CENTER, p.model.name().to_uppercase(), egui::FontId::proportional(15.0), Color32::from_rgba_unmultiplied(0xff, 0xff, 0xff, 120));
    // The neck: a trapezoid from the nut to the body, wood texture, frets, inlays.
    let scale = (joint_x - nut_x) / (1.0 - 2f32.powf(-15.0 / 12.0));
    let half = |x: f32| (nut_w + (joint_w - nut_w) * ((x - nut_x) / (joint_x - nut_x)).clamp(0.0, 1.0)) / 2.0;
    let (tl, tr, br, bl) = (Pos2::new(nut_x, cy - half(nut_x)), Pos2::new(joint_x, cy - half(joint_x)), Pos2::new(joint_x, cy + half(joint_x)), Pos2::new(nut_x, cy + half(nut_x)));
    if let Some(t) = texture(&ctx, "board", BOARD_JPG) {
        let mut mesh = egui::Mesh::with_texture(t.id());
        for (pos, uv) in [(tl, Pos2::new(0.0, 0.0)), (tr, Pos2::new(1.0, 0.0)), (br, Pos2::new(1.0, 1.0)), (bl, Pos2::new(0.0, 1.0))] {
            mesh.vertices.push(egui::epaint::Vertex { pos, uv, color: Color32::WHITE });
        }
        mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
        pa.add(egui::Shape::mesh(mesh));
    } else {
        pa.add(egui::Shape::convex_polygon(vec![tl, tr, br, bl], Color32::from_rgb(0x3a, 0x1c, 0x14), Stroke::NONE));
    }
    // Binding.
    pa.line_segment([tl, tr], Stroke::new(1.5_f32, Color32::from_rgb(0xe8, 0xdc, 0xc0)));
    pa.line_segment([bl, br], Stroke::new(1.5_f32, Color32::from_rgb(0xe8, 0xdc, 0xc0)));
    for f in 1..=15 {
        let x = fret_x(nut_x, scale, f as f32);
        if x >= joint_x {
            break;
        }
        let hh = half(x);
        pa.line_segment([Pos2::new(x, cy - hh), Pos2::new(x, cy + hh)], Stroke::new(2.0_f32, Color32::from_rgb(0xc8, 0xc8, 0xc0)));
        pa.line_segment([Pos2::new(x + 1.5, cy - hh), Pos2::new(x + 1.5, cy + hh)], Stroke::new(0.8_f32, Color32::from_black_alpha(120)));
    }
    for f in [3, 5, 7, 9, 12] {
        let x = (fret_x(nut_x, scale, f as f32 - 1.0) + fret_x(nut_x, scale, f as f32)) / 2.0;
        if x >= joint_x {
            continue;
        }
        let pearl = Color32::from_rgb(0xec, 0xe8, 0xdf);
        if f == 12 {
            pa.circle_filled(Pos2::new(x, cy - half(x) * 0.45), 3.6, pearl);
            pa.circle_filled(Pos2::new(x, cy + half(x) * 0.45), 3.6, pearl);
        } else {
            pa.circle_filled(Pos2::new(x, cy), 3.6, pearl);
        }
    }
    // Nut.
    pa.rect_filled(Rect::from_min_max(Pos2::new(nut_x - 3.0, cy - nut_w / 2.0 - 1.0), Pos2::new(nut_x + 1.0, cy + nut_w / 2.0 + 1.0)), 1.0, Color32::from_rgb(0xf0, 0xe8, 0xd0));
    // Headstock and body art.
    if let Some(t) = texture(&ctx, "head", HEAD_PNG) {
        pa.image(t.id(), head_rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
    }
    if let Some(t) = texture(&ctx, art.name, art.bytes) {
        pa.image(t.id(), body_rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
    }
    // Strings: from the nut to the body, where the picture's own strings carry on. String 0 (lowest)
    // at the bottom, as on a guitar lying on your lap.
    let string_y = |i: usize, x: f32| -> f32 {
        let t = if n > 1 { i as f32 / (n - 1) as f32 } else { 0.5 };
        let at_nut = cy + (0.5 - t) * (nut_w - 8.0);
        let img = art.strings[5 - ((t * 5.0).round() as usize).min(5)];
        let at_joint = body_rect.top() + img * bs;
        let k = ((x - nut_x) / (joint_x - nut_x)).clamp(0.0, 1.0);
        at_nut + (at_joint - at_nut) * k
    };
    let time = ui.input(|i| i.time) as f32;
    let mut any = false;
    for (i, &fret) in frets.iter().enumerate().take(n) {
        let wound = i < n.saturating_sub(2) && n >= 4;
        let col = if wound { Color32::from_rgb(0xc9, 0xa0, 0x6a) } else { Color32::from_rgb(0xe6, 0xe6, 0xe6) };
        let width = if wound { 1.6 + (n - 1 - i) as f32 * 0.12 } else { 1.0 };
        let playing = fret >= 0;
        let (x0, x1) = (nut_x - 2.0, joint_x + 40.0);
        if playing {
            any = true;
            // A vibrating string: a soft band that shimmers.
            let amp = 1.6 + 0.6 * (time * 37.0 + i as f32).sin();
            pa.line_segment([Pos2::new(x0, string_y(i, x0)), Pos2::new(x1, string_y(i, x1))], Stroke::new(width + amp * 2.0, col.gamma_multiply(0.25)));
        }
        pa.line_segment([Pos2::new(x0, string_y(i, x0)), Pos2::new(x1, string_y(i, x1))], Stroke::new(width, col));
        // From the nut to the tuners.
        let peg = head_rect.left() + head_rect.width() * (0.3 + 0.27 * (i % 3) as f32);
        let peg_y = if (i as f32) < n as f32 / 2.0 { cy + nut_w * 0.38 } else { cy - nut_w * 0.38 };
        pa.line_segment([Pos2::new(nut_x - 3.0, string_y(i, nut_x)), Pos2::new(peg, peg_y)], Stroke::new(width * 0.8, col.gamma_multiply(0.8)));
    }
    // Where the notes are fretted.
    for (i, &f) in frets.iter().enumerate().take(n) {
        if f < 0 {
            continue;
        }
        let f = f as f32;
        let x = if f == 0.0 { nut_x - 9.0 } else { (fret_x(nut_x, scale, f - 1.0) + fret_x(nut_x, scale, f)) / 2.0 };
        let y = string_y(i, x);
        pa.circle_filled(Pos2::new(x, y), 7.0, LIT.gamma_multiply(0.22));
        if f == 0.0 {
            pa.circle_stroke(Pos2::new(x, y), 3.6, Stroke::new(1.8_f32, LIT));
        } else {
            pa.circle_filled(Pos2::new(x, y), 4.2, LIT);
            pa.circle_filled(Pos2::new(x - 1.0, y - 1.0), 1.5, Color32::from_rgb(0xff, 0xe0, 0xb0));
        }
    }
    if any {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(30));
    }
    // Capo.
    if p.capo > 0 {
        let x = fret_x(nut_x, scale, p.capo as f32) - 5.0;
        let hh = half(x) + 4.0;
        pa.rect_filled(Rect::from_min_max(Pos2::new(x - 5.0, cy - hh), Pos2::new(x + 5.0, cy + hh)), 3.0, Color32::from_rgb(0x22, 0x22, 0x26));
        pa.text(Pos2::new(x, cy + hh + 10.0), egui::Align2::CENTER_CENTER, format!("capo {}", p.capo), egui::FontId::proportional(10.0), GOLD);
    }
    // Articulation chips under the neck (like a player's switches).
    let chips_at = Pos2::new(nut_x + 6.0, rect.top() + 214.0);
    let mut chip_ui = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_size(chips_at, Vec2::new(620.0, 22.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
    for a in Articulation::ALL {
        let key = dragon_engine::instruments::note_name(a.key());
        if pill(&mut chip_ui, p.articulation == a, a.name()).on_hover_text(format!("Key switch: {key}")).clicked() {
            st.session.edit("Articulation", |pr| set(pr, cid, |g| g.articulation = a));
        }
        chip_ui.add_space(2.0);
    }
    chip_ui.add_space(10.0);
    for m in PlayMode::ALL {
        if pill(&mut chip_ui, p.mode == m, m.name()).clicked() {
            st.session.edit("Play mode", |pr| set(pr, cid, |g| g.mode = m));
        }
        chip_ui.add_space(2.0);
    }
}

// ------------------------------------------------------------------------------------------------ control strip

fn controls(ui: &mut Ui, st: &mut State, cid: Id, p: &GuitarParams) {
    let h = 118.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(W - 12.0, h), Sense::hover());
    let ctx = ui.ctx().clone();
    let pa = ui.painter_at(rect);
    if let Some(t) = texture(&ctx, "leather", LEATHER_JPG) {
        pa.image(t.id(), rect, Rect::from_min_max(Pos2::new(0.0, 0.3), Pos2::new(1.0, 0.3 + rect.height() / rect.width() * 1024.0 / 573.0)), Color32::WHITE);
    }
    pa.rect_stroke(rect, 4.0, Stroke::new(1.0_f32, Color32::from_gray(50)), egui::StrokeKind::Inside);
    pa.line_segment([rect.left_top() + Vec2::new(4.0, 2.0), rect.right_top() + Vec2::new(-4.0, 2.0)], Stroke::new(1.0_f32, Color32::from_white_alpha(18)));
    let mut cui = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(Vec2::new(12.0, 8.0))).layout(egui::Layout::left_to_right(egui::Align::Min)));
    let ui = &mut cui;
    let sep = |ui: &mut Ui| {
        let r = ui.available_rect_before_wrap();
        ui.painter().line_segment([Pos2::new(r.left() + 6.0, r.top() + 6.0), Pos2::new(r.left() + 6.0, r.top() + 96.0)], Stroke::new(1.0_f32, Color32::from_white_alpha(25)));
        ui.add_space(14.0);
    };
    let electric = matches!(p.model, GuitarModel::Electric | GuitarModel::Bass);
    ui.vertical(|ui| {
        group_label(ui, "STRING");
        ui.horizontal(|ui| {
            kn(ui, st, cid, p.sustain, 0.2, 15.0, 6.0, "SUSTAIN", |v| format!("{v:.1} s"), |g, v| g.sustain = v);
            kn(ui, st, cid, p.brightness, 0.0, 1.0, 0.6, "BRIGHT", widgets::pct, |g, v| g.brightness = v);
            kn(
                ui,
                st,
                cid,
                p.pick_position,
                0.03,
                0.5,
                0.18,
                "PICK POS",
                |v| {
                    if v < 0.12 {
                        "bridge".into()
                    } else if v > 0.3 {
                        "neck".into()
                    } else {
                        "middle".into()
                    }
                },
                |g, v| g.pick_position = v,
            );
            kn(ui, st, cid, p.pick_hardness, 0.0, 1.0, 0.6, "PICK", |v| if v < 0.35 { "finger".into() } else { format!("pick {:.0}%", v * 100.0) }, |g, v| g.pick_hardness = v);
        });
    });
    sep(ui);
    ui.vertical(|ui| {
        if electric {
            group_label(ui, "PICKUP & AMP");
            ui.horizontal(|ui| {
                kn(
                    ui,
                    st,
                    cid,
                    p.pickup,
                    0.0,
                    1.0,
                    0.3,
                    "PICKUP",
                    |v| {
                        if v < 0.33 {
                            "bridge".into()
                        } else if v > 0.66 {
                            "neck".into()
                        } else {
                            "middle".into()
                        }
                    },
                    |g, v| g.pickup = v,
                );
                kn(ui, st, cid, p.drive, 0.0, 1.0, 0.0, "DRIVE", widgets::pct, |g, v| g.drive = v);
                kn(ui, st, cid, p.tone, 0.0, 1.0, 0.5, "TONE", widgets::pct, |g, v| g.tone = v);
            });
        } else {
            group_label(ui, "BODY");
            ui.horizontal(|ui| {
                kn(ui, st, cid, p.body, 0.0, 1.0, 0.7, "BODY", widgets::pct, |g, v| g.body = v);
                kn(
                    ui,
                    st,
                    cid,
                    p.body_size,
                    0.4,
                    1.6,
                    1.0,
                    "SIZE",
                    |v| {
                        if v < 0.8 {
                            "small".into()
                        } else if v > 1.2 {
                            "jumbo".into()
                        } else {
                            "dreadnought".into()
                        }
                    },
                    |g, v| g.body_size = v,
                );
                kn(ui, st, cid, p.tone, 0.0, 1.0, 0.5, "AIR", widgets::pct, |g, v| g.tone = v);
            });
        }
    });
    sep(ui);
    ui.vertical(|ui| {
        group_label(ui, "STRUM");
        ui.horizontal(|ui| {
            kn(ui, st, cid, p.strum_time, 0.0, 120.0, 14.0, "SPEED", |v| format!("{v:.0} ms"), |g, v| g.strum_time = v);
            ui.vertical(|ui| {
                ui.add_space(4.0);
                let mut d = p.strum_dir;
                egui::ComboBox::from_id_salt(("gtr.dir", cid)).selected_text(d.name()).width(84.0).show_ui(ui, |ui| {
                    for x in StrumDir::ALL {
                        ui.selectable_value(&mut d, x, x.name());
                    }
                });
                if d != p.strum_dir {
                    st.session.edit("Strum direction", |pr| set(pr, cid, |g| g.strum_dir = d));
                }
                let mut pt = p.pattern;
                egui::ComboBox::from_id_salt(("gtr.pat", cid)).selected_text(PATTERNS[pt.min(PATTERNS.len() - 1)].0).width(84.0).show_ui(ui, |ui| {
                    for (k, (name, steps, _)) in PATTERNS.iter().enumerate() {
                        ui.selectable_value(&mut pt, k, *name).on_hover_text(*steps);
                    }
                });
                if pt != p.pattern {
                    st.session.edit("Pattern", |pr| set(pr, cid, |g| g.pattern = pt));
                }
                ui.label(RichText::new("DIR · PATTERN").size(10.0).color(GOLD_DIM));
            });
        });
    });
    sep(ui);
    ui.vertical(|ui| {
        group_label(ui, "FEEL");
        ui.horizontal(|ui| {
            kn(ui, st, cid, p.humanize, 0.0, 1.0, 0.3, "HUMAN", widgets::pct, |g, v| g.humanize = v);
            kn(ui, st, cid, p.noise, 0.0, 1.0, 0.3, "NOISE", widgets::pct, |g, v| g.noise = v);
            kn(ui, st, cid, p.velocity, 0.0, 1.0, 0.6, "VELOCITY", widgets::pct, |g, v| g.velocity = v);
            kn(ui, st, cid, p.release, 0.01, 2.0, 0.25, "RELEASE", |v| format!("{:.0} ms", v * 1000.0), |g, v| g.release = v);
        });
    });
    sep(ui);
    ui.vertical(|ui| {
        group_label(ui, "OUTPUT");
        ui.horizontal(|ui| {
            kn(ui, st, cid, p.width, 0.0, 1.0, 0.6, "WIDTH", widgets::pct, |g, v| g.width = v);
            kn(ui, st, cid, p.master, 0.0, 1.5, 0.8, "MAIN", widgets::db_fmt, |g, v| g.master = v);
        });
    });
    sep(ui);
    // What is sounding, as a chord name (a small display like a tuner's).
    let frets = st.audio.meters().snapshot().frets.get(&cid).copied().unwrap_or([-1; 8]);
    let open = p.open_strings();
    let notes: Vec<u8> = open.iter().enumerate().filter(|(i, _)| frets[*i] >= 0).map(|(i, o)| (*o as i32 + frets[i] as i32 - p.capo as i32).clamp(0, 127) as u8).collect();
    let text = match notes.len() {
        0 => "–".to_string(),
        1 => dragon_engine::instruments::note_name(notes[0]),
        _ => detect_chord(&notes).map(|(r, k)| format!("{} {}", ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"][(r % 12) as usize], CHORDS[k].0)).unwrap_or_else(|| "?".into()),
    };
    ui.vertical(|ui| {
        group_label(ui, "PLAYING");
        let (r, _) = ui.allocate_exact_size(Vec2::new(150.0, 62.0), Sense::hover());
        let pa = ui.painter();
        pa.rect_filled(r, 4.0, Color32::from_rgb(0x0a, 0x0c, 0x0a));
        pa.rect_stroke(r, 4.0, Stroke::new(1.0_f32, Color32::from_gray(45)), egui::StrokeKind::Inside);
        pa.text(r.center() - Vec2::new(0.0, 6.0), egui::Align2::CENTER_CENTER, text, egui::FontId::proportional(22.0), Color32::from_rgb(0xff, 0xb0, 0x60));
        pa.text(r.center_bottom() - Vec2::new(0.0, 9.0), egui::Align2::CENTER_CENTER, format!("{} · {}", p.mode.name(), p.articulation.name()), egui::FontId::proportional(10.0), GOLD_DIM);
    });
}

// ------------------------------------------------------------------------------------------------ pages

fn strummer_page(ui: &mut Ui, st: &mut State, cid: Id, p: &GuitarParams) {
    ui.set_min_height(378.0);
    ui.horizontal(|ui| {
        group_label(ui, "PLAY MODE");
        for m in PlayMode::ALL {
            if pill(ui, p.mode == m, m.name()).clicked() {
                st.session.edit("Play mode", |pr| set(pr, cid, |g| g.mode = m));
            }
        }
    });
    let help = match p.mode {
        PlayMode::Solo => "Each key plays one string, the way a player would finger it (a new note on a busy string stops the old one).",
        PlayMode::Strum => "Keys pressed together are strummed across the strings, low to high (down) or back (up).",
        PlayMode::Chord => "Play any chord – or just one key for a whole chord – and it is voiced on the neck like a guitarist would and strummed.",
        PlayMode::Pattern => "Hold a chord: it is strummed in the rhythm below, in time with the song. Change chords and the rhythm carries on.",
    };
    ui.label(RichText::new(help).small().color(theme::TEXT_DIM));
    ui.add_space(8.0);
    group_label(ui, "RHYTHM PATTERNS (pattern mode)");
    egui::Grid::new(("gtr.patterns", cid)).spacing([10.0, 6.0]).show(ui, |ui| {
        for (k, (name, steps, beats)) in PATTERNS.iter().enumerate() {
            let on = p.pattern == k;
            if pill(ui, on, name).clicked() {
                st.session.edit("Pattern", |pr| {
                    set(pr, cid, |g| {
                        g.pattern = k;
                        g.mode = PlayMode::Pattern;
                    })
                });
            }
            // The steps as blocks: tall = full strum, short = light, X = muted chuck, B = bass.
            let cell = 384.0 / steps.len() as f32;
            let (rect, _) = ui.allocate_exact_size(Vec2::new(384.0, 26.0), Sense::hover());
            let pa = ui.painter_at(rect);
            pa.rect_filled(rect, 3.0, Color32::from_gray(16));
            for (j, c) in steps.chars().enumerate() {
                let x = rect.left() + j as f32 * cell;
                let r = Rect::from_min_size(Pos2::new(x + 2.0, rect.top() + 3.0), Vec2::new(cell - 4.0, 20.0));
                let (col, hgt, label) = match c {
                    'D' => (LIT, 1.0, "D"),
                    'd' => (LIT.gamma_multiply(0.6), 0.6, "d"),
                    'U' => (GOLD, 1.0, "U"),
                    'u' => (GOLD.gamma_multiply(0.6), 0.6, "u"),
                    'X' => (Color32::from_gray(150), 0.5, "X"),
                    'B' => (Color32::from_rgb(0x6f, 0xb5, 0xff), 0.8, "B"),
                    '.' => (Color32::from_gray(70), 0.2, "."),
                    _ => (Color32::from_gray(40), 0.15, ""),
                };
                let rr = Rect::from_min_max(Pos2::new(r.left(), r.bottom() - r.height() * hgt), r.max);
                pa.rect_filled(rr, 2.0, if on { col } else { col.gamma_multiply(0.5) });
                if !label.is_empty() {
                    pa.text(rr.center(), egui::Align2::CENTER_CENTER, label, egui::FontId::monospace(9.0), Color32::BLACK);
                }
            }
            ui.label(
                RichText::new(if *beats < 0.3 {
                    "16ths"
                } else if *beats < 0.4 {
                    "triplets"
                } else {
                    "8ths"
                })
                .small()
                .color(theme::TEXT_DIM),
            );
            if k % 2 == 1 {
                ui.end_row();
            }
        }
    });
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        group_label(ui, "ONE KEY PLAYS A");
        for (k, (name, _)) in CHORDS.iter().enumerate() {
            if pill(ui, p.single_key_chord == k, name).on_hover_text("The chord a single held key plays in chord and pattern mode").clicked() {
                st.session.edit("Single-key chord", |pr| set(pr, cid, |g| g.single_key_chord = k));
            }
        }
    });
    ui.label(RichText::new("Key switches: C0 sustain · C#0 palm mute · D0 harmonic · D#0 dead note · E0 legato slide · F#0 strum down · G0 strum up").small().color(theme::TEXT_DIM));
}

fn settings_page(ui: &mut Ui, st: &mut State, cid: Id, p: &GuitarParams) {
    ui.set_min_height(378.0);
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            group_label(ui, "INSTRUMENT");
            egui::Grid::new(("gtr.models", cid)).spacing([6.0, 6.0]).show(ui, |ui| {
                for (k, m) in GuitarModel::ALL.into_iter().enumerate() {
                    if pill(ui, p.model == m, m.name()).on_hover_text("Changes the strings, body and tuning; your playing settings stay").clicked() && p.model != m {
                        st.session.edit("Instrument", |pr| {
                            set(pr, cid, |g| {
                                let mut n = GuitarParams::for_model(m);
                                n.mode = g.mode;
                                n.strum_time = g.strum_time;
                                n.strum_dir = g.strum_dir;
                                n.pattern = g.pattern;
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
            group_label(ui, "TUNING");
            ui.horizontal_wrapped(|ui| {
                ui.set_max_width(320.0);
                for (name, t) in TUNINGS {
                    let on = p.tuning.as_slice() == t;
                    if pill(ui, on, name).clicked() {
                        let t = t.to_vec();
                        st.session.edit("Tuning", |pr| set(pr, cid, |g| g.tuning = t));
                    }
                }
                if pill(ui, p.tuning == p.model.default_tuning(), "Instrument default").clicked() {
                    st.session.edit("Tuning", |pr| set(pr, cid, |g| g.tuning = g.model.default_tuning()));
                }
            });
            ui.horizontal(|ui| {
                for (i, n) in p.tuning.iter().enumerate() {
                    ui.vertical(|ui| {
                        let mut v = *n as i32;
                        let r = ui.add(egui::DragValue::new(&mut v).range(12..=96).speed(0.1).custom_formatter(|v, _| dragon_engine::instruments::note_name(v as u8)));
                        widgets::apply(st, &r, "String tuning", |pr| {
                            set(pr, cid, |g| {
                                if let Some(s) = g.tuning.get_mut(i) {
                                    *s = v as u8
                                }
                            })
                        });
                        ui.label(RichText::new(format!("{}", i + 1)).small().color(theme::TEXT_DIM));
                    });
                }
            });
        });
        ui.add_space(24.0);
        ui.vertical(|ui| {
            group_label(ui, "PLAYING");
            ui.horizontal(|ui| {
                kn(ui, st, cid, p.capo as f32, 0.0, 12.0, 0.0, "CAPO", |v| if v < 0.5 { "none".into() } else { format!("fret {:.0}", v.round()) }, |g, v| g.capo = v.round() as u8);
                kn(ui, st, cid, p.slide_time, 0.005, 0.5, 0.08, "SLIDE", |v| format!("{:.0} ms", v * 1000.0), |g, v| g.slide_time = v);
                kn(ui, st, cid, p.doubling, 0.0, 1.0, 0.0, "DOUBLE", |v| if v < 0.01 { "off".into() } else { format!("{:.0}%", v * 100.0) }, |g, v| g.doubling = v);
                kn(ui, st, cid, p.doubling_detune, 0.0, 20.0, 4.0, "DETUNE", |v| format!("{v:.1} ct"), |g, v| g.doubling_detune = v);
                kn(ui, st, cid, p.buzz, 0.0, 1.0, 0.0, "BUZZ", widgets::pct, |g, v| g.buzz = v);
                kn(ui, st, cid, p.trim_db, -36.0, 36.0, 0.0, "TRIM", |v| format!("{v:+.1} dB"), |g, v| g.trim_db = v);
            });
            ui.label(RichText::new("Doubling adds the second string of each course (12-string: octaves on the low strings; mandolin: unison). Buzz is a sitar-style bridge. Trim is the preset's level, set for every factory preset so they all play equally loud.").small().color(theme::TEXT_DIM));
            ui.add_space(8.0);
            group_label(ui, "PRESET");
            ui.label(RichText::new(&p.preset).strong());
            if let Some(l) = gtr_library::find(&p.preset) {
                ui.label(format!("{} · {}", l.category.name(), l.kind));
            }
            ui.label(RichText::new(format!("{} factory presets · your presets live in {}", gtr_library::library().len(), user_dir(st).display())).small().color(theme::TEXT_DIM));
            if ui.button("Open user preset folder").clicked() {
                let d = user_dir(st);
                let _ = std::fs::create_dir_all(&d);
                crate::chains::reveal(&d);
            }
        });
    });
}

fn browser(ui: &mut Ui, st: &mut State, cid: Id, p: &GuitarParams) {
    let h = 382.0;
    ui.horizontal_top(|ui| {
        egui::Frame::NONE.fill(Color32::from_rgb(0x18, 0x18, 0x1b)).corner_radius(4.0).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
            ui.set_width(190.0);
            ui.set_min_height(h);
            ui.vertical(|ui| {
                group_label(ui, "PRESETS");
                let lib = gtr_library::library();
                let nfav = favs(st).len();
                let nuser = user_presets(st).len();
                let mut rows: Vec<(Option<String>, String)> =
                    vec![(None, format!("All  ({})", lib.len() + nuser)), (Some("Favourites".into()), format!("Favourites  ({nfav})")), (Some("User".into()), format!("User  ({nuser})"))];
                for c in Category::ALL {
                    let n = lib.iter().filter(|p| p.category == c).count();
                    rows.push((Some(c.name().to_string()), format!("{}  ({n})", c.name())));
                }
                egui::ScrollArea::vertical().id_salt("gtr.cats").max_height(h - 24.0).show(ui, |ui| {
                    for (f, label) in rows {
                        let on = st.ui.guitar.filter == f;
                        if ui
                            .add_sized([176.0, 20.0], egui::Button::new(RichText::new(label).color(if on { Color32::BLACK } else { theme::TEXT })).fill(if on { GOLD } else { Color32::from_rgb(0x18, 0x18, 0x1b) }))
                            .clicked()
                        {
                            st.ui.guitar.filter = f;
                        }
                    }
                });
            });
        });
        egui::Frame::NONE.fill(Color32::from_rgb(0x18, 0x18, 0x1b)).corner_radius(4.0).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
            ui.set_width(W - 230.0);
            ui.set_min_height(h);
            ui.vertical(|ui| {
                let list = entries(st);
                ui.horizontal(|ui| {
                    let mut q = st.ui.guitar.search.clone();
                    if ui.add(egui::TextEdit::singleline(&mut q).hint_text("Search: name, style or instrument (e.g. \"fingerstyle\", \"bossa\", \"crunch\")").desired_width(380.0)).changed() {
                        st.ui.guitar.search = q;
                    }
                    if !st.ui.guitar.search.is_empty() && ui.small_button("×").clicked() {
                        st.ui.guitar.search.clear();
                    }
                    ui.label(RichText::new(format!("{} presets", list.len())).small().color(theme::TEXT_DIM));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Close").clicked() {
                            st.ui.guitar.open = false;
                        }
                        if ui.button("Random").clicked() {
                            step_preset(st, cid, &p.preset, 0);
                        }
                    });
                });
                ui.label(RichText::new("Click to load and play it on the keyboard below; double-click to load and close.").small().color(theme::TEXT_DIM));
                let fav = favs(st).clone();
                let row_h = 20.0;
                egui::ScrollArea::vertical().id_salt("gtr.list").max_height(h - 50.0).auto_shrink([false, false]).show_rows(ui, row_h, list.len(), |ui, range| {
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
                                st.ui.guitar.open = false;
                            }
                        });
                    }
                });
            });
        });
    });
}

// ------------------------------------------------------------------------------------------------ keyboard

fn is_black(n: u8) -> bool {
    matches!(n % 12, 1 | 3 | 6 | 8 | 10)
}

/// The keyboard: key switches lit orange, the instrument's range bright, the rest dimmed; the notes
/// sounding glow. Click to play.
fn keyboard(ui: &mut Ui, st: &mut State, cid: Id, p: &GuitarParams) {
    let held_id = egui::Id::new(("gtr.kbd", cid));
    let held: Option<u8> = ui.data(|d| d.get_temp(held_id)).flatten();
    let open = p.open_strings();
    let lo = open.iter().copied().min().unwrap_or(40);
    let hi = open.iter().copied().max().unwrap_or(64) + p.model.frets();
    let frets = st.audio.meters().snapshot().frets.get(&cid).copied().unwrap_or([-1; 8]);
    let sounding: Vec<u8> = open.iter().enumerate().filter(|&(i, _)| frets.get(i).copied().unwrap_or(-1) >= 0).map(|(i, o)| (*o as i32 + frets[i] as i32 - p.capo as i32).clamp(0, 127) as u8).collect();
    let (rect, r) = ui.allocate_exact_size(Vec2::new(W - 12.0, 84.0), Sense::click_and_drag());
    let pa = ui.painter_at(rect);
    pa.rect_filled(rect, 4.0, Color32::from_gray(12));
    let (k0, k1) = (12u8, 108u8);
    let whites: Vec<u8> = (k0..=k1).filter(|n| !is_black(*n)).collect();
    let kb = rect.shrink2(Vec2::new(6.0, 4.0));
    let ww = kb.width() / whites.len() as f32;
    let mut keys: Vec<(u8, Rect)> = Vec::new();
    let mut blacks: Vec<(u8, Rect)> = Vec::new();
    for (i, n) in whites.iter().enumerate() {
        keys.push((*n, Rect::from_min_size(Pos2::new(kb.left() + i as f32 * ww, kb.top() + 6.0), Vec2::new(ww - 1.0, kb.height() - 6.0))));
    }
    for n in k0..=k1 {
        if is_black(n) {
            let lw = whites.iter().position(|w| *w == n - 1).unwrap_or(0);
            let x = kb.left() + (lw + 1) as f32 * ww - ww * 0.3;
            blacks.push((n, Rect::from_min_size(Pos2::new(x, kb.top() + 6.0), Vec2::new(ww * 0.6, (kb.height() - 6.0) * 0.6))));
        }
    }
    let ks = |n: u8| (12..=19).contains(&n);
    let in_range = |n: u8| n >= lo && n <= hi;
    // Zone bar above the keys.
    for (n, kr) in keys.iter().chain(blacks.iter()) {
        let col = if ks(*n) {
            LIT
        } else if in_range(*n) {
            Color32::from_rgb(0x4d, 0x9b, 0xd8)
        } else {
            continue;
        };
        pa.rect_filled(Rect::from_min_size(Pos2::new(kr.left(), kb.top()), Vec2::new(kr.width() + 1.0, 4.0)), 0.0, col);
    }
    for (n, kr) in &keys {
        let base = if ks(*n) {
            Color32::from_rgb(0xf3, 0xc0, 0x8a)
        } else if in_range(*n) {
            Color32::from_rgb(0xf2, 0xf0, 0xea)
        } else {
            Color32::from_gray(150)
        };
        let col = if held == Some(*n) {
            GOLD
        } else if sounding.contains(n) {
            Color32::from_rgb(0xff, 0xb8, 0x70)
        } else {
            base
        };
        pa.rect_filled(*kr, 2.0, col);
        if n % 12 == 0 {
            pa.text(kr.center_bottom() - Vec2::new(0.0, 3.0), egui::Align2::CENTER_BOTTOM, format!("C{}", *n as i32 / 12 - 1), egui::FontId::proportional(9.0), Color32::from_gray(80));
        }
    }
    for (n, kr) in &blacks {
        let col = if held == Some(*n) {
            GOLD
        } else if sounding.contains(n) {
            LIT
        } else if ks(*n) {
            Color32::from_rgb(0x9a, 0x5a, 0x20)
        } else {
            Color32::from_rgb(0x18, 0x18, 0x1b)
        };
        pa.rect_filled(*kr, 2.0, col);
    }
    let at = |q: Pos2| blacks.iter().chain(keys.iter()).find(|(_, kr)| kr.contains(q)).map(|(n, _)| *n);
    let pointer = ui.ctx().pointer_latest_pos();
    let down = r.is_pointer_button_down_on() || r.dragged();
    let want = if down { pointer.and_then(at) } else { None };
    if held.is_some() || want.is_some() {
        // The engine's fret display changes without any input: keep drawing while a key is held.
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(30));
    }
    if want != held {
        if let Some(n) = held {
            st.audio.send(Command::Live(LiveEvent::NoteOff { channel: cid, note: n }));
        }
        if let Some(n) = want {
            let vel = pointer.map(|q| (0.4 + 0.6 * (q.y - kb.top()) / kb.height()).clamp(0.1, 1.0)).unwrap_or(0.8);
            st.audio.send(Command::Live(LiveEvent::NoteOn { channel: cid, note: n, velocity: vel }));
        }
        ui.data_mut(|d| d.insert_temp(held_id, want));
    }
    let tip = format!(
        "Orange: key switches (C0 – G0). Blue: the {}'s range ({} – {}). Click to play – lower on a key is louder.",
        p.model.name().to_lowercase(),
        dragon_engine::instruments::note_name(lo),
        dragon_engine::instruments::note_name(hi)
    );
    r.on_hover_text(tip);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_artwork_decodes() {
        for b in [BODY_PNG, EBODY_PNG, HEAD_PNG, BOARD_JPG, LEATHER_JPG] {
            assert!(image::load_from_memory(b).is_ok());
        }
    }

    #[test]
    fn the_editor_draws_every_page_and_the_browser_steps_presets() {
        let mut st = State::new(redhive_client_framework::AppContext::new("en"), crate::state::Settings { show_splash: false, ..Default::default() });
        st.new_project();
        let cid = st.session.edit("x", |p| p.add_channel("Guitar", Instrument::Guitar(Box::new(gtr_library::library()[0].params.clone()))));
        let ctx = egui::Context::default();
        let draw = |st: &mut State, page: u8| {
            let _ = ctx.run(egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1400.0, 1000.0))), ..Default::default() }, |ctx| {
                ctx.data_mut(|d| d.insert_temp(egui::Id::new(("gtr.page", cid)), page));
                egui::CentralPanel::default().show(ctx, |ui| {
                    let Some(Instrument::Guitar(p)) = st.session.project.channel(cid).map(|c| c.instrument.clone()) else { panic!() };
                    editor(ui, st, cid, *p);
                });
            });
        };
        for page in 0..4u8 {
            draw(&mut st, page);
        }
        st.ui.guitar.open = true;
        draw(&mut st, 0);
        let cur = |st: &State| match st.session.project.channel(cid).map(|c| c.instrument.clone()) {
            Some(Instrument::Guitar(p)) => p.preset,
            _ => panic!(),
        };
        st.ui.guitar.filter = Some("Bass".into());
        step_preset(&mut st, cid, "", 1);
        let first = cur(&st);
        assert_eq!(gtr_library::find(&first).unwrap().category, Category::Bass);
        step_preset(&mut st, cid, &first, 1);
        assert_ne!(cur(&st), first);
        st.ui.guitar.filter = None;
        st.ui.guitar.search = "bossa".into();
        assert!(entries(&mut st).len() >= 10);
    }
}
