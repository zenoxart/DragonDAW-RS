//! Album master (Mastering tab, projects set up as an album): several mixdowns, each on its own track and
//! mixer channel, all through one album bus (optionally via group buses) into the master chain. "Match
//! loudness" measures every song and sets its fader so all of them hit the chain at the same loudness, so
//! the album comes out even.

use crate::state::{BalanceJob, State};
use crate::theme;
use dragon_engine::model::{ChannelKind, Id};
use dragon_engine::time::TempoMap;
use egui::{RichText, Ui};
use redhive_contracts::ui::Severity;
use std::sync::{Arc, Mutex};

/// Silence between songs when they are added (seconds).
const GAP: f64 = 2.0;

fn album_bus(st: &State) -> Option<Id> {
    st.session.project.mixer.channels.iter().find(|c| c.name == crate::modes::ALBUM_BUS).map(|c| c.id)
}

/// Group buses: buses that feed the album bus and carry no track of their own.
fn groups(st: &State, album: Id) -> Vec<(Id, String)> {
    let p = &st.session.project;
    p.mixer.channels.iter().filter(|c| c.output == Some(album) && !p.tracks.iter().any(|t| t.mixer == c.id)).map(|c| (c.id, c.name.clone())).collect()
}

/// Puts each file on its own track (with its own mixer channel to the album bus), one after the other.
pub fn add_songs(st: &mut State, files: &[std::path::PathBuf], album: Id) {
    for path in files {
        let end = st.session.project.tracks.iter().flat_map(|t| t.clips.iter()).map(|c| c.end()).fold(0.0f64, f64::max);
        let tm = TempoMap::from_project(&st.session.project);
        let start = if end > 0.0 { tm.seconds_to_beat(tm.beat_to_seconds(end) + GAP) } else { 0.0 };
        let name = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Song".into());
        let tid = st.session.edit("Add song", |p| {
            let t = p.add_track(&name);
            let m = p.track(t).map(|x| x.mixer).unwrap_or(0);
            if let Some(c) = p.mixer.get_mut(m) {
                c.output = Some(album);
            }
            t
        });
        st.add_audio_clip(path, Some(tid), start);
    }
}

fn start_match(st: &mut State) {
    if st.album_job.is_some() {
        return;
    }
    let project = st.session.project.clone();
    let cache = st.session.cache.clone();
    let rate = st.audio.rate() as f32;
    let progress = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let result = Arc::new(Mutex::new(None));
    let (p2, r2) = (progress.clone(), result.clone());
    std::thread::spawn(move || {
        let mut pool = dragon_engine::render::Pool::with_plugins(Box::new(dragon_engine::plugins::Host));
        // A whole album: up to two hours.
        let l = dragon_engine::render::measure_strip_loudness(&project, &cache, rate, &mut pool, 7200.0, &mut |x| p2.store((x * 1000.0) as u32, std::sync::atomic::Ordering::Relaxed));
        if let Ok(mut r) = r2.lock() {
            *r = Some(l);
        }
        redhive_client_framework::runtime::request_repaint();
    });
    st.album_job = Some(BalanceJob { progress, result });
}

/// The faders that bring every song to the same loudness (the median of the measured songs, so the
/// least moves): (channel, new volume). Songs that measured silent keep theirs.
pub fn matched_faders(songs: &[Id], loudness: &[(Id, f32)]) -> (f32, Vec<(Id, f32)>) {
    let mut l: Vec<(Id, f32)> = songs.iter().filter_map(|s| loudness.iter().find(|(id, _)| id == s).copied()).filter(|(_, v)| *v > -60.0).collect();
    if l.is_empty() {
        return (0.0, Vec::new());
    }
    let mut v: Vec<f32> = l.iter().map(|x| x.1).collect();
    v.sort_by(f32::total_cmp);
    let target = v[v.len() / 2];
    l.iter_mut().for_each(|x| x.1 = dragon_engine::dsp::db_to_gain((target - x.1).clamp(-24.0, 24.0)));
    (target, l)
}

/// Applies a finished measurement (called every frame).
pub fn poll(st: &mut State) {
    let Some(job) = &st.album_job else { return };
    let Some(l) = job.result.lock().ok().and_then(|mut r| r.take()) else { return };
    st.album_job = None;
    let songs: Vec<Id> = st.session.project.tracks.iter().map(|t| t.mixer).collect();
    st.ui.album_lufs = l.clone();
    let (target, faders) = matched_faders(&songs, &l);
    if faders.is_empty() {
        st.toast("Album: no song with audio to match", Severity::Warning);
        return;
    }
    let n = faders.len();
    st.session.edit("Match album loudness", |p| {
        for (id, v) in &faders {
            if let Some(c) = p.mixer.get_mut(*id) {
                c.volume = *v;
            }
        }
    });
    st.toast(format!("Matched {n} song{} to {target:.1} LUFS before the master chain – Ctrl+Z to undo", if n == 1 { "" } else { "s" }), Severity::Info);
}

