//! The auto-balance window: a step-by-step progress view while the song is measured, then the result in
//! plain words – channels grouped (vocals, drums, low end, music, effects), each with what it was taken
//! for, a level bar showing where it sat before and where it sits now, how far its fader moved and
//! anything that needs a hand. The mix style, a Before / After switch to compare by ear, "keep my fader"
//! per channel and role corrections all re-balance at once from the same measurement.

use crate::state::State;
use crate::theme;
use dragon_engine::balance::{Entry, Group, Role, RoleSource, Style};
use dragon_engine::model::Id;
use egui::{Align, Color32, CornerRadius, Layout, RichText, Sense, Stroke, Ui, Vec2};

enum Action {
    None,
    Role(Id, Option<Role>),
    Style(Style),
    Keep(Id),
    Hear(bool),
    Remeasure,
    Revert,
    Done,
}

fn group_color(g: Group) -> Color32 {
    match g {
        Group::Vocals => Color32::from_rgb(0xe8, 0x6a, 0xa8),
        Group::Drums => theme::ACCENT_2,
        Group::LowEnd => Color32::from_rgb(0x9a, 0x7a, 0xff),
        Group::Music => Color32::from_rgb(0x4a, 0xc8, 0xb0),
        Group::Fx => Color32::from_rgb(0x5a, 0x9a, 0xff),
    }
}

/// The level range the bars show, under the loudest channel (dB).
const RANGE_DB: f32 = 24.0;

pub fn window(ctx: &egui::Context, st: &mut State) {
    let running = st.balance_job.as_ref().filter(|j| !j.hidden).map(|j| j.progress.load(std::sync::atomic::Ordering::Relaxed) as f32 / 1000.0);
    let report_open = st.balance_report.as_ref().is_some_and(|r| r.open);
    if running.is_none() && !report_open {
        return;
    }
    let mut open = true;
    let mut act = Action::None;
    egui::Window::new("Auto-balance").id(egui::Id::new("auto_balance_report")).open(&mut open).collapsible(false).resizable(true).default_size([780.0, 560.0]).min_width(640.0).show(ctx, |ui| {
        if let Some(p) = running {
            progress(ui, p);
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
            return;
        }
        let Some(rep) = st.balance_report.clone() else { return };
        header(ui, &rep);
        ui.add_space(8.0);
        controls(ui, &rep, &mut act);
        ui.add_space(6.0);
        ui.separator();
        let footer_h = 44.0;
        egui::ScrollArea::vertical().max_height((ui.available_height() - footer_h).max(120.0)).auto_shrink([false, true]).show(ui, |ui| list(ui, &rep, &mut act));
        ui.separator();
        footer(ui, &mut act);
    });
    match act {
        Action::None => {}
        Action::Role(id, r) => st.set_balance_role(id, r),
        Action::Style(s) => st.set_balance_style(s),
        Action::Keep(id) => st.toggle_balance_keep(id),
        Action::Hear(b) => st.hear_balance_before(b),
        Action::Remeasure => st.ctx.run("dragon.mixer.autobalance", serde_json::Value::Null),
        Action::Revert => st.revert_balance(),
        Action::Done => st.close_balance(),
    }
    if !open {
        if let Some(j) = &mut st.balance_job {
            // Measuring goes on in the background; the result opens when it's done.
            j.hidden = true;
        } else {
            st.close_balance();
        }
    }
}

