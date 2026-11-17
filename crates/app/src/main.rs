//! **Dragon DAW** – an offline digital audio workstation on the Red Hive desktop framework.
//!
//! ```text
//! dragon-daw [project.dragon]
//! ```

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod audio;
mod chains;
mod commands;
mod crash;
mod gif_anim;
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
    // `dragon-daw [--safe] [project.dragon]` – `--safe` opens the project with third-party plugins off.
    let args: Vec<String> = std::env::args().skip(1).collect();
    let safe_flag = args.iter().any(|a| a == "--safe");
    let open = args.iter().find(|a| !a.starts_with("--")).map(std::path::PathBuf::from);
    let ctx = AppContext::new("en");
    crash::install(&ctx.paths.local);
    let crashed = crash::take(&ctx.paths.local);
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
            match (open, &crashed) {
                // The last run died opening this very project: ask first instead of crashing again.
                (Some(p), Some(c)) if c.project.as_deref() == Some(p.as_path()) && !safe_flag => {}
                (Some(p), _) => st.open_path_mode(&p, safe_flag.then_some(crash::SafeMode::NoPlugins)),
                _ => {}
            }
            st.crash = crashed;
            // DRAGON_OPEN_FX=<effect id> adds that stock effect to the master and opens its editor
            // (used for documentation screenshots).
            // DRAGON_STARTUP_COMMANDS=<id>,<id>… runs commands at startup (e.g. "file.new_demo,view.mixer").
            if let Ok(list) = std::env::var("DRAGON_STARTUP_COMMANDS") {
                for id in list.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                    commands::execute(&mut st, id, &cc.egui_ctx);
                }
            }
            if let Ok(id) = std::env::var("DRAGON_OPEN_FX") {
                st.open_effect_on_master(&id);
            }
            Ok(Box::new(app::DragonApp::new(cc, st)))
        }),
    )
}
