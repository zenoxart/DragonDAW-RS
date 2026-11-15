//! Loads this crate's own plugins through Dragon DAW's CLAP and VST3 hosts.

use dragon_engine::effects::Effect;
use dragon_engine::instruments::{InstrumentVoice, NoteOn};
use dragon_engine::model::PluginFormat;
use dragon_engine::plugins::{clap::ClapPlugin, scan_file, vst3::Vst3Plugin};
use std::path::{Path, PathBuf};

fn built_library() -> PathBuf {
    // The cdylib lives in target/<profile>/deps (rebuilt by `cargo test`) and target/<profile> (refreshed only by
    // `cargo build`). Where the test binary itself sits depends on the Cargo version (target/<profile>/deps, or
    // target/<profile>/build/<pkg>/<hash>/out with the newer build-dir layout), so walk up from it and prefer
    // the deps copy at each level. DRAGON_TEST_PLUGIN overrides the lookup.
    let name = if cfg!(target_os = "windows") {
        "dragon_test_plugins.dll"
    } else if cfg!(target_os = "macos") {
        "libdragon_test_plugins.dylib"
    } else {
        "libdragon_test_plugins.so"
    };
    if let Some(p) = std::env::var_os("DRAGON_TEST_PLUGIN").map(PathBuf::from).filter(|p| p.exists()) {
        return p;
    }
    let exe = std::env::current_exe().unwrap();
    let found = exe.ancestors().skip(1).flat_map(|a| [a.join("deps").join(name), a.join(name)]).find(|p| p.exists());
    found.unwrap_or_else(|| panic!("cdylib {name} not found above {}", exe.display()))
}

/// Installs the library as a `.clap` file and a `.vst3` bundle in a temp folder.
fn install(tmp: &Path) -> (PathBuf, PathBuf) {
    let lib = built_library();
    let clap = tmp.join("DragonTest.clap");
    std::fs::copy(&lib, &clap).unwrap();
    let bundle = tmp.join("DragonTest.vst3");
    let arch = if cfg!(target_os = "windows") {
        "x86_64-win".to_string()
    } else if cfg!(target_os = "macos") {
        "MacOS".to_string()
    } else {
        format!("{}-linux", std::env::consts::ARCH)
    };
    let bin_dir = bundle.join("Contents").join(arch);
    std::fs::create_dir_all(&bin_dir).unwrap();
    let file = if cfg!(target_os = "windows") {
        "DragonTest.vst3"
    } else if cfg!(target_os = "macos") {
        "DragonTest"
    } else {
        "DragonTest.so"
    };
    std::fs::copy(&lib, bin_dir.join(file)).unwrap();
    (clap, bundle)
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
}

#[test]
fn scan_finds_both_formats() {
    let tmp = tempfile::tempdir().unwrap();
    let (clap, vst3) = install(tmp.path());
    let found = dragon_engine::plugins::find_plugin_files(&[tmp.path().to_path_buf()]);
    assert_eq!(found.len(), 2);

    let c = scan_file(PluginFormat::Clap, &clap).unwrap();
    assert_eq!(c.len(), 2);
    assert_eq!(c[0].id, "dragon.test.gain");
    assert!(!c[0].instrument);
    assert!(c[1].instrument);
    assert_eq!(c[1].vendor, "ZenoxArt");

    let v = scan_file(PluginFormat::Vst3, &vst3).unwrap();
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].name, "Dragon Test Gain");
    assert_eq!(v[0].vendor, "ZenoxArt");
}

