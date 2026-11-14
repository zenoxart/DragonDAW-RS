//! **Dragon DAW** – an offline digital audio workstation on the Red Hive desktop framework.
//!
//! ```text
//! dragon-daw [project.dragon]
//! ```

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod audio;
mod commands;
mod session;
mod splash;
mod state;
mod theme;
mod views;

use redhive_client_framework::AppContext;

fn icon() -> egui::IconData {
    // Ember hexagon drawn at runtime – no binary assets needed.
    let size = 64u32;
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    let c = size as f32 / 2.0;
    let r = size as f32 * 0.46;
    for y in 0..size {
        for x in 0..size {
            let (dx, dy) = (x as f32 - c + 0.5, y as f32 - c + 0.5);
            let (ax, ay) = (dx.abs(), dy.abs());
            if ax <= r && ay <= r * 0.866 && 0.866 * ax + 0.5 * ay <= r * 0.866 {
                let i = ((y * size + x) * 4) as usize;
                let k = (dy + r) / (2.0 * r);
                rgba[i] = (232.0 + 23.0 * k) as u8;
                rgba[i + 1] = (74.0 + 80.0 * k) as u8;
                rgba[i + 2] = (44.0 + 16.0 * k) as u8;
                rgba[i + 3] = 255;
            }
        }
    }
    egui::IconData { rgba, width: size, height: size }
}

fn main() -> eframe::Result {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,eframe=warn".into())).init();
    let open = std::env::args().nth(1).map(std::path::PathBuf::from);
    let ctx = AppContext::new("en");
    let settings: state::Settings = ctx.paths.load_json("settings.json").unwrap_or_default();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("Dragon DAW").with_inner_size([1440.0, 900.0]).with_min_inner_size([720.0, 480.0]).with_icon(icon()).with_app_id("dragon-daw").with_drag_and_drop(true),
        persist_window: true,
        ..Default::default()
    };
    eframe::run_native(
        "Dragon DAW",
        options,
        Box::new(move |cc| {
            let mut st = state::State::new(ctx, settings);
            if let Some(p) = open {
                st.open_path(&p);
            }
            Ok(Box::new(app::DragonApp::new(cc, st)))
        }),
    )
}
