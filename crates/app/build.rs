//! Gives the Windows executable the Dragon DAW icon (assets/dragondaw.ico, from the original Dragon DAW).

fn main() {
    println!("cargo:rerun-if-changed=assets/dragondaw.ico");
    // What this build is, for the update check: the release workflow sets both; a local build asks git.
    println!("cargo:rerun-if-env-changed=DRAGON_BUILD_SHA");
    println!("cargo:rerun-if-env-changed=DRAGON_BUILD_CHANNEL");
    let sha = std::env::var("DRAGON_BUILD_SHA")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| std::process::Command::new("git").args(["rev-parse", "HEAD"]).output().ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()));
    let channel = std::env::var("DRAGON_BUILD_CHANNEL").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "dev".into());
    println!("cargo:rustc-env=DRAGON_BUILD_SHA={}", sha.unwrap_or_default());
    println!("cargo:rustc-env=DRAGON_BUILD_CHANNEL={channel}");
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/dragondaw.ico");
        res.set("ProductName", "Dragon DAW");
        res.set("FileDescription", "Dragon DAW");
        if let Err(e) = res.compile() {
            println!("cargo:warning=could not embed the icon: {e}");
        }
    }
}
