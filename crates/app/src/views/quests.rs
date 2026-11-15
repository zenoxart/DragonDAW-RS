//! Dragon Quests – gamification. Guided quests that teach the workflow (make a beat, write a melody,
//! arrange, mix, route, compare with a reference, save, export, record …), experience points and dragon
//! levels, badges, a daily streak, and a celebration when a quest is done. Progress is kept per user in
//! `quests.json` next to the settings; it can be switched off in the Quests window.

use crate::state::State;
use crate::theme;
use dragon_engine::model::*;
use egui::{Color32, Pos2, Rect, RichText, Stroke, Ui, Vec2};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Things that happen outside the project state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    Saved,
    Exported,
    Recorded,
    MixAnalysed,
    PluginGui,
}

pub struct Quest {
    pub id: &'static str,
    pub title: &'static str,
    /// How to do it.
    pub hint: &'static str,
    pub xp: u32,
    pub icon: &'static str,
}

/// The quests in the order they are suggested.
pub const QUESTS: &[Quest] = &[
    Quest { id: "beat", title: "First beat", hint: "Channel rack: switch on at least 4 steps in a pattern.", xp: 50, icon: "♪" },
    Quest { id: "melody", title: "Melody maker", hint: "Piano roll: draw 8 notes or more.", xp: 50, icon: "🎹" },
    Quest { id: "arrange", title: "Arranger", hint: "Drag patterns from the picker into the playlist: 4 clips.", xp: 60, icon: "⊞" },
    Quest { id: "split", title: "Pattern surgeon", hint: "Clone, split or make a pattern unique (the ⏷ menu on a pattern clip).", xp: 40, icon: "✂" },
    Quest { id: "effect", title: "Sound sculptor", hint: "Mixer: put an effect into an empty slot of an insert.", xp: 60, icon: "⚙" },
    Quest { id: "route", title: "Cable guy", hint: "Mixer: pull a cable from an insert's jack onto another insert (or click its arrow).", xp: 70, icon: "🔌" },
    Quest { id: "automation", title: "Automation artist", hint: "Create an automation lane, then drag its clip from the picker into the playlist.", xp: 60, icon: "↗" },
    Quest { id: "save", title: "Safe keeper", hint: "Save the project (it gets its own folder).", xp: 30, icon: "💾" },
    Quest { id: "reference", title: "Ear training", hint: "Mastering: load a reference song and analyse your mix against it.", xp: 60, icon: "🎧" },
    Quest { id: "plugin", title: "Plugin wrangler", hint: "Open a VST3 / CLAP plugin's own window.", xp: 50, icon: "🔌" },
    Quest { id: "record", title: "Studio session", hint: "Arm a track and record audio.", xp: 80, icon: "⏺" },
    Quest { id: "export", title: "Release day", hint: "File › Export your song.", xp: 100, icon: "🚀" },
];

const LEVELS: &[(&str, u32)] = &[("Hatchling", 0), ("Whelp", 80), ("Fledgling", 200), ("Drake", 360), ("Wyvern", 560), ("Fire dragon", 800), ("Elder dragon", 1100)];

/// Saved progress.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Progress {
    pub enabled: bool,
    pub xp: u32,
    pub done: BTreeSet<String>,
    /// Day number (days since 1970) of the last session and the streak of consecutive days.
    pub last_day: i64,
    pub streak: u32,
    pub best_streak: u32,
}

impl Default for Progress {
    fn default() -> Self {
        Progress { enabled: true, xp: 0, done: BTreeSet::new(), last_day: 0, streak: 0, best_streak: 0 }
    }
}

impl Progress {
    /// (level index, title, xp into the level, xp needed for the next level or 0 at the top).
    pub fn level(&self) -> (usize, &'static str, u32, u32) {
        let i = LEVELS.iter().rposition(|(_, at)| self.xp >= *at).unwrap_or(0);
        let next = LEVELS.get(i + 1).map(|(_, at)| *at - LEVELS[i].1).unwrap_or(0);
        (i, LEVELS[i].0, self.xp - LEVELS[i].1, next)
    }

