//! Surround mixing (5.1 / 7.1 / 7.1.4): every top-level mixer channel is rendered as a stem and placed in the
//! surround field with vector-base amplitude panning (VBAP) between the two nearest speakers of a layer; with
//! height speakers the source is crossfaded (constant power) between the ear-level and the height layer by its
//! `z`. A low-passed share goes to the LFE. The result is written as one multichannel WAV in WAVE / SMPTE order.
//!
//! For Dolby Atmos and other object-based formats, [`export_adm`] writes the same stems as *objects* in an ADM
//! Broadcast WAV (ITU-R BS.2076 metadata in an `axml` chunk, BS.2088 `chna` track map): every top-level channel
//! becomes two mono objects (its left and right side) with their position, which the Dolby Atmos Renderer,
//! DaVinci Resolve, Nuendo and other ADM-capable tools import as an object-based mix.
//!
//! The stereo renderer is reused unchanged, so effects, sends and buses behave exactly as in the stereo mix;
//! the master bus inserts are not applied to the surround mix (a stereo master chain has no meaning there).

use crate::dsp::{Biquad, BiquadKind};
use crate::export::{render_mix, ExportOptions};
use crate::model::*;
use crate::render::Pool;
use crate::samples::{AudioData, BitDepth, SampleCache};
use crate::{EngineError, Result};
use std::path::Path;

/// Speaker azimuths in degrees (0 = front, positive = right) per output channel; `None` = LFE.
pub fn layout(f: SurroundFormat) -> &'static [Option<f32>] {
    match f {
        SurroundFormat::S5_1 => &[Some(-30.0), Some(30.0), Some(0.0), None, Some(-110.0), Some(110.0)],
        SurroundFormat::S7_1 => &[Some(-30.0), Some(30.0), Some(0.0), None, Some(-150.0), Some(150.0), Some(-90.0), Some(90.0)],
        SurroundFormat::S7_1_4 => &[Some(-30.0), Some(30.0), Some(0.0), None, Some(-150.0), Some(150.0), Some(-90.0), Some(90.0), Some(-45.0), Some(45.0), Some(-135.0), Some(135.0)],
    }
}

/// Speaker elevations in degrees per output channel (0 = ear level).
pub fn elevation(f: SurroundFormat) -> &'static [f32] {
    match f {
        SurroundFormat::S5_1 => &[0.0; 6],
        SurroundFormat::S7_1 => &[0.0; 8],
        SurroundFormat::S7_1_4 => &[0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 45.0, 45.0, 45.0, 45.0],
    }
}

/// Display name of a format.
pub fn label(f: SurroundFormat) -> &'static str {
    match f {
        SurroundFormat::S5_1 => "5.1",
        SurroundFormat::S7_1 => "7.1",
        SurroundFormat::S7_1_4 => "7.1.4",
    }
}

/// Elevation in degrees of a pan position's height.
pub fn height_to_elevation(z: f32) -> f32 {
    z.clamp(0.0, 1.0) * 45.0
}

pub fn channel_names(f: SurroundFormat) -> &'static [&'static str] {
    match f {
        SurroundFormat::S5_1 => &["L", "R", "C", "LFE", "Ls", "Rs"],
        SurroundFormat::S7_1 => &["L", "R", "C", "LFE", "Lb", "Rb", "Ls", "Rs"],
        SurroundFormat::S7_1_4 => &["L", "R", "C", "LFE", "Lb", "Rb", "Ls", "Rs", "Ltf", "Rtf", "Ltr", "Rtr"],
    }
}

/// Constant-power gains for an ear-level source at `azimuth` degrees (VBAP between the two adjacent speakers).
pub fn vbap(f: SurroundFormat, azimuth: f32) -> Vec<f32> {
    vbap3(f, azimuth, 0.0)
}

