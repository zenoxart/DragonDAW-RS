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
    h.frame(piano, key(egui::Key::ArrowUp, Modifiers::NONE), Modifiers::NONE);
    let n = h.notes();
    assert_eq!((n[0].start, n[0].pitch), (PPQ + PPQ / 4, 61), "{n:?}");
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
        for _ in 0..3000 {
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

// ---- plugin windows (with a fake plugin: the real CLAP / VST3 hosts are tested in dragon-test-plugins)

use dragon_engine::effects::{Effect, PluginParam};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering as AtOrd};
use std::sync::Arc;

/// Shared switches of the fake plugin: whether its GUI may open, whether it is open, a pending GUI edit.
#[derive(Default)]
struct FakeGui {
    allow: AtomicBool,
    open: AtomicBool,
    opened: AtomicU32,
    edit: std::sync::Mutex<Vec<(u32, f64)>>,
}

struct FakePlugin(Arc<FakeGui>);

impl Effect for FakePlugin {
    fn set_param(&mut self, _id: &str, _v: f32) {}
    fn reset(&mut self) {}
    fn process(&mut self, _l: &mut [f32], _r: &mut [f32], _sc: Option<(&[f32], &[f32])>) {}
    fn plugin_params(&self) -> Vec<PluginParam> {
        vec![PluginParam { id: 7, name: "Cutoff".into(), default: 0.5, steps: 0, automatable: true }]
    }
    fn editor(&mut self, open: bool) -> bool {
        let ok = open && self.0.allow.load(AtOrd::Relaxed);
        if ok {
            self.0.opened.fetch_add(1, AtOrd::Relaxed);
        }
        self.0.open.store(ok, AtOrd::Relaxed);
        ok
    }
    fn idle(&mut self) -> bool {
        self.0.open.load(AtOrd::Relaxed)
    }
    fn editor_error(&self) -> Option<String> {
        (!self.0.allow.load(AtOrd::Relaxed)).then(|| "the editor refused to attach".to_string())
    }
    fn take_param_edits(&mut self) -> Vec<(u32, f64)> {
        std::mem::take(&mut *self.0.edit.lock().unwrap())
    }
}

struct FakeFactory(Arc<FakeGui>);

impl dragon_engine::render::PluginFactory for FakeFactory {
    fn create_effect(&mut self, _p: &PluginRef, _rate: f32, _max: usize) -> Option<Box<dyn Effect>> {
        Some(Box::new(FakePlugin(self.0.clone())))
    }
    fn create_instrument(&mut self, _p: &PluginRef, _rate: f32, _max: usize) -> Option<Box<dyn dragon_engine::instruments::InstrumentVoice>> {
        None
    }
}

fn plugin_harness(allow: bool) -> (Harness, Arc<FakeGui>, Id, Id) {
    let mut h = Harness::new();
    let gui = Arc::new(FakeGui::default());
    gui.allow.store(allow, AtOrd::Relaxed);
    h.st.session.pool = dragon_engine::render::Pool::with_plugins(Box::new(FakeFactory(gui.clone())));
    let m = h.st.session.project.mixer.master.id;
    let iid = h.st.session.edit("add", |p| {
        let iid = p.next_id();
        let plugin = PluginRef { format: PluginFormat::Vst3, path: "/plugins/Fake.vst3".into(), id: "00".into(), name: "Fake Synth FX".into(), state: Vec::new(), params: Default::default() };
        p.mixer.master.inserts.push(Insert { id: iid, effect: "plugin".into(), enabled: true, mix: 1.0, params: Default::default(), sidechain: None, plugin: Some(plugin), preset: String::new() });
        iid
    });
    h.st.session.compile(48000.0);
    h.st.ui.effect_windows.push((m, iid));
    (h, gui, m, iid)
}

fn windows_and_pump(ui: &mut egui::Ui, st: &mut State) {
    super::windows::show(ui.ctx(), st);
    st.pump_plugin_editors();
}

