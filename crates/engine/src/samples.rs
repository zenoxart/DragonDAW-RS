//! Audio files: decoding (WAV, FLAC, MP3, OGG Vorbis, AIFF, M4A/AAC), resampling to the engine rate,
//! a shared cache and waveform peaks for drawing.

use crate::{EngineError, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Decoded stereo audio (mono files are duplicated to both sides).
#[derive(Debug, Clone, Default)]
pub struct AudioData {
    pub rate: u32,
    pub l: Vec<f32>,
    pub r: Vec<f32>,
    /// Channels in the source file.
    pub source_channels: u16,
    pub source_rate: u32,
}

impl AudioData {
    pub fn frames(&self) -> usize {
        self.l.len()
    }
    pub fn duration(&self) -> f64 {
        self.frames() as f64 / self.rate.max(1) as f64
    }
    pub fn from_planar(rate: u32, l: Vec<f32>, r: Vec<f32>) -> Self {
        AudioData { rate, source_channels: 2, source_rate: rate, l, r }
    }

    /// Linear-interpolated stereo sample at a fractional frame (0 outside).
    #[inline]
    pub fn sample_at(&self, pos: f64) -> (f32, f32) {
        if pos < 0.0 {
            return (0.0, 0.0);
        }
        let i = pos as usize;
        let n = self.l.len();
        if i + 1 >= n {
            return if i < n { (self.l[i], self.r[i]) } else { (0.0, 0.0) };
        }
        let f = (pos - i as f64) as f32;
        (self.l[i] + (self.l[i + 1] - self.l[i]) * f, self.r[i] + (self.r[i + 1] - self.r[i]) * f)
    }

    /// Peak level (max |sample|).
    pub fn peak(&self) -> f32 {
        self.l.iter().chain(self.r.iter()).fold(0.0f32, |m, s| m.max(s.abs()))
    }

    /// Min/max pairs for `buckets` equal slices of `[start, end)` frames (for waveform drawing).
    pub fn peaks(&self, start: usize, end: usize, buckets: usize) -> Vec<(f32, f32)> {
        let end = end.min(self.frames());
        if buckets == 0 || start >= end {
            return Vec::new();
        }
        let span = (end - start) as f64 / buckets as f64;
        (0..buckets)
            .map(|b| {
                let a = start + (b as f64 * span) as usize;
                let z = (start + ((b + 1) as f64 * span) as usize).min(end).max(a + 1).min(end);
                let mut lo = 0.0f32;
                let mut hi = 0.0f32;
                // Stride through long buckets so drawing stays cheap.
                let step = ((z - a) / 64).max(1);
                let mut i = a;
                while i < z {
                    let s = (self.l[i] + self.r[i]) * 0.5;
                    lo = lo.min(s);
                    hi = hi.max(s);
                    i += step;
                }
                (lo, hi)
            })
            .collect()
    }
}

/// Supported file extensions for import.
pub const EXTENSIONS: &[&str] = &["wav", "flac", "mp3", "ogg", "aif", "aiff", "m4a", "aac", "wave"];

pub fn is_audio_file(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

/// Decodes a file at its own sample rate.
pub fn decode(path: &Path) -> Result<AudioData> {
    use symphonia::core::audio::SampleBuffer;
    use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
    use symphonia::core::errors::Error as SErr;
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;

    let file = std::fs::File::open(path).map_err(|e| EngineError::Io(format!("{}: {e}", path.display())))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe().format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default()).map_err(|e| EngineError::Decode(format!("{}: {e}", path.display())))?;
    let mut format = probed.format;
    let track = format.tracks().iter().find(|t| t.codec_params.codec != CODEC_TYPE_NULL).ok_or_else(|| EngineError::Decode("no audio track".into()))?;
    let track_id = track.id;
    let rate = track.codec_params.sample_rate.unwrap_or(44100);
    let mut decoder = symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default()).map_err(|e| EngineError::Decode(e.to_string()))?;
    let mut l = Vec::new();
    let mut r = Vec::new();
    let mut channels = 1u16;
    let mut buf: Option<SampleBuffer<f32>> = None;
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SErr::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(SErr::ResetRequired) => break,
            Err(e) => return Err(EngineError::Decode(e.to_string())),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(SErr::DecodeError(_)) => continue,
            Err(e) => return Err(EngineError::Decode(e.to_string())),
        };
        let spec = *decoded.spec();
        channels = spec.channels.count() as u16;
        let need = decoded.capacity() as u64;
        if buf.as_ref().map(|b| (b.capacity() as u64) < need * channels as u64).unwrap_or(true) {
            buf = Some(SampleBuffer::<f32>::new(need, spec));
        }
        let b = buf.as_mut().unwrap();
        b.copy_interleaved_ref(decoded);
        let s = b.samples();
        let ch = channels as usize;
        for frame in s.chunks_exact(ch.max(1)) {
            let a = frame[0];
            let z = if ch > 1 { frame[1] } else { a };
            l.push(a);
            r.push(z);
        }
    }
    Ok(AudioData { rate, l, r, source_channels: channels, source_rate: rate })
}

