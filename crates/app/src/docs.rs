//! The online manual: where each page lives in the repository's `docs/` folder, the help entries of every
//! knob's right-click menu (reset, copy, paste, open its manual page), and the generator of the per-plugin
//! pages (`docs/<plugin>/<section>/<knob>/README.md`), which a test keeps in step with the effects.

#[cfg(test)]
use dragon_engine::effects::{self, ParamKind};
use dragon_engine::effects::{EffectDef, ParamInfo};

/// The manual on GitHub (the repository's `docs` folder).
pub const DOCS_URL: &str = "https://github.com/zenoxart/DragonDAW-RS/blob/main/docs";

#[cfg(test)]
/// The application's sections, each with a hand-written page `docs/<slug>/README.md`.
pub const SECTIONS: &[(&str, &str, &str)] = &[
    ("getting-started", "Getting started", "Audio setup, the first beat, the window layout"),
    ("transport", "Transport", "Play, stop, record, loop and punch, tempo, metronome, markers"),
    ("playlist", "Playlist", "Arranging clips and patterns, tools, zoom, splitting, markers"),
    ("channel-rack", "Channel rack", "Channels, the step sequencer, patterns, drum pads"),
    ("piano-roll", "Piano roll", "Notes, velocity and pan lanes, scales, stamps, quantize and the edit tools"),
    ("mixer", "Mixer", "Channels, meters and faders, inserts, sends, routing, the channel menu"),
    ("browser", "Browser", "Finding sounds, previewing, adding them to the rack"),
    ("recording", "Recording", "Audio and MIDI recording, count-in, takes, the recording template"),
    ("automation", "Automation", "Automation lanes, MIDI learn, modulation"),
    ("instruments", "Instruments", "Sampler, synths, the Transmuter, piano, guitar, drum pads"),
    ("effects", "Effects", "The stock effects, presets, the preset bar and the knob menu"),
    ("pitch-correction", "Pitch correction", "Correcting the pitch of recorded audio and one-shots"),
    ("audio-repair", "Audio repair", "Removing noise, clicks, hum and clipping"),
    ("mastering", "Mastering", "Loudness, reference tracks, the master chain"),
    ("export", "Export", "Rendering songs and stems"),
    ("templates", "Templates", "Starting projects from templates"),
    ("controllers", "Controllers", "MIDI keyboards, controller buttons, control surfaces"),
    ("plugins", "Plugins", "Hosting VST3 and CLAP plugins, scanning, presets"),
    ("options", "Options", "Settings, shortcuts, performance"),
];

fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}

/// A band parameter (`b3_freq`) without its band number (`freq`), and whether it was one.
fn unband(id: &str) -> (&str, bool) {
    let b = id.as_bytes();
    if b.first() == Some(&b'b') && b.len() > 2 {
        let digits = b[1..].iter().take_while(|c| c.is_ascii_digit()).count();
        if digits > 0 && b.get(1 + digits) == Some(&b'_') {
            return (&id[2 + digits..], true);
        }
    }
    (id, false)
}

/// The panel section a parameter is shown in (its slug), or "controls".
fn section_of(def: &EffectDef, id: &str) -> String {
    let (rest, banded) = unband(id);
    let look = if banded { format!("b1_{rest}") } else { id.to_string() };
    if let Some(panel) = crate::views::fx_panel::panel(def.id, 1) {
        for s in &panel.sections {
            for c in &s.ctls {
                use crate::views::fx_panel::Ctl;
                let cid = match c {
                    Ctl::Knob(x, _) | Ctl::Big(x, _) | Ctl::Fader(x, _) | Ctl::Param(x, _) => x.as_str(),
                    _ => continue,
                };
                if cid == look {
                    let t = slug(&s.title);
                    // "band-1" → "band": one page for the knob of every band.
                    let t = t.trim_end_matches(|c: char| c.is_ascii_digit() || c == '-');
                    return if t.is_empty() { "controls".into() } else { t.to_string() };
                }
            }
        }
    }
    if banded {
        "band".into()
    } else {
        "controls".into()
    }
}

fn plugin_dir(def: &EffectDef) -> String {
    slug(def.id)
}

/// `<plugin>/<section>/<knob>` of an effect parameter.
pub fn param_path(def: &EffectDef, id: &str) -> String {
    format!("{}/{}/{}", plugin_dir(def), section_of(def, id), slug(unband(id).0))
}

