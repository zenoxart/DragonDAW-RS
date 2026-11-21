//! Update check: asks GitHub for the newest Dragon DAW release, tells the user when there is one, and
//! downloads it (on Windows also installs it and restarts).
//!
//! A build knows its version, its commit and its channel (set by the release workflow, see build.rs):
//! - **stable** (a `v*` tag) is offered newer versioned releases;
//! - **nightly** (the automatic build of `main`) is offered the newest nightly when its commit differs,
//!   and a versioned release when that is newer than its version;
//! - **dev** (built locally) only checks when asked (Help › Check for updates).
//!
//! The requests go through `curl` (built into Windows 10 and later, macOS and Linux), so the app needs
//! no HTTP library.

use crate::state::State;
use crate::theme;
use egui::{Color32, RichText};
use redhive_contracts::ui::Severity;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

const REPO: &str = "zenoxart/DragonDAW-RS";

/// What this build is.
#[derive(Clone, Debug, PartialEq)]
pub struct Build {
    pub version: String,
    /// The commit it was built from (may be empty).
    pub sha: String,
    /// "stable", "nightly" or "dev".
    pub channel: String,
}

impl Build {
    pub fn current() -> Self {
        Build { version: env!("CARGO_PKG_VERSION").into(), sha: env!("DRAGON_BUILD_SHA").into(), channel: env!("DRAGON_BUILD_CHANNEL").into() }
    }

    /// How the build is shown ("2.0.0", "nightly 1a2b3c4").
    pub fn label(&self) -> String {
        match self.channel.as_str() {
            "stable" => self.version.clone(),
            c => format!("{} {} ({})", self.version, short(&self.sha), c),
        }
    }
}

fn short(sha: &str) -> &str {
    &sha[..sha.len().min(7)]
}

/// A release that is newer than this build.
#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    /// Identifies it (tag, or "nightly-<sha>") – also what "Skip this version" remembers.
    pub id: String,
    /// How it is shown ("2.1.0", "nightly 1a2b3c4").
    pub label: String,
    /// The release page (notes).
    pub page: String,
    pub asset_name: String,
    pub asset_url: String,
    pub size: u64,
}

/// The archive name part for this platform (as the release workflow names them).
pub fn platform() -> &'static str {
    if cfg!(windows) {
        "windows-x86_64"
    } else if cfg!(target_os = "macos") {
        if cfg!(target_arch = "aarch64") {
            "macos-arm64"
        } else {
            "macos-x86_64"
        }
    } else {
        "linux-x86_64"
    }
}

