//! The start screen ("What do you want to do?") and the project setups behind its choices: make a beat,
//! record, mix, master, stem master, album master, film sound design. Each builds a fitting project and
//! opens the views that job needs.

use crate::state::State;
use crate::theme;
use dragon_engine::model::{Id, Insert, Project};
use egui::{Color32, Pos2, Rect, RichText, Stroke, Vec2};
use redhive_contracts::ui::Severity;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Beat,
    Record,
    Mix,
    Master,
    Stems,
    Album,
    Film,
}

/// (mode, key stored in the project, title, what it sets up)
pub const MODES: [(Mode, &str, &str, &str); 7] = [
    (Mode::Beat, "beat", "Make a beat", "Drum kit in the channel rack, an empty pattern, piano roll ready"),
    (Mode::Record, "record", "Record", "An armed audio track, the mixer at hand"),
    (Mode::Mix, "mix", "Mix", "Bring your stems: each gets a mixer channel"),
    (Mode::Master, "master", "Master", "A track for your mixdown, a mastering chain on the master"),
    (Mode::Stems, "stems", "Stem master", "Four group buses: Drums, Melody, All vocals, SFX"),
    (Mode::Album, "album", "Album master", "Several mixdowns, one album bus, matched loudness"),
    (Mode::Film, "film", "Film sound design", "Picture, dialogue, music, effects, foley, ambience"),
];

/// EQ → multiband → imager → limiter (−1 dBTP) on the master, skipping what is already there; the
/// limiter stays last (the mastering project modes start with it).
pub fn add_mastering_chain(p: &mut Project) {
    for id in ["eq", "multiband", "imager", "limiter"] {
        if p.mixer.master.inserts.iter().any(|i| i.effect == id) {
            continue;
        }
        let iid = p.next_id();
        let mut params = dragon_engine::effects::find(id).map(|d| d.defaults()).unwrap_or_default();
        if id == "limiter" {
            params.insert("ceiling".into(), -1.0);
        }
        p.mixer.master.inserts.push(Insert { id: iid, effect: id.into(), enabled: true, mix: 1.0, params, sidechain: None, plugin: None, preset: String::new(), mods: Vec::new() });
    }
    p.mixer.master.inserts.sort_by_key(|i| i.effect == "limiter");
}

/// The one-click mastering chain on the master, all stock: a gentle glue compressor → the Obsidian soft
/// clipper (takes the peaks off) → Philosopher's Stone (the finishing chain, last). Effects already on
/// the master are kept; what is missing is added, in this order, behind them.
pub fn add_stone_chain(p: &mut Project) {
    const CHAIN: [(&str, &[(&str, f32)]); 3] = [
        ("compressor", &[("threshold", -14.0), ("ratio", 2.0), ("knee", 8.0), ("attack", 30.0), ("release", 200.0), ("auto_makeup", 1.0)]),
        ("clipper", &[("threshold", -3.0), ("ceiling", -0.3), ("oversampling", 2.0)]),
        ("dragon_particle", &[]),
    ];
    for (id, set) in CHAIN {
        if p.mixer.master.inserts.iter().any(|i| i.effect == id) {
            continue;
        }
        let iid = p.next_id();
        let mut params = dragon_engine::effects::find(id).map(|d| d.defaults()).unwrap_or_default();
        for (k, v) in set {
            params.insert((*k).into(), *v);
        }
        p.mixer.master.inserts.push(Insert { id: iid, effect: id.into(), enabled: true, mix: 1.0, params, sidechain: None, plugin: None, preset: String::new(), mods: Vec::new() });
    }
    // The Stone finishes the chain.
    p.mixer.master.inserts.sort_by_key(|i| i.effect == "dragon_particle");
}

/// A playlist track with its own mixer channel (which serves as the group bus for what lands on it).
fn track(p: &mut Project, name: &str, icon: &str) -> (Id, Id) {
    let t = p.add_track(name);
    let m = p.track(t).map(|x| x.mixer).unwrap_or(0);
    if let Some(c) = p.mixer.get_mut(m) {
        c.icon = icon.into();
    }
    (t, m)
}

/// A bus on the mixer (no playlist track), routed to the master.
pub fn bus(p: &mut Project, name: &str, icon: &str) -> Id {
    let m = p.add_mixer_channel(name);
    if let Some(c) = p.mixer.get_mut(m) {
        c.icon = icon.into();
    }
    m
}