/// The manual page of an effect parameter.
pub fn param_url(def: &EffectDef, id: &str) -> String {
    format!("{DOCS_URL}/{}/README.md", param_path(def, id))
}

/// The manual page of an application section.
pub fn section_url(section: &str) -> String {
    format!("{DOCS_URL}/{section}/README.md")
}

/// The manual's start page.
pub fn index_url() -> String {
    format!("{DOCS_URL}/README.md")
}

/// The copied knob value (shared by every knob: copy on one, paste on another of the same kind).
#[derive(Clone)]
struct Copied {
    value: f32,
    unit: &'static str,
    normalized: f32,
}

fn clip_id() -> egui::Id {
    egui::Id::new("knob.clipboard")
}

/// The entries of a knob's right-click menu: set value, reset, copy, paste and the knob's manual page.
/// Returns a new value (reset or paste) for the caller to apply.
pub fn knob_menu_items(ui: &mut egui::Ui, resp: &egui::Response, def: &EffectDef, prm: &ParamInfo, value: f32) -> Option<f32> {
    let mut out = None;
    crate::views::widgets::set_value_item(ui, resp);
    if ui.button("Reset to default").clicked() {
        out = Some(prm.default);
        ui.close_menu();
    }
    if ui.button(format!("Copy value ({})", prm.format(value))).clicked() {
        ui.ctx().copy_text(prm.format(value));
        let c = Copied { value, unit: prm.unit, normalized: prm.normalize(value) };
        ui.ctx().data_mut(|d| d.insert_temp(clip_id(), c));
        ui.close_menu();
    }
    let copied: Option<Copied> = ui.ctx().data(|d| d.get_temp(clip_id()));
    let label = match &copied {
        Some(c) => format!("Paste value ({})", if c.unit == prm.unit { prm.format(c.value.clamp(prm.min.min(prm.max), prm.max.max(prm.min))) } else { prm.format(prm.denormalize(c.normalized)) }),
        None => "Paste value".to_string(),
    };
    if ui.add_enabled(copied.is_some(), egui::Button::new(label)).on_disabled_hover_text("Copy a knob's value first").clicked() {
        if let Some(c) = copied {
            // Same unit: the value itself; otherwise the same position on this knob's scale.
            out = Some(if c.unit == prm.unit { c.value.clamp(prm.min.min(prm.max), prm.max.max(prm.min)) } else { prm.denormalize(c.normalized) });
        }
        ui.close_menu();
    }
    ui.separator();
    if ui.button(format!("Help: {}", prm.name)).on_hover_text(param_url(def, prm.id)).clicked() {
        ui.ctx().open_url(egui::OpenUrl::new_tab(param_url(def, prm.id)));
        ui.close_menu();
    }
    out
}

/// Attaches the knob menu to a face's own knob (Jadefang, Dragonforge, Draconic Ghost).
pub fn knob_menu(resp: &egui::Response, def: &EffectDef, prm: &ParamInfo, value: f32) -> Option<f32> {
    let mut out = None;
    resp.context_menu(|ui| {
        ui.label(egui::RichText::new(format!("{}  {}", prm.name, prm.format(value))).strong());
        ui.separator();
        out = knob_menu_items(ui, resp, def, prm, value);
    });
    out
}

// ------------------------------------------------------------------------------------------ pages