    /// Counts today for the streak; returns the bonus XP for the first session of a day.
    pub fn visit(&mut self, today: i64) -> u32 {
        if today == self.last_day {
            return 0;
        }
        self.streak = if today == self.last_day + 1 { self.streak + 1 } else { 1 };
        self.best_streak = self.best_streak.max(self.streak);
        self.last_day = today;
        10 + 5 * self.streak.min(6)
    }
}

/// A running celebration (quest done, level up, streak).
#[derive(Debug, Clone)]
pub struct Celebration {
    pub title: String,
    pub subtitle: String,
    pub icon: &'static str,
    pub started: f64,
    pub seed: u64,
}

/// Runtime state of the quests.
pub struct Quests {
    pub progress: Progress,
    pub celebrations: Vec<Celebration>,
    pub window_open: bool,
    last_check: f64,
    events: Vec<Event>,
}

impl Default for Quests {
    fn default() -> Self {
        Quests { progress: Progress::default(), celebrations: Vec::new(), window_open: false, last_check: -10.0, events: Vec::new() }
    }
}

impl Quests {
    pub fn load(paths: &redhive_client_framework::context::Paths) -> Self {
        Quests { progress: paths.load_json("quests.json").unwrap_or_default(), ..Default::default() }
    }
}

/// Records an event; it is evaluated with the next check.
pub fn event(st: &mut State, e: Event) {
    st.quests.events.push(e);
    st.quests.last_check = -10.0;
}

/// Whether a quest's goal is met by the project / the recorded events.
fn achieved(id: &str, p: &Project, events: &[Event]) -> bool {
    let steps_on = p.patterns.iter().flat_map(|pt| pt.channels.values()).map(|c| c.steps.iter().filter(|s| s.on).count()).max().unwrap_or(0);
    let notes: usize = p.patterns.iter().flat_map(|pt| pt.channels.values()).map(|c| c.notes.len()).sum();
    let pattern_clips = p.tracks.iter().flat_map(|t| t.clips.iter()).filter(|c| matches!(c.kind, ClipKind::Pattern { .. })).count();
    match id {
        "beat" => steps_on >= 4,
        "melody" => notes >= 8,
        "arrange" => pattern_clips >= 4,
        "split" => p.patterns.len() >= 2,
        "effect" => p.mixer.channels.iter().any(|c| !c.inserts.is_empty()),
        "automation" => p.tracks.iter().flat_map(|t| t.clips.iter()).any(|c| matches!(c.kind, ClipKind::Automation { .. })),
        "route" => p.mixer.channels.iter().any(|c| !c.sends.is_empty() || (c.main_out && c.output.is_some())),
        "reference" => p.reference.is_some() && events.contains(&Event::MixAnalysed),
        "save" => events.contains(&Event::Saved),
        "export" => events.contains(&Event::Exported),
        "record" => events.contains(&Event::Recorded),
        "plugin" => events.contains(&Event::PluginGui),
        _ => false,
    }
}

fn today() -> i64 {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    (secs / 86_400) as i64
}

/// Awards XP, celebrating level-ups.
fn award(st: &mut State, xp: u32, now: f64) {
    let before = st.quests.progress.level().0;
    st.quests.progress.xp += xp;
    let (after, title, _, _) = st.quests.progress.level();
    if after > before {
        st.quests.celebrations.push(Celebration { title: format!("Level {} – {title}!", after + 1), subtitle: "Your dragon grew".into(), icon: "🐉", started: now, seed: now.to_bits() ^ 0x9e37 });
    }
}