/// "2.10.1" → [2, 10, 1] (a "-beta" suffix and a leading "v" are ignored).
fn version_key(v: &str) -> Vec<u64> {
    v.trim_start_matches('v').split(['-', '+']).next().unwrap_or("").split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

fn asset(release: &Value, platform: &str) -> Option<(String, String, u64)> {
    release["assets"].as_array()?.iter().find_map(|a| {
        let name = a["name"].as_str()?;
        (name.contains(platform) && (name.ends_with(".zip") || name.ends_with(".tar.gz"))).then(|| (name.to_string(), a["browser_download_url"].as_str().unwrap_or_default().to_string(), a["size"].as_u64().unwrap_or(0)))
    })
}

/// Picks the release to offer from GitHub's release list (newest first), or `None` when this build is
/// up to date.
pub fn pick(releases: &Value, build: &Build, platform: &str) -> Option<Release> {
    let list = releases.as_array()?;
    let mine = version_key(&build.version);
    // The newest versioned release with a build for this platform.
    let stable = list
        .iter()
        .filter(|r| !r["draft"].as_bool().unwrap_or(false))
        .filter(|r| r["tag_name"].as_str().is_some_and(|t| t.starts_with('v')))
        .filter(|r| build.channel != "stable" || !r["prerelease"].as_bool().unwrap_or(false))
        .filter_map(|r| asset(r, platform).map(|a| (r, a)))
        .max_by(|a, b| version_key(a.0["tag_name"].as_str().unwrap_or("")).cmp(&version_key(b.0["tag_name"].as_str().unwrap_or(""))));
    if let Some((r, (name, url, size))) = stable {
        let tag = r["tag_name"].as_str().unwrap_or("");
        if version_key(tag) > mine {
            return Some(Release { id: tag.into(), label: tag.trim_start_matches('v').into(), page: r["html_url"].as_str().unwrap_or("").into(), asset_name: name, asset_url: url, size });
        }
    }
    if build.channel == "stable" {
        return None;
    }
    // Nightly: a different commit is a newer build (the nightly release is replaced on every green main).
    let nightly = list.iter().find(|r| r["tag_name"].as_str() == Some("nightly"))?;
    let sha = nightly["target_commitish"].as_str().unwrap_or("");
    let (name, url, size) = asset(nightly, platform)?;
    // The commit is also in the archive name ("dragon-daw-nightly-1a2b3c4-…").
    let sha = if sha.len() >= 7 { sha.to_string() } else { name.split("nightly-").nth(1).and_then(|s| s.split('-').next()).unwrap_or("").to_string() };
    if sha.is_empty() || (!build.sha.is_empty() && (sha.starts_with(&build.sha) || build.sha.starts_with(&sha))) {
        return None;
    }
    Some(Release { id: format!("nightly-{}", short(&sha)), label: format!("nightly {}", short(&sha)), page: nightly["html_url"].as_str().unwrap_or("").into(), asset_name: name, asset_url: url, size })
}

fn curl() -> Command {
    let mut cmd = Command::new(if cfg!(windows) { "curl.exe" } else { "curl" });
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    cmd.stdin(Stdio::null());
    cmd
}

/// Asks GitHub (blocking).
pub fn check(build: &Build) -> Result<Option<Release>, String> {
    let out = curl()
        .args(["-sSfL", "--max-time", "20", "-H", "Accept: application/vnd.github+json", "-A", "DragonDAW-update-check"])
        .arg(format!("https://api.github.com/repos/{REPO}/releases?per_page=20"))
        .output()
        .map_err(|e| format!("curl could not be started: {e}"))?;
    if !out.status.success() {
        return Err(format!("GitHub could not be reached ({})", String::from_utf8_lossy(&out.stderr).trim()));
    }
    let json: Value = serde_json::from_slice(&out.stdout).map_err(|e| format!("unexpected answer from GitHub: {e}"))?;
    Ok(pick(&json, build, platform()))
}

/// Downloads `release` into `dir` (blocking); returns the file.
pub fn download(release: &Release, dir: &Path) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let file = dir.join(&release.asset_name);
    let part = dir.join(format!("{}.part", release.asset_name));
    let out = curl().args(["-sSfL", "--max-time", "1800", "-o"]).arg(&part).arg(&release.asset_url).output().map_err(|e| format!("curl could not be started: {e}"))?;
    if !out.status.success() {
        let _ = std::fs::remove_file(&part);
        return Err(format!("the download failed ({})", String::from_utf8_lossy(&out.stderr).trim()));
    }
    std::fs::rename(&part, &file).map_err(|e| e.to_string())?;
    Ok(file)
}

/// The Downloads folder (falls back to the temp folder).
fn downloads() -> PathBuf {
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from);
    home.map(|h| h.join("Downloads")).filter(|d| d.is_dir()).unwrap_or_else(std::env::temp_dir)
}

