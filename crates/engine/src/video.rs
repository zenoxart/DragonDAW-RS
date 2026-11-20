//! Video for scoring and post-production, through the `ffmpeg` / `ffprobe` command-line tools (looked up
//! in the app's tools folder, next to the executable, in the usual install places and on `PATH`, or set
//! `DRAGON_FFMPEG` / `DRAGON_FFPROBE`; on Windows the app can download them). Video is decoded on demand, one frame at a time,
//! so nothing is kept in memory; audio is extracted to WAV and placed as an ordinary clip.

use crate::model::VideoRef;
use crate::{EngineError, Result};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Where Dragon DAW keeps tools it installed itself (set by the app: next to its settings).
static TOOLS_DIR: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

/// Sets the folder for tools the app installs (`install_ffmpeg` puts ffmpeg there).
pub fn set_tools_dir(dir: PathBuf) {
    if let Ok(mut d) = TOOLS_DIR.lock() {
        *d = Some(dir);
    }
}

fn tools_dir() -> Option<PathBuf> {
    TOOLS_DIR.lock().ok().and_then(|d| d.clone())
}

fn exe(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

/// The folders searched for ffmpeg / ffprobe before `PATH`: the app's own tools folder, next to the
/// executable, and the usual install places (winget, scoop, Chocolatey, C:\ffmpeg, Homebrew).
fn search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(t) = tools_dir() {
        dirs.push(t.join("ffmpeg"));
        dirs.push(t);
    }
    if let Some(d) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        dirs.push(d.join("ffmpeg").join("bin"));
        dirs.push(d.join("ffmpeg"));
        dirs.push(d);
    }
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    if cfg!(windows) {
        if let Some(l) = env("LOCALAPPDATA") {
            dirs.push(l.join("Microsoft").join("WinGet").join("Links"));
        }
        if let Some(u) = env("USERPROFILE") {
            dirs.push(u.join("scoop").join("shims"));
        }
        if let Some(p) = env("ProgramData") {
            dirs.push(p.join("chocolatey").join("bin"));
        }
        if let Some(p) = env("ProgramFiles") {
            dirs.push(p.join("ffmpeg").join("bin"));
        }
        dirs.push(PathBuf::from(r"C:\ffmpeg\bin"));
    } else {
        dirs.push(PathBuf::from("/opt/homebrew/bin"));
        dirs.push(PathBuf::from("/usr/local/bin"));
    }
    dirs
}

/// The program `name`: `var` when set, else the first copy in `search_dirs`, else `name` itself (PATH).
fn tool(var: &str, name: &str) -> PathBuf {
    if let Some(p) = std::env::var_os(var) {
        return PathBuf::from(p);
    }
    search_dirs().into_iter().map(|d| d.join(exe(name))).find(|p| p.is_file()).unwrap_or_else(|| PathBuf::from(name))
}

/// Every place a program could be, best first: `var`, the search folders, then the bare name (PATH).
fn candidates(var: &str, name: &str) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::env::var_os(var).map(PathBuf::from).into_iter().collect();
    v.extend(search_dirs().into_iter().map(|d| d.join(exe(name))).filter(|p| p.is_file()));
    v.push(PathBuf::from(name));
    v.dedup();
    v
}

/// Runs `p -version`; the error says why it could not be used.
fn runs(p: &Path) -> std::result::Result<(), String> {
    let mut c = Command::new(p);
    no_window(&mut c);
    match c.arg("-version").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()).output() {
        Ok(o) if o.status.success() => Ok(()),
        Ok(o) => Err(format!("exits with {} {}", o.status, String::from_utf8_lossy(&o.stderr).lines().last().unwrap_or("").trim())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err("not found".into()),
        Err(e) => Err(e.to_string()),
    }
}

/// The working ffmpeg and ffprobe, found once (a copy that doesn't run is skipped, so a broken
/// download can't hide the copy shipped with the app). [`recheck`] looks again.
static FOUND: std::sync::Mutex<Option<std::result::Result<(PathBuf, PathBuf), String>>> = std::sync::Mutex::new(None);