/// Constant-power gains for a source at `azimuth` degrees and height `z` (0 … 1). Without height speakers `z`
/// is ignored; with them the source is crossfaded between the ear-level and the height layer.
pub fn vbap3(f: SurroundFormat, azimuth: f32, z: f32) -> Vec<f32> {
    let lay = layout(f);
    let el = elevation(f);
    let mut gains = vec![0.0f32; lay.len()];
    let has_height = el.iter().any(|e| *e > 0.0);
    let z = if has_height { z.clamp(0.0, 1.0) } else { 0.0 };
    let (g_bed, g_top) = ((z * std::f32::consts::FRAC_PI_2).cos(), (z * std::f32::consts::FRAC_PI_2).sin());
    for (top, layer_gain) in [(false, g_bed), (true, g_top)] {
        if layer_gain < 1e-6 {
            continue;
        }
        let speakers: Vec<(usize, f32)> = lay.iter().enumerate().filter(|(i, _)| (el[*i] > 0.0) == top).filter_map(|(i, a)| a.map(|a| (i, a))).collect();
        for (i, g) in pan_ring(speakers, azimuth) {
            gains[i] += g * layer_gain;
        }
    }
    gains
}

/// Pairwise constant-power panning on a ring of speakers `(channel, azimuth)`.
fn pan_ring(mut speakers: Vec<(usize, f32)>, azimuth: f32) -> Vec<(usize, f32)> {
    speakers.sort_by(|a, b| a.1.total_cmp(&b.1));
    let az = ((azimuth + 180.0).rem_euclid(360.0)) - 180.0;
    let n = speakers.len();
    for k in 0..n {
        let (ia, a) = speakers[k];
        let (ib, mut b) = speakers[(k + 1) % n];
        if b <= a {
            b += 360.0;
        }
        let mut x = az;
        if x < a {
            x += 360.0;
        }
        if x >= a && x <= b {
            let t = (x - a) / (b - a);
            return vec![(ia, (t * std::f32::consts::FRAC_PI_2).cos()), (ib, (t * std::f32::consts::FRAC_PI_2).sin())];
        }
    }
    Vec::new()
}

/// Azimuth of a pan position (x right, y front).
pub fn azimuth(p: &SurroundPan) -> f32 {
    p.x.atan2(p.y).to_degrees()
}

/// Renders every top-level mixer channel as a stereo stem (soloed, with its sources, buses and sends), so the
/// stems add up to the mix. Master inserts are left out.
fn top_level_stems(p: &Project, cache: &SampleCache, opts: &ExportOptions, make_pool: &mut dyn FnMut() -> Pool, progress: &mut dyn FnMut(f32)) -> Vec<(MixerChannel, AudioData)> {
    let top: Vec<MixerChannel> = p.mixer.channels.iter().filter(|c| c.kind == ChannelKind::Audio && c.output.is_none()).cloned().collect();
    let mut base = p.clone();
    base.mixer.master.inserts.clear();
    let opts = ExportOptions { normalize_peak: None, loudness_target: None, ..opts.clone() };
    let count = top.len().max(1);
    let mut out = Vec::new();
    for (k, ch) in top.into_iter().enumerate() {
        let mut q = base.clone();
        for c in q.mixer.channels.iter_mut() {
            c.solo = c.id == ch.id;
        }
        let mut pool = make_pool();
        let stem: AudioData = render_mix(&q, cache, &opts, &mut pool, &mut |x| progress((k as f32 + x) / count as f32));
        out.push((ch, stem));
    }
    out
}

