//! Computer keyboard as a piano (FL Studio layout): Z S X D C V G B H N J M = lower octave,
//! Q 2 W 3 E R 5 T 6 Y 7 U I = upper octave, comma / period shift the octave.

use crate::state::State;
use egui::Key;

const LOWER: &[(Key, u8)] = &[(Key::Z, 0), (Key::S, 1), (Key::X, 2), (Key::D, 3), (Key::C, 4), (Key::V, 5), (Key::G, 6), (Key::B, 7), (Key::H, 8), (Key::N, 9), (Key::J, 10), (Key::M, 11)];
const UPPER: &[(Key, u8)] =
    &[(Key::Q, 12), (Key::Num2, 13), (Key::W, 14), (Key::Num3, 15), (Key::E, 16), (Key::R, 17), (Key::Num5, 18), (Key::T, 19), (Key::Num6, 20), (Key::Y, 21), (Key::Num7, 22), (Key::U, 23), (Key::I, 24)];

pub fn typing_keyboard(st: &mut State, ctx: &egui::Context) {
    let (pressed, released, oct_down, oct_up) = ctx.input(|i| {
        let mut p = Vec::new();
        let mut r = Vec::new();
        for e in &i.events {
            if let egui::Event::Key { key, pressed, repeat: false, modifiers, .. } = e {
                if modifiers.ctrl || modifiers.alt || modifiers.command {
                    continue;
                }
                if *pressed {
                    p.push(*key);
                } else {
                    r.push(*key);
                }
            }
        }
        (p, r, i.key_pressed(Key::Comma), i.key_pressed(Key::Period))
    });
    if oct_down {
        st.ui.octave = (st.ui.octave - 1).max(0);
        st.ui.status = format!("Typing keyboard octave {}", st.ui.octave);
    }
    if oct_up {
        st.ui.octave = (st.ui.octave + 1).min(9);
        st.ui.status = format!("Typing keyboard octave {}", st.ui.octave);
    }
    for k in pressed {
        if let Some((_, off)) = LOWER.iter().chain(UPPER).find(|(key, _)| *key == k) {
            let note = (st.ui.octave * 12 + *off as i32).clamp(0, 127) as u8;
            st.ui.held_keys.push((k, note));
            st.live_note(true, note, 0.8);
        }
    }
    for k in released {
        if let Some(pos) = st.ui.held_keys.iter().position(|(key, _)| *key == k) {
            let (_, note) = st.ui.held_keys.remove(pos);
            st.live_note(false, note, 0.0);
        }
    }
}

pub fn is_black(note: u8) -> bool {
    matches!(note % 12, 1 | 3 | 6 | 8 | 10)
}