/// Windows: a script that waits for this app to close, unpacks `zip` over the app's folder and starts it
/// again. Returns the script's path.
fn write_installer(zip: &Path) -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = exe.parent().ok_or("the app's folder is unknown")?;
    let tmp = std::env::temp_dir().join("dragon-update");
    let ps = |p: &Path| p.to_string_lossy().replace('\'', "''");
    let script = format!(
        "$ErrorActionPreference='Stop'\r\n\
         try {{ Wait-Process -Id {pid} -Timeout 600 -ErrorAction SilentlyContinue }} catch {{}}\r\n\
         Start-Sleep -Milliseconds 500\r\n\
         if (Test-Path '{tmp}') {{ Remove-Item -Recurse -Force '{tmp}' }}\r\n\
         try {{\r\n\
           Expand-Archive -Force -Path '{zip}' -DestinationPath '{tmp}'\r\n\
           $src = Get-ChildItem -Path '{tmp}' -Directory | Select-Object -First 1\r\n\
           Copy-Item -Recurse -Force (Join-Path $src.FullName '*') '{dir}'\r\n\
         }} catch {{\r\n\
           Add-Type -AssemblyName PresentationFramework\r\n\
           [System.Windows.MessageBox]::Show(\"The update could not be installed: $($_.Exception.Message)`nThe download is in {zip_shown}\", 'Dragon DAW') | Out-Null\r\n\
         }}\r\n\
         Start-Process -FilePath '{exe}'\r\n",
        pid = std::process::id(),
        tmp = ps(&tmp),
        zip = ps(zip),
        zip_shown = zip.to_string_lossy().replace('"', "").replace('\'', "''"),
        dir = ps(dir),
        exe = ps(&exe),
    );
    let path = std::env::temp_dir().join("dragon-update.ps1");
    std::fs::write(&path, script).map_err(|e| e.to_string())?;
    Ok(path)
}

/// Starts the installer and asks the app to close.
fn install_and_restart(st: &mut State, zip: &Path) {
    match write_installer(zip) {
        Ok(script) => {
            let mut cmd = Command::new("powershell");
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                cmd.creation_flags(0x0800_0000);
            }
            match cmd.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"]).arg(&script).spawn() {
                Ok(_) => st.ui.update.quit = true,
                Err(e) => st.toast(format!("The installer could not be started: {e}"), Severity::Error),
            }
        }
        Err(e) => st.toast(format!("The installer could not be written: {e}"), Severity::Error),
    }
}

type Slot<T> = Arc<Mutex<Option<Result<T, String>>>>;

/// The update bar's state.
#[derive(Default)]
pub struct UpdateUi {
    checking: Option<Slot<Option<Release>>>,
    /// Checked because the user asked (then "up to date" and errors are shown too).
    manual: bool,
    pub found: Option<Release>,
    hidden: bool,
    downloading: Option<Slot<PathBuf>>,
    pub downloaded: Option<PathBuf>,
    /// Close the app (the installer is waiting for it).
    pub quit: bool,
    /// The "new version" window (opened at start when a release is found, and from the top-right chip).
    pub popup: bool,
}

/// Checks in the background; `manual` = the user asked (Help › Check for updates).
pub fn start_check(st: &mut State, manual: bool) {
    if st.ui.update.checking.is_some() {
        return;
    }
    let build = Build::current();
    if !manual && (build.channel == "dev" || !st.settings.check_updates) {
        return;
    }
    let slot: Slot<Option<Release>> = Arc::new(Mutex::new(None));
    let out = slot.clone();
    std::thread::spawn(move || {
        let r = check(&build);
        if let Ok(mut g) = out.lock() {
            *g = Some(r);
        }
        redhive_client_framework::runtime::request_repaint();
    });
    st.ui.update.checking = Some(slot);
    st.ui.update.manual = manual;
    if manual {
        st.ui.update.hidden = false;
        st.toast("Looking for a new version …", Severity::Info);
    }
}

fn take<T>(slot: &Option<Slot<T>>) -> Option<Result<T, String>> {
    slot.as_ref().and_then(|s| s.lock().ok().and_then(|mut g| g.take()))
}

