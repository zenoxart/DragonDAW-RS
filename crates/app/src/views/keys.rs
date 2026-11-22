//! the side-specific modifiers (Right-Shift / Right-Alt / Left-Shift + mouse) and the numeric-pad 0, read
//! from the OS keyboard state (see `dragon_engine::keystate`); tests set `UiState::test_keys` instead.

use crate::state::State;
pub use dragon_engine::keystate::SideKey;

/// Whether `k` is held (`false` where the platform cannot tell).
pub fn side(st: &State, k: SideKey) -> bool {
    st.ui.test_keys.contains(&k) || dragon_engine::keystate::is_down(k) == Some(true)
}

/// middle + right click opens the context tool menu (the tools, at the pointer).
pub fn tool_menu(ui: &egui::Ui, st: &mut State, resp: &egui::Response) {
    if resp.hovered() && ui.input(|i| i.pointer.middle_down() && i.pointer.button_pressed(egui::PointerButton::Secondary)) {
        st.ui.tool_menu = ui.input(|i| i.pointer.interact_pos());
    }
    let Some(at) = st.ui.tool_menu else { return };
    let mut close = false;
    let area = egui::Area::new(egui::Id::new("fl.tool_menu")).fixed_pos(at).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            ui.label(egui::RichText::new("Tools").small().strong());
            for (t, icon, name) in crate::state::Tool::ALL {
                if ui.selectable_label(st.ui.tool == t, format!("{icon}  {name}")).clicked() {
                    st.ui.tool = t;
                    close = true;
                }
            }
        });
    });
    if close || (ui.input(|i| i.pointer.any_pressed()) && !area.response.contains_pointer() && !ui.input(|i| i.pointer.middle_down())) {
        st.ui.tool_menu = None;
    }
}
