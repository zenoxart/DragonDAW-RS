//! Interaction tests: views are driven with synthetic egui input (pointer, buttons, modifiers) and the
//! resulting project / view state is checked.

use crate::state::{Settings, State, Tool};
use dragon_engine::model::*;
use egui::{Event, Modifiers, PointerButton, Pos2, Rect};

struct Harness {
    ctx: egui::Context,
    st: State,
}

impl Harness {
    fn new() -> Self {
        let st = State::new(redhive_client_framework::AppContext::new("en"), Settings { show_splash: false, ..Settings::default() });
        Harness { ctx: egui::Context::default(), st }
    }

    fn frame(&mut self, view: fn(&mut egui::Ui, &mut State), events: Vec<Event>, modifiers: Modifiers) {
        let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1400.0, 900.0))), events, modifiers, ..Default::default() };
        let st = &mut self.st;
        let _ = self.ctx.run(raw, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| view(ui, st));
        });
    }

    fn idle(&mut self, view: fn(&mut egui::Ui, &mut State), n: usize) {
        for _ in 0..n {
            self.frame(view, vec![], Modifiers::NONE);
        }
    }

    /// Press at `a`, move to `b` in steps, release.
    fn drag(&mut self, view: fn(&mut egui::Ui, &mut State), a: Pos2, b: Pos2, button: PointerButton, m: Modifiers) {
        self.frame(view, vec![Event::PointerMoved(a)], m);
        self.frame(view, vec![Event::PointerButton { pos: a, button, pressed: true, modifiers: m }], m);
        for k in 1..=8 {
            let p = a + (b - a) * (k as f32 / 8.0);
            self.frame(view, vec![Event::PointerMoved(p)], m);
        }
        self.frame(view, vec![Event::PointerButton { pos: b, button, pressed: false, modifiers: m }], m);
        self.frame(view, vec![], Modifiers::NONE);
    }

    fn click(&mut self, view: fn(&mut egui::Ui, &mut State), a: Pos2, button: PointerButton, m: Modifiers) {
        self.frame(view, vec![Event::PointerMoved(a)], m);
        self.frame(view, vec![Event::PointerButton { pos: a, button, pressed: true, modifiers: m }], m);
        self.frame(view, vec![Event::PointerButton { pos: a, button, pressed: false, modifiers: m }], m);
        self.frame(view, vec![], Modifiers::NONE);
    }

    fn rect(&self, key: &str) -> Rect {
        self.ctx.data(|d| d.get_temp::<Rect>(egui::Id::new(key))).expect("view drew its area")
    }

    fn notes(&self) -> Vec<Note> {
        let ch = self.st.ui.piano_channel.unwrap();
        self.st.session.project.pattern(self.st.ui.pattern).and_then(|p| p.channels.get(&ch)).map(|c| c.notes.clone()).unwrap_or_default()
    }

    /// Screen position of a tick / pitch in the piano roll.
    fn pr_pos(&self, tick: f64, pitch: u8) -> Pos2 {
        let g = self.rect("pr.grid");
        let x = g.left() + ((tick / PPQ as f64 - self.st.ui.pr_x) * self.st.ui.piano_zoom as f64) as f32;
        let y = g.top() + (127 - pitch as i32) as f32 * self.st.ui.key_height - self.st.ui.pr_y + self.st.ui.key_height / 2.0;
        Pos2::new(x, y)
    }
}

fn piano(ui: &mut egui::Ui, st: &mut State) {
    super::piano_roll::show(ui, st);
}

fn playlist(ui: &mut egui::Ui, st: &mut State) {
    super::playlist::show(ui, st);
}

fn piano_with_notes(notes: &[Note]) -> Harness {
    let mut h = Harness::new();
    let ch = h.st.session.project.channels[0].id;
    h.st.ui.piano_channel = Some(ch);
    let pat = h.st.ui.pattern;
    let notes = notes.to_vec();
    h.st.session.edit("setup", |p| p.pattern_mut(pat).unwrap().channels.entry(ch).or_default().notes = notes);
    h.st.ui.snap = 0.25;
    h.idle(piano, 3);
    h
}