/// Renders the surround mix: one buffer per output channel.
pub fn render_surround(p: &Project, cache: &SampleCache, format: SurroundFormat, opts: &ExportOptions, make_pool: &mut dyn FnMut() -> Pool, progress: &mut dyn FnMut(f32)) -> (u32, Vec<Vec<f32>>) {
    let mut out: Vec<Vec<f32>> = Vec::new();
    let mut rate = opts.render_rate;
    for (ch, stem) in top_level_stems(p, cache, opts, make_pool, progress) {
        rate = stem.rate;
        if out.is_empty() {
            out = vec![vec![0.0; stem.frames()]; layout(format).len()];
        }
        let len = stem.frames().min(out[0].len());
        if stem.peak() < 1e-6 {
            continue;
        }
        let az = azimuth(&ch.surround);
        let half = ch.surround.spread.clamp(0.0, 180.0) / 2.0;
        let gl = vbap3(format, az - half, ch.surround.z);
        let gr = vbap3(format, az + half, ch.surround.z);
        for (o, (a, b)) in out.iter_mut().zip(gl.iter().zip(&gr)) {
            if *a == 0.0 && *b == 0.0 {
                continue;
            }
            for i in 0..len {
                o[i] += stem.l[i] * a + stem.r[i] * b;
            }
        }
        if ch.surround.lfe > 0.0 {
            let lfe = layout(format).iter().position(|a| a.is_none()).unwrap();
            let mut lp = Biquad::new(BiquadKind::LowPass, 120.0, 0.707, 0.0, rate as f32);
            for i in 0..len {
                out[lfe][i] += lp.process((stem.l[i] + stem.r[i]) * 0.5) * ch.surround.lfe;
            }
        }
    }
    (rate, out)
}

/// Writes a multichannel WAV (WAVE_FORMAT_EXTENSIBLE with the standard channel mask).
pub fn write_multichannel(path: &Path, channels: &[Vec<f32>], rate: u32, depth: BitDepth) -> Result<()> {
    let spec = match depth {
        BitDepth::Int16 => hound::WavSpec { channels: channels.len() as u16, sample_rate: rate, bits_per_sample: 16, sample_format: hound::SampleFormat::Int },
        BitDepth::Int24 => hound::WavSpec { channels: channels.len() as u16, sample_rate: rate, bits_per_sample: 24, sample_format: hound::SampleFormat::Int },
        BitDepth::Float32 => hound::WavSpec { channels: channels.len() as u16, sample_rate: rate, bits_per_sample: 32, sample_format: hound::SampleFormat::Float },
    };
    if let Some(d) = path.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let mut w = hound::WavWriter::create(path, spec).map_err(|e| EngineError::Io(e.to_string()))?;
    let n = channels.first().map(|c| c.len()).unwrap_or(0);
    let full = match depth {
        BitDepth::Int16 => 32767.0,
        BitDepth::Int24 => 8_388_607.0,
        BitDepth::Float32 => 1.0,
    };
    for i in 0..n {
        for c in channels {
            let s = c[i];
            match depth {
                BitDepth::Float32 => w.write_sample(s),
                _ => w.write_sample((s as f64 * full).round().clamp(-full - 1.0, full) as i32),
            }
            .map_err(|e| EngineError::Io(e.to_string()))?;
        }
    }
    w.finalize().map_err(|e| EngineError::Io(e.to_string()))
}

/// Renders and writes the project's surround mix.
pub fn export_surround(p: &Project, cache: &SampleCache, path: &Path, opts: &ExportOptions, make_pool: &mut dyn FnMut() -> Pool, progress: &mut dyn FnMut(f32)) -> Result<Vec<f32>> {
    let format = p.surround.ok_or_else(|| EngineError::Project("the project has no surround format".into()))?;
    let (rate, ch) = render_surround(p, cache, format, opts, make_pool, progress);
    if ch.is_empty() {
        return Err(EngineError::Project("nothing to export".into()));
    }
    // Peak-protect: scale the whole mix if any channel clips.
    let peak = ch.iter().flat_map(|c| c.iter()).fold(0.0f32, |m, x| m.max(x.abs()));
    let ch: Vec<Vec<f32>> = if peak > 0.999 { ch.into_iter().map(|c| c.into_iter().map(|x| x * 0.999 / peak).collect()).collect() } else { ch };
    let depth = match opts.format {
        crate::export::ExportFormat::Wav(d) => d,
        _ => BitDepth::Int24,
    };
    write_multichannel(path, &ch, rate, depth)?;
    Ok(ch.iter().map(|c| c.iter().fold(0.0f32, |m, x| m.max(x.abs()))).collect())
}

