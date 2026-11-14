//! **Red Hive** – the desktop host.
//!
//! A long-running shell that signs in, loads the user's modules from the server (signed view packs),
//! renders them in a VS-style dock layout and swaps them while running when a new version is rolled out.
//!
//! ```text
//! redhive [--server http://localhost:8080] [redhive://navigate?target=documents.invoices&parameter=<id>]
//! ```

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod login;
mod overlays;
mod settings;
mod shell;
mod shell_views;

use eframe::egui;

fn icon() -> egui::IconData {
    // A red hexagon (the hive) drawn at runtime – no binary assets needed.
    let size = 64u32;
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    let c = size as f32 / 2.0;
    let r = size as f32 * 0.46;
    for y in 0..size {
        for x in 0..size {
            let (dx, dy) = (x as f32 - c + 0.5, y as f32 - c + 0.5);
            // flat-top hexagon with circumradius r
            let (ax, ay) = (dx.abs(), dy.abs());
            let inside = ax <= r && ay <= r * 0.866 && 0.866 * ax + 0.5 * ay <= r * 0.866;
            if inside {
                let i = ((y * size + x) * 4) as usize;
                let shade = 1.0 - (dy + r) / (2.0 * r) * 0.35;
                rgba[i] = (198.0 * shade) as u8;
                rgba[i + 1] = (40.0 * shade) as u8;
                rgba[i + 2] = (40.0 * shade) as u8;
                rgba[i + 3] = 255;
            }
        }
    }
    egui::IconData { rgba, width: size, height: size }
}

fn main() -> eframe::Result {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,wgpu=warn,eframe=warn".into())).init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut server = None;
    let mut deep_link = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--server" if i + 1 < args.len() => {
                server = Some(args[i + 1].clone());
                i += 1;
            }
            a if a.starts_with("redhive://") => deep_link = Some(a.to_string()),
            _ => {}
        }
        i += 1;
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("Red Hive").with_inner_size([1360.0, 860.0]).with_min_inner_size([360.0, 480.0]).with_icon(icon()).with_app_id("redhive"),
        persist_window: true,
        ..Default::default()
    };
    eframe::run_native("Red Hive", options, Box::new(move |cc| Ok(Box::new(shell::Shell::new(cc, server, deep_link)))))
}