/// Checks the quests (twice a second at most) and the daily streak; call every frame.
pub fn tick(st: &mut State, now: f64) {
    if !st.quests.progress.enabled {
        st.quests.events.clear();
        return;
    }
    if now - st.quests.last_check < 0.5 {
        return;
    }
    st.quests.last_check = now;
    let mut changed = false;
    let bonus = st.quests.progress.visit(today());
    if bonus > 0 {
        let streak = st.quests.progress.streak;
        award(st, bonus, now);
        if streak > 1 {
            st.quests.celebrations.push(Celebration { title: format!("{streak}-day streak!"), subtitle: format!("+{bonus} XP for coming back"), icon: "🔥", started: now, seed: 7 });
        }
        changed = true;
    }
    let events = std::mem::take(&mut st.quests.events);
    for q in QUESTS {
        if st.quests.progress.done.contains(q.id) || !achieved(q.id, &st.session.project, &events) {
            continue;
        }
        st.quests.progress.done.insert(q.id.to_string());
        st.quests.celebrations.push(Celebration { title: format!("Quest complete: {}", q.title), subtitle: format!("+{} XP", q.xp), icon: q.icon, started: now, seed: q.xp as u64 * 31 + q.id.len() as u64 });
        award(st, q.xp, now);
        changed = true;
    }
    if changed {
        st.ctx.paths.save_json("quests.json", &st.quests.progress);
    }
}

/// The next quest to suggest.
pub fn next_quest(st: &State) -> Option<&'static Quest> {
    QUESTS.iter().find(|q| !st.quests.progress.done.contains(q.id))
}

/// The dragon-level badge with an XP bar, for the transport bar. Click opens the quests.
pub fn badge(ui: &mut Ui, st: &mut State) {
    if !st.quests.progress.enabled {
        if ui.small_button("Quests").clicked() {
            st.quests.window_open = true;
        }
        return;
    }
    let (lvl, title, into, need) = st.quests.progress.level();
    let (rect, r) = ui.allocate_exact_size(Vec2::new(170.0, 26.0), egui::Sense::click());
    let p = ui.painter();
    let hover = r.hovered();
    p.rect_filled(rect, 13.0, if hover { theme::PANEL_2 } else { theme::BG });
    p.rect_stroke(rect, 13.0, Stroke::new(1.0_f32, theme::ACCENT.gamma_multiply(0.6)), egui::StrokeKind::Inside);
    // Level gem.
    let c = Pos2::new(rect.left() + 13.0, rect.center().y);
    p.circle_filled(c, 10.0, theme::ACCENT);
    p.text(c, egui::Align2::CENTER_CENTER, format!("{}", lvl + 1), egui::FontId::proportional(11.0), Color32::BLACK);
    p.text(Pos2::new(rect.left() + 28.0, rect.top() + 7.0), egui::Align2::LEFT_CENTER, title, egui::FontId::proportional(10.0), theme::TEXT);
    let bar = Rect::from_min_size(Pos2::new(rect.left() + 28.0, rect.top() + 15.0), Vec2::new(rect.width() - 38.0, 5.0));
    p.rect_filled(bar, 2.5, theme::GRID);
    let f = if need == 0 { 1.0 } else { into as f32 / need as f32 };
    p.rect_filled(Rect::from_min_size(bar.min, Vec2::new(bar.width() * f.clamp(0.0, 1.0), bar.height())), 2.5, theme::ACCENT_2);
    if st.quests.progress.streak > 1 {
        p.text(Pos2::new(rect.right() - 8.0, rect.top() + 7.0), egui::Align2::RIGHT_CENTER, format!("🔥{}", st.quests.progress.streak), egui::FontId::proportional(10.0), theme::PLAYHEAD);
    }
    let tip = match next_quest(st) {
        Some(q) => format!("{} XP – next quest: {} ({})", st.quests.progress.xp, q.title, q.hint),
        None => format!("{} XP – all quests done!", st.quests.progress.xp),
    };
    if r.on_hover_text(tip).clicked() {
        st.quests.window_open = !st.quests.window_open;
    }
}

/// The Quests window and the celebration overlay; call once per frame after the panels.
pub fn show(ctx: &egui::Context, st: &mut State) {
    let now = ctx.input(|i| i.time);
    tick(st, now);
    let mut open = st.quests.window_open;
    egui::Window::new("Dragon Quests").open(&mut open).default_width(420.0).resizable(false).collapsible(false).show(ctx, |ui| window(ui, st));
    st.quests.window_open = open;
    celebrations(ctx, st, now);
}