/// High-quality resampling (sinc) to `rate`.
pub fn resample(data: &AudioData, rate: u32) -> AudioData {
    if data.rate == rate || data.frames() == 0 {
        let mut d = data.clone();
        d.rate = rate.max(d.rate);
        if data.rate == rate {
            return d;
        }
    }
    use rubato::{Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction};
    let ratio = rate as f64 / data.rate as f64;
    let params = SincInterpolationParameters { sinc_len: 128, f_cutoff: 0.95, interpolation: SincInterpolationType::Linear, oversampling_factor: 128, window: WindowFunction::BlackmanHarris2 };
    let chunk = 4096;
    let mut rs = match SincFixedIn::<f32>::new(ratio, 1.0, params, chunk, 2) {
        Ok(r) => r,
        Err(_) => return linear_resample(data, rate),
    };
    let n = data.frames();
    let mut out_l = Vec::with_capacity((n as f64 * ratio) as usize + chunk);
    let mut out_r = Vec::with_capacity(out_l.capacity());
    let mut pos = 0;
    while pos < n {
        let end = (pos + chunk).min(n);
        let mut a = data.l[pos..end].to_vec();
        let mut b = data.r[pos..end].to_vec();
        a.resize(chunk, 0.0);
        b.resize(chunk, 0.0);
        match rs.process(&[a, b], None) {
            Ok(o) => {
                out_l.extend_from_slice(&o[0]);
                out_r.extend_from_slice(&o[1]);
            }
            Err(_) => return linear_resample(data, rate),
        }
        pos = end;
    }
    // Remove the filter delay and the padding of the last chunk.
    let delay = rs.output_delay();
    let want = (n as f64 * ratio).round() as usize;
    let take = |v: Vec<f32>| -> Vec<f32> { v.into_iter().skip(delay).take(want).collect() };
    AudioData { rate, l: take(out_l), r: take(out_r), source_channels: data.source_channels, source_rate: data.source_rate }
}

fn linear_resample(data: &AudioData, rate: u32) -> AudioData {
    let ratio = data.rate as f64 / rate as f64;
    let n = (data.frames() as f64 / ratio) as usize;
    let mut l = Vec::with_capacity(n);
    let mut r = Vec::with_capacity(n);
    for i in 0..n {
        let (a, b) = data.sample_at(i as f64 * ratio);
        l.push(a);
        r.push(b);
    }
    AudioData { rate, l, r, source_channels: data.source_channels, source_rate: data.source_rate }
}

/// Shared cache of decoded files at the engine rate. Cheap to clone.
#[derive(Clone, Default)]
pub struct SampleCache {
    inner: Arc<Mutex<HashMap<(PathBuf, u32), Arc<AudioData>>>>,
    /// Base folder for relative paths (the project folder).
    base: Arc<Mutex<Option<PathBuf>>>,
}

impl SampleCache {
    pub fn set_base(&self, base: Option<PathBuf>) {
        *self.base.lock().unwrap() = base;
    }

    pub fn resolve(&self, path: &Path) -> PathBuf {
        if path.is_absolute() {
            return path.to_path_buf();
        }
        match &*self.base.lock().unwrap() {
            Some(b) => b.join(path),
            None => path.to_path_buf(),
        }
    }