fn first_working(var: &str, name: &str) -> std::result::Result<PathBuf, String> {
    let mut tried = Vec::new();
    for p in candidates(var, name) {
        match runs(&p) {
            Ok(()) => return Ok(p),
            Err(e) => tried.push(format!("{} ({e})", p.display())),
        }
    }
    Err(format!("no working {name}: tried {}", tried.join("; ")))
}

/// Where ffmpeg and ffprobe are, or why they can't be used.
pub fn check() -> std::result::Result<(PathBuf, PathBuf), String> {
    let mut f = FOUND.lock().unwrap_or_else(|e| e.into_inner());
    f.get_or_insert_with(|| Ok((first_working("DRAGON_FFMPEG", "ffmpeg")?, first_working("DRAGON_FFPROBE", "ffprobe")?))).clone()
}

/// Forgets what [`check`] found (after installing ffmpeg).
pub fn recheck() {
    *FOUND.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

fn ffmpeg() -> PathBuf {
    check().map(|p| p.0).unwrap_or_else(|_| tool("DRAGON_FFMPEG", "ffmpeg"))
}

fn ffprobe() -> PathBuf {
    check().map(|p| p.1).unwrap_or_else(|_| tool("DRAGON_FFPROBE", "ffprobe"))
}

/// `true` when ffmpeg and ffprobe can be run.
pub fn available() -> bool {
    check().is_ok()
}

/// No console window flashing up for each call on Windows.
fn no_window(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    #[cfg(not(windows))]
    let _ = cmd;
}

/// The LGPL ffmpeg builds (BtbN's official automated builds) for Windows and Linux.
pub const FFMPEG_WIN_URL: &str = "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-lgpl.zip";
pub const FFMPEG_LINUX_URL: &str = "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-linux64-lgpl.tar.xz";

/// Whether [`install_ffmpeg`] can fetch ffmpeg on this system (Windows and x86-64 Linux).
pub fn can_install() -> bool {
    cfg!(windows) || (cfg!(target_os = "linux") && cfg!(target_arch = "x86_64"))
}

/// The download link [`install_ffmpeg`] uses here.
pub fn install_url() -> &'static str {
    if cfg!(windows) {
        FFMPEG_WIN_URL
    } else {
        FFMPEG_LINUX_URL
    }
}