pub fn show(ui: &mut Ui, st: &mut State) {
    ui.heading("Album");
    let Some(album) = album_bus(st) else {
        if ui.button("Create the album bus").clicked() {
            st.session.edit("Album bus", |p| {
                crate::modes::bus(p, crate::modes::ALBUM_BUS, "bus");
            });
        }
        return;
    };
    // Songs dropped from the file manager go straight to the master: route them through the album bus.
    let stray: Vec<Id> = {
        let p = &st.session.project;
        p.tracks.iter().map(|t| t.mixer).filter(|m| *m != album && p.mixer.get(*m).is_some_and(|c| c.output.is_none() && c.kind == ChannelKind::Audio)).collect()
    };
    if !stray.is_empty() {
        st.session.edit("Route songs to the album bus", |p| {
            for m in &stray {
                if let Some(c) = p.mixer.get_mut(*m) {
                    c.output = Some(album);
                }
            }
        });
    }
    ui.horizontal(|ui| {
        if ui.button("＋ Add songs…").on_hover_text("Each mixdown gets its own track and mixer channel, one after the other").clicked() {
            if let Some(files) = rfd::FileDialog::new().add_filter("Audio", dragon_engine::samples::EXTENSIONS).pick_files() {
                add_songs(st, &files, album);
            }
        }
        match &st.album_job {
            Some(job) => {
                ui.spinner();
                ui.label(format!("Measuring … {:.0} %", job.progress.load(std::sync::atomic::Ordering::Relaxed) as f32 / 10.0));
            }
            None => {
                if ui
                    .add_enabled(!st.session.project.tracks.is_empty(), egui::Button::new("Match loudness"))
                    .on_hover_text("Measures every song and sets its fader so all of them reach the master chain equally loud")
                    .clicked()
                {
                    start_match(st);
                }
            }
        }
        if ui.button("＋ Group bus").on_hover_text("A bus between some songs and the album bus (for songs that need their own treatment)").clicked() {
            let n = groups(st, album).len() + 1;
            st.session.edit("Group bus", |p| {
                let g = crate::modes::bus(p, &format!("Group {n}"), "bus");
                if let Some(c) = p.mixer.get_mut(g) {
                    c.output = Some(album);
                }
            });
        }
    });
    let groups = groups(st, album);
    let tm = TempoMap::from_project(&st.session.project);
    let songs: Vec<(Id, Id, String, f64, f64, bool)> = st
        .session
        .project
        .tracks
        .iter()
        .map(|t| {
            let start = t.clips.iter().map(|c| c.start).fold(f64::MAX, f64::min);
            let end = t.clips.iter().map(|c| c.end()).fold(0.0f64, f64::max);
            let start = if start == f64::MAX { 0.0 } else { start };
            (t.id, t.mixer, t.name.clone(), start, tm.beat_to_seconds(end) - tm.beat_to_seconds(start), t.solo)
        })
        .collect();
    if songs.is_empty() {
        ui.label(RichText::new("No songs yet: add your mixdowns (or drop them onto the playlist).").color(theme::TEXT_DIM));
        return;
    }
    egui::Grid::new("album.songs").striped(true).num_columns(7).spacing([14.0, 6.0]).show(ui, |ui| {
        for h in ["#", "Song", "Length", "Measured", "Gain", "Goes to", ""] {
            ui.label(RichText::new(h).small().color(theme::TEXT_DIM));
        }
        ui.end_row();
        for (k, (tid, mixer, name, start, secs, solo)) in songs.iter().enumerate() {
            ui.label(format!("{}", k + 1));
            ui.label(RichText::new(name).strong());
            ui.label(format!("{}:{:02}", (*secs as u32) / 60, (*secs as u32) % 60));
            let lufs = st.ui.album_lufs.iter().find(|(id, _)| id == mixer).map(|x| x.1);
            ui.label(RichText::new(lufs.filter(|v| *v > -60.0).map(|v| format!("{v:.1} LUFS")).unwrap_or_else(|| "–".into())).monospace());
            let vol = st.session.project.mixer.get(*mixer).map(|c| c.volume).unwrap_or(1.0);
            ui.label(RichText::new(format!("{:+.1} dB", dragon_engine::dsp::gain_to_db(vol.max(1e-6)))).monospace());
            let out = st.session.project.mixer.get(*mixer).and_then(|c| c.output).unwrap_or(album);
            let out_name = if out == album { crate::modes::ALBUM_BUS.to_string() } else { groups.iter().find(|g| g.0 == out).map(|g| g.1.clone()).unwrap_or_else(|| "Master".into()) };
            let mut to = out;
            egui::ComboBox::from_id_salt(("album.to", tid)).selected_text(out_name).width(120.0).show_ui(ui, |ui| {
                ui.selectable_value(&mut to, album, crate::modes::ALBUM_BUS);
                for (g, n) in &groups {
                    ui.selectable_value(&mut to, *g, n);
                }
            });
            if to != out {
                let m = *mixer;
                st.session.edit("Song routing", |p| {
                    if let Some(c) = p.mixer.get_mut(m) {
                        c.output = Some(to);
                    }
                });
            }
            ui.horizontal(|ui| {
                if ui.small_button("▶").on_hover_text("Play from this song's start").clicked() {
                    st.seek(*start);
                    if !st.transport().playing {
                        st.toggle_play();
                    }
                }
                if super::widgets::toggle(ui, *solo, "S", theme::OK).on_hover_text("Solo this song").clicked() {
                    let t = *tid;
                    st.session.edit("Solo song", |p| p.track_mut(t).iter_mut().for_each(|x| x.solo = !x.solo));
                }
            });
            ui.end_row();
        }
    });
    ui.label(
        RichText::new("Every song goes through the album bus into the master chain. Match loudness sets each song's fader so they all reach the chain equally loud; the chain then treats them alike.")
            .small()
            .color(theme::TEXT_DIM),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching_brings_every_song_to_the_median() {
        let (target, f) = matched_faders(&[1, 2, 3, 4], &[(1, -10.0), (2, -14.0), (3, -20.0), (4, -80.0)]);
        assert_eq!(target, -14.0);
        assert_eq!(f.len(), 3, "the silent song keeps its fader");
        let g = |id: Id| dragon_engine::dsp::gain_to_db(f.iter().find(|x| x.0 == id).unwrap().1);
        assert!((g(1) + 4.0).abs() < 1e-3 && g(2).abs() < 1e-3 && (g(3) - 6.0).abs() < 1e-3);
    }

    /// A 16-bit mono WAV of a sine at `amp`.
    fn wav(path: &std::path::Path, amp: f32, secs: f32) {
        let rate = 48_000u32;
        let n = (secs * rate as f32) as u32;
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + n * 2).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&rate.to_le_bytes());
        b.extend_from_slice(&(rate * 2).to_le_bytes());
        b.extend_from_slice(&2u16.to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&(n * 2).to_le_bytes());
        for i in 0..n {
            let v = (amp * (std::f32::consts::TAU * 440.0 * i as f32 / rate as f32).sin() * 32767.0) as i16;
            b.extend_from_slice(&v.to_le_bytes());
        }
        std::fs::write(path, b).unwrap();
    }

    #[test]
    fn an_album_of_two_songs_comes_out_equally_loud() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("Loud.wav"), dir.path().join("Quiet.wav"));
        wav(&a, 0.5, 3.0);
        wav(&b, 0.1, 3.0);
        let mut st = State::new(redhive_client_framework::AppContext::new("en"), crate::state::Settings { show_splash: false, ..Default::default() });
        crate::modes::start(&mut st, crate::modes::Mode::Album);
        let album = album_bus(&st).unwrap();
        add_songs(&mut st, &[a, b], album);
        let p = &st.session.project;
        assert_eq!(p.tracks.len(), 2);
        assert!(p.tracks.iter().all(|t| p.mixer.get(t.mixer).unwrap().output == Some(album)), "both through the album bus");
        let first_end = p.tracks[0].clips[0].end();
        assert!(p.tracks[1].clips[0].start > first_end, "one after the other, with a gap");
        let measure = |st: &State| {
            let mut pool = dragon_engine::render::Pool::new();
            dragon_engine::render::measure_strip_loudness(&st.session.project, &st.session.cache, 48_000.0, &mut pool, 60.0, &mut |_| {})
        };
        let songs: Vec<Id> = st.session.project.tracks.iter().map(|t| t.mixer).collect();
        let (_, faders) = matched_faders(&songs, &measure(&st));
        assert_eq!(faders.len(), 2);
        st.session.edit("match", |p| {
            for (id, v) in &faders {
                p.mixer.get_mut(*id).unwrap().volume = *v;
            }
        });
        // Post-fader, both songs now reach the album bus equally loud.
        let l = measure(&st);
        let post = |m: Id| l.iter().find(|x| x.0 == m).unwrap().1 + dragon_engine::dsp::gain_to_db(st.session.project.mixer.get(m).unwrap().volume);
        assert!((post(songs[0]) - post(songs[1])).abs() < 0.5, "{} vs {}", post(songs[0]), post(songs[1]));
    }
}