fn window(ui: &mut Ui, st: &mut State) {
    let (lvl, title, into, need) = st.quests.progress.level();
    ui.horizontal(|ui| {
        dragon_mark(ui, 44.0, lvl);
        ui.vertical(|ui| {
            ui.label(RichText::new(format!("Level {} · {title}", lvl + 1)).heading().color(theme::ACCENT));
            ui.label(RichText::new(if need == 0 { format!("{} XP – top level", st.quests.progress.xp) } else { format!("{} XP · {into} / {need} to the next level", st.quests.progress.xp) }).small());
            ui.label(RichText::new(format!("Streak: {} day(s) · best {}", st.quests.progress.streak, st.quests.progress.best_streak)).small().color(theme::PLAYHEAD));
        });
    });
    ui.separator();
    let done = st.quests.progress.done.len();
    ui.label(RichText::new(format!("QUESTS  {done} / {}", QUESTS.len())).small().color(theme::TEXT_DIM));
    let next = next_quest(st).map(|q| q.id);
    for q in QUESTS {
        let is_done = st.quests.progress.done.contains(q.id);
        let is_next = Some(q.id) == next;
        let fill = if is_next { theme::PANEL_2 } else { theme::BG };
        egui::Frame::NONE.fill(fill).corner_radius(6.0).inner_margin(egui::Margin::symmetric(8, 5)).stroke(Stroke::new(1.0_f32, if is_next { theme::ACCENT } else { theme::GRID })).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(Vec2::splat(22.0), egui::Sense::hover());
                ui.painter().circle_filled(r.center(), 10.0, if is_done { theme::OK } else { theme::GRID_BAR });
                ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, if is_done { "✔" } else { q.icon }, egui::FontId::proportional(12.0), Color32::WHITE);
                ui.vertical(|ui| {
                    ui.label(RichText::new(q.title).strong().color(if is_done { theme::TEXT_DIM } else { theme::TEXT }));
                    if !is_done {
                        ui.label(RichText::new(q.hint).small().color(theme::TEXT_DIM));
                    }
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(format!("+{} XP", q.xp)).small().color(if is_done { theme::OK } else { theme::ACCENT_2 }));
                });
            });
        });
    }
    ui.separator();
    ui.horizontal(|ui| {
        let mut on = st.quests.progress.enabled;
        if ui.checkbox(&mut on, "Show quests and celebrations").changed() {
            st.quests.progress.enabled = on;
            st.ctx.paths.save_json("quests.json", &st.quests.progress);
        }
        if ui.small_button("Reset progress").clicked() {
            st.quests.progress = Progress { enabled: st.quests.progress.enabled, ..Default::default() };
            st.ctx.paths.save_json("quests.json", &st.quests.progress);
        }
    });
}

/// A small dragon emblem whose flame grows with the level.
pub fn dragon_mark(ui: &mut Ui, size: f32, level: usize) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
    let p = ui.painter();
    let t = ui.input(|i| i.time) as f32;
    let c = rect.center();
    p.circle_filled(c, size * 0.48, theme::BG);
    p.circle_stroke(c, size * 0.48, Stroke::new(2.0_f32, theme::ACCENT));
    // Flame: layered teardrops that flicker.
    let flames = 3 + level.min(6);
    for k in 0..flames {
        let a = k as f32 / flames as f32 * std::f32::consts::TAU + t * 0.6;
        let r = size * (0.14 + 0.06 * ((t * 3.0 + k as f32).sin() * 0.5 + 0.5));
        let pos = c + Vec2::new(a.cos(), a.sin()) * size * 0.16;
        p.circle_filled(pos, r, theme::ACCENT.lerp_to_gamma(theme::PLAYHEAD, k as f32 / flames as f32).gamma_multiply(0.85));
    }
    p.circle_filled(c, size * 0.12, Color32::from_rgb(255, 240, 200));
    ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
}