#[test]
fn plugin_window_opens_the_plugins_own_gui_by_itself() {
    let (mut h, gui, m, iid) = plugin_harness(true);
    h.idle(windows_and_pump, 3);
    assert!(gui.open.load(AtOrd::Relaxed), "the plugin's GUI opened without a click");
    assert_eq!(gui.opened.load(AtOrd::Relaxed), 1, "opened once, not every frame");
    assert!(h.st.ui.native_editors.contains(&(m, Some(iid))));
    // A knob turned in the plugin's window is stored in the project as one undo step.
    gui.edit.lock().unwrap().push((7, 0.9));
    h.idle(windows_and_pump, 2);
    gui.edit.lock().unwrap().push((7, 0.95));
    h.idle(windows_and_pump, 2);
    let stored = |h: &Harness| h.st.session.project.mixer.master.inserts.iter().find(|x| x.id == iid).unwrap().plugin.as_ref().unwrap().params.get(&7).copied();
    assert_eq!(stored(&h), Some(0.95));
    std::thread::sleep(std::time::Duration::from_millis(450));
    h.idle(windows_and_pump, 1);
    assert!(h.st.ui.plugin_edit_run.is_none(), "the edit run closed after a pause");
    h.st.session.undo();
    assert_eq!(stored(&h), None, "one undo removes the whole knob turn");
    // Closing the plugin window and reopening the effect window opens it again.
    gui.open.store(false, AtOrd::Relaxed);
    h.idle(windows_and_pump, 2);
    assert!(h.st.ui.native_editors.is_empty());
    h.st.ui.effect_windows.clear();
    h.idle(windows_and_pump, 4);
    h.st.ui.effect_windows.push((m, iid));
    h.idle(windows_and_pump, 2);
    assert_eq!(gui.opened.load(AtOrd::Relaxed), 2);
}

#[test]
fn plugin_window_failure_shows_the_reason() {
    let (mut h, gui, m, iid) = plugin_harness(false);
    h.idle(windows_and_pump, 3);
    assert!(!gui.open.load(AtOrd::Relaxed));
    assert!(h.st.ui.native_editors.is_empty());
    let shown = h.ctx.data(|d| d.get_temp::<String>(egui::Id::new(("plugin.window.error", (m, Some(iid)))))).unwrap();
    assert!(shown.contains("refused to attach"), "{shown}");
}

#[test]
fn dragging_from_the_picker_places_patterns_and_automation_clips() {
    let mut h = Harness::new();
    // A second ("split") pattern and an automation lane to pick.
    let lane = h.st.session.edit("setup", |p| {
        let pid = p.add_pattern();
        p.pattern_mut(pid).unwrap().name = "Split 2".into();
        let id = p.next_id();
        let master = p.mixer.master.id;
        p.automation.push(AutomationLane {
            id,
            target: Target::MixerVolume(master),
            points: vec![AutoPoint { beat: 0.0, value: 0.2, tension: 0.0 }, AutoPoint { beat: 8.0, value: 0.9, tension: 0.0 }],
            mode: AutomationMode::Read,
            visible: true,
        });
        id
    });
    let split = h.st.session.project.patterns.last().unwrap().id;
    h.idle(playlist, 3);
    let t = h.rect("pl.timeline");
    let picker = h.rect("pl.picker");
    assert!(picker.right() <= t.left() + 1.0, "picker sits left of the timeline: {picker:?} {t:?}");
    let rows: Vec<(String, Rect)> = h.ctx.data(|d| d.get_temp(egui::Id::new("pl.picker.rows"))).unwrap();
    let row = |k: &str| rows.iter().find(|r| r.0 == k).unwrap_or_else(|| panic!("{k} listed in {rows:?}")).1;
    let zoom = h.st.ui.playlist_zoom;
    let sx = h.st.ui.pl_x as f32;
    let x = |beat: f32| t.left() + (beat - sx) * zoom;
    let track0 = h.st.session.project.tracks[0].id;
    h.drag(playlist, row(&format!("pattern:{split}")).center(), Pos2::new(x(8.1), t.top() + 10.0), PointerButton::Primary, Modifiers::NONE);
    let placed: Vec<&Clip> = h.st.session.project.tracks[0].clips.iter().filter(|c| matches!(c.kind, ClipKind::Pattern { pattern } if pattern == split)).collect();
    assert_eq!(placed.len(), 1, "pattern placed on the first track: {:?}", h.st.session.project.tracks[0].clips);
    assert!((placed[0].start - 8.0).abs() < 1e-6, "snapped to beat 8: {}", placed[0].start);
    assert_eq!(h.st.session.project.tracks[0].id, track0);
    // Automation clip onto the second track.
    h.idle(playlist, 2);
    let rows: Vec<(String, Rect)> = h.ctx.data(|d| d.get_temp(egui::Id::new("pl.picker.rows"))).unwrap();
    let r = rows.iter().find(|r| r.0 == format!("automation:{lane}")).expect("automation listed").1;
    let y2 = t.top() + h.st.session.project.tracks[0].height.clamp(28.0, 200.0) * h.st.ui.pl_vzoom + 6.0;
    h.drag(playlist, r.center(), Pos2::new(x(0.1), y2), PointerButton::Primary, Modifiers::NONE);
    let auto: Vec<&Clip> = h.st.session.project.tracks.iter().flat_map(|t| t.clips.iter()).filter(|c| matches!(c.kind, ClipKind::Automation { lane: l } if l == lane)).collect();
    assert_eq!(auto.len(), 1, "automation clip placed");
    assert!((auto[0].length - 8.0).abs() < 1e-6, "clip spans the curve: {}", auto[0].length);
}

