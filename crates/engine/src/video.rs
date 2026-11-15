//! Video for scoring and post-production, through the `ffmpeg` / `ffprobe` command-line tools (looked up
//! on `PATH`, or set `DRAGON_FFMPEG` / `DRAGON_FFPROBE`). Video is decoded on demand, one frame at a time,
//! so nothing is kept in memory; audio is extracted to WAV and placed as an ordinary clip.

use crate::model::VideoRef;
use crate::{EngineError, Result};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn tool(var: &str, name: &str) -> PathBuf {
    std::env::var_os(var).map(PathBuf::from).unwrap_or_else(|| PathBuf::from(name))
}

fn ffmpeg() -> PathBuf {
    tool("DRAGON_FFMPEG", "ffmpeg")
}

fn ffprobe() -> PathBuf {
    tool("DRAGON_FFPROBE", "ffprobe")
}

/// `true` when ffmpeg and ffprobe can be run.
pub fn available() -> bool {
    let ok = |p: PathBuf| Command::new(p).arg("-version").stdout(Stdio::null()).stderr(Stdio::null()).status().map(|s| s.success()).unwrap_or(false);
    ok(ffmpeg()) && ok(ffprobe())
}

fn run(cmd: &mut Command) -> Result<Vec<u8>> {
    let out = cmd.stdin(Stdio::null()).output().map_err(|e| EngineError::Io(format!("cannot run {:?}: {e} (install ffmpeg)", cmd.get_program())))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(EngineError::Io(format!("{:?} failed: {}", cmd.get_program(), err.lines().last().unwrap_or(""))));
    }
    Ok(out.stdout)
}

/// Reads duration, frame rate and size of a video file.
pub fn probe(path: &Path) -> Result<VideoRef> {
    let out = run(Command::new(ffprobe()).args(["-v", "error", "-select_streams", "v:0", "-show_entries", "stream=width,height,r_frame_rate:format=duration", "-of", "default=noprint_wrappers=1"]).arg(path))?;
    let text = String::from_utf8_lossy(&out);
    let mut v = VideoRef { path: path.to_path_buf(), ..Default::default() };
    for line in text.lines() {
        let Some((k, val)) = line.split_once('=') else { continue };
        match k {
            "width" => v.width = val.parse().unwrap_or(0),
            "height" => v.height = val.parse().unwrap_or(0),
            "duration" => v.duration = val.parse().unwrap_or(0.0),
            "r_frame_rate" => {
                if let Some((a, b)) = val.split_once('/') {
                    let (a, b): (f64, f64) = (a.parse().unwrap_or(25.0), b.parse().unwrap_or(1.0));
                    if b > 0.0 {
                        v.fps = a / b;
                    }
                }
            }
            _ => {}
        }
    }
    if v.width == 0 || v.height == 0 {
        return Err(EngineError::Io(format!("{} has no video stream", path.display())));
    }
    Ok(v)
}

/// Extracts the soundtrack to a 48 kHz stereo WAV. Returns `false` when the file has no audio.
pub fn extract_audio(path: &Path, out: &Path) -> Result<bool> {
    if let Some(d) = out.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let has_audio = !run(Command::new(ffprobe()).args(["-v", "error", "-select_streams", "a", "-show_entries", "stream=index", "-of", "csv=p=0"]).arg(path))?.is_empty();
    if !has_audio {
        return Ok(false);
    }
    run(Command::new(ffmpeg()).args(["-v", "error", "-y", "-i"]).arg(path).args(["-vn", "-ac", "2", "-ar", "48000", "-c:a", "pcm_s24le"]).arg(out))?;
    Ok(true)
}

/// One RGBA frame at `seconds`, scaled to `width` pixels wide. Returns `(width, height, rgba)`.
pub fn frame_at(path: &Path, seconds: f64, width: u32, src_w: u32, src_h: u32) -> Result<(u32, u32, Vec<u8>)> {
    let width = width.max(16) & !1;
    let height = (((src_h as f64 / src_w.max(1) as f64) * width as f64).round() as u32).max(2) & !1;
    let rgba = run(Command::new(ffmpeg()).args(["-v", "error", "-ss", &format!("{:.3}", seconds.max(0.0)), "-i"]).arg(path).args([
        "-frames:v",
        "1",
        "-vf",
        &format!("scale={width}:{height}"),
        "-f",
        "rawvideo",
        "-pix_fmt",
        "rgba",
        "-",
    ]))?;
    if rgba.len() != (width * height * 4) as usize {
        return Err(EngineError::Io("no frame at this position".into()));
    }
    Ok((width, height, rgba))
}

/// Writes a copy of the video with its soundtrack replaced by `mix` (the picture is copied, not re-encoded).
pub fn mux(video: &VideoRef, mix: &Path, out: &Path) -> Result<()> {
    if let Some(d) = out.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let audio_codec = match out.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref() {
        Some("mov") => "pcm_s24le",
        Some("mkv") => "flac",
        _ => "aac",
    };
    run(Command::new(ffmpeg())
        .args(["-v", "error", "-y", "-ss", &format!("{:.3}", video.offset)])
        .arg("-i")
        .arg(&video.path)
        .arg("-i")
        .arg(mix)
        .args(["-map", "0:v:0", "-map", "1:a:0", "-c:v", "copy", "-c:a", audio_codec, "-b:a", "320k", "-shortest"])
        .arg(out))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_frames_audio_and_mux() {
        if !available() {
            eprintln!("skipped: ffmpeg not installed");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("clip.mp4");
        // 2 s test pattern at 25 fps with a 440 Hz tone.
        run(Command::new(ffmpeg())
            .args(["-v", "error", "-y", "-f", "lavfi", "-i", "testsrc=size=160x120:rate=25:duration=2", "-f", "lavfi", "-i", "sine=frequency=440:duration=2", "-c:v", "mpeg4", "-c:a", "aac", "-shortest"])
            .arg(&src))
        .unwrap();
        let v = probe(&src).unwrap();
        assert_eq!((v.width, v.height), (160, 120));
        assert!((v.fps - 25.0).abs() < 0.01 && (v.duration - 2.0).abs() < 0.1, "{v:?}");
        let (w, h, rgba) = frame_at(&src, 1.0, 80, v.width, v.height).unwrap();
        assert_eq!((w, h), (80, 60));
        assert_eq!(rgba.len(), 80 * 60 * 4);
        assert!(rgba.chunks(4).any(|p| p[0] > 200), "the test pattern has bright pixels");
        let wav = dir.path().join("a.wav");
        assert!(extract_audio(&src, &wav).unwrap());
        let a = crate::samples::decode(&wav).unwrap();
        assert!(a.duration() > 1.8 && a.peak() > 0.05, "{} s, peak {}", a.duration(), a.peak());
        let out = dir.path().join("out.mp4");
        mux(&v, &wav, &out).unwrap();
        assert!(probe(&out).is_ok());
    }
}