#[cfg(test)]
/// What a parameter does, in a few sentences, from its name, unit and range.
fn explain(def: &EffectDef, prm: &ParamInfo) -> String {
    let (key, banded) = unband(prm.id);
    let last = key.rsplit('_').next().unwrap_or(key);
    let band = if banded { " of the selected band" } else { "" };
    let s = match last {
        "mix" => {
            format!("Blends the processed signal with the dry one{band}: 0 leaves the input untouched, the top of the range is fully processed. The dry path is delay-compensated, so blending never smears or combs.")
        }
        "threshold" | "thr" | "thresh" => format!("The level{band} above which the effect starts working. Lower it to act on more of the signal."),
        "ratio" => format!("How strongly the level is changed once it passes the threshold{band}: 4:1 lets 1 dB through for every 4 dB above it."),
        "attack" => format!("How quickly the effect reacts when the signal rises{band}. Short times catch transients, longer ones let them through."),
        "release" => format!("How quickly the effect lets go after the signal falls{band}. Short times pump, long ones sound smoother."),
        "knee" => "How gently the effect starts around the threshold: 0 is a hard corner, higher values ease in.".into(),
        "hold" => "How long the effect stays engaged after the signal drops before it starts to release.".into(),
        "lookahead" => "Lets the detector see the signal slightly in advance so the effect can react before a peak arrives (adds this much latency, compensated by the mixer).".into(),
        "freq" | "cutoff" | "center" | "tone" => format!("The frequency{band} the effect works around. Drag it to move the effect through the spectrum."),
        "q" | "resonance" | "reso" => format!("How narrow the affected range is{band}; high values focus on a small range and can ring."),
        "gain" if banded => "How much the selected band raises (positive) or lowers (negative) its frequency range.".into(),
        "gain" | "output" | "trim" | "post" | "level" | "makeup" => "The output level after the effect, to match the loudness before and after (compare with the power button).".into(),
        "input" => "The level going into the effect; more input drives the processing harder.".into(),
        "drive" => format!("How hard the signal is pushed into the saturation stage{band}; more drive adds harmonics and density."),
        "depth" | "amount" => "How strong the effect is overall.".into(),
        "rate" => "How fast the modulation moves (in Hz, or locked to the song when Sync is on).".into(),
        "sync" => "Locks the timing to the song's tempo in note values instead of free milliseconds / Hz.".into(),
        "feedback" | "fb" | "cross" => "How much of the output is fed back into the effect; more feedback means more repeats or a stronger resonance.".into(),
        "width" => "The stereo width: 0 is mono, 100 % unchanged, above that wider.".into(),
        "decay" => "How long the tail rings out.".into(),
        "size" => "The size of the simulated space.".into(),
        "predelay" => "The gap before the reverb starts, keeping the dry sound clear in front of it.".into(),
        "damping" => "Rolls off the high frequencies of the tail as it decays, like soft surfaces in a room.".into(),
        "lowcut" | "low_cut" | "hpf" => "Removes low frequencies below this point from the processed signal.".into(),
        "highcut" | "high_cut" => "Removes high frequencies above this point from the processed signal.".into(),
        "sidechain" | "sc" => "Uses another mixer channel (chosen in the Sidechain input menu) to control the effect.".into(),
        "listen" | "solo" => "Plays only what the detector or band hears, to set it up by ear.".into(),
        "bypass" => "Switches the processing off while keeping its settings.".into(),
        "mode" | "style" | "type" | "character" | "model" | "engine" | "shape" => "Chooses the variant of the processing; each option changes the character of the effect.".into(),
        "on" => format!("Switches the band{band} on or off."),
        "slope" => "How steeply the filter cuts beyond its frequency (dB per octave).".into(),
        "stereo" => "Which part of the stereo signal the band works on: both, mid, side, left or right.".into(),
        "link" => "How much the left and right (or mid and side) detectors move together.".into(),
        "pan" => "Places the signal between left and right.".into(),
        "bits" => "The bit depth the signal is reduced to; fewer bits sound grittier.".into(),
        "downsample" => "Holds every n-th sample, folding high frequencies back down for a lo-fi sound.".into(),
        "ceiling" => "The highest level the output may reach.".into(),
        "range" => "The most the effect may change the level.".into(),
        "speed" => "How quickly the pitch is pulled to the target note.".into(),
        "key" | "scale" => "The key and scale the notes are corrected to.".into(),
        _ => format!("Sets the {}{band}.", prm.name.to_lowercase()),
    };
    format!("{s}\n\nPart of **{}** – {}", def.name, def.description)
}

#[cfg(test)]
fn range_text(prm: &ParamInfo) -> String {
    match prm.kind {
        ParamKind::Choice(opts) => format!("Options: {} (default: {})", opts.join(", "), opts.get(prm.default as usize).copied().unwrap_or("")),
        ParamKind::Toggle => format!("On / off (default: {})", if prm.default > 0.5 { "on" } else { "off" }),
        _ => format!("{} … {} (default: {}{})", prm.format(prm.min), prm.format(prm.max), prm.format(prm.default), if matches!(prm.kind, ParamKind::Log) { ", logarithmic scale" } else { "" }),
    }
}