/// One mono object of an ADM BWF file.
pub struct AdmObject {
    pub name: String,
    /// Degrees, 0 = front, positive = right (converted to the ADM convention, positive = left, when written).
    pub azimuth: f32,
    /// Degrees above ear level.
    pub elevation: f32,
    pub samples: Vec<f32>,
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Builds the ADM XML (ITU-R BS.2076) describing `objects` as static object-type channels.
pub fn adm_xml(title: &str, objects: &[AdmObject], rate: u32) -> String {
    let frames = objects.first().map(|o| o.samples.len()).unwrap_or(0) as f64;
    let dur = |secs: f64| {
        let h = (secs / 3600.0).floor();
        let m = ((secs - h * 3600.0) / 60.0).floor();
        let s = secs - h * 3600.0 - m * 60.0;
        format!("{:02}:{:02}:{:08.5}", h as u32, m as u32, s)
    };
    let length = dur(frames / rate.max(1) as f64);
    let mut x = String::new();
    x.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ebuCoreMain xmlns=\"urn:ebu:metadata-schema:ebuCore_2016\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\"><coreMetadata><format><audioFormatExtended version=\"ITU-R_BS.2076-2\">\n");
    x.push_str(&format!("<audioProgramme audioProgrammeID=\"APR_1001\" audioProgrammeName=\"{}\" start=\"00:00:00.00000\" end=\"{length}\"><audioContentIDRef>ACO_1001</audioContentIDRef></audioProgramme>\n", xml_escape(title)));
    x.push_str(&format!("<audioContent audioContentID=\"ACO_1001\" audioContentName=\"{}\">", xml_escape(title)));
    for k in 0..objects.len() {
        x.push_str(&format!("<audioObjectIDRef>AO_{:04X}</audioObjectIDRef>", 0x1001 + k));
    }
    x.push_str("</audioContent>\n");
    for (k, o) in objects.iter().enumerate() {
        let n = 0x1001 + k;
        let name = xml_escape(&o.name);
        // ADM azimuth is counter-clockwise (positive = left).
        let az = -(((o.azimuth + 180.0).rem_euclid(360.0)) - 180.0);
        x.push_str(&format!("<audioObject audioObjectID=\"AO_{n:04X}\" audioObjectName=\"{name}\" start=\"00:00:00.00000\" duration=\"{length}\"><audioPackFormatIDRef>AP_0003{n:04X}</audioPackFormatIDRef><audioTrackUIDRef>ATU_{:08X}</audioTrackUIDRef></audioObject>\n", k + 1));
        x.push_str(&format!("<audioPackFormat audioPackFormatID=\"AP_0003{n:04X}\" audioPackFormatName=\"{name}\" typeLabel=\"0003\" typeDefinition=\"Objects\"><audioChannelFormatIDRef>AC_0003{n:04X}</audioChannelFormatIDRef></audioPackFormat>\n"));
        x.push_str(&format!("<audioChannelFormat audioChannelFormatID=\"AC_0003{n:04X}\" audioChannelFormatName=\"{name}\" typeLabel=\"0003\" typeDefinition=\"Objects\"><audioBlockFormat audioBlockFormatID=\"AB_0003{n:04X}_00000001\" rtime=\"00:00:00.00000\" duration=\"{length}\"><position coordinate=\"azimuth\">{az:.2}</position><position coordinate=\"elevation\">{:.2}</position><position coordinate=\"distance\">1.0</position><gain>1.0</gain></audioBlockFormat></audioChannelFormat>\n", o.elevation));
        x.push_str(&format!("<audioStreamFormat audioStreamFormatID=\"AS_0003{n:04X}\" audioStreamFormatName=\"PCM_{name}\" formatLabel=\"0001\" formatDefinition=\"PCM\"><audioChannelFormatIDRef>AC_0003{n:04X}</audioChannelFormatIDRef><audioTrackFormatIDRef>AT_0003{n:04X}_01</audioTrackFormatIDRef></audioStreamFormat>\n"));
        x.push_str(&format!("<audioTrackFormat audioTrackFormatID=\"AT_0003{n:04X}_01\" audioTrackFormatName=\"PCM_{name}\" formatLabel=\"0001\" formatDefinition=\"PCM\"><audioStreamFormatIDRef>AS_0003{n:04X}</audioStreamFormatIDRef></audioTrackFormat>\n"));
        x.push_str(&format!("<audioTrackUID UID=\"ATU_{:08X}\" sampleRate=\"{rate}\" bitDepth=\"24\"><audioTrackFormatIDRef>AT_0003{n:04X}_01</audioTrackFormatIDRef><audioPackFormatIDRef>AP_0003{n:04X}</audioPackFormatIDRef></audioTrackUID>\n", k + 1));
    }
    x.push_str("</audioFormatExtended></format></coreMetadata></ebuCoreMain>\n");
    x
}