/// Downloads ffmpeg and ffprobe into the tools folder (with curl and tar, which Windows 10+ ships too).
/// On macOS it says how to install them with Homebrew.
pub fn install_ffmpeg() -> Result<PathBuf> {
    let io = |e: std::io::Error| EngineError::Io(e.to_string());
    let dir = tools_dir().ok_or_else(|| EngineError::Io("no tools folder set".into()))?.join("ffmpeg");
    if !can_install() {
        let how = if cfg!(target_os = "macos") { "brew install ffmpeg" } else { "your package manager (e.g. sudo apt install ffmpeg)" };
        return Err(EngineError::Io(format!("install ffmpeg with {how}")));
    }
    let tmp = std::env::temp_dir().join(format!("dragon-ffmpeg-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(io)?;
    let archive = tmp.join(if cfg!(windows) { "ffmpeg.zip" } else { "ffmpeg.tar.xz" });
    let result = (|| {
        run(Command::new(if cfg!(windows) { "curl.exe" } else { "curl" }).args(["-fL", "--retry", "3", "-o"]).arg(&archive).arg(install_url()))?;
        // tar unpacks zip files too on Windows (bsdtar); fall back to PowerShell if it is missing.
        let tar = run(Command::new(if cfg!(windows) { "tar.exe" } else { "tar" }).arg("-xf").arg(&archive).arg("-C").arg(&tmp));
        if tar.is_err() && cfg!(windows) {
            let ps = |p: &Path| p.to_string_lossy().replace('\'', "''");
            run(Command::new("powershell").args(["-NoProfile", "-NonInteractive", "-Command"]).arg(format!("Expand-Archive -Force -Path '{}' -DestinationPath '{}'", ps(&archive), ps(&tmp))))?;
        } else {
            tar?;
        }
        let bin = find_file(&tmp, &exe("ffmpeg")).and_then(|p| p.parent().map(Path::to_path_buf)).ok_or_else(|| EngineError::Io("the download did not contain ffmpeg".into()))?;
        std::fs::create_dir_all(&dir).map_err(io)?;
        for name in ["ffmpeg", "ffprobe"] {
            let to = dir.join(exe(name));
            let _ = std::fs::remove_file(&to);
            std::fs::copy(bin.join(exe(name)), &to).map_err(io)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&to, std::fs::Permissions::from_mode(0o755)).map_err(io)?;
            }
        }
        // Shared builds keep their DLLs next to the programs.
        for e in std::fs::read_dir(&bin).map_err(io)?.flatten() {
            if e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("dll")) {
                let _ = std::fs::copy(e.path(), dir.join(e.file_name()));
            }
        }
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    result?;
    recheck();
    let (a, b) = (dir.join(exe("ffmpeg")), dir.join(exe("ffprobe")));
    runs(&a).and_then(|_| runs(&b)).map_err(|e| EngineError::Io(format!("ffmpeg was downloaded to {} but does not run: {e}", dir.display())))?;
    Ok(dir)
}

fn find_file(root: &Path, name: &str) -> Option<PathBuf> {
    for e in std::fs::read_dir(root).ok()?.flatten() {
        let p = e.path();
        if p.is_dir() {
            if let Some(f) = find_file(&p, name) {
                return Some(f);
            }
        } else if p.file_name().is_some_and(|n| n.eq_ignore_ascii_case(name)) {
            return Some(p);
        }
    }
    None
}

fn run(cmd: &mut Command) -> Result<Vec<u8>> {
    no_window(cmd);
    let out = cmd.stdin(Stdio::null()).output().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            EngineError::Io(format!("{} was not found – video needs ffmpeg (Video › Get ffmpeg, or install it)", cmd.get_program().to_string_lossy()))
        } else {
            EngineError::Io(format!("cannot run {:?}: {e}", cmd.get_program()))
        }
    })?;
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

    #[test]
    fn tools_are_found_in_the_tools_folder_before_path() {
        let dir = tempfile::tempdir().unwrap();
        let name = "dragon-fake-tool";
        std::fs::create_dir_all(dir.path().join("ffmpeg")).unwrap();
        let file = dir.path().join("ffmpeg").join(exe(name));
        std::fs::write(&file, b"").unwrap();
        // Not there yet: the bare name (PATH).
        assert_eq!(tool("DRAGON_TEST_UNSET_VAR", "dragon-other-tool"), PathBuf::from("dragon-other-tool"));
        set_tools_dir(dir.path().to_path_buf());
        assert_eq!(tool("DRAGON_TEST_UNSET_VAR", name), file);
        // An explicit variable wins.
        std::env::set_var("DRAGON_TEST_SET_VAR", "/x/y");
        assert_eq!(tool("DRAGON_TEST_SET_VAR", name), PathBuf::from("/x/y"));
    }

    #[cfg(unix)]
    #[test]
    fn a_copy_that_does_not_run_is_skipped() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let bad = dir.path().join("dragon-tool-bad");
        std::fs::write(&bad, b"#!/bin/sh\nexit 3\n").unwrap();
        std::fs::set_permissions(&bad, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(runs(&bad).unwrap_err().contains("exit"));
        let good = dir.path().join("dragon-tool-good");
        std::fs::write(&good, b"#!/bin/sh\necho ok\n").unwrap();
        std::fs::set_permissions(&good, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(runs(&good).is_ok());
        assert_eq!(runs(Path::new("dragon-no-such-program-anywhere")).unwrap_err(), "not found");
    }

    #[test]
    fn a_missing_tool_says_what_to_do() {
        let err = run(&mut Command::new("dragon-no-such-program-anywhere")).unwrap_err().to_string();
        assert!(err.contains("not found") && err.contains("ffmpeg"), "{err}");
    }

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