/// Builds the project for `mode` (a new session) and opens its views.
pub fn start(st: &mut State, mode: Mode) {
    let key = MODES.iter().find(|m| m.0 == mode).map(|m| m.1).unwrap_or("");
    match mode {
        Mode::Beat => {
            // The starter kit, but nothing written yet.
            st.new_demo_project();
            st.session.edit("Make a beat", |p| {
                for t in &mut p.tracks {
                    t.clips.clear();
                }
                for pat in &mut p.patterns {
                    for c in pat.channels.values_mut() {
                        c.notes.clear();
                    }
                }
            });
        }
        _ => st.new_project(),
    }
    let p = &mut st.session.project;
    p.mode = key.into();
    match mode {
        Mode::Beat => {
            p.meta.name = "New beat".into();
        }
        Mode::Record => {
            p.meta.name = "New recording".into();
            let (t, _) = track(p, "Recording", "vocal");
            if let Some(x) = p.track_mut(t) {
                x.armed = true;
            }
        }
        Mode::Mix => {
            p.meta.name = "New mix".into();
        }
        Mode::Master => {
            p.meta.name = "New master".into();
            track(p, "Mixdown", "master");
            add_mastering_chain(p);
        }
        Mode::Stems => {
            p.meta.name = "New stem master".into();
            for (name, icon) in [("Drums", "kick"), ("Melody", "synth"), ("All vocals", "vocal"), ("SFX", "sfx")] {
                track(p, name, icon);
            }
            add_mastering_chain(p);
        }
        Mode::Album => {
            p.meta.name = "New album".into();
            bus(p, ALBUM_BUS, "bus");
            add_mastering_chain(p);
        }
        Mode::Film => {
            p.meta.name = "New film mix".into();
            for (name, icon) in [("Dialogue", "vocal"), ("Music", "keys"), ("Effects", "sfx"), ("Foley", "perc"), ("Ambience", "pad")] {
                track(p, name, icon);
            }
            // Broadcast loudness (EBU R128).
            p.loudness_target = -23.0;
        }
    }
    // The project starts clean (the setup is not an undo step).
    st.session.history.clear();
    st.session.force_recompile();
    let views: &[&str] = match mode {
        Mode::Beat => &["dragon.playlist", "dragon.piano", "dragon.rack"],
        Mode::Record => &["dragon.mixer", "dragon.playlist"],
        Mode::Mix => &["dragon.browser", "dragon.playlist", "dragon.mixer"],
        Mode::Master | Mode::Stems | Mode::Album => &["dragon.browser", "dragon.playlist", "dragon.master"],
        Mode::Film => &["dragon.video", "dragon.browser", "dragon.playlist"],
    };
    for v in views {
        st.ctx.open(v, None);
    }
    let hint = match mode {
        Mode::Beat => "Click steps in the channel rack, or open the piano roll – then press Space",
        Mode::Record => "Press record (R) to record onto the armed track – Help › How to record walks you through it",
        Mode::Mix => "Drag your stems from the browser or your file manager onto the playlist",
        Mode::Master => "Drop your mixdown onto the Mixdown track; the mastering chain is on the master",
        Mode::Stems => "Drop each stem onto its group: Drums, Melody, All vocals or SFX",
        Mode::Album => "Add your mixdowns in the Mastering tab's Album panel, then match their loudness",
        Mode::Film => "Load the picture with Add › Video, then drop dialogue, music and effects on their tracks",
    };
    st.toast(hint, Severity::Info);
    crate::views::quests::event(st, crate::views::quests::Event::ModeChosen);
}

/// The album bus every song of an album master goes through.
pub const ALBUM_BUS: &str = "Album bus";

// ------------------------------------------------------------------------------------------- start screen

