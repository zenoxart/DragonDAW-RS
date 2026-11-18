//! The tour: an optional click-through tutorial over the real interface. Each step dims the screen
//! except the part it talks about (the play button, the channel rack, the mixer …), says what it is for
//! and, where it fits, gives a small "try it" goal that ticks off when done. Next / Back / Skip at any
//! time; it starts from the start screen or Help › Take the tour, and finishing it completes a quest.

use crate::state::State;
use crate::theme;
use egui::{Color32, Context, Id, Pos2, Rect, RichText, Stroke, Vec2};

/// Records where `key` (a view key such as "dragon.mixer", or "transport.play" …) is on screen this frame.
pub fn anchor(ctx: &Context, key: &str, rect: Rect) {
    let frame = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| d.insert_temp(Id::new(("tour.anchor", key)), (rect, frame)));
}

/// Where `key` was drawn (this or the last frame).
fn anchor_rect(ctx: &Context, key: &str) -> Option<Rect> {
    let now = ctx.cumulative_pass_nr();
    ctx.data(|d| d.get_temp::<(Rect, u64)>(Id::new(("tour.anchor", key)))).filter(|(_, f)| now.saturating_sub(*f) <= 2).map(|(r, _)| r)
}

/// A step's small goal: what to do, and whether it is done.
pub type Goal = (&'static str, fn(&State) -> bool);

pub struct Step {
    /// What to point at (an anchor key); `None` = a card in the middle.
    pub target: Option<&'static str>,
    /// A view to open first.
    pub open: Option<&'static str>,
    pub title: &'static str,
    pub text: &'static str,
    /// A small goal; the step shows it ticked once `done` says so.
    pub try_it: Option<Goal>,
}

fn steps_on(st: &State) -> usize {
    st.session.project.patterns.iter().flat_map(|p| p.channels.values()).map(|c| c.steps.iter().filter(|s| s.on).count()).sum()
}

pub const STEPS: &[Step] = &[
    Step {
        target: None,
        open: None,
        title: "Welcome to Dragon DAW",
        text: "A two-minute tour of the parts you will use most. Every step points at the real thing – try it as you go, or just click Next. You can leave any time.",
        try_it: None,
    },
    Step {
        target: Some("transport.play"),
        open: None,
        title: "Play and stop",
        text: "Space plays and pauses, Ctrl+Space stops. PAT plays the pattern you are editing, SONG the whole playlist.",
        try_it: Some(("Press play (or Space)", |st| st.transport().playing)),
    },
    Step { target: Some("transport.tempo"), open: None, title: "Tempo", text: "Drag the BPM up or down, double-click to type it, or tap the tempo with TAP.", try_it: None },
    Step {
        target: Some("dragon.rack"),
        open: Some("dragon.rack"),
        title: "Channel rack",
        text: "Your instruments and samples. Click the steps to write a drum pattern; right-click a channel for its piano roll, its sound and its mixer insert.",
        try_it: Some(("Switch on 4 steps", |st| steps_on(st) >= 4)),
    },
    Step {
        target: Some("dragon.piano"),
        open: Some("dragon.piano"),
        title: "Piano roll",
        text: "Melodies and chords: click to draw notes, drag to move or lengthen them. Chord stamps, scales and the arpeggiator are in its toolbar.",
        try_it: None,
    },
    Step {
        target: Some("dragon.playlist"),
        open: Some("dragon.playlist"),
        title: "Playlist",
        text: "The song from left to right. Drag patterns, audio files and automation onto the tracks; cut with the slice tool; drag a cut piece below the last track for a grouped track.",
        try_it: None,
    },
    Step { target: Some("dragon.browser"), open: Some("dragon.browser"), title: "Browser", text: "Samples, presets and plugins. Click a sound to hear it, drag it into the rack or the playlist.", try_it: None },
    Step {
        target: Some("dragon.mixer"),
        open: Some("dragon.mixer"),
        title: "Mixer",
        text: "Every channel's level, pan and effects. Click a channel to see its cables, drag an effect onto another channel to copy it, save a chain to reuse it.",
        try_it: None,
    },
    Step {
        target: Some("dragon.master"),
        open: Some("dragon.master"),
        title: "Mastering",
        text: "The finish: loudness and true peak, the master chain as a signal flow, a reference song to compare with – and the album tools for several songs.",
        try_it: None,
    },
    Step { target: Some("quests"), open: None, title: "Quests and your dragon", text: "Quests teach the rest step by step and grow your dragon. Click the badge any time to see what is next.", try_it: None },
    Step {
        target: None,
        open: None,
        title: "That's it!",
        text: "File › Start screen sets a project up for a beat, a recording, a mix, a master or film sound; Help › Take the tour brings this back. Have fun.",
        try_it: None,
    },
];

/// The recording tour: from the interface to a clean take.
pub const RECORD_STEPS: &[Step] = &[
    Step {
        target: None,
        open: None,
        title: "Recording, step by step",
        text: "You need an audio interface (or your computer's mic input), your mic or instrument, and headphones – speakers would leak into the microphone. This walks you through one good take.",
        try_it: None,
    },
    Step {
        target: Some("mixer.add_rec"),
        open: Some("dragon.mixer"),
        title: "Add a recording channel",
        text: "Click + record in the mixer. It makes a mixer channel and an armed track that records into it. Give every mic or instrument its own.",
        try_it: Some(("Add a recording channel", |st| st.session.project.tracks.iter().any(|t| t.armed))),
    },
    Step {
        target: Some("mixer.rec_input"),
        open: Some("dragon.mixer"),
        title: "Pick the input",
        text: "Choose where it is plugged in: In 1 for one mic, a guitar or a bass (mono); In 1+2 for a keyboard or a stereo pair. No interface? Choose it in Options › Audio first.",
        try_it: Some(("Open an input device", |st| st.audio.can_record())),
    },
    Step {
        target: Some("mixer.rec_strip"),
        open: Some("dragon.mixer"),
        title: "Set the level",
        text: "Play or sing the loudest part and watch the channel's meter. Turn the gain knob on your interface – not the fader – until the peaks sit around −12 to −6 dB: green, never red. Too quiet can be fixed later; clipping cannot.",
        try_it: None,
    },
    Step {
        target: Some("mixer.rec_monitor"),
        open: Some("dragon.mixer"),
        title: "Hear yourself",
        text: "Auto plays your input through the channel while it is armed, effects included. Hear an echo or a delay? Choose Off and use your interface's direct monitoring, or lower the buffer size in Options › Audio (128–256).",
        try_it: None,
    },
    Step {
        target: Some("transport.click"),
        open: None,
        title: "Click and count-in",
        text: "CLICK turns on the metronome and COUNT gives you a bar to get ready, so you come in on time. With headphones on, the click is not recorded.",
        try_it: None,
    },
    Step {
        target: Some("transport.record"),
        open: None,
        title: "Record",
        text: "Press R or this button. After the count-in, perform; Space stops. The take lands on the armed track. With LOOP on, every pass is kept as a take, so you can pick the best parts.",
        try_it: Some(("Start a recording", |st| st.ui.record_started.is_some())),
    },
    Step {
        target: None,
        open: None,
        title: "After the take",
        text: "Drag the small squares at a clip's top corners to fade it in and out. Double-click the clip to trim it, clean noise, hum and clicks with Audio repair, or correct the pitch. Disarm the track when you're done.",
        try_it: None,
    },
];

/// Which tour runs.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum TourKind {
    /// The general tour (Help › Take the tour).
    #[default]
    Basics,
    /// How to record (Help › How to record, the mixer's recording panel).
    Recording,
}