#[test]
fn dragging_a_selected_note_moves_the_whole_selection() {
    let a = Note { pitch: 60, start: 0, length: PPQ, velocity: 0.8, ..Default::default() };
    let b = Note { pitch: 64, start: PPQ, length: PPQ, velocity: 0.8, ..Default::default() };
    let mut h = piano_with_notes(&[a, b]);
    h.st.ui.notes = vec![0, 1];
    h.st.ui.tool = Tool::Draw;
    // Grab note a in its middle and drag one beat right and two semitones up.
    let from = h.pr_pos(PPQ as f64 * 0.4, 60);
    let to = h.pr_pos(PPQ as f64 * 1.4, 62);
    h.drag(piano, from, to, PointerButton::Primary, Modifiers::NONE);
    let n = h.notes();
    assert_eq!(n.len(), 2, "moving must not draw notes");
    assert_eq!((n[0].start, n[0].pitch), (PPQ, 62), "{n:?}");
    assert_eq!((n[1].start, n[1].pitch), (2 * PPQ, 66), "selection moved together: {n:?}");
    assert!(!h.st.session.in_gesture());
    // One undo step restores both.
    h.st.session.undo();
    let n = h.notes();
    assert_eq!((n[0].start, n[0].pitch, n[1].start, n[1].pitch), (0, 60, PPQ, 64));
}

#[test]
fn shift_drag_copies_and_draw_drag_places_a_new_note() {
    let a = Note { pitch: 60, start: 0, length: PPQ, velocity: 0.8, ..Default::default() };
    let mut h = piano_with_notes(&[a]);
    h.st.ui.tool = Tool::Draw;
    let from = h.pr_pos(PPQ as f64 * 0.5, 60);
    let to = h.pr_pos(PPQ as f64 * 2.5, 60);
    h.drag(piano, from, to, PointerButton::Primary, Modifiers::SHIFT);
    let n = h.notes();
    assert_eq!(n.len(), 2, "{n:?}");
    assert_eq!(n[0].start, 0, "original stays");
    assert_eq!(n[1].start, 2 * PPQ, "copy moved");
    // Pressing on empty space with the draw tool and dragging places a note where it is released.
    let from = h.pr_pos(PPQ as f64 * 4.1, 67);
    let to = h.pr_pos(PPQ as f64 * 5.1, 69);
    h.drag(piano, from, to, PointerButton::Primary, Modifiers::NONE);
    let n = h.notes();
    assert_eq!(n.len(), 3, "{n:?}");
    assert_eq!((n[2].start, n[2].pitch), (5 * PPQ, 69), "{n:?}");
}

#[test]
fn ctrl_right_drag_zooms_into_the_box_and_ctrl_right_click_zooms_out() {
    let mut h = piano_with_notes(&[]);
    let z0 = h.st.ui.piano_zoom;
    let k0 = h.st.ui.key_height;
    // A box one beat wide and six keys high around the middle of the visible keys.
    let g = h.rect("pr.grid");
    let centre = (127.0 - ((g.center().y - g.top() + h.st.ui.pr_y) / h.st.ui.key_height).floor()) as u8;
    let a = h.pr_pos(0.0, centre + 3);
    let b = h.pr_pos(PPQ as f64, centre - 3);
    h.drag(piano, a, b, PointerButton::Secondary, Modifiers::CTRL);
    h.idle(piano, 40);
    assert!(h.st.ui.pr_anim.is_none(), "animation finished");
    let g = h.rect("pr.grid");
    // One beat now spans the grid width (clamped to the max zoom).
    let expect = (g.width() / 1.0).min(800.0);
    assert!((h.st.ui.piano_zoom - expect).abs() < 2.0, "zoom {} -> {} (expected {expect})", z0, h.st.ui.piano_zoom);
    assert!(h.st.ui.key_height > k0, "keys taller: {k0} -> {}", h.st.ui.key_height);
    assert!(h.notes().is_empty(), "Ctrl + right gesture must not draw or delete");
    let z1 = h.st.ui.piano_zoom;
    let g = h.rect("pr.grid");
    let p = g.center();
    h.click(piano, p, PointerButton::Secondary, Modifiers::CTRL);
    h.idle(piano, 40);
    assert!(h.st.ui.piano_zoom < z1 * 0.6, "zoomed out: {z1} -> {}", h.st.ui.piano_zoom);
}