fn icon(p: &egui::Painter, r: Rect, mode: Mode, c: Color32) {
    let s = Stroke::new(2.2_f32, c);
    let ctr = r.center();
    let u = r.width().min(r.height()) / 2.0;
    match mode {
        Mode::Beat => {
            // A 4 × 2 step grid with a few steps on.
            for row in 0..2 {
                for col in 0..4 {
                    let on = matches!((row, col), (0, 0) | (0, 2) | (1, 1) | (1, 3) | (0, 3));
                    let q = Rect::from_center_size(Pos2::new(ctr.x - u * 0.75 + col as f32 * u * 0.5, ctr.y - u * 0.25 + row as f32 * u * 0.5), Vec2::splat(u * 0.38));
                    if on {
                        p.rect_filled(q, 2.0, c);
                    } else {
                        p.rect_stroke(q, 2.0, Stroke::new(1.5_f32, c.gamma_multiply(0.5)), egui::StrokeKind::Inside);
                    }
                }
            }
        }
        Mode::Record => {
            // A microphone.
            let head = Rect::from_center_size(ctr - Vec2::new(0.0, u * 0.25), Vec2::new(u * 0.55, u * 0.9));
            p.rect_stroke(head, u * 0.27, s, egui::StrokeKind::Middle);
            p.add(egui::Shape::line(
                (0..=16)
                    .map(|k| {
                        let a = std::f32::consts::PI * k as f32 / 16.0;
                        Pos2::new(ctr.x - u * 0.5 * a.cos(), ctr.y + u * 0.05 + u * 0.45 * a.sin())
                    })
                    .collect(),
                s,
            ));
            p.line_segment([ctr + Vec2::new(0.0, u * 0.5), ctr + Vec2::new(0.0, u * 0.8)], s);
            p.line_segment([ctr + Vec2::new(-u * 0.3, u * 0.8), ctr + Vec2::new(u * 0.3, u * 0.8)], s);
        }
        Mode::Mix => {
            // Three faders.
            for (k, h) in [0.2f32, -0.3, 0.1].iter().enumerate() {
                let x = ctr.x - u * 0.55 + k as f32 * u * 0.55;
                p.line_segment([Pos2::new(x, ctr.y - u * 0.75), Pos2::new(x, ctr.y + u * 0.75)], Stroke::new(1.5_f32, c.gamma_multiply(0.5)));
                p.rect_filled(Rect::from_center_size(Pos2::new(x, ctr.y + h * u), Vec2::new(u * 0.38, u * 0.2)), 2.0, c);
            }
        }
        Mode::Master => {
            // A loudness meter with a ceiling.
            for k in 0..5 {
                let h = [0.5f32, 0.8, 1.1, 1.3, 1.4][k] * u;
                let x = ctr.x - u * 0.7 + k as f32 * u * 0.35;
                p.rect_filled(Rect::from_min_max(Pos2::new(x, ctr.y + u * 0.75 - h), Pos2::new(x + u * 0.22, ctr.y + u * 0.75)), 1.0, c.gamma_multiply(0.4 + 0.12 * k as f32));
            }
            p.line_segment([Pos2::new(ctr.x - u * 0.85, ctr.y - u * 0.7), Pos2::new(ctr.x + u * 0.85, ctr.y - u * 0.7)], s);
        }
        Mode::Stems => {
            // Four lanes merging into one.
            for k in 0..4 {
                let y = ctr.y - u * 0.6 + k as f32 * u * 0.4;
                p.add(egui::Shape::CubicBezier(egui::epaint::CubicBezierShape::from_points_stroke(
                    [Pos2::new(ctr.x - u * 0.85, y), Pos2::new(ctr.x - u * 0.1, y), Pos2::new(ctr.x, ctr.y), Pos2::new(ctr.x + u * 0.45, ctr.y)],
                    false,
                    Color32::TRANSPARENT,
                    Stroke::new(1.8_f32, c),
                )));
            }
            p.circle_filled(Pos2::new(ctr.x + u * 0.62, ctr.y), u * 0.2, c);
        }
        Mode::Album => {
            // A vinyl record.
            p.circle_stroke(ctr, u * 0.85, s);
            p.circle_stroke(ctr, u * 0.55, Stroke::new(1.0_f32, c.gamma_multiply(0.5)));
            p.circle_filled(ctr, u * 0.25, c);
            p.circle_filled(ctr, u * 0.06, theme::BG);
        }
        Mode::Film => {
            // A clapperboard.
            let body = Rect::from_min_max(Pos2::new(ctr.x - u * 0.8, ctr.y - u * 0.25), Pos2::new(ctr.x + u * 0.8, ctr.y + u * 0.75));
            p.rect_stroke(body, 3.0, s, egui::StrokeKind::Middle);
            let top = [Pos2::new(ctr.x - u * 0.8, ctr.y - u * 0.3), Pos2::new(ctr.x + u * 0.7, ctr.y - u * 0.75), Pos2::new(ctr.x + u * 0.8, ctr.y - u * 0.45), Pos2::new(ctr.x - u * 0.7, ctr.y - u * 0.02)];
            p.add(egui::Shape::closed_line(top.to_vec(), s));
            for k in 0..3 {
                let a = top[0] + (top[1] - top[0]) * (0.2 + 0.28 * k as f32);
                p.line_segment([a, a + Vec2::new(u * 0.12, u * 0.27)], Stroke::new(1.5_f32, c));
            }
        }
    }
}

