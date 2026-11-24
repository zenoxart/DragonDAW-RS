//! The effect gallery: every stock effect as a card showing the unit exactly as its editor looks – the
//! faceplate in its finish, the nameplate with emblem and meters, and its display (or, for Dragonforge
//! and Jadefang, their own face) – scaled down to the card, with the description and a button that puts
//! it into the selected mixer insert. A friendly way to discover what the effects do.

use super::fx_ui::{self, Hero};
use crate::state::State;
use crate::theme;
use dragon_engine::effects::{self, EffectDef};
use dragon_engine::model::MAX_INSERTS;
use egui::emath::TSTransform;
use egui::{Pos2, Rect, RichText, Ui, Vec2};
use std::collections::BTreeMap;

/// The width a standard unit (nameplate and display) is laid out at before it's scaled to the card –
/// about an effect window's.
const UNIT_W: f32 = 620.0;

pub fn show(ctx: &egui::Context, st: &mut State) {
    if !st.ui.fx_gallery {
        return;
    }
    let mut open = true;
    egui::Window::new("Effect gallery").id(egui::Id::new("fx.gallery")).open(&mut open).default_size([980.0, 680.0]).max_height(ctx.screen_rect().height() - 80.0).resizable(true).show(ctx, |ui| body(ui, st));
    st.ui.fx_gallery = open;
}

/// The width a unit is laid out at (its editor's own width for the effects with their own face).
fn natural_width(id: &str) -> f32 {
    if super::fx_forge::is_forge(id) {
        super::fx_forge::window_width()
    } else if super::fx_ghost::is_ghost(id) {
        super::fx_ghost::window_width()
    } else if super::fx_jade::own_face(id) {
        super::fx_jade::window_width()
    } else {
        UNIT_W
    }
}

/// Draws `add` at its natural width `natural_w` on a layer of its own, scaled down to the width available
/// (never up), and allocates the scaled size. Clipped to the parent (the gallery's scroll area).
fn scaled(ui: &mut Ui, id: egui::Id, natural_w: f32, add: impl FnOnce(&mut Ui)) {
    let w = ui.available_width();
    let s = (w / natural_w).min(1.0);
    let top_left = ui.cursor().min;
    let x0 = top_left.x + (w - natural_w * s) / 2.0;
    let layer = egui::LayerId::new(ui.layer_id().order, id);
    let ctx = ui.ctx().clone();
    ctx.set_sublayer(ui.layer_id(), layer);
    let t = TSTransform::new(Vec2::new(x0, top_left.y), s);
    ctx.set_transform_layer(layer, t);
    let local = Rect::from_min_size(Pos2::ZERO, Vec2::new(natural_w, 4000.0));
    let mut child = ui.new_child(egui::UiBuilder::new().layer_id(layer).max_rect(local));
    child.set_clip_rect(t.inverse().mul_rect(ui.clip_rect()));
    add(&mut child);
    let h = child.min_rect().height();
    ui.allocate_exact_size(Vec2::new(w, h * s), egui::Sense::hover());
}

/// The unit as its editor draws it: faceplate, nameplate (Dragonforge engraves its own), then the
/// display in its bezel – or the effect's own face. Edits made in the preview are dropped.
fn unit(ui: &mut Ui, def: &'static EffectDef, params: &BTreeMap<String, f32>, accent: egui::Color32, time: Option<f32>) {
    let finish = super::analog::finish_of(def.id);
    let face = finish.base();
    let plate_at = ui.painter().add(egui::Shape::Noop);
    let ear = super::analog::EAR as i8;
    let key = egui::Id::new(("gallery", def.id));
    let gr = if def.category == "Dynamics" { 4.5 } else { 0.0 };
    // A steady demo level for the meters (−14 dB RMS), gently moving when motion is on.
    let lv = 0.2 * (1.0 + 0.15 * time.map(|t| (t * 1.7).sin()).unwrap_or(0.0));
    let forge = super::fx_forge::is_forge(def.id);
    let ghost = super::fx_ghost::is_ghost(def.id);
    let dev = egui::Frame::NONE.inner_margin(egui::Margin { left: 14 + ear, right: 14 + ear, top: 8, bottom: 14 }).show(ui, |ui| {
        if !forge && !ghost {
            super::analog::nameplate(ui, &super::analog::Plate { id: def.id, name: def.name, category: def.category, enabled: true, gr, level: (lv, lv * 0.95), time, key: key.with("plate") });
        }
        ui.add_space(6.0);
        if forge {
            let live = super::fx_forge::Live { tele: &[], level: (lv, lv * 0.95), time, enabled: true };
            let _ = super::fx_forge::body(ui, def, params, &live, key.with("forge"));
        } else if ghost {
            let live = super::fx_forge::Live { tele: &[], level: (lv, lv * 0.95), time, enabled: true };
            let _ = super::fx_ghost::body(ui, def, params, &live, key.with("ghost"));
        } else if super::fx_jade::own_face(def.id) {
            let _ = super::fx_jade::body(ui, def, params, accent, key.with("jade"));
        } else {
            let stroke = if finish.gnostic() { egui::Stroke::new(2.0_f32, super::analog::GNOSTIC_GOLD.gamma_multiply(0.75)) } else { egui::Stroke::new(2.0_f32, shade(face, 0.55)) };
            egui::Frame::NONE.fill(egui::Color32::from_rgb(0x08, 0x09, 0x0b)).corner_radius(7.0).inner_margin(egui::Margin::same(3)).stroke(stroke).show(ui, |ui| {
                let _ = fx_ui::hero(ui, &Hero { def, params, gr, enabled: true, color: accent, time, tele: &[], key: key.with("hero") });
            });
        }
    });
    ui.painter().set(plate_at, egui::Shape::Vec(super::analog::faceplate(ui.ctx(), dev.response.rect, finish)));
}