#[test]
fn arrow_keys_nudge_selected_notes() {
    let a = Note { pitch: 60, start: PPQ, length: PPQ, velocity: 0.8, ..Default::default() };
    let mut h = piano_with_notes(&[a]);
    h.st.ui.notes = vec![0];
    let over = h.pr_pos(PPQ as f64 * 3.0, 50);
    h.frame(piano, vec![Event::PointerMoved(over)], Modifiers::NONE);
    let key = |k: egui::Key, m: Modifiers| vec![Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: m }];
    h.frame(piano, key(egui::Key::ArrowRight, Modifiers::NONE), Modifiers::NONE);
    h.frame(piano, key(egui::Key::ArrowUp, Modifiers::SHIFT), Modifiers::SHIFT);
    let n = h.notes();
    assert_eq!((n[0].start, n[0].pitch), (PPQ + PPQ / 4, 72), "{n:?}");
}

#[test]
fn playlist_area_zoom_sets_time_range_and_track_height() {
    let mut h = Harness::new();
    h.idle(playlist, 3);
    let t = h.rect("pl.timeline");
    let zoom = h.st.ui.playlist_zoom;
    // Box from beat 4 to beat 8 over the first track.
    let x = |beat: f32| t.left() + (beat - h.st.ui.pl_x as f32) * zoom;
    let a = Pos2::new(x(4.0), t.top() + 4.0);
    let b = Pos2::new(x(8.0), t.top() + 40.0);
    let clips = h.st.session.project.tracks.iter().map(|t| t.clips.len()).sum::<usize>();
    h.drag(playlist, a, b, PointerButton::Secondary, Modifiers::CTRL);
    h.idle(playlist, 40);
    assert!((h.st.ui.pl_x - 4.0).abs() < 0.05, "starts at beat 4: {}", h.st.ui.pl_x);
    let expect = (t.width() / 4.0).min(400.0);
    assert!((h.st.ui.playlist_zoom - expect).abs() < 1.0, "{} vs {expect}", h.st.ui.playlist_zoom);
    assert!(h.st.ui.pl_vzoom > 1.4, "tracks taller: {}", h.st.ui.pl_vzoom);
    assert_eq!(clips, h.st.session.project.tracks.iter().map(|t| t.clips.len()).sum::<usize>(), "no clips added or deleted");
    // Middle-drag pans back.
    let before = h.st.ui.pl_x;
    let c = t.center();
    h.drag(playlist, c, c + egui::vec2(200.0, 0.0), PointerButton::Middle, Modifiers::NONE);
    assert!(h.st.ui.pl_x < before, "panned left: {before} -> {}", h.st.ui.pl_x);
}

#[test]
fn ctrl_drag_on_empty_space_box_selects() {
    let g0 = Note { pitch: 60, start: 0, length: PPQ, velocity: 0.8, ..Default::default() };
    let g1 = Note { pitch: 62, start: PPQ, length: PPQ, velocity: 0.8, ..Default::default() };
    let far = Note { pitch: 60, start: 8 * PPQ, length: PPQ, velocity: 0.8, ..Default::default() };
    let mut h = piano_with_notes(&[g0, g1, far]);
    h.st.ui.tool = Tool::Draw;
    let a = h.pr_pos(PPQ as f64 * 2.6, 64);
    let b = h.pr_pos(PPQ as f64 * 0.1, 59);
    h.drag(piano, a, b, PointerButton::Primary, Modifiers::CTRL);
    assert_eq!(h.notes().len(), 3, "box select must not draw");
    let mut sel = h.st.ui.notes.clone();
    sel.sort();
    assert_eq!(sel, vec![0, 1]);
}