/// The start screen. Returns `false` once it is closed (a choice was made or it was skipped).
pub fn show(ctx: &egui::Context, st: &mut State) -> bool {
    let screen = ctx.screen_rect();
    let mut open = true;
    egui::Area::new(egui::Id::new("dragon.start")).order(egui::Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
        let (bg, _) = ui.allocate_exact_size(screen.size(), egui::Sense::click());
        ui.painter().rect_filled(bg, 0.0, Color32::from_rgba_unmultiplied(12, 8, 8, 245));
        let w = (screen.width() - 80.0).min(1040.0);
        let card_w = ((w - 3.0 * 16.0) / 4.0).floor();
        let top = screen.top() + (screen.height() * 0.08).max(30.0);
        let left = screen.center().x - w / 2.0;
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_size(Pos2::new(left, top), Vec2::new(w, screen.bottom() - top - 20.0))));
        let ui = &mut child;
        ui.horizontal(|ui| {
            if let Some(tex) = crate::brand::mark(ui.ctx()) {
                ui.add(egui::Image::new(&tex).fit_to_exact_size(Vec2::splat(54.0)).corner_radius(8.0));
            }
            ui.vertical(|ui| {
                ui.label(RichText::new("What do you want to do?").size(28.0).strong().color(Color32::WHITE));
                ui.label(RichText::new("Pick a start and Dragon DAW sets up the project and the views for it. You can always change everything later.").size(13.0).color(theme::TEXT_DIM));
            });
        });
        ui.add_space(16.0);
        // The tour: a big, obvious first step for new users – three short chapters, Basics first.
        {
            let (r, _) = ui.allocate_exact_size(Vec2::new(w, 92.0), egui::Sense::hover());
            let t = ui.input(|i| i.time) as f32;
            let pulse = 0.55 + 0.45 * (t * 2.4).sin().abs();
            let p = ui.painter().clone();
            p.rect_filled(r, 14.0, Color32::from_rgb(0x30, 0x1c, 0x12));
            p.rect_stroke(r, 14.0, Stroke::new(2.0 + pulse, theme::ACCENT_2.gamma_multiply(0.5 + 0.5 * pulse)), egui::StrokeKind::Inside);
            p.text(Pos2::new(r.left() + 22.0, r.top() + 22.0), egui::Align2::LEFT_CENTER, "New here? Take the tour", egui::FontId::proportional(22.0), Color32::WHITE);
            p.text(
                Pos2::new(r.left() + 22.0, r.top() + 44.0),
                egui::Align2::LEFT_CENTER,
                "Short guided chapters that point at the real controls – Help › Take the tour brings them back any time.",
                egui::FontId::proportional(12.5),
                theme::TEXT_DIM,
            );
            let mut x = r.left() + 22.0;
            for (k, (kind, title, mins)) in crate::tutorial::TourKind::CHAPTERS.into_iter().enumerate() {
                let label = format!("▶  {}. {title}  ·  {mins} min", k + 1);
                let first = k == 0;
                let b = egui::Button::new(RichText::new(label).size(14.0).strong().color(if first { Color32::BLACK } else { Color32::WHITE }))
                    .fill(if first { theme::ACCENT_2 } else { Color32::from_white_alpha(18) })
                    .corner_radius(8.0)
                    .min_size(Vec2::new(0.0, 28.0));
                let br = Rect::from_min_size(Pos2::new(x, r.bottom() - 38.0), Vec2::new(200.0, 28.0));
                let resp = ui.put(br, b);
                x += 210.0;
                if resp.clicked() {
                    open = false;
                    crate::tutorial::start_chapter(st, kind);
                }
            }
            ui.ctx().request_repaint();
        }
        ui.add_space(16.0);
        let mut chosen = None;
        for row in MODES.chunks(4) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 16.0;
                for (mode, _, title, sub) in row {
                    let (r, resp) = ui.allocate_exact_size(Vec2::new(card_w, 150.0), egui::Sense::click());
                    let hot = resp.hovered();
                    let p = ui.painter();
                    let lift = if hot { -3.0 } else { 0.0 };
                    let r = r.translate(Vec2::new(0.0, lift));
                    p.rect_filled(r, 12.0, if hot { Color32::from_rgb(0x3a, 0x1c, 0x16) } else { Color32::from_rgb(0x22, 0x18, 0x18) });
                    p.rect_stroke(r, 12.0, Stroke::new(if hot { 2.0_f32 } else { 1.0 }, if hot { theme::ACCENT } else { Color32::from_white_alpha(28) }), egui::StrokeKind::Inside);
                    icon(p, Rect::from_center_size(Pos2::new(r.left() + 40.0, r.top() + 40.0), Vec2::splat(44.0)), *mode, if hot { theme::ACCENT_2 } else { theme::ACCENT });
                    p.text(Pos2::new(r.left() + 16.0, r.top() + 80.0), egui::Align2::LEFT_TOP, *title, egui::FontId::proportional(17.0), Color32::WHITE);
                    let g = p.layout(sub.to_string(), egui::FontId::proportional(11.5), theme::TEXT_DIM, card_w - 32.0);
                    p.galley(Pos2::new(r.left() + 16.0, r.top() + 104.0), g, theme::TEXT_DIM);
                    if hot {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if resp.clicked() {
                        chosen = Some(*mode);
                    }
                }
            });
            ui.add_space(16.0);
        }
        // Recent projects and the way out.
        ui.add_space(4.0);
        let recent: Vec<std::path::PathBuf> = st.settings.recent.iter().take(5).cloned().collect();
        let mut open_path = None;
        ui.horizontal(|ui| {
            ui.label(RichText::new("Recent").color(theme::TEXT_DIM));
            if recent.is_empty() {
                ui.label(RichText::new("none yet").color(theme::TEXT_DIM).italics());
            }
            for path in &recent {
                let name = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                if ui.button(&name).on_hover_text(path.display().to_string()).clicked() {
                    open_path = Some(path.clone());
                }
            }
            if ui.button("Open…").clicked() {
                open = false;
                st.open_dialog();
            }
        });
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui.button(RichText::new("Just an empty project").size(13.0)).clicked() {
                open = false;
            }
            if ui.button(RichText::new("From a template…").size(13.0)).on_hover_text("Mixing and beat templates with group buses, your own templates, and the default for File › New").clicked() {
                open = false;
                st.templates_open = true;
            }
            let mut show = st.settings.show_start;
            if ui.checkbox(&mut show, "Show this at start").changed() {
                st.settings.show_start = show;
                st.save_settings();
            }
        });
        if let Some(m) = chosen {
            start(st, m);
            open = false;
        }
        if let Some(path) = open_path {
            st.open_path(&path);
            open = false;
        }
    });
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        open = false;
    }
    open
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> State {
        State::new(redhive_client_framework::AppContext::new("en"), crate::state::Settings { show_splash: false, ..Default::default() })
    }

    #[test]
    fn stone_chain_is_compressor_clipper_stone() {
        let mut p = Project::default();
        add_stone_chain(&mut p);
        let chain: Vec<&str> = p.mixer.master.inserts.iter().map(|i| i.effect.as_str()).collect();
        assert_eq!(chain, ["compressor", "clipper", "dragon_particle"]);
        add_stone_chain(&mut p);
        assert_eq!(p.mixer.master.inserts.len(), 3, "nothing doubled");
    }

    #[test]
    fn every_start_sets_up_a_working_project() {
        for (mode, key, _, _) in MODES {
            let mut st = state();
            start(&mut st, mode);
            let p = &st.session.project;
            assert_eq!(p.mode, key);
            assert!(st.session.history.undo_len() == 0, "{key}: the setup is not an undo step");
            let names: Vec<&str> = p.tracks.iter().map(|t| t.name.as_str()).collect();
            let chain: Vec<&str> = p.mixer.master.inserts.iter().map(|i| i.effect.as_str()).collect();
            match mode {
                Mode::Beat => {
                    assert!(!p.channels.is_empty(), "a kit");
                    assert!(p.patterns.iter().all(|pt| pt.channels.values().all(|c| c.notes.is_empty())), "nothing written yet");
                    assert!(p.tracks.iter().all(|t| t.clips.is_empty()));
                }
                Mode::Record => assert!(p.tracks.len() == 1 && p.tracks[0].armed),
                Mode::Mix => assert!(p.tracks.is_empty()),
                Mode::Master => {
                    assert_eq!(names, ["Mixdown"]);
                    assert_eq!(chain.last(), Some(&"limiter"));
                }
                Mode::Stems => {
                    assert_eq!(names, ["Drums", "Melody", "All vocals", "SFX"]);
                    // Each group track goes to its own bus.
                    let buses: std::collections::HashSet<Id> = p.tracks.iter().map(|t| t.mixer).collect();
                    assert_eq!(buses.len(), 4);
                    assert!(chain.contains(&"multiband"));
                }
                Mode::Album => {
                    assert!(p.mixer.channels.iter().any(|c| c.name == ALBUM_BUS));
                    assert_eq!(chain, ["eq", "multiband", "imager", "limiter"]);
                }
                Mode::Film => {
                    assert_eq!(names, ["Dialogue", "Music", "Effects", "Foley", "Ambience"]);
                    assert_eq!(p.loudness_target, -23.0);
                }
            }
            st.session.compile(48000.0);
        }
    }
}