/// Builds the `chna` chunk body (ITU-R BS.2088): track index → UID → track format → pack format.
pub fn chna_chunk(count: usize) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(&(count as u16).to_le_bytes());
    b.extend_from_slice(&(count as u16).to_le_bytes());
    for k in 0..count {
        let n = 0x1001 + k;
        b.extend_from_slice(&((k + 1) as u16).to_le_bytes());
        b.extend_from_slice(format!("ATU_{:08X}", k + 1).as_bytes());
        b.extend_from_slice(format!("AT_0003{n:04X}_01").as_bytes());
        b.extend_from_slice(format!("AP_0003{n:04X}").as_bytes());
        b.push(0);
    }
    b
}

/// Writes an ADM Broadcast WAV: 24-bit PCM, one track per object, with `chna` and `axml` chunks.
pub fn write_adm_bwf(path: &Path, title: &str, objects: &[AdmObject], rate: u32) -> Result<()> {
    use std::io::Write;
    if objects.is_empty() {
        return Err(EngineError::Project("nothing to export".into()));
    }
    let ch = objects.len();
    let frames = objects.iter().map(|o| o.samples.len()).min().unwrap_or(0);
    let data_len = frames as u64 * ch as u64 * 3;
    if data_len > u32::MAX as u64 - 1_000_000 {
        return Err(EngineError::Project("the ADM file would exceed 4 GB (RF64/BW64 is not supported); export a shorter range".into()));
    }
    let chna = chna_chunk(ch);
    let axml = adm_xml(title, objects, rate).into_bytes();
    let mut out = Vec::with_capacity(data_len as usize + axml.len() + chna.len() + 64);
    let chunk = |out: &mut Vec<u8>, id: &[u8; 4], body: &[u8]| {
        out.extend_from_slice(id);
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(body);
        if body.len() % 2 == 1 {
            out.push(0);
        }
    };
    out.extend_from_slice(b"RIFF\0\0\0\0WAVE");
    let mut fmt = Vec::new();
    fmt.extend_from_slice(&1u16.to_le_bytes());
    fmt.extend_from_slice(&(ch as u16).to_le_bytes());
    fmt.extend_from_slice(&rate.to_le_bytes());
    fmt.extend_from_slice(&(rate * ch as u32 * 3).to_le_bytes());
    fmt.extend_from_slice(&(ch as u16 * 3).to_le_bytes());
    fmt.extend_from_slice(&24u16.to_le_bytes());
    chunk(&mut out, b"fmt ", &fmt);
    chunk(&mut out, b"chna", &chna);
    let mut data = Vec::with_capacity(data_len as usize);
    for i in 0..frames {
        for o in objects {
            let v = (o.samples[i] as f64 * 8_388_607.0).round().clamp(-8_388_608.0, 8_388_607.0) as i32;
            data.extend_from_slice(&v.to_le_bytes()[..3]);
        }
    }
    chunk(&mut out, b"data", &data);
    chunk(&mut out, b"axml", &axml);
    let riff = (out.len() - 8) as u32;
    out[4..8].copy_from_slice(&riff.to_le_bytes());
    if let Some(d) = path.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    std::fs::File::create(path).and_then(|mut f| f.write_all(&out)).map_err(|e| EngineError::Io(e.to_string()))
}