fn mixer(ui: &mut egui::Ui, st: &mut State) {
    super::mixer::show(ui, st);
}

#[test]
fn pulling_a_cable_between_mixer_strips_routes_them() {
    let mut h = Harness::new();
    let (a, b) = (h.st.session.project.mixer.channels[0].id, h.st.session.project.mixer.channels[1].id);
    assert!(h.st.session.project.mixer.channels[0].sends.iter().all(|s| s.target != b));
    h.idle(mixer, 3);
    let rects: Vec<(Id, Rect)> = h.ctx.data(|d| d.get_temp(egui::Id::new("mixer.strips"))).expect("mixer drew its strips");
    let ra = rects.iter().find(|r| r.0 == a).unwrap().1;
    let rb = rects.iter().find(|r| r.0 == b).unwrap().1;
    let jack = Pos2::new(ra.center().x + 14.0, ra.bottom() - 58.0);
    h.drag(mixer, jack, rb.center(), PointerButton::Primary, Modifiers::NONE);
    let ch = h.st.session.project.mixer.get(a).unwrap();
    let routed = ch.output == Some(b) || ch.sends.iter().any(|s| s.target == b);
    assert!(routed, "insert 1 now feeds insert 2: output {:?} sends {:?}", ch.output, ch.sends.len());
    assert_eq!(h.st.ui.mixer, a, "the source insert is selected");
}

fn transport(ui: &mut egui::Ui, st: &mut State) {
    super::transport::show(ui, st);
}

/// FL-style tempo display: dragging the whole number changes whole BPM and keeps the decimals; dragging
/// the decimals changes only the decimals. Each drag is one undo step.
#[test]
fn bpm_display_drags_whole_numbers_or_decimals() {
    let mut h = Harness::new();
    h.st.session.edit("setup", |p| p.bpm = 140.25);
    h.idle(transport, 3);
    let (int_r, dec_r): (Rect, Rect) = h.ctx.data(|d| d.get_temp(egui::Id::new("bpm.parts"))).expect("bpm display drawn");
    let a = int_r.center();
    h.drag(transport, a, a - egui::vec2(0.0, 40.0), PointerButton::Primary, Modifiers::NONE);
    let bpm = h.st.session.project.bpm;
    assert!((bpm - 150.25).abs() < 1e-6, "whole steps, decimals kept: {bpm}");
    h.idle(transport, 2);
    let (_, dec_r2): (Rect, Rect) = h.ctx.data(|d| d.get_temp(egui::Id::new("bpm.parts"))).unwrap();
    let _ = dec_r;
    let b = dec_r2.center();
    h.drag(transport, b, b - egui::vec2(0.0, 20.0), PointerButton::Primary, Modifiers::NONE);
    let bpm = h.st.session.project.bpm;
    assert!((bpm - 150.45).abs() < 1e-6, "only the decimals moved: {bpm}");
    // Two drags = two undo steps.
    h.st.session.undo();
    assert!((h.st.session.project.bpm - 150.25).abs() < 1e-6, "{}", h.st.session.project.bpm);
}