fn rack(ui: &mut egui::Ui, st: &mut State) {
    super::rack::show(ui, st);
}

#[test]
fn dropping_a_sample_on_a_rack_channel_replaces_its_sound() {
    let mut h = Harness::new();
    h.idle(rack, 3);
    let rows: (u64, Vec<(Id, Rect)>) = h.ctx.data(|d| d.get_temp(egui::Id::new("rack.rows"))).unwrap();
    let (cid, r) = rows.1[1];
    let count = h.st.session.project.channels.len();
    let path = std::path::PathBuf::from("/samples/new_clap.wav");
    // A browser drag carries the path as the drag payload; release it over the second channel.
    let at = Pos2::new(r.left() + 40.0, r.center().y);
    h.frame(rack, vec![Event::PointerMoved(at)], Modifiers::NONE);
    h.frame(rack, vec![Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE }], Modifiers::NONE);
    egui::DragAndDrop::set_payload(&h.ctx, path.clone());
    h.frame(rack, vec![Event::PointerMoved(at + egui::vec2(5.0, 0.0))], Modifiers::NONE);
    h.frame(rack, vec![Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE }], Modifiers::NONE);
    h.idle(rack, 1);
    let c = h.st.session.project.channel(cid).unwrap();
    match &c.instrument {
        Instrument::Sampler(s) => assert_eq!(s.path, path),
        other => panic!("not a sampler: {other:?}"),
    }
    assert_eq!(h.st.session.project.channels.len(), count, "replaced, not added");
}

fn mastering(ui: &mut egui::Ui, st: &mut State) {
    super::master::show(ui, st);
}

#[test]
fn reference_track_region_is_analysed_and_compared_with_the_mix() {
    // A 6 s reference: quiet first half, loud second half.
    let dir = std::env::temp_dir().join(format!("dragon-ref-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("ref.wav");
    let rate = 44100u32;
    let mut rng = dragon_engine::dsp::Rng::new(5);
    let l: Vec<f32> = (0..rate as usize * 6).map(|i| rng.bipolar() * if i < rate as usize * 3 { 0.05 } else { 0.5 }).collect();
    dragon_engine::samples::write_wav(&path, &l, &l, rate, dragon_engine::samples::BitDepth::Float32, false, false).unwrap();

    let mut h = Harness::new();
    h.st.session.edit("ref", |p| p.reference = Some(ReferenceTrack { path: path.clone(), region: Some((3.0, 6.0)) }));
    // Decode and analyse in the background; keep drawing until both results are in.
    let wait = |h: &mut Harness, what: &dyn Fn(&ReferenceStateProbe) -> bool| {
        for _ in 0..600 {
            h.idle(mastering, 1);
            if what(&probe(&h.st)) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("timed out");
    };
    wait(&mut h, &|p| p.reference_lufs.is_some());
    let loud = probe(&h.st).reference_lufs.unwrap();
    // Selecting the quiet half changes the analysis.
    h.st.session.edit("region", |p| p.reference.as_mut().unwrap().region = Some((0.0, 3.0)));
    wait(&mut h, &|p| p.reference_lufs.is_some_and(|v| v < loud - 10.0));
    // Analyse the mix (the starter beat).
    super::reference::analyse_mix_now(&mut h.st);
    wait(&mut h, &|p| p.mix_lufs.is_some());
    assert!(probe(&h.st).mix_lufs.unwrap() > -40.0, "the starter beat is audible");
    let _ = std::fs::remove_dir_all(dir);
}

struct ReferenceStateProbe {
    reference_lufs: Option<f32>,
    mix_lufs: Option<f32>,
}

fn probe(st: &State) -> ReferenceStateProbe {
    let (r, m) = st.reference.results();
    ReferenceStateProbe { reference_lufs: r.map(|a| a.integrated), mix_lufs: m.map(|a| a.integrated) }
}