fn shade(c: egui::Color32, k: f32) -> egui::Color32 {
    let f = |v: u8| ((v as f32 * k).clamp(0.0, 255.0)) as u8;
    egui::Color32::from_rgb(f(c.r()), f(c.g()), f(c.b()))
}

pub fn body(ui: &mut egui::Ui, st: &mut State) {
    let target = st.ui.mixer;
    let target_name = st.session.project.mixer.get(target).map(|c| c.name.clone()).unwrap_or_default();
    let full = st.session.project.mixer.get(target).map(|c| c.inserts.len() >= MAX_INSERTS).unwrap_or(true);
    ui.label(RichText::new(format!("All {} stock effects, as they look in their editors. \"Add\" puts one into the selected mixer insert ({target_name}).", effects::registry().len())).color(theme::TEXT_DIM));
    let time = if st.settings.reduced_motion { None } else { Some(ui.input(|i| i.time) as f32) };
    if time.is_some() {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(40));
    }
    let mut add: Option<&'static str> = None;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        let cols = ((ui.available_width() / 460.0).floor() as usize).max(1);
        let w = (ui.available_width() - 10.0 * (cols as f32 - 1.0)) / cols as f32;
        let mut rects = Vec::new();
        let clip = ui.clip_rect();
        for (ri, row) in effects::registry().chunks(cols).enumerate() {
            // Rows out of view are only measured (their last height), not drawn: the units are rich.
            let hkey = egui::Id::new(("fx.gallery.row", ri, cols));
            let last_h: Option<f32> = ui.data(|d| d.get_temp(hkey));
            let top = ui.cursor().min.y;
            if let Some(h) = last_h {
                if top > clip.bottom() || top + h < clip.top() {
                    let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), h), egui::Sense::hover());
                    for (k, def) in row.iter().enumerate() {
                        let x = r.left() + k as f32 * (w + 10.0);
                        rects.push((def.id, Rect::from_min_size(Pos2::new(x, r.top()), Vec2::new(w, h))));
                    }
                    ui.add_space(6.0);
                    continue;
                }
            }
            let resp = ui.horizontal_top(|ui| {
                for def in row {
                    let ccol = super::mixer::category_color(def.category);
                    let params = def.defaults();
                    let r = egui::Frame::NONE.fill(theme::PANEL).stroke(egui::Stroke::new(1.0_f32, ccol.gamma_multiply(0.35))).corner_radius(8.0).inner_margin(8.0).show(ui, |ui| {
                        ui.set_width(w - 18.0);
                        // Cards in a row share its height (a tall unit beside a short one).
                        if let Some(h) = last_h {
                            ui.set_min_height(h - 18.0);
                        }
                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                let (bar, _) = ui.allocate_exact_size(Vec2::new(4.0, 20.0), egui::Sense::hover());
                                ui.painter().rect_filled(bar, 2.0, ccol);
                                let brand = if def.name.starts_with("Dragon") { def.name.to_string() } else { format!("Dragon {}", def.name) };
                                ui.add(egui::Label::new(RichText::new(brand).size(14.0).strong()).truncate());
                                ui.label(RichText::new(format!("|  {}", def.category)).small().color(theme::TEXT_DIM));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    let b = ui.add_enabled(!full, egui::Button::new(RichText::new("+ Add").color(egui::Color32::BLACK)).fill(ccol));
                                    if b.on_hover_text(format!("Put {} into {target_name}", def.name)).clicked() {
                                        add = Some(def.id);
                                    }
                                });
                            });
                            ui.add_space(4.0);
                            scaled(ui, egui::Id::new(("fx.gallery.unit", def.id)), natural_width(def.id), |ui| unit(ui, def, &params, ccol, time));
                            ui.add_space(4.0);
                            ui.label(RichText::new(def.description).small().color(theme::TEXT_DIM));
                        });
                    });
                    rects.push((def.id, r.response.rect));
                }
            });
            ui.data_mut(|d| d.insert_temp(hkey, resp.response.rect.height()));
            ui.add_space(6.0);
        }
        ui.data_mut(|d| d.insert_temp(egui::Id::new("fx.gallery.cards"), rects));
    });
    if let Some(id) = add {
        let Some(def) = effects::find(id) else { return };
        let params = def.defaults();
        let iid = st.session.edit("Add effect", |p| {
            let iid = p.next_id();
            if let Some(c) = p.mixer.get_mut(target) {
                c.inserts.push(dragon_engine::model::Insert { id: iid, effect: id.to_string(), enabled: true, mix: 1.0, params, sidechain: None, plugin: None, preset: String::new(), mods: Vec::new(), file: None });
            }
            iid
        });
        st.ui.effect_windows.push((target, iid));
        st.toast(format!("{} added to {target_name}", def.name), redhive_contracts::ui::Severity::Success);
    }
}