#[test]
fn clap_effect_params_and_state() {
    let tmp = tempfile::tempdir().unwrap();
    let (clap, _) = install(tmp.path());
    let info = scan_file(PluginFormat::Clap, &clap).unwrap().remove(0);
    let mut p = ClapPlugin::load(&info.to_ref(), 48000.0, 512, false).unwrap();

    let params = Effect::plugin_params(&p);
    assert_eq!(params.len(), 1);
    assert_eq!(params[0].name, "Gain");
    assert!((params[0].default - 0.5).abs() < 1e-9);

    let (mut l, mut r) = (vec![0.5f32; 256], vec![0.5f32; 256]);
    Effect::process(&mut p, &mut l, &mut r, None);
    assert!((l[10] - 0.5).abs() < 1e-6, "unity gain by default");

    // normalized 1.0 → plain 2.0
    Effect::set_param(&mut p, "0", 1.0);
    let (mut l, mut r) = (vec![0.5f32; 256], vec![0.5f32; 256]);
    Effect::process(&mut p, &mut l, &mut r, None);
    assert!((l[10] - 1.0).abs() < 1e-6 && (r[200] - 1.0).abs() < 1e-6);
    assert!((Effect::plugin_param(&p, 0).unwrap() - 1.0).abs() < 1e-9);

    // State roundtrip into a fresh instance.
    let state = Effect::save_state(&mut p).unwrap();
    let mut r2 = info.to_ref();
    r2.state = state;
    let mut q = ClapPlugin::load(&r2, 48000.0, 512, false).unwrap();
    let (mut l, mut r) = (vec![0.25f32; 64], vec![0.25f32; 64]);
    Effect::process(&mut q, &mut l, &mut r, None);
    assert!((l[0] - 0.5).abs() < 1e-6, "restored gain 2.0");
}

#[test]
fn clap_instrument_plays_notes() {
    let tmp = tempfile::tempdir().unwrap();
    let (clap, _) = install(tmp.path());
    let info = scan_file(PluginFormat::Clap, &clap).unwrap().remove(1);
    let mut p = ClapPlugin::load(&info.to_ref(), 48000.0, 512, true).unwrap();
    let (mut l, mut r) = (vec![0.0f32; 512], vec![0.0f32; 512]);
    InstrumentVoice::process(&mut p, &mut l, &mut r);
    assert_eq!(rms(&l), 0.0);
    p.note_on(NoteOn { note: 69, velocity: 1.0, pan: 0.0, detune: 0.0, slide: false });
    let (mut l, mut r) = (vec![0.0f32; 512], vec![0.0f32; 512]);
    InstrumentVoice::process(&mut p, &mut l, &mut r);
    assert!((rms(&l) - 0.5 / 2f32.sqrt()).abs() < 0.02, "sine at -6 dB, rms {}", rms(&l));
    p.note_off(69);
    let (mut l, mut r) = (vec![0.0f32; 512], vec![0.0f32; 512]);
    InstrumentVoice::process(&mut p, &mut l, &mut r);
    assert_eq!(rms(&l), 0.0);
}

#[test]
fn vst3_effect_params_and_state() {
    let tmp = tempfile::tempdir().unwrap();
    let (_, bundle) = install(tmp.path());
    let info = scan_file(PluginFormat::Vst3, &bundle).unwrap().remove(0);
    let mut p = Vst3Plugin::load(&info.to_ref(), 48000.0, 512, false).unwrap();
    let params = Effect::plugin_params(&p);
    assert_eq!(params.len(), 1);
    assert!(params[0].automatable);

    let (mut l, mut r) = (vec![0.5f32; 256], vec![0.5f32; 256]);
    Effect::process(&mut p, &mut l, &mut r, None);
    assert!((l[5] - 0.5).abs() < 1e-6, "normalized 0.5 = unity");

    Effect::set_param(&mut p, "0", 0.25);
    let (mut l, mut r) = (vec![0.5f32; 256], vec![0.5f32; 256]);
    Effect::process(&mut p, &mut l, &mut r, None);
    assert!((l[5] - 0.25).abs() < 1e-6 && (r[255] - 0.25).abs() < 1e-6);
    assert!((Effect::plugin_param(&p, 0).unwrap() - 0.25).abs() < 1e-9);

    let state = Effect::save_state(&mut p).unwrap();
    let mut r2 = info.to_ref();
    r2.state = state;
    let mut q = Vst3Plugin::load(&r2, 48000.0, 512, false).unwrap();
    let (mut l, mut r) = (vec![1.0f32; 32], vec![1.0f32; 32]);
    Effect::process(&mut q, &mut l, &mut r, None);
    assert!((l[0] - 0.5).abs() < 1e-6, "restored normalized 0.25 → gain 0.5");
}