/// Renders every top-level mixer channel and writes it as two mono objects (left and right side, placed by its
/// surround panner, spread and height) in an ADM BWF file for Dolby Atmos and other object-based tools.
/// Works whether or not the project has a surround bed format. Returns the number of objects.
pub fn export_adm(p: &Project, cache: &SampleCache, path: &Path, opts: &ExportOptions, make_pool: &mut dyn FnMut() -> Pool, progress: &mut dyn FnMut(f32)) -> Result<usize> {
    let mut rate = opts.render_rate;
    let mut objects = Vec::new();
    for (ch, stem) in top_level_stems(p, cache, opts, make_pool, progress) {
        rate = stem.rate;
        if stem.peak() < 1e-6 {
            continue;
        }
        let az = azimuth(&ch.surround);
        let half = ch.surround.spread.clamp(0.0, 180.0) / 2.0;
        let el = height_to_elevation(ch.surround.z);
        objects.push(AdmObject { name: format!("{} L", ch.name), azimuth: az - half, elevation: el, samples: stem.l.clone() });
        objects.push(AdmObject { name: format!("{} R", ch.name), azimuth: az + half, elevation: el, samples: stem.r.clone() });
    }
    if objects.is_empty() {
        return Err(EngineError::Project("nothing to export".into()));
    }
    let peak = objects.iter().flat_map(|o| o.samples.iter()).fold(0.0f32, |m, x| m.max(x.abs()));
    if peak > 0.999 {
        for o in objects.iter_mut() {
            o.samples.iter_mut().for_each(|x| *x *= 0.999 / peak);
        }
    }
    let title = if p.meta.name.is_empty() { "DragonDAW mix".to_string() } else { p.meta.name.clone() };
    write_adm_bwf(path, &title, &objects, rate)?;
    Ok(objects.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vbap_hits_speakers_and_keeps_power() {
        let f = SurroundFormat::S5_1;
        let g = vbap(f, 0.0);
        assert!((g[2] - 1.0).abs() < 1e-5, "centre: {g:?}");
        let g = vbap(f, -110.0);
        assert!((g[4] - 1.0).abs() < 1e-5, "left surround: {g:?}");
        for az in [-170.0f32, -70.0, 15.0, 80.0, 179.0] {
            let p: f32 = vbap(f, az).iter().map(|x| x * x).sum();
            assert!((p - 1.0).abs() < 1e-4, "{az}: power {p}");
        }
        // Rear centre sits between Ls and Rs.
        let g = vbap(f, 180.0);
        assert!(g[4] > 0.6 && g[5] > 0.6 && g[0] == 0.0, "{g:?}");
    }

    #[test]
    fn surround_export_places_channels() {
        let dir = tempfile::tempdir().unwrap();
        let tone = dir.path().join("t.wav");
        let s: Vec<f32> = (0..48000).map(|i| (i as f32 * 0.05).sin() * 0.4).collect();
        crate::samples::write_wav(&tone, &s, &s, 48000, BitDepth::Float32, false, false).unwrap();
        let mut p = Project::new_default();
        p.bpm = 120.0;
        p.surround = Some(SurroundFormat::S5_1);
        // Track 1 in the rear left, track 2 front centre with no spread.
        for (k, pan) in [SurroundPan { x: -0.94, y: -0.34, z: 0.0, spread: 0.0, lfe: 0.5 }, SurroundPan { x: 0.0, y: 1.0, z: 0.0, spread: 0.0, lfe: 0.0 }].into_iter().enumerate() {
            let id = p.next_id();
            p.tracks[k].clips.push(Clip { id, name: "t".into(), start: 0.0, length: 2.0, muted: false, color: Rgb::default(), kind: ClipKind::Audio(AudioClip { source: tone.clone(), ..Default::default() }) });
            let m = p.tracks[k].mixer;
            p.mixer.get_mut(m).unwrap().surround = pan;
        }
        let out = dir.path().join("mix51.wav");
        let peaks = export_surround(&p, &SampleCache::default(), &out, &ExportOptions { tail: 0.0, ..Default::default() }, &mut Pool::new, &mut |_| {}).unwrap();
        // L R C LFE Ls Rs
        assert!(peaks[4] > 0.2, "rear-left stem in Ls: {peaks:?}");
        assert!(peaks[2] > 0.2, "centre stem in C: {peaks:?}");
        assert!(peaks[3] > 0.01, "LFE share: {peaks:?}");
        assert!(peaks[1] < 1e-3 && peaks[5] < 0.05, "nothing on the right: {peaks:?}");
        let r = hound::WavReader::open(&out).unwrap();
        assert_eq!(r.spec().channels, 6);
    }
    #[test]
    fn height_layer_crossfade() {
        let f = SurroundFormat::S7_1_4;
        let g = vbap3(f, -45.0, 1.0);
        assert!((g[8] - 1.0).abs() < 1e-5, "top front left: {g:?}");
        let g = vbap3(f, 0.0, 0.5);
        let bed: f32 = g[..8].iter().map(|x| x * x).sum();
        let top: f32 = g[8..].iter().map(|x| x * x).sum();
        assert!((bed - 0.5).abs() < 1e-4 && (top - 0.5).abs() < 1e-4, "{g:?}");
        // No height speakers: z is ignored.
        assert_eq!(vbap3(SurroundFormat::S5_1, 0.0, 1.0), vbap(SurroundFormat::S5_1, 0.0));
    }

    #[test]
    fn adm_bwf_has_objects_and_chunks() {
        let dir = tempfile::tempdir().unwrap();
        let tone = dir.path().join("t.wav");
        let s: Vec<f32> = (0..24000).map(|i| (i as f32 * 0.05).sin() * 0.4).collect();
        crate::samples::write_wav(&tone, &s, &s, 48000, BitDepth::Float32, false, false).unwrap();
        let mut p = Project::new_default();
        let id = p.next_id();
        p.tracks[0].clips.push(Clip { id, name: "t".into(), start: 0.0, length: 1.0, muted: false, color: Rgb::default(), kind: ClipKind::Audio(AudioClip { source: tone.clone(), ..Default::default() }) });
        let m = p.tracks[0].mixer;
        p.mixer.get_mut(m).unwrap().surround = SurroundPan { x: 0.0, y: 1.0, z: 1.0, spread: 60.0, lfe: 0.0 };
        let out = dir.path().join("atmos.wav");
        let n = export_adm(&p, &SampleCache::default(), &out, &ExportOptions { tail: 0.0, ..Default::default() }, &mut Pool::new, &mut |_| {}).unwrap();
        assert_eq!(n, 2);
        let bytes = std::fs::read(&out).unwrap();
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize, bytes.len() - 8);
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("chna") && text.contains("axml"));
        assert!(text.contains("typeDefinition=\"Objects\""));
        assert!(text.contains("<position coordinate=\"elevation\">45.00</position>"));
        // Left side at -30° (right-positive) is +30 in ADM; right side -30.
        assert!(text.contains("<position coordinate=\"azimuth\">30.00</position>") && text.contains("<position coordinate=\"azimuth\">-30.00</position>"));
        let r = hound::WavReader::open(&out).unwrap();
        assert_eq!(r.spec().channels, 2);
        assert_eq!(r.spec().bits_per_sample, 24);
    }
}