/// FL-style playlist selection: Ctrl + drag box-selects across tracks, Shift + drag on a clip clones the
/// selection, and dragging a multi-track selection moves every clip by the same number of tracks.
#[test]
fn playlist_box_select_clone_and_multi_track_move() {
    let mut h = Harness::new();
    let pat = h.st.session.project.patterns[0].id;
    let (t0, t1, t2) = (h.st.session.project.tracks[0].id, h.st.session.project.tracks[1].id, h.st.session.project.tracks[2].id);
    h.st.session.edit("setup", |p| {
        for t in &mut p.tracks {
            t.clips.clear();
        }
        super::playlist::place_pattern(p, t0, pat, 0.0, 4.0);
        super::playlist::place_pattern(p, t1, pat, 0.0, 4.0);
    });
    h.idle(playlist, 3);
    let t = h.rect("pl.timeline");
    let zoom = h.st.ui.playlist_zoom;
    let sx = h.st.ui.pl_x as f32;
    let x = |beat: f32| t.left() + (beat - sx) * zoom;
    let row_h = h.st.session.project.tracks[0].height.clamp(28.0, 200.0) * h.st.ui.pl_vzoom;
    // Ctrl + drag a box over both clips (it starts on a clip: still a selection, FL style).
    h.drag(playlist, Pos2::new(x(0.5), t.top() + 8.0), Pos2::new(x(2.0), t.top() + row_h + 8.0), PointerButton::Primary, Modifiers::CTRL);
    assert_eq!(h.st.ui.clips.len(), 2, "both clips selected");
    let total = |h: &Harness| h.st.session.project.tracks.iter().map(|t| t.clips.len()).sum::<usize>();
    assert_eq!(total(&h), 2, "a selection does not move or add clips");
    // Shift + drag clones the selection 8 beats later.
    h.drag(playlist, Pos2::new(x(1.0), t.top() + 8.0), Pos2::new(x(9.0), t.top() + 8.0), PointerButton::Primary, Modifiers::SHIFT);
    assert_eq!(total(&h), 4, "two copies");
    let starts: Vec<f64> = h.st.session.project.tracks[0].clips.iter().map(|c| c.start).collect();
    assert!(starts.contains(&0.0) && starts.contains(&8.0), "{starts:?}");
    // Drag the copies one track down: both move (tracks 1 and 2).
    h.drag(playlist, Pos2::new(x(9.0), t.top() + 8.0), Pos2::new(x(9.0), t.top() + row_h + 8.0), PointerButton::Primary, Modifiers::NONE);
    let at8 = |h: &Harness, tid: Id| h.st.session.project.track(tid).unwrap().clips.iter().filter(|c| (c.start - 8.0).abs() < 1e-9).count();
    assert_eq!((at8(&h, t0), at8(&h, t1), at8(&h, t2)), (0, 1, 1), "the selection moved down one track");
}