    /// Decoded audio at `rate`, loading it on first use.
    pub fn get(&self, path: &Path, rate: u32) -> Result<Arc<AudioData>> {
        let full = self.resolve(path);
        let key = (full.clone(), rate);
        if let Some(d) = self.inner.lock().unwrap().get(&key) {
            return Ok(d.clone());
        }
        let raw = decode(&full)?;
        let d = Arc::new(if raw.rate == rate { raw } else { resample(&raw, rate) });
        self.inner.lock().unwrap().insert(key, d.clone());
        Ok(d)
    }

    /// Already-loaded audio (never blocks on disk).
    pub fn peek(&self, path: &Path, rate: u32) -> Option<Arc<AudioData>> {
        let full = self.resolve(path);
        self.inner.lock().unwrap().get(&(full, rate)).cloned()
    }

    /// Registers audio that does not come from a file on disk yet (e.g. a recording in progress).
    pub fn insert(&self, path: &Path, data: Arc<AudioData>) {
        let full = self.resolve(path);
        self.inner.lock().unwrap().insert((full, data.rate), data);
    }

    pub fn forget(&self, path: &Path) {
        let full = self.resolve(path);
        self.inner.lock().unwrap().retain(|(p, _), _| *p != full);
    }

    pub fn clear(&self) {
        self.inner.lock().unwrap().clear();
    }
}

/// Bit depth of a written WAV file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum BitDepth {
    Int16,
    Int24,
    Float32,
}

/// Writes stereo (or mono when `mono`) audio to a WAV file. Integer formats get TPDF dither when `dither`.
pub fn write_wav(path: &Path, l: &[f32], r: &[f32], rate: u32, depth: BitDepth, mono: bool, dither: bool) -> Result<()> {
    let channels = if mono { 1 } else { 2 };
    let spec = match depth {
        BitDepth::Int16 => hound::WavSpec { channels, sample_rate: rate, bits_per_sample: 16, sample_format: hound::SampleFormat::Int },
        BitDepth::Int24 => hound::WavSpec { channels, sample_rate: rate, bits_per_sample: 24, sample_format: hound::SampleFormat::Int },
        BitDepth::Float32 => hound::WavSpec { channels, sample_rate: rate, bits_per_sample: 32, sample_format: hound::SampleFormat::Float },
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let mut w = hound::WavWriter::create(path, spec).map_err(|e| EngineError::Io(e.to_string()))?;
    let mut rng = crate::dsp::Rng::new(0x5EED);
    let full = match depth {
        BitDepth::Int16 => 32767.0,
        BitDepth::Int24 => 8_388_607.0,
        BitDepth::Float32 => 1.0,
    };
    let mut put = |w: &mut hound::WavWriter<std::io::BufWriter<std::fs::File>>, s: f32| -> Result<()> {
        match depth {
            BitDepth::Float32 => w.write_sample(s).map_err(|e| EngineError::Io(e.to_string())),
            _ => {
                let d = if dither { (rng.next_f32() - rng.next_f32()) as f64 } else { 0.0 };
                let v = (s as f64 * full + d).round().clamp(-full - 1.0, full) as i32;
                w.write_sample(v).map_err(|e| EngineError::Io(e.to_string()))
            }
        }
    };
    for i in 0..l.len() {
        if mono {
            put(&mut w, (l[i] + r[i]) * 0.5)?;
        } else {
            put(&mut w, l[i])?;
            put(&mut w, r[i])?;
        }
    }
    w.finalize().map_err(|e| EngineError::Io(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_roundtrip_and_resample() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("sine.wav");
        let n = 44100;
        let l: Vec<f32> = (0..n).map(|i| (i as f32 * 440.0 * std::f32::consts::TAU / 44100.0).sin() * 0.5).collect();
        write_wav(&p, &l, &l, 44100, BitDepth::Int24, false, false).unwrap();
        let d = decode(&p).unwrap();
        assert_eq!(d.frames(), n);
        assert!((d.peak() - 0.5).abs() < 0.01);
        let cache = SampleCache::default();
        let r = cache.get(&p, 48000).unwrap();
        assert!((r.frames() as i64 - 48000).abs() < 4, "{}", r.frames());
        assert!((r.peak() - 0.5).abs() < 0.02);
        assert!(cache.peek(&p, 48000).is_some());
        let peaks = r.peaks(0, r.frames(), 10);
        assert_eq!(peaks.len(), 10);
    }
}