#[test]
fn sidechain_reaches_clap_and_vst3_plugins() {
    let tmp = tempfile::tempdir().unwrap();
    let (clap, bundle) = install(tmp.path());
    let c = scan_file(PluginFormat::Clap, &clap).unwrap().remove(0);
    let v = scan_file(PluginFormat::Vst3, &bundle).unwrap().remove(0);
    let mut cp = ClapPlugin::load(&c.to_ref(), 48000.0, 512, false).unwrap();
    let mut vp = Vst3Plugin::load(&v.to_ref(), 48000.0, 512, false).unwrap();
    for (name, p) in [("clap", &mut cp as &mut dyn Effect), ("vst3", &mut vp as &mut dyn Effect)] {
        // No sidechain: unity.
        let (mut l, mut r) = (vec![0.5f32; 128], vec![0.5f32; 128]);
        p.process(&mut l, &mut r, None);
        assert!((l[0] - 0.5).abs() < 1e-6);
        // Sidechain at 0.5 ducks by half, at 1.0 silences.
        let half = vec![0.5f32; 128];
        let (mut l, mut r) = (vec![0.5f32; 128], vec![0.5f32; 128]);
        p.process(&mut l, &mut r, Some((&half, &half)));
        assert!((l[0] - 0.25).abs() < 1e-6, "{name}: {}", l[0]);
        let full = vec![1.0f32; 128];
        let (mut l, mut r) = (vec![0.5f32; 128], vec![0.5f32; 128]);
        p.process(&mut l, &mut r, Some((&full, &full)));
        assert!(l[0].abs() < 1e-6 && r[100].abs() < 1e-6);
    }
}