#[cfg(test)]
const KNOB_MENU: &str = "Right-click the control for its menu: **Set value…** (a box with the current value: type a new one in the control's units – `-6 dB`, `250 ms`, `2.5k`, `30L` – and press Enter; one undo step), **Reset to default**, **Copy value** / **Paste value** (between any two knobs; a value of another unit lands at the same position on the scale), MIDI learn, an automation lane, and **Help**, which opens this page.\n\nDrag up / down to turn it (Shift: fine), turn the mouse wheel over it, double-click or Ctrl+click to reset.";

#[cfg(test)]
/// Every generated page: (path below `docs/`, content).
pub fn pages() -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut plugins = String::new();
    for def in effects::registry() {
        let dir = plugin_dir(def);
        // section → [(knob slug, name, example id)]
        let mut sections: std::collections::BTreeMap<String, Vec<(String, &ParamInfo)>> = Default::default();
        for prm in def.params {
            let (key, _) = unband(prm.id);
            let sec = section_of(def, prm.id);
            let k = slug(key);
            let list = sections.entry(sec).or_default();
            if !list.iter().any(|(x, _)| *x == k) {
                list.push((k, prm));
            }
        }
        let presets = effects::factory::PER_EFFECT;
        let mut idx = format!(
            "# {} ({})\n\n*{}*\n\n{}\n\nThe preset bar on top of the unit holds {presets} factory presets in eight characters (Subtle, Warm, Bright, Punchy, Wide, Dark, Heavy, Creative) and your own presets.\n\n## Sections\n\n",
            def.name, def.id, def.category, def.description
        );
        for (sec, knobs) in &sections {
            idx.push_str(&format!("- [{}]({sec}/README.md) – {}\n", title_case(sec), knobs.iter().map(|(_, p)| strip_band(p.name)).collect::<Vec<_>>().join(", ")));
            let mut sp = format!("# {} › {}\n\n[Back to {}](../README.md)\n\n| Control | Range |\n|---|---|\n", def.name, title_case(sec), def.name);
            for (k, prm) in knobs {
                sp.push_str(&format!("| [{}]({k}/README.md) | {} |\n", strip_band(prm.name), range_text(prm)));
                let body = format!(
                    "# {} › {} › {}\n\n[Back to {}](../README.md) · [{}](../../README.md)\n\n{}\n\n**Range:** {}{}\n\n{}\n{KNOB_MENU}\n",
                    def.name,
                    title_case(sec),
                    strip_band(prm.name),
                    title_case(sec),
                    def.name,
                    explain(def, prm),
                    range_text(prm),
                    if unband(prm.id).1 { "\n\nEvery band has this control; pick the band with the band buttons." } else { "" },
                    knob_math(def, prm)
                );
                out.push((format!("{dir}/{sec}/{k}/README.md"), body));
            }
            out.push((format!("{dir}/{sec}/README.md"), sp));
        }
        if let Some(m) = math(def.id) {
            idx.push_str(&format!(
                "\n## How it works\n\n{}\n\nThe formulas of each control are on its page above; shared building blocks (decibels, smoothing, RBJ filters, oversampling) are in [DSP foundations](../dsp/README.md).\n",
                m.0.replace("{DSP}", "../dsp/README.md").trim()
            ));
        }
        idx.push_str("\n[All effects](../effects/README.md) · [Manual](../README.md)\n");
        out.push((format!("{dir}/README.md"), idx));
        plugins.push_str(&format!("| [{}]({dir}/README.md) | {} | {} |\n", def.name, def.category, def.description.split(". ").next().unwrap_or("")));
    }
    let mut index = String::from(
        "# Dragon DAW manual\n\nEvery feature of Dragon DAW, by section. In the app, **Help › Documentation** opens this page, and every knob's right-click menu has a **Help** entry that opens the knob's own page.\n\n## Sections\n\n",
    );
    for (s, t, d) in SECTIONS {
        index.push_str(&format!("- [{t}]({s}/README.md) – {d}\n"));
    }
    index.push_str("- [DSP notes](dsp/README.md) – the mathematics shared by every effect: decibels, smoothing, envelope followers, filters, oversampling, pan laws\n");
    index.push_str("\nEvery section page ends with **How it works** (the algorithms and formulas behind it), every effect page has the same, and every knob page a **Mathematics** section: what the control does in the algorithm, the exact mapping from its position to its value, and how changes are smoothed.\n");
    index.push_str("\n## Stock effects\n\n| Effect | Category | What it does |\n|---|---|---|\n");
    index.push_str(&plugins);
    out.push(("README.md".into(), index));
    out
}