/// While measuring: the three steps, the current one lit, with a progress bar.
fn progress(ui: &mut Ui, p: f32) {
    ui.add_space(6.0);
    ui.label(RichText::new("Listening to your song …").size(18.0).strong());
    ui.label(RichText::new("This plays the song in the background, faster than real time. You can keep working.").color(theme::TEXT_DIM));
    ui.add_space(12.0);
    let step = if p < 0.65 {
        0
    } else if p < 1.0 {
        1
    } else {
        2
    };
    let steps = [
        ("Listening to every channel", "How loud each one plays, how punchy it is and where its sound sits"),
        ("Checking the headroom", "Summing the balanced channels so the master gets room to work"),
        ("Setting the faders", "One undo step – Ctrl+Z takes it all back"),
    ];
    for (k, (title, sub)) in steps.iter().enumerate() {
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(Vec2::splat(26.0), Sense::hover());
            let c = r.center();
            let (fill, txt) = if k < step {
                (theme::OK, theme::BG)
            } else if k == step {
                (theme::ACCENT, Color32::WHITE)
            } else {
                (theme::PANEL_2, theme::TEXT_DIM)
            };
            ui.painter().circle_filled(c, 11.0, fill);
            if k < step {
                // A tick.
                ui.painter().line_segment([c + Vec2::new(-5.0, 0.0), c + Vec2::new(-1.5, 4.0)], Stroke::new(2.2_f32, txt));
                ui.painter().line_segment([c + Vec2::new(-1.5, 4.0), c + Vec2::new(5.5, -4.5)], Stroke::new(2.2_f32, txt));
            } else {
                ui.painter().text(c, egui::Align2::CENTER_CENTER, (k + 1).to_string(), egui::FontId::proportional(13.0), txt);
            }
            ui.vertical(|ui| {
                let col = if k == step { theme::TEXT } else { theme::TEXT_DIM };
                ui.label(RichText::new(*title).strong().color(col));
                ui.label(RichText::new(*sub).small().color(theme::TEXT_DIM));
            });
        });
        ui.add_space(4.0);
    }
    ui.add_space(8.0);
    ui.add(egui::ProgressBar::new(p).show_percentage().animate(true));
}

fn chip(ui: &mut Ui, text: String, color: Color32, hover: &str) {
    egui::Frame::new()
        .fill(theme::PANEL_2)
        .stroke(Stroke::new(1.0_f32, color.gamma_multiply(0.7)))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(egui::Margin::symmetric(10, 4))
        .show(ui, |ui| {
            ui.label(RichText::new(text).color(color));
        })
        .response
        .on_hover_text(hover);
}

fn header(ui: &mut Ui, rep: &crate::state::BalanceReport) {
    ui.label(RichText::new("Every channel's volume is set for an even starting mix. Pan, effects and sends are not touched.").color(theme::TEXT_DIM));
    ui.add_space(6.0);
    ui.horizontal_wrapped(|ui| {
        let n = rep.entries.iter().filter(|e| !e.keep).count();
        chip(ui, format!("{n} channel{} balanced", if n == 1 { "" } else { "s" }), theme::OK, "Channels whose fader auto-balance set");
        let peak = rep.sum.1;
        let ok = peak <= -5.5;
        chip(
            ui,
            format!("Headroom {:.0} dB {}", -peak, if ok { "- good" } else { "- tight" }),
            if ok { theme::OK } else { theme::WARN },
            "How far the loudest peak of all channels together stays under 0 dBFS before the master effects. About 6 dB leaves the master chain room to work.",
        );
        let need = rep.entries.iter().filter(|e| !e.keep && e.short_db.abs() > 0.5).count();
        if need > 0 {
            chip(ui, format!("{need} need a hand"), theme::WARN, "These channels would need their fader outside -40 ... +10 dB. Their rows say what to do.");
        }
        let kept = rep.entries.iter().filter(|e| e.keep).count();
        if kept > 0 {
            chip(ui, format!("{kept} kept as you set them"), theme::TEXT_DIM, "Channels where you chose to keep your own fader");
        }
    });
}

fn controls(ui: &mut Ui, rep: &crate::state::BalanceReport, act: &mut Action) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("Mix style").strong());
        for s in Style::ALL {
            if ui.selectable_label(rep.style == s, s.label()).on_hover_text(s.hint()).clicked() && rep.style != s {
                *act = Action::Style(s);
            }
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let after = ui.selectable_label(!rep.hearing_before, RichText::new("  After  ").strong()).on_hover_text("The balanced faders");
            let before = ui.selectable_label(rep.hearing_before, RichText::new("  Before  ").strong()).on_hover_text("Your old faders – press play and switch to hear the difference");
            if after.clicked() && rep.hearing_before {
                *act = Action::Hear(false);
            }
            if before.clicked() && !rep.hearing_before {
                *act = Action::Hear(true);
            }
            ui.label(RichText::new("Compare").strong());
        });
    });
}