/// Embedded editors on Linux (needs an X server: set DISPLAY, e.g. Xvfb) and Windows: the plugin's GUI
/// embeds into the host's window, the host applies its resize request, and a control change made in the GUI
/// reaches the processor and the app (component handler / CLAP output events).
#[cfg(any(target_os = "linux", windows))]
#[test]
fn editors_embed_resize_and_report_gui_edits() {
    if cfg!(target_os = "linux") && std::env::var_os("DISPLAY").is_none() {
        eprintln!("skipped: no DISPLAY");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let (clap, bundle) = install(tmp.path());
    let c = scan_file(PluginFormat::Clap, &clap).unwrap().remove(0);
    let v = scan_file(PluginFormat::Vst3, &bundle).unwrap().remove(0);
    let mut cp = ClapPlugin::load(&c.to_ref(), 48000.0, 512, false).unwrap();
    let mut vp = Vst3Plugin::load(&v.to_ref(), 48000.0, 512, false).unwrap();
    for (name, p) in [("clap", &mut cp as &mut dyn Effect), ("vst3", &mut vp as &mut dyn Effect)] {
        // The project's stored value (as a compile applies it).
        p.set_param("0", 0.5);
        assert!(p.editor(true), "{name}: editor opens: {:?}", p.editor_error());
        assert!(p.editor_error().is_none());
        let (mut l, mut r) = (vec![0.5f32; 64], vec![0.5f32; 64]);
        for _ in 0..20 {
            assert!(p.idle(), "{name}: editor stays open");
            p.process(&mut l, &mut r, None);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        // The GUI saw the host-applied resize and set the gain to 0.75 (normalised).
        let v = p.plugin_param(0).unwrap();
        assert!((v - 0.75).abs() < 1e-6, "{name}: parameter {v}");
        // …and the processor got it: 0.5 in × gain 1.5.
        let (mut l, mut r) = (vec![0.5f32; 64], vec![0.5f32; 64]);
        p.process(&mut l, &mut r, None);
        assert!((l[10] - 0.75).abs() < 1e-4, "{name}: processed {}", l[10]);
        // A recompile before the app picked up the edit re-applies the old project value (0.5): unchanged
        // since the host last sent it, so it must not undo the GUI edit.
        p.set_param("0", 0.5);
        p.process(&mut l, &mut r, None);
        assert!((p.plugin_param(0).unwrap() - 0.75).abs() < 1e-6, "{name}: GUI edit kept");
        // …and the app is told, so the project stores it.
        let edits = p.take_param_edits();
        assert!(edits.iter().any(|(id, v)| *id == 0 && (v - 0.75).abs() < 1e-6), "{name}: edits {edits:?}");
        // A host-side change (generic slider) still goes through.
        p.set_param("0", 0.25);
        p.process(&mut l, &mut r, None);
        assert!((p.plugin_param(0).unwrap() - 0.25).abs() < 1e-6, "{name}: host edit applied");
        p.editor(false);
        assert!(!p.idle(), "{name}: closed");
    }
    // The embedded child window lives inside the host's editor window.
    assert!(Effect::editor(&mut cp, true));
    assert_eq!(cp.editor_window().unwrap().child_count(), 1);
    assert!(Effect::editor(&mut vp, true));
    assert_eq!(vp.editor_window().unwrap().child_count(), 1);
}

#[test]
fn plugins_render_inside_a_project() {
    use dragon_engine::model::*;
    use dragon_engine::render::*;
    use dragon_engine::samples::SampleCache;
    let tmp = tempfile::tempdir().unwrap();
    let (clap, bundle) = install(tmp.path());
    let sine = scan_file(PluginFormat::Clap, &clap).unwrap().remove(1);
    let vgain = scan_file(PluginFormat::Vst3, &bundle).unwrap().remove(0);

    let render = |master_gain: Option<f64>| {
        let mut p = Project::new_default();
        p.bpm = 120.0;
        let ch = p.add_channel("Sine", Instrument::Plugin(sine.to_ref()));
        let pat = p.patterns[0].id;
        p.patterns[0].channels.insert(ch, PatternChannel { steps: vec![], notes: vec![Note { pitch: 69, start: 0, length: PPQ * 2, velocity: 1.0, ..Default::default() }] });
        let id = p.next_id();
        p.tracks[0].clips.push(Clip { id, name: "p".into(), start: 0.0, length: 4.0, muted: false, color: Rgb::default(), kind: ClipKind::Pattern { pattern: pat } });
        if let Some(g) = master_gain {
            let mut r = vgain.to_ref();
            r.params.insert(0, g);
            let iid = p.next_id();
            p.mixer.master.inserts.push(Insert { id: iid, effect: "plugin".into(), enabled: true, mix: 1.0, params: Default::default(), sidechain: None, plugin: Some(r), preset: String::new(), mods: Vec::new() });
        }
        let cache = SampleCache::default();
        let mut pool = Pool::with_plugins(Box::new(dragon_engine::plugins::Host));
        let mut r = offline_renderer(&p, &cache, 48000.0, &mut pool);
        r.play(PlayMode::Song);
        let (mut l, mut rr) = (vec![0.0f32; 24000], vec![0.0f32; 24000]);
        r.render(&mut l, &mut rr, &[]);
        rms(&l[4800..19200])
    };
    let dry = render(None);
    assert!(dry > 0.05, "plugin instrument should sound, rms {dry}");
    let unity = render(Some(0.5));
    assert!((unity - dry).abs() < dry * 0.02, "VST3 gain at unity: {unity} vs {dry}");
    let muted = render(Some(0.0));
    assert!(muted < 1e-4, "VST3 gain at zero should silence the master, rms {muted}");
}