/// Several picker entries selected with Ctrl+click are dragged in together, one per track.
#[test]
fn picker_drags_several_selected_items() {
    let mut h = Harness::new();
    let a = h.st.session.project.patterns[0].id;
    let b = h.st.session.edit("setup", |p| {
        for t in &mut p.tracks {
            t.clips.clear();
        }
        p.add_pattern()
    });
    h.idle(playlist, 3);
    let rows: Vec<(String, Rect)> = h.ctx.data(|d| d.get_temp(egui::Id::new("pl.picker.rows"))).unwrap();
    let row = |k: String| rows.iter().find(|r| r.0 == k).unwrap().1;
    h.click(playlist, row(format!("pattern:{a}")).center(), PointerButton::Primary, Modifiers::NONE);
    h.click(playlist, row(format!("pattern:{b}")).center(), PointerButton::Primary, Modifiers::CTRL);
    assert_eq!(h.st.ui.picker_sel.len(), 2);
    let t = h.rect("pl.timeline");
    let x = t.left() + (4.1 - h.st.ui.pl_x as f32) * h.st.ui.playlist_zoom;
    h.drag(playlist, row(format!("pattern:{a}")).center(), Pos2::new(x, t.top() + 10.0), PointerButton::Primary, Modifiers::NONE);
    let tracks = &h.st.session.project.tracks;
    let has = |i: usize, pat: Id| tracks[i].clips.iter().any(|c| matches!(c.kind, ClipKind::Pattern { pattern } if pattern == pat) && (c.start - 4.0).abs() < 1e-9);
    assert!(has(0, a) && has(1, b), "one per track: {:?}", tracks.iter().map(|t| t.clips.len()).collect::<Vec<_>>());
}

/// FL shortcuts: the gestures are bound to the right commands.
#[test]
fn shortcuts_follow_fl_studio() {
    let mut h = Harness::new();
    crate::commands::register(&mut h.st);
    let reg = &h.st.ctx.commands;
    let key = |id: &str| reg.get(&format!("dragon.{id}")).and_then(|d| d.descriptor.shortcuts.first().map(|s| s.gesture.to_string())).unwrap_or_default();
    for (id, want) in [
        ("edit.duplicate", "Ctrl+B"),
        ("edit.deselect", "Ctrl+D"),
        ("edit.undo_toggle", "Ctrl+Z"),
        ("edit.quantize", "Alt+Q"),
        ("edit.quick_quantize", "Shift+Q"),
        ("transport.play", "Space"),
        ("transport.pause", "Ctrl+Space"),
        ("transport.panic", "Ctrl+H"),
        ("transport.precount", "Ctrl+P"),
        ("transport.record", "R"),
        ("edit.snap_toggle", "Backspace"),
        ("edit.split", "Insert"),
        ("edit.mute_sel", "Alt+M"),
        ("edit.unmute_sel", "Shift+M"),
        ("edit.invert_sel", "Shift+I"),
        ("edit.move_left", "Shift+Left"),
        ("edit.move_up", "Shift+Up"),
        ("edit.glue", "Ctrl+G"),
        ("edit.chop", "Ctrl+U"),
        ("edit.flam", "Alt+F"),
        ("ntool.randomize", "Alt+R"),
        ("ntool.strum", "Alt+S"),
        ("ntool.arp", "Alt+A"),
        ("ntool.riff", "Alt+E"),
        ("ntool.limit", "Alt+K"),
        ("ntool.articulate", "Alt+L"),
        ("ntool.lfo", "Alt+O"),
        ("ntool.chop", "Alt+U"),
        ("ntool.claw", "Alt+W"),
        ("ntool.levels", "Alt+X"),
        ("ntool.flip", "Alt+Y"),
        ("tool.slip", "S"),
        ("tool.playback", "Y"),
        ("tool.zoom", "Z"),
        ("zoom.in", "PageUp"),
        ("zoom.out", "PageDown"),
        ("zoom.center", "Shift+0"),
        ("zoom.all", "Shift+4"),
        ("zoom.selection", "Shift+5"),
        ("edit.insert_time", "Ctrl+Insert"),
        ("edit.delete_time", "Ctrl+Delete"),
        ("rack.mute1", "1"),
        ("rack.mute0", "0"),
        ("rack.solo3", "Ctrl+3"),
        ("edit.transpose_up", "Ctrl+Up"),
        ("view.picker", "Alt+P"),
        ("view.playlist", "F5"),
        ("view.rack", "F6"),
        ("view.piano", "F7"),
        ("view.mixer", "F9"),
        ("view.close_windows", "F12"),
        ("file.export_midi", "Ctrl+Shift+M"),
        ("edit.legato", "Ctrl+L"),
        ("transport.metronome", "Ctrl+M"),
        ("transport.mode", "L"),
        ("file.save_new_version", "Ctrl+N"),
        ("file.export", "Ctrl+R"),
        ("file.export_mp3", "Ctrl+Shift+R"),
        ("add.pattern", "F4"),
        ("view.fx_gallery", "F8"),
        ("tool.draw", "P"),
        ("tool.paint", "B"),
        ("tool.delete", "D"),
        ("tool.mute", "T"),
        ("tool.slice", "C"),
        ("tool.select", "E"),
        ("add.marker", "Alt+T"),
    ] {
        assert_eq!(key(id), want, "{id}");
    }
}