fn list(ui: &mut Ui, rep: &crate::state::BalanceReport, act: &mut Action) {
    let top = rep.entries.iter().map(|e| e.after_lufs()).fold(-200.0f32, f32::max);
    for g in Group::ALL {
        let rows: Vec<&Entry> = rep.entries.iter().filter(|e| e.role.group() == g).collect();
        if rows.is_empty() {
            continue;
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(Vec2::new(10.0, 14.0), Sense::hover());
            ui.painter().circle_filled(r.center(), 5.0, group_color(g));
            ui.label(RichText::new(g.label()).strong().size(15.0));
            ui.label(RichText::new(format!("{} channel{}", rows.len(), if rows.len() == 1 { "" } else { "s" })).small().color(theme::TEXT_DIM));
        });
        for e in rows {
            row(ui, e, top, act);
        }
    }
    ui.add_space(4.0);
}

fn row(ui: &mut Ui, e: &Entry, top: f32, act: &mut Action) {
    let col = group_color(e.role.group());
    egui::Frame::new().fill(theme::PANEL).corner_radius(CornerRadius::same(6)).inner_margin(egui::Margin::symmetric(8, 4)).show(ui, |ui| {
        ui.horizontal(|ui| {
            // Name.
            let (r, _) = ui.allocate_exact_size(Vec2::new(4.0, 22.0), Sense::hover());
            ui.painter().rect_filled(r, 2.0, col);
            ui.allocate_ui_with_layout(Vec2::new(140.0, 22.0), Layout::left_to_right(Align::Center), |ui| {
                ui.set_width(140.0);
                ui.add(egui::Label::new(RichText::new(&e.name).strong()).truncate());
            });

            // Role pill with its menu.
            let how = match e.source {
                RoleSource::Name => "Recognised from its name",
                RoleSource::Sound => "Guessed from its sound – check it",
                RoleSource::User => "Set by you",
            };
            let pill = RichText::new(format!("{}  v", e.role.label())).color(col);
            ui.scope(|ui| {
                ui.set_width(180.0);
                ui.menu_button(pill, |ui| role_menu(ui, e, act)).response.on_hover_text(format!("{how}. Click to change what this channel is."));
            });
            if e.source == RoleSource::Sound {
                ui.label(RichText::new("?").color(theme::WARN)).on_hover_text("No name said what this is, so it was guessed from the sound. If the guess is wrong, pick the right role.");
            }

            // Level bar.
            level_bar(ui, e, top, col);

            // Fader change.
            change(ui, e);

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let mut keep = e.keep;
                if ui.checkbox(&mut keep, "Keep mine").on_hover_text("Leave this channel's fader where you had it").changed() {
                    *act = Action::Keep(e.id);
                }
                if !e.keep && e.short_db > 0.5 {
                    ui.label(RichText::new(format!("source too quiet by {:.0} dB", e.short_db)).small().color(theme::WARN))
                        .on_hover_text("The fader is already at its +10 dB top. Turn the source up instead: the clip's gain, the instrument's volume or the input gain when recording.");
                } else if !e.keep && e.short_db < -0.5 {
                    ui.label(RichText::new(format!("source too loud by {:.0} dB", -e.short_db)).small().color(theme::WARN))
                        .on_hover_text("The fader is already at its -40 dB bottom. Turn the source down instead: the clip's gain or the instrument's volume.");
                }
            });
        });
    });
    ui.add_space(2.0);
}

