//! The Dragon DAW brand, taken over from the original Dragon DAW (github.com/zenoxart/DragonDaw): the red
//! dragon logo (window and executable icon, splash, menu mark) and the startup sound. All embedded, so a
//! bare executable carries them.

use egui::{Context, TextureHandle, TextureOptions};

/// The full logo (dragon and "DRAGON" lettering), 512 px.
pub const LOGO_PNG: &[u8] = include_bytes!("../assets/dragon-logo.png");
/// The logo at 256 px, for the window icon.
pub const ICON_PNG: &[u8] = include_bytes!("../assets/dragon-icon.png");
/// The dragon's head alone, for small marks.
pub const MARK_PNG: &[u8] = include_bytes!("../assets/dragon-mark.png");
/// The startup sound (DragonSound.wav).
pub const STARTUP_WAV: &[u8] = include_bytes!("../assets/DragonSound.wav");

fn decode(bytes: &[u8]) -> Option<image::RgbaImage> {
    image::load_from_memory_with_format(bytes, image::ImageFormat::Png).ok().map(|i| i.to_rgba8())
}

/// The window icon.
pub fn window_icon() -> egui::IconData {
    match decode(ICON_PNG) {
        Some(img) => egui::IconData { width: img.width(), height: img.height(), rgba: img.into_raw() },
        None => egui::IconData::default(),
    }
}

fn texture(ctx: &Context, name: &'static str, bytes: &[u8]) -> Option<TextureHandle> {
    let id = egui::Id::new(("brand", name));
    if let Some(t) = ctx.data(|d| d.get_temp::<TextureHandle>(id)) {
        return Some(t);
    }
    let img = decode(bytes)?;
    let size = [img.width() as usize, img.height() as usize];
    let tex = ctx.load_texture(name, egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw()), TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, tex.clone()));
    Some(tex)
}

/// The full logo as a texture.
pub fn logo(ctx: &Context) -> Option<TextureHandle> {
    texture(ctx, "dragon-logo", LOGO_PNG)
}

/// The dragon's head as a texture.
pub fn mark(ctx: &Context) -> Option<TextureHandle> {
    texture(ctx, "dragon-mark", MARK_PNG)
}

/// Writes the startup sound next to the settings (once) and returns its path, for the engine's preview
/// voice to play.
pub fn startup_sound(dir: &std::path::Path) -> Option<std::path::PathBuf> {
    let path = dir.join("DragonSound.wav");
    let fresh = std::fs::metadata(&path).map(|m| m.len() as usize != STARTUP_WAV.len()).unwrap_or(true);
    if fresh {
        std::fs::create_dir_all(dir).ok()?;
        std::fs::write(&path, STARTUP_WAV).ok()?;
    }
    Some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_brand_assets_decode() {
        for png in [LOGO_PNG, ICON_PNG, MARK_PNG] {
            let img = decode(png).expect("png decodes");
            assert_eq!(img.width(), img.height());
        }
        let icon = window_icon();
        assert_eq!(icon.rgba.len() as u32, icon.width * icon.height * 4);
        let dir = tempfile::tempdir().unwrap();
        let path = startup_sound(dir.path()).expect("written");
        let snd = dragon_engine::samples::decode(&path).expect("the startup sound decodes");
        assert!(snd.l.len() > 1000);
    }
}
