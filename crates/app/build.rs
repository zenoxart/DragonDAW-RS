//! Gives the Windows executable the Dragon DAW icon (assets/dragondaw.ico, from the original Dragon DAW).

fn main() {
    println!("cargo:rerun-if-changed=assets/dragondaw.ico");
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