fn role_menu(ui: &mut Ui, e: &Entry, act: &mut Action) {
    ui.set_min_width(300.0);
    if ui.selectable_label(e.source != RoleSource::User, "Automatic").on_hover_text("From its names, or its sound when no name says").clicked() {
        *act = Action::Role(e.id, None);
        ui.close_menu();
    }
    for g in Group::ALL {
        ui.separator();
        ui.label(RichText::new(g.label()).small().color(group_color(g)));
        for r in Role::ALL.into_iter().filter(|r| r.group() == g) {
            let resp = ui.selectable_label(e.role == r && e.source == RoleSource::User, r.label()).on_hover_text(r.hint());
            if resp.clicked() {
                *act = Action::Role(e.id, Some(r));
                ui.close_menu();
            }
        }
    }
}

/// Where the channel sits under the loudest one: a bar for now, a white tick for before.
fn level_bar(ui: &mut Ui, e: &Entry, top: f32, col: Color32) {
    let (r, resp) = ui.allocate_exact_size(Vec2::new(200.0, 14.0), Sense::hover());
    let p = ui.painter_at(r);
    p.rect_filled(r, 3.0, theme::PANEL_2);
    let x = |lufs: f32| r.left() + ((lufs - (top - RANGE_DB)) / RANGE_DB).clamp(0.0, 1.0) * r.width();
    let after = e.after_lufs();
    let bar = egui::Rect::from_min_max(r.min, egui::pos2(x(after), r.max.y));
    p.rect_filled(bar, 3.0, if e.keep { theme::TEXT_DIM.gamma_multiply(0.6) } else { col.gamma_multiply(0.85) });
    let bx = x(e.before_lufs());
    p.line_segment([egui::pos2(bx, r.top() - 1.0), egui::pos2(bx, r.bottom() + 1.0)], Stroke::new(2.0_f32, Color32::WHITE));
    resp.on_hover_text(format!(
        "Now {:.1} dB under the loudest channel (before: {:.1} dB).\nThe white line is where it was, the bar where it is now.\n\nPlays in {:.0} % of the song, {:.1} LUFS in its busy parts before the fader, {:.0} % of its energy below 250 Hz.",
        top - after,
        top - e.before_lufs(),
        e.profile.presence * 100.0,
        e.profile.active,
        e.profile.low_share() * 100.0
    ));
}

/// The fader move, with a small arrow.
fn change(ui: &mut Ui, e: &Entry) {
    let d = e.fader_db - e.before_db;
    let (r, _) = ui.allocate_exact_size(Vec2::new(86.0, 18.0), Sense::hover());
    let p = ui.painter();
    if e.keep || d.abs() < 0.05 {
        p.text(r.left_center(), egui::Align2::LEFT_CENTER, if e.keep { "kept" } else { "no change" }, egui::FontId::proportional(12.0), theme::TEXT_DIM);
        return;
    }
    let (c, up) = if d > 0.0 { (theme::PLAYHEAD, true) } else { (theme::ACCENT_2, false) };
    let a = r.left_center() + Vec2::new(6.0, 0.0);
    let tri = if up { vec![a + Vec2::new(-5.0, 4.0), a + Vec2::new(5.0, 4.0), a + Vec2::new(0.0, -5.0)] } else { vec![a + Vec2::new(-5.0, -4.0), a + Vec2::new(5.0, -4.0), a + Vec2::new(0.0, 5.0)] };
    p.add(egui::Shape::convex_polygon(tri, c, Stroke::NONE));
    p.text(r.left_center() + Vec2::new(16.0, 0.0), egui::Align2::LEFT_CENTER, format!("{:+.1} dB", d), egui::FontId::proportional(13.0), c);
}

fn footer(ui: &mut Ui, act: &mut Action) {
    ui.horizontal(|ui| {
        if ui.button("Measure again").on_hover_text("Listens to the song again – after you changed sounds, levels or the arrangement").clicked() {
            *act = Action::Remeasure;
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let done = egui::Button::new(RichText::new("   Done   ").strong().color(Color32::WHITE)).fill(theme::ACCENT);
            if ui.add(done).on_hover_text("Keep this balance").clicked() {
                *act = Action::Done;
            }
            if ui.button("Undo balance").on_hover_text("Put every fader back where it was").clicked() {
                *act = Action::Revert;
            }
        });
    });
}