/// Picks up finished checks and downloads (every frame).
pub fn poll(st: &mut State) {
    if let Some(res) = take(&st.ui.update.checking) {
        st.ui.update.checking = None;
        let manual = st.ui.update.manual;
        match res {
            Ok(Some(r)) => {
                let skipped = !manual && st.settings.skipped_update.as_deref() == Some(r.id.as_str());
                st.ui.update.hidden = skipped;
                st.ui.update.popup = !skipped;
                st.ui.update.found = Some(r);
            }
            Ok(None) if manual => st.toast(format!("Dragon DAW is up to date ({})", Build::current().label()), Severity::Success),
            Err(e) if manual => st.toast(format!("Could not check for updates: {e}"), Severity::Error),
            _ => {}
        }
    }
    if let Some(res) = take(&st.ui.update.downloading) {
        st.ui.update.downloading = None;
        match res {
            Ok(file) => {
                st.toast(format!("Downloaded to {}", file.display()), Severity::Success);
                st.ui.update.downloaded = Some(file);
            }
            Err(e) => st.toast(format!("Download failed: {e}"), Severity::Error),
        }
    }
}

fn start_download(st: &mut State, release: Release) {
    let slot: Slot<PathBuf> = Arc::new(Mutex::new(None));
    let out = slot.clone();
    let dir = downloads();
    std::thread::spawn(move || {
        let r = download(&release, &dir);
        if let Ok(mut g) = out.lock() {
            *g = Some(r);
        }
        redhive_client_framework::runtime::request_repaint();
    });
    st.ui.update.downloading = Some(slot);
}

/// Whether the update bar has something to show: a download running or done (the offer itself is the
/// window at start and the chip in the top-right corner).
pub fn visible(st: &State) -> bool {
    st.ui.update.found.is_some() && !st.ui.update.hidden && (st.ui.update.downloading.is_some() || st.ui.update.downloaded.is_some())
}

/// The chip left of the quest level (top right): shown while a new version is available; opens the
/// update window.
pub fn chip(ui: &mut egui::Ui, st: &mut State) {
    let Some(r) = st.ui.update.found.clone() else { return };
    if st.settings.skipped_update.as_deref() == Some(r.id.as_str()) && st.ui.update.hidden {
        return;
    }
    let t = ui.input(|i| i.time) as f32;
    let pulse = 0.75 + 0.25 * (t * 2.0).sin().abs();
    let b = egui::Button::new(RichText::new(format!("⬆ Update {}", r.label)).small().strong().color(Color32::BLACK)).fill(theme::ACCENT_2.gamma_multiply(pulse)).corner_radius(10.0);
    if ui.add(b).on_hover_text(format!("Dragon DAW {} is available (you have {}) – click for details", r.label, Build::current().label())).clicked() {
        st.ui.update.popup = true;
    }
    ui.ctx().request_repaint_after(std::time::Duration::from_millis(60));
}

/// The update window: what is new, Download / Install, Later and Skip.
pub fn popup(ctx: &egui::Context, st: &mut State) {
    let Some(r) = st.ui.update.found.clone() else { return };
    if !st.ui.update.popup {
        return;
    }
    let mut open = true;
    egui::Window::new("A new version of Dragon DAW is available")
        .id(egui::Id::new("dragon.update.popup"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        // Above the start screen too.
        .order(egui::Order::Tooltip)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.set_width(420.0);
            ui.label(RichText::new(format!("Dragon DAW {}", r.label)).size(22.0).strong().color(theme::ACCENT_2));
            ui.label(RichText::new(format!("You have {}.", Build::current().label())).color(theme::TEXT_DIM));
            if !r.page.is_empty() {
                ui.hyperlink_to("What's new (release notes)", &r.page);
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if st.ui.update.downloading.is_some() {
                    ui.spinner();
                    ui.label(format!("Downloading {} …", mb(r.size)));
                } else if let Some(file) = st.ui.update.downloaded.clone() {
                    if cfg!(windows) && file.extension().is_some_and(|e| e == "zip") {
                        let b = egui::Button::new(RichText::new("Install and restart").strong().color(Color32::BLACK)).fill(theme::ACCENT);
                        if ui.add(b).clicked() {
                            st.ui.update.popup = false;
                            install_and_restart(st, &file);
                        }
                    } else if ui.button("Show in folder").clicked() {
                        crate::chains::reveal(&file.parent().map(Path::to_path_buf).unwrap_or_default());
                    }
                } else {
                    let b = egui::Button::new(RichText::new(format!("Download ({})", mb(r.size))).strong().color(Color32::BLACK)).fill(theme::ACCENT);
                    if ui.add(b).on_hover_text(format!("Saves {} to your Downloads folder", r.asset_name)).clicked() {
                        st.ui.update.hidden = false;
                        start_download(st, r.clone());
                    }
                }
                if ui.button("Later").on_hover_text("The Update chip in the top-right corner brings this back").clicked() {
                    st.ui.update.popup = false;
                }
                if st.ui.update.downloading.is_none() && ui.button("Skip this version").clicked() {
                    st.settings.skipped_update = Some(r.id.clone());
                    st.save_settings();
                    st.ui.update.hidden = true;
                    st.ui.update.popup = false;
                }
            });
        });
    if !open {
        st.ui.update.popup = false;
    }
}