/// Ctrl+Z undoes, pressed again it redoes (FL), Ctrl+Alt+Z keeps stepping back; copy / paste of clips.
#[test]
fn undo_toggle_and_clip_clipboard() {
    let mut h = Harness::new();
    h.st.session.edit("a", |p| p.bpm = 100.0);
    h.st.session.edit("b", |p| p.bpm = 110.0);
    let run = |h: &mut Harness, id: &str| {
        let ctx = h.ctx.clone();
        crate::commands::execute(&mut h.st, id, &ctx);
    };
    run(&mut h, "dragon.edit.undo_toggle");
    assert_eq!(h.st.session.project.bpm, 100.0);
    run(&mut h, "dragon.edit.undo_toggle");
    assert_eq!(h.st.session.project.bpm, 110.0, "second Ctrl+Z redoes");
    run(&mut h, "dragon.edit.undo_toggle");
    run(&mut h, "dragon.edit.undo");
    assert_ne!(h.st.session.project.bpm, 100.0, "Ctrl+Alt+Z steps further back");
    // Copy two clips and paste them at the playhead.
    let before: usize = h.st.session.project.tracks.iter().map(|t| t.clips.len()).sum();
    h.st.ui.editor = crate::state::Editor::Playlist;
    h.st.ui.clips = h.st.session.project.tracks[0].clips.iter().take(2).map(|c| c.id).collect();
    let n = h.st.ui.clips.len();
    run(&mut h, "dragon.edit.copy");
    run(&mut h, "dragon.edit.paste");
    let after: usize = h.st.session.project.tracks.iter().map(|t| t.clips.len()).sum();
    assert_eq!(after, before + n);
    assert_eq!(h.st.ui.clips.len(), n, "pasted clips are selected");
}

/// FL slice tool in the piano roll: a dragged line cuts every note it crosses, where it crosses.
#[test]
fn piano_roll_knife_cuts_crossed_notes() {
    let a = Note { pitch: 60, start: 0, length: PPQ * 4, velocity: 0.8, ..Default::default() };
    let b = Note { pitch: 64, start: 0, length: PPQ * 4, velocity: 0.8, ..Default::default() };
    let c = Note { pitch: 72, start: 0, length: PPQ * 4, velocity: 0.8, ..Default::default() };
    let mut h = piano_with_notes(&[a, b, c]);
    h.st.ui.tool = Tool::Slice;
    h.st.ui.snap = 0.25;
    let from = h.pr_pos(PPQ as f64 * 2.0, 65);
    let to = h.pr_pos(PPQ as f64 * 2.0, 59);
    h.drag(piano, from, to, PointerButton::Primary, Modifiers::NONE);
    let n = h.notes();
    assert_eq!(n.len(), 5, "the two crossed notes are cut, the one above is not: {n:?}");
    for p in [60u8, 64] {
        let mut parts: Vec<(u32, u32)> = n.iter().filter(|x| x.pitch == p).map(|x| (x.start, x.length)).collect();
        parts.sort();
        assert_eq!(parts, vec![(0, PPQ * 2), (PPQ * 2, PPQ * 2)], "pitch {p}");
    }
}