/// Celebration banners with an ember burst; the reduced-motion setting keeps only the banner.
fn celebrations(ctx: &egui::Context, st: &mut State, now: f64) {
    const LIFE: f64 = 3.2;
    st.quests.celebrations.retain(|c| now - c.started < LIFE);
    let Some(c) = st.quests.celebrations.first().cloned() else { return };
    let age = (now - c.started) as f32;
    let screen = ctx.screen_rect();
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("quest.celebration")));
    let fade = if age < 0.25 {
        age / 0.25
    } else if age > LIFE as f32 - 0.5 {
        (LIFE as f32 - age) / 0.5
    } else {
        1.0
    }
    .clamp(0.0, 1.0);
    let slide = (1.0 - (age * 5.0).min(1.0)).powi(2) * 40.0;
    let card = Rect::from_center_size(Pos2::new(screen.center().x, screen.top() + 110.0 - slide), Vec2::new(380.0, 74.0));
    if !st.settings.reduced_motion {
        // Embers bursting from the card.
        let mut rng = c.seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        for _ in 0..70 {
            rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let a = (rng >> 33) as f32 / (1u64 << 31) as f32 * std::f32::consts::TAU;
            rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let speed = 120.0 + (rng >> 33) as f32 / (1u64 << 31) as f32 * 260.0;
            let pos = card.center() + Vec2::new(a.cos(), a.sin() * 0.6) * speed * age + Vec2::new(0.0, 90.0 * age * age);
            let col = if rng % 3 == 0 {
                theme::ACCENT_2
            } else if rng % 3 == 1 {
                theme::ACCENT
            } else {
                theme::PLAYHEAD
            };
            painter.circle_filled(pos, (3.0 - age).max(0.6), col.gamma_multiply(fade * (1.0 - age / LIFE as f32)));
        }
    }
    painter.rect_filled(card.expand(3.0), 14.0, theme::ACCENT.gamma_multiply(0.35 * fade));
    painter.rect_filled(card, 12.0, theme::PANEL.gamma_multiply(fade));
    painter.rect_stroke(card, 12.0, Stroke::new(2.0_f32, theme::ACCENT.gamma_multiply(fade)), egui::StrokeKind::Inside);
    painter.text(Pos2::new(card.left() + 36.0, card.center().y), egui::Align2::CENTER_CENTER, c.icon, egui::FontId::proportional(28.0), Color32::WHITE.gamma_multiply(fade));
    painter.text(Pos2::new(card.left() + 66.0, card.center().y - 11.0), egui::Align2::LEFT_CENTER, &c.title, egui::FontId::proportional(16.0), theme::ACCENT.gamma_multiply(fade));
    painter.text(Pos2::new(card.left() + 66.0, card.center().y + 13.0), egui::Align2::LEFT_CENTER, &c.subtitle, egui::FontId::proportional(12.0), theme::TEXT.gamma_multiply(fade));
    ctx.request_repaint();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_and_streaks() {
        let mut p = Progress::default();
        assert_eq!(p.level().1, "Hatchling");
        p.xp = 210;
        let (i, t, into, need) = p.level();
        assert_eq!((i, t, into, need), (2, "Fledgling", 10, 160));
        p.xp = 5000;
        assert_eq!(p.level().3, 0, "top level");
        assert!(p.visit(100) > 0);
        assert_eq!(p.streak, 1);
        assert_eq!(p.visit(100), 0, "same day: no bonus");
        p.visit(101);
        p.visit(102);
        assert_eq!(p.streak, 3);
        p.visit(110);
        assert_eq!((p.streak, p.best_streak), (1, 3));
    }

    #[test]
    fn quests_follow_the_project() {
        let mut p = Project::new_default();
        assert!(!achieved("route", &p, &[]));
        let b = p.add_mixer_channel("Bus");
        let ch = p.mixer.channels[0].id;
        p.mixer.get_mut(ch).unwrap().sends.push(MixerSend { target: b, level: 1.0, pre_fader: false });
        assert!(achieved("route", &p, &[]));
        assert!(!achieved("export", &p, &[]));
        assert!(achieved("export", &p, &[Event::Exported]));
        let pat = p.patterns[0].id;
        assert!(!achieved("split", &p, &[]));
        crate::views::quests::tests::clone(&mut p, pat);
        assert!(achieved("split", &p, &[]));
    }

    fn clone(p: &mut Project, pat: Id) {
        dragon_engine::edit::clone_pattern(p, pat);
    }
}