/// The bar under the transport: "a new version is available" with Download / install buttons.
pub fn bar(ui: &mut egui::Ui, st: &mut State) {
    let Some(r) = st.ui.update.found.clone() else { return };
    ui.horizontal_centered(|ui| {
        ui.label(RichText::new("New version").strong().color(Color32::BLACK).background_color(theme::ACCENT_2));
        ui.label(RichText::new(format!("Dragon DAW {} is available", r.label)).strong());
        ui.label(RichText::new(format!("you have {}", Build::current().label())).small().color(theme::TEXT_DIM));
        if !r.page.is_empty() {
            ui.hyperlink_to("What's new", &r.page);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("×").on_hover_text("Hide until the next start").clicked() {
                st.ui.update.hidden = true;
            }
            if st.ui.update.downloading.is_none() && st.ui.update.downloaded.is_none() && ui.small_button("Skip this version").clicked() {
                st.settings.skipped_update = Some(r.id.clone());
                st.save_settings();
                st.ui.update.hidden = true;
            }
            if st.ui.update.downloading.is_some() {
                ui.label(format!("Downloading {} …", mb(r.size)));
                ui.spinner();
            } else if let Some(file) = st.ui.update.downloaded.clone() {
                let folder = file.parent().map(Path::to_path_buf).unwrap_or_default();
                if ui.button("Show in folder").clicked() {
                    crate::chains::reveal(&folder);
                }
                if cfg!(windows) && file.extension().is_some_and(|e| e == "zip") {
                    let b = egui::Button::new(RichText::new("Install and restart").strong().color(Color32::BLACK)).fill(theme::ACCENT);
                    if ui.add(b).on_hover_text("Closes Dragon DAW (asks to save first), puts the new version in place and starts it again").clicked() {
                        install_and_restart(st, &file);
                    }
                }
            } else {
                let b = egui::Button::new(RichText::new(format!("Download ({})", mb(r.size))).strong().color(Color32::BLACK)).fill(theme::ACCENT);
                if ui.add(b).on_hover_text(format!("Saves {} to your Downloads folder", r.asset_name)).clicked() {
                    start_download(st, r.clone());
                }
            }
        });
    });
}

