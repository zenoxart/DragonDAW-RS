//! Offline bounce: the whole song, the loop, a range or one pattern, to WAV (16/24-bit with dither, 32-bit
//! float) or FLAC, at any sample rate (high-quality resampling), optionally normalised to a peak or a
//! loudness target, as one mix or as stems (one file per mixer channel).

use crate::dsp::loudness::{measure, LoudnessStats};
use crate::model::*;
use crate::render::{offline_renderer, PlayMode, Pool};
use crate::samples::{resample, write_wav, AudioData, BitDepth, SampleCache};
use crate::{EngineError, Result};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ExportFormat {
    Wav(BitDepth),
    Flac16,
    Flac24,
    /// MP3 (LAME), constant bitrate in kbit/s.
    Mp3(u32),
    /// Ogg Vorbis, quality −0.1 … 1.0.
    Ogg(f32),
}

impl ExportFormat {
    pub fn extension(&self) -> &'static str {
        match self {
            ExportFormat::Wav(_) => "wav",
            ExportFormat::Mp3(_) => "mp3",
            ExportFormat::Ogg(_) => "ogg",
            _ => "flac",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ExportRange {
    Song,
    Loop,
    Range(Range),
    Pattern(Id),
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ExportOptions {
    pub format: ExportFormat,
    /// Output sample rate (the mix renders at `render_rate` and is resampled when they differ).
    pub rate: u32,
    pub render_rate: u32,
    pub range: ExportRange,
    /// Extra seconds rendered after the end for reverb/delay tails.
    pub tail: f64,
    /// Normalise the peak to this level (dBFS).
    pub normalize_peak: Option<f32>,
    /// Normalise integrated loudness to this target (LUFS); the peak is then limited to −1 dBTP by gain only.
    pub loudness_target: Option<f32>,
    pub dither: bool,
    pub mono: bool,
    /// One file per mixer channel (instead of the mix).
    pub stems: bool,
}

impl Default for ExportOptions {
    fn default() -> Self {
        ExportOptions { format: ExportFormat::Wav(BitDepth::Int24), rate: 48000, render_rate: 48000, range: ExportRange::Song, tail: 2.0, normalize_peak: None, loudness_target: None, dither: true, mono: false, stems: false }
    }
}

#[derive(Debug, Clone)]
pub struct ExportReport {
    pub files: Vec<PathBuf>,
    pub loudness: LoudnessStats,
    pub seconds: f64,
}

/// Renders the mix for the options' range (no file I/O). `progress` gets 0 … 1.
pub fn render_mix(p: &Project, cache: &SampleCache, opts: &ExportOptions, pool: &mut Pool, progress: &mut dyn FnMut(f32)) -> AudioData {
    let rate = opts.render_rate as f32;
    let mut r = offline_renderer(p, cache, rate, pool);
    let (start, end, mode) = match opts.range {
        ExportRange::Song => (0.0, p.song_length(), PlayMode::Song),
        ExportRange::Loop => p.loop_range.map(|l| (l.start, l.end, PlayMode::Song)).unwrap_or((0.0, p.song_length(), PlayMode::Song)),
        ExportRange::Range(rg) => (rg.start, rg.end, PlayMode::Song),
        ExportRange::Pattern(id) => (0.0, p.pattern(id).map(|x| x.length_beats()).unwrap_or(4.0), PlayMode::Pattern(id)),
    };
    let secs = r.song.tempo.beat_to_seconds(end) - r.song.tempo.beat_to_seconds(start);
    let latency = r.song.output_latency;
    let frames = ((secs + opts.tail.max(0.0)) * rate as f64) as usize + latency;
    r.seek(start);
    r.play(mode);
    // An export of the song never loops.
    if let Some(song) = std::sync::Arc::get_mut(&mut r.song) {
        song.loop_range = None;
    }
    let mut l = vec![0.0f32; frames];
    let mut rr = vec![0.0f32; frames];
    let block = 1024;
    let mut done = 0;
    while done < frames {
        let n = block.min(frames - done);
        // Keep playing past the song end (tails): re-arm the transport if it stopped itself.
        if !r.transport.playing {
            r.transport.playing = true;
        }
        r.render(&mut l[done..done + n], &mut rr[done..done + n], &[]);
        done += n;
        progress(done as f32 / frames as f32);
    }
    // Remove the plugin delay compensation latency so the bounce starts on time.
    l.drain(..latency.min(l.len()));
    rr.drain(..latency.min(rr.len()));
    AudioData::from_planar(opts.render_rate, l, rr)
}

fn apply_gain(d: &mut AudioData, gain: f32) {
    for x in d.l.iter_mut().chain(d.r.iter_mut()) {
        *x *= gain;
    }
}

/// Normalises a rendered mix according to the options; returns the measurement after processing.
pub fn finish(mut d: AudioData, opts: &ExportOptions) -> (AudioData, LoudnessStats) {
    if opts.rate != d.rate {
        d = resample(&d, opts.rate);
    }
    if let Some(target) = opts.loudness_target {
        let st = measure(&d.l, &d.r, d.rate as f32);
        if st.integrated > -70.0 {
            let mut gain = crate::dsp::db_to_gain(target - st.integrated);
            // Never push the true peak above −1 dBTP.
            let tp_after = st.true_peak_db + crate::dsp::gain_to_db(gain);
            if tp_after > -1.0 {
                gain *= crate::dsp::db_to_gain(-1.0 - tp_after);
            }
            apply_gain(&mut d, gain);
        }
    } else if let Some(peak_db) = opts.normalize_peak {
        let peak = d.peak();
        if peak > 1e-6 {
            apply_gain(&mut d, crate::dsp::db_to_gain(peak_db) / peak);
        }
    }
    let st = measure(&d.l, &d.r, d.rate as f32);
    (d, st)
}

/// Writes audio in the chosen format.
pub fn write(path: &Path, d: &AudioData, opts: &ExportOptions) -> Result<()> {
    match opts.format {
        ExportFormat::Wav(depth) => write_wav(path, &d.l, &d.r, d.rate, depth, opts.mono, opts.dither),
        ExportFormat::Flac16 => write_flac(path, d, 16, opts.mono, opts.dither),
        ExportFormat::Mp3(kbps) => write_mp3(path, d, kbps, opts.mono),
        ExportFormat::Ogg(q) => write_ogg(path, d, q, opts.mono),
        ExportFormat::Flac24 => write_flac(path, d, 24, opts.mono, opts.dither),
    }
}

/// Lossless FLAC (pure Rust encoder).
pub fn write_flac(path: &Path, d: &AudioData, bits: u32, mono: bool, dither: bool) -> Result<()> {
    use flacenc::component::BitRepr;
    use flacenc::error::Verify;
    let full = ((1i64 << (bits - 1)) - 1) as f64;
    let mut rng = crate::dsp::Rng::new(77);
    let mut q = |s: f32| -> i32 {
        let dt = if dither { (rng.next_f32() - rng.next_f32()) as f64 } else { 0.0 };
        (s as f64 * full + dt).round().clamp(-full - 1.0, full) as i32
    };
    let channels = if mono { 1 } else { 2 };
    let mut samples = Vec::with_capacity(d.frames() * channels);
    for i in 0..d.frames() {
        if mono {
            samples.push(q((d.l[i] + d.r[i]) * 0.5));
        } else {
            samples.push(q(d.l[i]));
            samples.push(q(d.r[i]));
        }
    }
    // Encode frame by frame so the last frame is shortened instead of zero-padded.
    use flacenc::source::{Context, FrameBuf, Seekable, Source};
    let fe = |e: &dyn std::fmt::Debug| EngineError::Io(format!("FLAC: {e:?}"));
    let config = flacenc::config::Encoder::default().into_verified().map_err(|e| fe(&e))?;
    let bs = config.block_size;
    let mut src = flacenc::source::MemSource::from_samples(&samples, channels, bits as usize, d.rate as usize);
    let total = src.len();
    let proto = flacenc::component::Stream::new(d.rate as usize, channels, bits as usize).map_err(|e| fe(&e))?;
    let mut frames = Vec::new();
    let mut fb = (FrameBuf::with_size(channels, bs).map_err(|e| fe(&e))?, Context::new(bits as usize, channels, bs));
    let mut pos = 0;
    while pos < total {
        let n = (total - pos).min(bs);
        if n < bs {
            fb.0.resize(n);
        }
        let read = src.read_samples(n, &mut fb).map_err(|e| fe(&e))?;
        if read == 0 {
            break;
        }
        frames.push(flacenc::encode_fixed_size_frame(&config, &fb.0, fb.1.current_frame_number().unwrap_or(0), proto.stream_info()).map_err(|e| fe(&e))?);
        pos += read;
    }
    let mut info = proto.stream_info().clone();
    info.set_md5_digest(&fb.1.md5_digest());
    info.set_total_samples(total);
    let mut stream = flacenc::component::Stream::with_stream_info(info);
    for f in frames {
        stream.add_frame(f);
    }
    let mut sink = flacenc::bitsink::ByteSink::new();
    stream.write(&mut sink).map_err(|e| fe(&e))?;
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    // A fixed-blocksize stream must announce min = max block size (the short last frame is allowed);
    // flacenc records the last frame's size as the minimum, which makes decoders assume variable blocks.
    let mut bytes = sink.as_slice().to_vec();
    if bytes.len() > 12 && &bytes[0..4] == b"fLaC" {
        bytes[8] = bytes[10];
        bytes[9] = bytes[11];
    }
    std::fs::write(path, bytes).map_err(|e| EngineError::Io(e.to_string()))
}

fn mono_mix(d: &AudioData) -> Vec<f32> {
    d.l.iter().zip(&d.r).map(|(a, b)| (a + b) * 0.5).collect()
}

/// MP3 via LAME (CBR). MP3 supports at most 48 kHz, so higher rates are resampled.
#[cfg(feature = "lossy")]
pub fn write_mp3(path: &Path, d: &AudioData, kbps: u32, mono: bool) -> Result<()> {
    use mp3lame_encoder::{Bitrate, Builder, DualPcm, FlushNoGap, MonoPcm, Quality};
    let resampled;
    let d = if d.rate > 48000 {
        resampled = resample(d, 48000);
        &resampled
    } else {
        d
    };
    let fe = |e: &dyn std::fmt::Display| EngineError::Io(format!("MP3: {e}"));
    let brate = match kbps {
        0..=96 => Bitrate::Kbps96,
        97..=128 => Bitrate::Kbps128,
        129..=160 => Bitrate::Kbps160,
        161..=192 => Bitrate::Kbps192,
        193..=256 => Bitrate::Kbps256,
        _ => Bitrate::Kbps320,
    };
    let mut b = Builder::new().ok_or_else(|| EngineError::Io("MP3: cannot create encoder".into()))?;
    b.set_num_channels(if mono { 1 } else { 2 }).map_err(|e| fe(&e))?;
    b.set_sample_rate(d.rate).map_err(|e| fe(&e))?;
    b.set_brate(brate).map_err(|e| fe(&e))?;
    b.set_quality(Quality::Best).map_err(|e| fe(&e))?;
    let mut enc = b.build().map_err(|e| fe(&e))?;
    let mut out = Vec::new();
    let mono_data = if mono { mono_mix(d) } else { Vec::new() };
    for start in (0..d.frames()).step_by(4096) {
        let end = (start + 4096).min(d.frames());
        out.reserve(mp3lame_encoder::max_required_buffer_size(end - start));
        if mono {
            enc.encode_to_vec(MonoPcm(&mono_data[start..end]), &mut out).map_err(|e| fe(&e))?;
        } else {
            enc.encode_to_vec(DualPcm { left: &d.l[start..end], right: &d.r[start..end] }, &mut out).map_err(|e| fe(&e))?;
        }
    }
    out.reserve(7200);
    enc.flush_to_vec::<FlushNoGap>(&mut out).map_err(|e| fe(&e))?;
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    std::fs::write(path, out).map_err(|e| EngineError::Io(e.to_string()))
}

/// Ogg Vorbis (aoTuV-tuned libvorbis), quality-based VBR.
#[cfg(feature = "lossy")]
pub fn write_ogg(path: &Path, d: &AudioData, quality: f32, mono: bool) -> Result<()> {
    use vorbis_rs::{VorbisBitrateManagementStrategy, VorbisEncoderBuilder};
    let fe = |e: &dyn std::fmt::Display| EngineError::Io(format!("Ogg: {e}"));
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let file = std::io::BufWriter::new(std::fs::File::create(path).map_err(|e| EngineError::Io(e.to_string()))?);
    let rate = std::num::NonZeroU32::new(d.rate).ok_or_else(|| EngineError::Io("Ogg: rate 0".into()))?;
    let channels = std::num::NonZeroU8::new(if mono { 1 } else { 2 }).unwrap();
    let mut builder = VorbisEncoderBuilder::new_with_serial(rate, channels, file, 0x0D4A_6011);
    builder.bitrate_management_strategy(VorbisBitrateManagementStrategy::QualityVbr { target_quality: quality.clamp(-0.1, 1.0) });
    let mut enc = builder.build().map_err(|e| fe(&e))?;
    let mono_data = if mono { mono_mix(d) } else { Vec::new() };
    for start in (0..d.frames()).step_by(4096) {
        let end = (start + 4096).min(d.frames());
        if mono {
            enc.encode_audio_block([&mono_data[start..end]]).map_err(|e| fe(&e))?;
        } else {
            enc.encode_audio_block([&d.l[start..end], &d.r[start..end]]).map_err(|e| fe(&e))?;
        }
    }
    enc.finish().map_err(|e| fe(&e))?;
    Ok(())
}

#[cfg(not(feature = "lossy"))]
pub fn write_mp3(_: &Path, _: &AudioData, _: u32, _: bool) -> Result<()> {
    Err(EngineError::Io("built without the `lossy` feature".into()))
}

#[cfg(not(feature = "lossy"))]
pub fn write_ogg(_: &Path, _: &AudioData, _: f32, _: bool) -> Result<()> {
    Err(EngineError::Io("built without the `lossy` feature".into()))
}

/// Exports the mix (or stems) to `path` (for stems: `path` is a folder or a file whose stem prefixes the
/// stem names).
pub fn export(p: &Project, cache: &SampleCache, path: &Path, opts: &ExportOptions, progress: &mut dyn FnMut(f32)) -> Result<ExportReport> {
    export_with(p, cache, path, opts, &mut Pool::new, progress)
}

/// [`export`] with a pool factory, so third-party plugins can be rendered
/// (e.g. `&mut || Pool::with_plugins(Box::new(plugins::Host))`).
pub fn export_with(p: &Project, cache: &SampleCache, path: &Path, opts: &ExportOptions, make_pool: &mut dyn FnMut() -> Pool, progress: &mut dyn FnMut(f32)) -> Result<ExportReport> {
    if !opts.stems {
        let mut pool = make_pool();
        let mix = render_mix(p, cache, opts, &mut pool, progress);
        let (d, st) = finish(mix, opts);
        write(path, &d, opts)?;
        return Ok(ExportReport { files: vec![path.to_path_buf()], loudness: st, seconds: d.duration() });
    }
    // Stems: solo each audio channel in turn (soloing keeps its sources and sends audible).
    let (dir, prefix) = if path.extension().is_some() { (path.parent().unwrap_or(Path::new(".")).to_path_buf(), path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()) } else { (path.to_path_buf(), String::new()) };
    std::fs::create_dir_all(&dir).map_err(|e| EngineError::Io(e.to_string()))?;
    let channels: Vec<(Id, String)> = p.mixer.channels.iter().filter(|c| c.kind == ChannelKind::Audio).map(|c| (c.id, c.name.clone())).collect();
    let mut files = Vec::new();
    let mut last = None;
    let count = channels.len().max(1);
    for (k, (id, name)) in channels.iter().enumerate() {
        let mut q = p.clone();
        for c in q.mixer.channels.iter_mut() {
            c.solo = c.id == *id;
        }
        let mut pool = make_pool();
        let mix = render_mix(&q, cache, &ExportOptions { normalize_peak: None, loudness_target: None, ..opts.clone() }, &mut pool, &mut |x| progress((k as f32 + x) / count as f32));
        if mix.peak() < 1e-5 {
            continue; // silent channel
        }
        let (d, st) = finish(mix, &ExportOptions { normalize_peak: None, loudness_target: None, ..opts.clone() });
        let safe: String = name.chars().map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' { c } else { '_' }).collect();
        let f = dir.join(if prefix.is_empty() { format!("{:02} {safe}.{}", k + 1, opts.format.extension()) } else { format!("{prefix} - {:02} {safe}.{}", k + 1, opts.format.extension()) });
        write(&f, &d, opts)?;
        files.push(f);
        last = Some((st, d.duration()));
    }
    let (st, secs) = last.unwrap_or((LoudnessStats { integrated: -70.0, true_peak_db: -180.0, sample_peak_db: -180.0 }, 0.0));
    Ok(ExportReport { files, loudness: st, seconds: secs })
}

/// Bounce in place: renders one track's mixer channel (soloed, pre-master effects included) to a file and
/// returns the audio (used for freeze / commit).
pub fn bounce_track(p: &Project, cache: &SampleCache, track: Id, path: &Path, rate: u32) -> Result<f64> {
    bounce_track_with(p, cache, track, path, rate, Pool::new())
}

/// [`bounce_track`] rendering with the given pool (plugins included).
pub fn bounce_track_with(p: &Project, cache: &SampleCache, track: Id, path: &Path, rate: u32, mut pool: Pool) -> Result<f64> {
    let t = p.track(track).ok_or_else(|| EngineError::Project("no such track".into()))?;
    let mut q = p.clone();
    for c in q.mixer.channels.iter_mut() {
        c.solo = c.id == t.mixer;
    }
    // Bypass the master chain so only the track's own processing is printed.
    q.mixer.master.inserts.clear();
    q.mixer.master.volume = 1.0;
    let start = t.clips.iter().map(|c| c.start).fold(f64::MAX, f64::min).max(0.0);
    let end = t.clips.iter().map(|c| c.end()).fold(0.0, f64::max);
    if end <= start {
        return Err(EngineError::Project("the track is empty".into()));
    }
    let opts = ExportOptions { format: ExportFormat::Wav(BitDepth::Float32), rate, render_rate: rate, range: ExportRange::Range(Range::new(start, end)), tail: 1.0, dither: false, ..Default::default() };
    let d = render_mix(&q, cache, &opts, &mut pool, &mut |_| {});
    write(path, &d, &opts)?;
    Ok(start)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project_with_tone(dir: &Path) -> Project {
        let f = dir.join("tone.wav");
        let s: Vec<f32> = (0..48000).map(|i| (i as f32 * 220.0 * std::f32::consts::TAU / 48000.0).sin() * 0.3).collect();
        write_wav(&f, &s, &s, 48000, BitDepth::Float32, false, false).unwrap();
        let mut p = Project::new_default();
        p.bpm = 120.0;
        let id = p.next_id();
        p.tracks[0].clips.push(Clip { id, name: "t".into(), start: 0.0, length: 2.0, muted: false, color: Rgb::default(), kind: ClipKind::Audio(AudioClip { source: f, ..Default::default() }) });
        p
    }

    #[test]
    fn export_wav_flac_and_loudness_target() {
        let dir = tempfile::tempdir().unwrap();
        let p = project_with_tone(dir.path());
        let cache = SampleCache::default();
        let out = dir.path().join("mix.wav");
        let opts = ExportOptions { tail: 0.0, loudness_target: Some(-14.0), ..Default::default() };
        let rep = export(&p, &cache, &out, &opts, &mut |_| {}).unwrap();
        assert!((rep.seconds - 2.0).abs() < 0.01, "{}", rep.seconds); // the song is at least one bar
        assert!((rep.loudness.integrated + 14.0).abs() < 0.5 || rep.loudness.true_peak_db <= -0.9, "{:?}", rep.loudness);
        let back = crate::samples::decode(&out).unwrap();
        assert_eq!(back.rate, 48000);
        let flac = dir.path().join("mix.flac");
        export(&p, &cache, &flac, &ExportOptions { format: ExportFormat::Flac24, rate: 44100, tail: 0.0, ..Default::default() }, &mut |_| {}).unwrap();
        let fb = crate::samples::decode(&flac).unwrap();
        assert_eq!(fb.rate, 44100);
        assert!((fb.frames() as i64 - 88200).abs() < 50, "{}", fb.frames());
        assert!((fb.peak() - 0.3 * 0.8).abs() < 0.03, "{}", fb.peak());
    }

    #[test]
    fn stems_one_file_per_channel_with_sound() {
        let dir = tempfile::tempdir().unwrap();
        let p = project_with_tone(dir.path());
        let cache = SampleCache::default();
        let rep = export(&p, &cache, &dir.path().join("stems"), &ExportOptions { stems: true, tail: 0.0, ..Default::default() }, &mut |_| {}).unwrap();
        assert_eq!(rep.files.len(), 1, "only the channel with audio gets a stem");
    }

    #[test]
    fn export_includes_patterns() {
        let dir = tempfile::tempdir().unwrap();
        let mut p = Project::new_default();
        let ch = p.add_channel("Synth", Instrument::Synth(SynthParams::default()));
        let pat = p.patterns[0].id;
        p.patterns[0].channels.insert(ch, PatternChannel { steps: vec![], notes: vec![Note { pitch: 57, start: 0, length: PPQ, ..Note::default() }] });
        let id = p.next_id();
        p.tracks[0].clips.push(Clip { id, name: "p".into(), start: 0.0, length: 4.0, muted: false, color: Rgb::default(), kind: ClipKind::Pattern { pattern: pat } });
        let cache = SampleCache::default();
        let rep = export(&p, &cache, &dir.path().join("p.wav"), &ExportOptions::default(), &mut |_| {}).unwrap();
        assert!(rep.loudness.sample_peak_db > -40.0, "{:?}", rep.loudness);
    }

    #[cfg(feature = "lossy")]
    #[test]
    fn export_mp3_and_ogg_decode_back() {
        let dir = tempfile::tempdir().unwrap();
        let p = project_with_tone(dir.path());
        let cache = SampleCache::default();
        for (fmt, name) in [(ExportFormat::Mp3(192), "mix.mp3"), (ExportFormat::Ogg(0.6), "mix.ogg")] {
            let out = dir.path().join(name);
            export(&p, &cache, &out, &ExportOptions { format: fmt, tail: 0.0, ..Default::default() }, &mut |_| {}).unwrap();
            let d = crate::samples::decode(&out).unwrap();
            assert!((d.duration() - 2.0).abs() < 0.1, "{name}: {} s", d.duration());
            // The 220 Hz tone survives the lossy codec at about the same level.
            let st = crate::dsp::loudness::measure(&d.l, &d.r, d.rate as f32);
            assert!((d.peak() - 0.24).abs() < 0.05, "{name}: peak {}", d.peak());
            assert!(st.integrated > -30.0, "{name}: {:?}", st);
        }
    }
}
