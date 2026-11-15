//! The effect gallery: every stock effect as a card with its live display, description and a button
//! that puts it into the selected mixer insert. A friendly way to discover what the effects do.

use super::fx_ui::{self, Hero};
use crate::state::State;
use crate::theme;
use dragon_engine::effects;
use dragon_engine::model::MAX_INSERTS;
use egui::{RichText, Vec2};

pub fn show(ctx: &egui::Context, st: &mut State) {
    if !st.ui.fx_gallery {
        return;
    }
    let mut open = true;
    egui::Window::new("Effect gallery").id(egui::Id::new("fx.gallery")).open(&mut open).default_size([980.0, 680.0]).max_height(ctx.screen_rect().height() - 80.0).resizable(true).show(ctx, |ui| body(ui, st));
    st.ui.fx_gallery = open;
}

pub fn body(ui: &mut egui::Ui, st: &mut State) {
    let target = st.ui.mixer;
    let target_name = st.session.project.mixer.get(target).map(|c| c.name.clone()).unwrap_or_default();
    let full = st.session.project.mixer.get(target).map(|c| c.inserts.len() >= MAX_INSERTS).unwrap_or(true);
    ui.label(RichText::new(format!("All {} stock effects. \"Add\" puts one into the selected mixer insert ({target_name}).", effects::registry().len())).color(theme::TEXT_DIM));
    let time = if st.settings.reduced_motion { None } else { Some(ui.input(|i| i.time) as f32) };
    if time.is_some() {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(40));
    }
    let mut add: Option<&'static str> = None;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        let cols = ((ui.available_width() / 460.0).floor() as usize).max(1);
        let w = (ui.available_width() - 10.0 * (cols as f32 - 1.0)) / cols as f32;
        let mut rects = Vec::new();
        for row in effects::registry().chunks(cols) {
            ui.horizontal_top(|ui| {
                for def in row {
                    let ccol = super::mixer::category_color(def.category);
                    let params = def.defaults();
                    let r = egui::Frame::NONE.fill(theme::PANEL).stroke(egui::Stroke::new(1.0_f32, ccol.gamma_multiply(0.35))).corner_radius(8.0).inner_margin(8.0).show(ui, |ui| {
                        ui.set_width(w - 18.0);
                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                let (bar, _) = ui.allocate_exact_size(Vec2::new(4.0, 20.0), egui::Sense::hover());
                                ui.painter().rect_filled(bar, 2.0, ccol);
                                let brand = if def.name.starts_with("Dragon") { def.name.to_string() } else { format!("Dragon {}", def.name) };
                                ui.label(RichText::new(brand).size(14.0).strong());
                                ui.label(RichText::new(format!("|  {}", def.category)).small().color(theme::TEXT_DIM));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    let b = ui.add_enabled(!full, egui::Button::new(RichText::new("+ Add").color(egui::Color32::BLACK)).fill(ccol));
                                    if b.on_hover_text(format!("Put {} into {target_name}", def.name)).clicked() {
                                        add = Some(def.id);
                                    }
                                });
                            });
                            let _ = fx_ui::hero(ui, &Hero { def, params: &params, gr: if def.category == "Dynamics" { 4.5 } else { 0.0 }, enabled: true, color: ccol, time, tele: &[], key: egui::Id::new(("gallery", def.id)) });
                            ui.label(RichText::new(def.description).small().color(theme::TEXT_DIM));
                        });
                    });
                    rects.push((def.id, r.response.rect));
                }
            });
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
                c.inserts.push(dragon_engine::model::Insert { id: iid, effect: id.to_string(), enabled: true, mix: 1.0, params, sidechain: None, plugin: None, preset: String::new() });
            }
            iid
        });
        st.ui.effect_windows.push((target, iid));
        st.toast(format!("{} added to {target_name}", def.name), redhive_contracts::ui::Severity::Success);
    }
}