fn steps(kind: TourKind) -> &'static [Step] {
    match kind {
        TourKind::Basics => STEPS,
        TourKind::Recording => RECORD_STEPS,
    }
}

/// The tour's state.
#[derive(Default)]
pub struct Tour {
    /// The step shown, when the tour is running.
    pub step: Option<usize>,
    /// The step whose view was opened already.
    opened: Option<usize>,
    pub kind: TourKind,
}

pub fn start(st: &mut State) {
    st.ui.tour = Tour { step: Some(0), opened: None, kind: TourKind::Basics };
}

/// Starts the recording tour.
pub fn start_recording(st: &mut State) {
    st.ui.tour = Tour { step: Some(0), opened: None, kind: TourKind::Recording };
}

fn finish(st: &mut State, completed: bool) {
    st.ui.tour.step = None;
    if completed && st.ui.tour.kind == TourKind::Basics {
        crate::views::quests::event(st, crate::views::quests::Event::TourDone);
    }
}

/// Draws the tour, when it runs; call once per frame after the panels.
pub fn show(ctx: &Context, st: &mut State) {
    let Some(k) = st.ui.tour.step else { return };
    let all = steps(st.ui.tour.kind);
    let Some(step) = all.get(k) else {
        finish(st, true);
        return;
    };
    if st.ui.tour.opened != Some(k) {
        if let Some(v) = step.open {
            st.ctx.open(v, None);
        }
        st.ui.tour.opened = Some(k);
    }
    let screen = ctx.screen_rect();
    let target = step.target.and_then(|t| anchor_rect(ctx, t)).map(|r| r.expand(6.0).intersect(screen));
    let t = ctx.input(|i| i.time) as f32;
    // Dim everything but the target (painted only: the target stays clickable).
    let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, Id::new("tour.dim")));
    let dim = Color32::from_rgba_unmultiplied(0, 0, 0, 165);
    match target {
        Some(h) => {
            p.rect_filled(Rect::from_min_max(screen.min, Pos2::new(screen.max.x, h.top())), 0.0, dim);
            p.rect_filled(Rect::from_min_max(Pos2::new(screen.min.x, h.bottom()), screen.max), 0.0, dim);
            p.rect_filled(Rect::from_min_max(Pos2::new(screen.min.x, h.top()), Pos2::new(h.left(), h.bottom())), 0.0, dim);
            p.rect_filled(Rect::from_min_max(Pos2::new(h.right(), h.top()), Pos2::new(screen.max.x, h.bottom())), 0.0, dim);
            let pulse = 0.5 + 0.5 * (t * 3.0).sin();
            p.rect_stroke(h, 8.0, Stroke::new(2.0 + pulse, theme::ACCENT), egui::StrokeKind::Outside);
        }
        None => {
            p.rect_filled(screen, 0.0, dim);
        }
    }
    ctx.request_repaint();
    // The card: beside the target where there is room, else in the middle.
    let card_w = 360.0;
    let card_h = 190.0;
    let pos = match target {
        Some(h) => {
            let below = Pos2::new((h.center().x - card_w / 2.0).clamp(screen.left() + 12.0, screen.right() - card_w - 12.0), h.bottom() + 14.0);
            let above = Pos2::new(below.x, h.top() - card_h - 14.0);
            let right = Pos2::new(h.right() + 14.0, (h.center().y - card_h / 2.0).clamp(screen.top() + 12.0, screen.bottom() - card_h - 12.0));
            let left = Pos2::new(h.left() - card_w - 14.0, right.y);
            // A big target (a whole view): the card sits in its bottom-right corner.
            if h.area() > screen.area() * 0.35 {
                Pos2::new(h.right() - card_w - 16.0, h.bottom() - card_h - 16.0).max(screen.min + Vec2::splat(8.0))
            } else if below.y + card_h < screen.bottom() - 8.0 {
                below
            } else if above.y > screen.top() + 8.0 {
                above
            } else if right.x + card_w < screen.right() - 8.0 {
                right
            } else if left.x > screen.left() + 8.0 {
                left
            } else {
                screen.center() - Vec2::new(card_w, card_h) / 2.0
            }
        }
        None => screen.center() - Vec2::new(card_w, card_h) / 2.0,
    };
    let mut go: Option<isize> = None;
    let mut skip = false;
    egui::Area::new(Id::new("tour.card")).order(egui::Order::Tooltip).fixed_pos(pos).show(ctx, |ui| {
        egui::Frame::NONE.fill(Color32::from_rgb(0x20, 0x16, 0x14)).stroke(Stroke::new(1.5_f32, theme::ACCENT)).corner_radius(10.0).inner_margin(egui::Margin::same(14)).show(ui, |ui| {
            ui.set_width(card_w - 28.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{} / {}", k + 1, all.len())).small().color(theme::TEXT_DIM));
                // Progress dots.
                for j in 0..all.len() {
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(7.0), egui::Sense::hover());
                    ui.painter().circle_filled(r.center(), 2.5, if j <= k { theme::ACCENT } else { Color32::from_white_alpha(40) });
                }
            });
            ui.label(RichText::new(step.title).size(17.0).strong().color(Color32::WHITE));
            ui.label(RichText::new(step.text).color(theme::TEXT));
            if let Some((goal, done)) = step.try_it {
                let ok = done(st);
                ui.add_space(4.0);
                ui.label(RichText::new(format!("{}  Try it: {goal}", if ok { "✔" } else { "○" })).color(if ok { theme::OK } else { theme::ACCENT_2 }));
            }
            if step.target.is_some() && target.is_none() {
                ui.label(RichText::new("(It is not on screen right now – it opens with its view.)").small().color(theme::TEXT_DIM));
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("Skip tour").clicked() {
                    skip = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let last = k + 1 == all.len();
                    let next = egui::Button::new(RichText::new(if last { "Finish" } else { "Next ›" }).strong().color(Color32::BLACK)).fill(theme::ACCENT);
                    if ui.add(next).clicked() {
                        go = Some(1);
                    }
                    if k > 0 && ui.button("‹ Back").clicked() {
                        go = Some(-1);
                    }
                });
            });
        });
    });
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        skip = true;
    }
    if skip {
        finish(st, false);
    } else if let Some(d) = go {
        let n = k as isize + d;
        if n as usize >= all.len() {
            finish(st, true);
        } else {
            st.ui.tour.step = Some(n.max(0) as usize);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> State {
        State::new(redhive_client_framework::AppContext::new("en"), crate::state::Settings { show_splash: false, ..Default::default() })
    }

    #[test]
    fn the_tour_runs_through_and_completes_its_quest() {
        let mut st = state();
        st.new_demo_project();
        start(&mut st);
        let ctx = Context::default();
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(1400.0, 900.0));
        // Click Next on every step (the card's Next button is found by its position each frame).
        for k in 0..STEPS.len() {
            assert_eq!(st.ui.tour.step, Some(k));
            let mut next_at = None;
            for frame in 0..3 {
                let mut events = Vec::new();
                if frame == 2 {
                    let at: Pos2 = next_at.expect("Next button drawn");
                    events = vec![
                        egui::Event::PointerMoved(at),
                        egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() },
                        egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() },
                    ];
                }
                let out = ctx.run(egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| anchor(ui.ctx(), "transport.play", Rect::from_min_size(Pos2::new(100.0, 10.0), Vec2::new(36.0, 30.0))));
                    show(ctx, &mut st);
                });
                // The card's Next / Finish button: the right-most, lowest widget of the card.
                let _ = out;
                let card = ctx.memory(|m| m.area_rect(Id::new("tour.card"))).expect("card drawn");
                next_at = Some(Pos2::new(card.right() - 40.0, card.bottom() - 24.0));
            }
        }
        assert_eq!(st.ui.tour.step, None, "finished");
        assert!(st.quests.events_pending(crate::views::quests::Event::TourDone), "the tour quest is reported");
    }

    #[test]
    fn every_step_has_words_and_a_real_view() {
        for s in STEPS.iter().chain(RECORD_STEPS) {
            assert!(!s.title.is_empty() && s.text.len() > 20);
            if let Some(v) = s.open {
                // It points at the view, or at something inside it ("dragon.mixer" → "mixer.add_rec").
                let inside = format!("{}.", v.trim_start_matches("dragon."));
                assert!(s.target == Some(v) || s.target.is_some_and(|t| t.starts_with(&inside)), "{} points into its view", s.title);
            }
        }
    }

    #[test]
    fn the_recording_tour_starts_on_its_own_steps() {
        let mut st = state();
        start_recording(&mut st);
        assert_eq!(st.ui.tour.kind, TourKind::Recording);
        let ctx = Context::default();
        let _ = ctx.run(egui::RawInput::default(), |ctx| show(ctx, &mut st));
        assert_eq!(st.ui.tour.step, Some(0));
        // Skipping does not count as the general tour.
        finish(&mut st, true);
        assert!(!st.quests.events_pending(crate::views::quests::Event::TourDone));
    }
}