/// The technical description of an effect: (overview, per-control sections), from `docs_math/<id>.md`.
/// Control sections start with `<!-- knob: <id> -->` (band controls without their `bN_` prefix).
#[cfg(test)]
fn math(id: &str) -> Option<(String, std::collections::BTreeMap<String, String>)> {
    let text = match id {
        "alter" => include_str!("../docs_math/alter.md"),
        "bitcrusher" => include_str!("../docs_math/bitcrusher.md"),
        "chorus" => include_str!("../docs_math/chorus.md"),
        "class_a_strip" => include_str!("../docs_math/class_a_strip.md"),
        "clean_voice" => include_str!("../docs_math/clean_voice.md"),
        "clipper" => include_str!("../docs_math/clipper.md"),
        "compressor" => include_str!("../docs_math/compressor.md"),
        "deesser" => include_str!("../docs_math/deesser.md"),
        "delay" => include_str!("../docs_math/delay.md"),
        "dragon_particle" => include_str!("../docs_math/dragon_particle.md"),
        "drum_bus" => include_str!("../docs_math/drum_bus.md"),
        "eq" => include_str!("../docs_math/eq.md"),
        "filter" => include_str!("../docs_math/filter.md"),
        "flanger" => include_str!("../docs_math/flanger.md"),
        "gate" => include_str!("../docs_math/gate.md"),
        "ghost" => include_str!("../docs_math/ghost.md"),
        "imager" => include_str!("../docs_math/imager.md"),
        "jadefang" => include_str!("../docs_math/jadefang.md"),
        "limiter" => include_str!("../docs_math/limiter.md"),
        "multiband" => include_str!("../docs_math/multiband.md"),
        "multisat" => include_str!("../docs_math/multisat.md"),
        "orbit" => include_str!("../docs_math/orbit.md"),
        "phaser" => include_str!("../docs_math/phaser.md"),
        "reverb" => include_str!("../docs_math/reverb.md"),
        "saturator" => include_str!("../docs_math/saturator.md"),
        "transient" => include_str!("../docs_math/transient.md"),
        "tremolo" => include_str!("../docs_math/tremolo.md"),
        "tune" => include_str!("../docs_math/tune.md"),
        "utility" => include_str!("../docs_math/utility.md"),
        _ => return None,
    };
    let mut parts = text.split("<!-- knob: ");
    let overview = parts.next().unwrap_or_default().to_string();
    let knobs = parts
        .filter_map(|p| {
            let (head, body) = p.split_once("-->")?;
            Some((head.trim().to_string(), body.trim().to_string()))
        })
        .collect();
    Some((overview, knobs))
}

/// A control's text: its own entry, else the entry of its family (numbered controls such as `dr_3` or
/// `lfo2_rate` are written once, for number 1 – or 2 where the family starts there).
#[cfg(test)]
fn knob_text<'a>(knobs: &'a std::collections::BTreeMap<String, String>, id: &str) -> Option<&'a String> {
    knobs.get(id).or_else(|| {
        ['1', '2'].iter().find_map(|d| {
            let mut out = String::new();
            let mut prev_digit = false;
            for c in id.chars() {
                if c.is_ascii_digit() {
                    if !prev_digit {
                        out.push(*d);
                    }
                    prev_digit = true;
                } else {
                    out.push(c);
                    prev_digit = false;
                }
            }
            knobs.get(&out)
        })
    })
}