fn mb(bytes: u64) -> String {
    if bytes == 0 {
        String::new()
    } else {
        format!("{:.0} MB", bytes as f64 / 1_048_576.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn rel(tag: &str, pre: bool, sha: &str, assets: &[&str]) -> Value {
        json!({
            "tag_name": tag, "prerelease": pre, "draft": false, "target_commitish": sha,
            "html_url": format!("https://github.com/{REPO}/releases/tag/{tag}"),
            "assets": assets.iter().map(|a| json!({"name": a, "browser_download_url": format!("https://example/{a}"), "size": 50_000_000})).collect::<Vec<_>>()
        })
    }

    fn build(version: &str, sha: &str, channel: &str) -> Build {
        Build { version: version.into(), sha: sha.into(), channel: channel.into() }
    }

    #[test]
    fn versions_compare_by_number() {
        assert!(version_key("v2.10.0") > version_key("2.9.3"));
        assert!(version_key("2.0.1-beta") > version_key("2.0.0"));
        assert_eq!(version_key("v2.0.0"), version_key("2.0.0"));
    }

    #[test]
    fn a_newer_versioned_release_is_offered_with_this_platforms_archive() {
        let list = json!([
            rel("nightly", true, "bbbbbbbbbb", &["dragon-daw-nightly-bbbbbbb-windows-x86_64.zip"]),
            rel("v2.1.0", false, "main", &["dragon-daw-2.1.0-linux-x86_64.tar.gz", "dragon-daw-2.1.0-windows-x86_64.zip", "SHA256SUMS.txt"]),
            rel("v2.0.0", false, "main", &["dragon-daw-2.0.0-windows-x86_64.zip"]),
        ]);
        let r = pick(&list, &build("2.0.0", "aaaaaaaaaa", "stable"), "windows-x86_64").unwrap();
        assert_eq!(r.id, "v2.1.0");
        assert_eq!(r.asset_name, "dragon-daw-2.1.0-windows-x86_64.zip");
        assert!(pick(&list, &build("2.1.0", "x", "stable"), "windows-x86_64").is_none(), "up to date");
        // A stable build is not offered nightlies or pre-releases.
        let pre = json!([rel("v2.2.0-beta", true, "main", &["dragon-daw-2.2.0-beta-windows-x86_64.zip"])]);
        assert!(pick(&pre, &build("2.1.0", "x", "stable"), "windows-x86_64").is_none());
    }

    #[test]
    fn a_nightly_build_is_offered_the_newest_nightly_by_commit() {
        let list = json!([rel("nightly", true, "bbbbbbbbbbbb", &["dragon-daw-nightly-bbbbbbb-windows-x86_64.zip"]), rel("v2.0.0", false, "main", &["dragon-daw-2.0.0-windows-x86_64.zip"])]);
        let r = pick(&list, &build("2.0.0", "aaaaaaaaaaaa", "nightly"), "windows-x86_64").unwrap();
        assert_eq!(r.id, "nightly-bbbbbbb");
        assert_eq!(r.label, "nightly bbbbbbb");
        assert!(pick(&list, &build("2.0.0", "bbbbbbbbbbbb", "nightly"), "windows-x86_64").is_none(), "same commit: up to date");
        assert!(pick(&list, &build("2.0.0", "bbbbbbb", "nightly"), "windows-x86_64").is_none(), "short sha matches too");
        // No build for this platform: nothing to offer.
        assert!(pick(&list, &build("2.0.0", "aaaa", "nightly"), "macos-arm64").is_none());
    }
}

#[cfg(test)]
mod bar_tests {
    use super::*;

    #[test]
    fn the_bar_shows_a_found_release_and_skip_hides_it() {
        let mut st = State::new(redhive_client_framework::AppContext::new("en"), crate::state::Settings { show_splash: false, ..Default::default() });
        assert!(!visible(&st));
        let rel = Release { id: "v9.0.0".into(), label: "9.0.0".into(), page: String::new(), asset_name: "dragon-daw-9.0.0-windows-x86_64.zip".into(), asset_url: "https://example/x.zip".into(), size: 60_000_000 };
        // A start-up check that finds a release opens the window (the chip stays in the corner).
        st.ui.update.checking = Some(Arc::new(Mutex::new(Some(Ok(Some(rel.clone()))))));
        poll(&mut st);
        assert!(st.ui.update.popup && st.ui.update.found.is_some());
        assert!(!visible(&st), "the bar only shows a download");
        st.ui.update.downloaded = Some(PathBuf::from("x.zip"));
        assert!(visible(&st));
        st.ui.update.downloaded = None;
        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            popup(ctx, &mut st);
            egui::CentralPanel::default().show(ctx, |ui| {
                bar(ui, &mut st);
                chip(ui, &mut st);
            });
        });
        st.settings.skipped_update = Some("v9.0.0".into());
        // A start-up check that finds the skipped version keeps the bar hidden.
        st.ui.update.checking = Some(Arc::new(Mutex::new(Some(Ok(st.ui.update.found.clone())))));
        st.ui.update.manual = false;
        poll(&mut st);
        assert!(!visible(&st));
    }
}