/// FL slice tool in the playlist: a line dragged down across two tracks cuts both clips at that beat.
#[test]
fn playlist_knife_cuts_across_tracks() {
    let mut h = Harness::new();
    let pat = h.st.session.project.patterns[0].id;
    let (t0, t1) = (h.st.session.project.tracks[0].id, h.st.session.project.tracks[1].id);
    h.st.session.edit("setup", |p| {
        for t in &mut p.tracks {
            t.clips.clear();
        }
        super::playlist::place_pattern(p, t0, pat, 0.0, 8.0);
        super::playlist::place_pattern(p, t1, pat, 0.0, 8.0);
    });
    h.st.ui.tool = Tool::Slice;
    h.idle(playlist, 3);
    let t = h.rect("pl.timeline");
    let x = t.left() + (4.0 - h.st.ui.pl_x as f32) * h.st.ui.playlist_zoom;
    let row_h = h.st.session.project.tracks[0].height.clamp(28.0, 200.0) * h.st.ui.pl_vzoom;
    h.drag(playlist, Pos2::new(x, t.top() + 4.0), Pos2::new(x + 2.0, t.top() + row_h + row_h / 2.0), PointerButton::Primary, Modifiers::NONE);
    for tid in [t0, t1] {
        let mut parts: Vec<(f64, f64)> = h.st.session.project.track(tid).unwrap().clips.iter().map(|c| (c.start, c.length)).collect();
        parts.sort_by(|a, b| a.0.total_cmp(&b.0));
        assert_eq!(parts, vec![(0.0, 4.0), (4.0, 4.0)], "track {tid}");
    }
}

/// FL note and clip commands: glue, chop, invert, mute, Shift+arrows, Backspace snap.
#[test]
fn fl_note_and_clip_commands() {
    let a = Note { pitch: 60, start: 0, length: PPQ, velocity: 0.8, ..Default::default() };
    let b = Note { pitch: 60, start: PPQ, length: PPQ, velocity: 0.8, ..Default::default() };
    let c = Note { pitch: 64, start: 0, length: PPQ, velocity: 0.8, ..Default::default() };
    let mut h = piano_with_notes(&[a, b, c]);
    h.st.ui.editor = crate::state::Editor::PianoRoll;
    let run = |h: &mut Harness, id: &str| {
        let ctx = h.ctx.clone();
        assert!(crate::commands::execute(&mut h.st, &format!("dragon.{id}"), &ctx), "{id} handled");
    };
    h.st.ui.notes = vec![0, 1];
    run(&mut h, "edit.glue");
    let n = h.notes();
    assert_eq!(n.len(), 2, "{n:?}");
    assert!(n.iter().any(|x| x.pitch == 60 && x.start == 0 && x.length == 2 * PPQ), "{n:?}");
    // Chop the long note into quarter-beat pieces.
    h.st.ui.notes = vec![n.iter().position(|x| x.pitch == 60).unwrap()];
    run(&mut h, "edit.chop");
    assert_eq!(h.notes().iter().filter(|x| x.pitch == 60).count(), 8);
    run(&mut h, "edit.invert_sel");
    assert_eq!(h.st.ui.notes.len(), 1, "only the E is left after inverting");
    run(&mut h, "edit.mute_sel");
    assert!(h.notes().iter().filter(|x| x.pitch == 64).all(|x| x.muted));
    run(&mut h, "edit.move_up");
    assert!(h.notes().iter().any(|x| x.pitch == 65), "Shift+Up moves a semitone");
    // Backspace toggles snap and restores it.
    run(&mut h, "edit.snap_toggle");
    assert_eq!(h.st.ui.snap, 0.0);
    run(&mut h, "edit.snap_toggle");
    assert_eq!(h.st.ui.snap, 0.25);
    // Clips: Shift+Down moves the selected clip to the next track.
    h.st.ui.editor = crate::state::Editor::Playlist;
    let (t0, clip) = h.st.session.project.tracks.iter().find_map(|t| t.clips.first().map(|c| (t.id, c.id))).expect("a clip");
    h.st.ui.clips = vec![clip];
    run(&mut h, "edit.move_down");
    let k = h.st.session.project.tracks.iter().position(|t| t.clips.iter().any(|c| c.id == clip)).unwrap();
    assert_ne!(h.st.session.project.tracks[k].id, t0);
    run(&mut h, "edit.move_up");
    assert_eq!(h.st.session.project.tracks.iter().find(|t| t.clips.iter().any(|c| c.id == clip)).unwrap().id, t0);
}