/// The "Mathematics" section of a control's page: what the effect does with it, then the exact mapping
/// from the knob's position to its value and how changes are smoothed.
#[cfg(test)]
fn knob_math(def: &EffectDef, prm: &ParamInfo) -> String {
    let mut out = String::from("## Mathematics\n\n");
    if let Some((_, knobs)) = math(def.id) {
        if let Some(t) = knob_text(&knobs, unband(prm.id).0) {
            out.push_str(&t.replace("{DSP}", "../../../dsp/README.md"));
            out.push_str("\n\n");
        }
    }
    let (lo, hi) = (prm.min, prm.max);
    let u = if prm.unit.is_empty() { String::new() } else { format!("\\ \\text{{{}}}", prm.unit.replace('%', "\\%")) };
    out.push_str("**Knob position \u{2192} value.** ");
    match prm.kind {
        ParamKind::Log if lo > 0.0 => out.push_str(&format!(
            "Logarithmic: $v = {lo}\\cdot\\left(\\frac{{{hi}}}{{{lo}}}\\right)^{{p}}{u}$, $p = \\frac{{\\ln(v/{lo})}}{{\\ln({})}}$ — every factor of {:.3} takes a tenth of the travel; the centre ($p = 0.5$) is {:.4}{}.\n\n",
            fmt_num(hi / lo),
            (hi / lo).powf(0.1),
            (lo * hi).sqrt(),
            if prm.unit.is_empty() { String::new() } else { format!(" {}", prm.unit) }
        )),
        ParamKind::Choice(opts) => out.push_str(&format!("A choice of {}: the value is the option's index $0 \\dots {}$.\n\n", opts.len(), opts.len().saturating_sub(1))),
        ParamKind::Toggle => out.push_str("A switch: 0 = off, 1 = on.\n\n"),
        _ => out.push_str(&format!("Linear: $v = {lo} + p\\,({}){u}$, so each tenth of the travel is {}{}.\n\n", fmt_num(hi - lo), fmt_num((hi - lo) / 10.0), if prm.unit.is_empty() { String::new() } else { format!(" {}", prm.unit) })),
    }
    match prm.kind {
        ParamKind::Linear | ParamKind::Log => out.push_str(&format!(
            "**Smoothing.** Changes (dragging, automation, presets) glide every 32 samples with $v \\leftarrow v + c\\,(v_t - v)$, $c = 1 - e^{{-32/(0.025 f_s)}}$ — a 25 ms time constant{}. See [DSP foundations](../../../dsp/README.md).\n",
            if matches!(prm.kind, ParamKind::Log) { ", in the log domain (even per ratio)" } else { "" }
        )),
        _ => out.push_str("**Smoothing.** Takes effect at once (no glide).\n"),
    }
    out
}

#[cfg(test)]
fn fmt_num(x: f32) -> String {
    let s = format!("{x:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    if s.is_empty() || s == "-0" {
        "0".into()
    } else {
        s
    }
}

#[cfg(test)]
fn strip_band(name: &str) -> String {
    // "Band 1 frequency" → "Frequency".
    let n = name.strip_prefix("Band ").map(|r| r.trim_start_matches(|c: char| c.is_ascii_digit()).trim_start()).unwrap_or(name);
    let mut c = n.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

#[cfg(test)]
fn title_case(s: &str) -> String {
    s.split('-')
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn docs_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs")
    }

    /// The pages in `docs/` match the effects. `DRAGON_WRITE_DOCS=1 cargo test -p dragon-daw docs::` rewrites them.
    #[test]
    fn manual_covers_every_knob() {
        let root = docs_dir();
        let write = std::env::var_os("DRAGON_WRITE_DOCS").is_some();
        let mut stale = Vec::new();
        for (path, body) in pages() {
            let f = root.join(&path);
            if write {
                std::fs::create_dir_all(f.parent().unwrap()).unwrap();
                std::fs::write(&f, &body).unwrap();
            } else if std::fs::read_to_string(&f).ok().map(|t| t.replace("\r\n", "\n")).as_deref() != Some(body.as_str()) {
                stale.push(path);
            }
        }
        assert!(stale.is_empty(), "{} manual pages are out of date (run with DRAGON_WRITE_DOCS=1): {:?}", stale.len(), &stale[..stale.len().min(10)]);
        for (s, _, _) in SECTIONS {
            assert!(root.join(s).join("README.md").exists(), "docs/{s}/README.md missing");
        }
        // Every knob of every effect has its page.
        for def in effects::registry() {
            for p in def.params {
                assert!(root.join(param_path(def, p.id)).join("README.md").exists(), "{} {}", def.id, p.id);
            }
        }
    }

    #[test]
    fn knob_paths() {
        let eq = effects::find("eq").unwrap();
        assert_eq!(unband("b12_freq"), ("freq", true));
        assert_eq!(unband("bits"), ("bits", false));
        assert_eq!(param_path(eq, "b3_freq"), "eq/curve/freq");
        assert_eq!(param_path(eq, "b3_freq"), param_path(eq, "b1_freq"));
        assert!(param_url(eq, "b1_gain").starts_with(DOCS_URL));
    }
}