/// Number keys follow FL's context: mute / solo channels in the rack, 7 / 8 / 9 transpose and reverse
/// clips in the playlist; Ctrl+Insert / Ctrl+Delete add or remove time; the piano-roll tool window.
#[test]
fn fl_context_keys_and_tools() {
    let mut h = Harness::new();
    let run = |h: &mut Harness, id: &str| {
        let ctx = h.ctx.clone();
        assert!(crate::commands::execute(&mut h.st, &format!("dragon.{id}"), &ctx), "{id}");
    };
    h.st.ui.editor = crate::state::Editor::Rack;
    run(&mut h, "rack.mute1");
    assert!(h.st.session.project.channels[0].muted);
    run(&mut h, "rack.solo2");
    assert!(h.st.session.project.channels[1].solo && !h.st.session.project.channels[0].solo);
    // Playlist: an audio clip, 8 transposes up, 9 reverses.
    h.st.ui.editor = crate::state::Editor::Playlist;
    let tid = h.st.session.project.tracks[0].id;
    let cid = h.st.session.edit("audio", |p| {
        let c = Clip { id: p.next_id(), name: "a".into(), start: 4.0, length: 4.0, muted: false, color: Default::default(), kind: ClipKind::Audio(Default::default()) };
        let id = c.id;
        p.track_mut(tid).unwrap().clips.push(c);
        id
    });
    h.st.ui.clips = vec![cid];
    run(&mut h, "rack.mute8");
    run(&mut h, "rack.mute9");
    let clip = |h: &Harness| h.st.session.project.tracks.iter().flat_map(|t| t.clips.iter()).find(|c| c.id == cid).unwrap().clone();
    match &clip(&h).kind {
        ClipKind::Audio(a) => assert!(a.pitch == 1.0 && a.reverse, "{a:?}"),
        _ => unreachable!(),
    }
    assert!(h.st.session.project.channels.iter().skip(1).all(|c| !c.muted), "number keys outside the rack don't mute");
    // Ctrl+Insert / Ctrl+Delete with the loop range 0…2.
    h.st.session.edit("loop", |p| p.loop_range = Some(Range::new(0.0, 2.0)));
    run(&mut h, "edit.insert_time");
    assert_eq!(clip(&h).start, 6.0);
    run(&mut h, "edit.delete_time");
    assert_eq!(clip(&h).start, 4.0);
    // Zoom to the selected clip.
    run(&mut h, "zoom.selection");
    assert!((h.st.ui.pl_x - 4.0).abs() < 1e-9);
    // Alt+K opens the tool window on Limit.
    run(&mut h, "ntool.limit");
    assert!(h.st.ui.note_tools.open && h.st.ui.note_tools.tool == crate::views::note_tools::Tool::Limit);
    // Tool keys.
    run(&mut h, "tool.zoom");
    assert_eq!(h.st.ui.tool, crate::state::Tool::Zoom);
}

/// No two commands share a default shortcut.
#[test]
fn shortcuts_do_not_collide() {
    let mut h = Harness::new();
    crate::commands::register(&mut h.st);
    let mut seen = std::collections::HashMap::new();
    for d in h.st.ctx.commands.all() {
        for s in &d.descriptor.shortcuts {
            if let Some(prev) = seen.insert(s.gesture.to_string(), d.descriptor.id.clone()) {
                panic!("{} used by {} and {}", s.gesture, prev, d.descriptor.id);
            }
        }
    }
}
