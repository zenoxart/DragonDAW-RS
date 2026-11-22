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

const ICON_ATLAS_PNG: &[u8] = include_bytes!("../assets/icons/atlas.png");

/// The app's icon set (made with Higgsfield): 48 glowing gold icons, 96 px each, 6 per row – the 18
/// track icons in `track_icons::ICONS` order, the playlist tools, marker, pattern and song, then the
/// transport and browser icons ([`icon`]).
pub fn icon_atlas(ctx: &Context) -> Option<TextureHandle> {
    let id = egui::Id::new(("brand", "icon-atlas"));
    if let Some(t) = ctx.data(|d| d.get_temp::<TextureHandle>(id)) {
        return Some(t);
    }
    let img = decode(ICON_ATLAS_PNG)?;
    let size = [img.width() as usize, img.height() as usize];
    // Mipmapped: the icons are drawn from 14 to 48 px.
    let opts = TextureOptions { magnification: egui::TextureFilter::Linear, minification: egui::TextureFilter::Linear, wrap_mode: egui::TextureWrapMode::ClampToEdge, mipmap_mode: Some(egui::TextureFilter::Linear) };
    let tex = ctx.load_texture("icon-atlas", egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw()), opts);
    ctx.data_mut(|d| d.insert_temp(id, tex.clone()));
    Some(tex)
}

/// Tile indices in the icon atlas beyond the track icons.
pub mod icon {
    pub const DRAW: usize = 18;
    pub const MARKER: usize = 27;
    pub const PATTERN: usize = 28;
    pub const SONG: usize = 29;
    pub const PLAY: usize = 30;
    pub const STOP: usize = 31;
    pub const RECORD: usize = 32;
    pub const PAUSE: usize = 33;
    pub const LOOP: usize = 34;
    pub const METRONOME: usize = 35;
    pub const COUNT_IN: usize = 36;
    pub const PUNCH: usize = 37;
    pub const FOLDER: usize = 38;
    pub const FOLDER_OPEN: usize = 39;
    pub const AUDIO_FILE: usize = 40;
    pub const PROJECT_FILE: usize = 41;
    pub const PLUGIN: usize = 42;
    pub const PRESET: usize = 43;
    /// Stock (Dragon's own) items.
    pub const STAR: usize = 44;
    pub const MIDI_FILE: usize = 46;
    pub const SEARCH: usize = 47;
    /// Tiles in the atlas (6 per row).
    pub const COUNT: usize = 48;
}

/// UV rectangle of atlas tile `k` (for `egui::Image::uv`).
pub fn icon_uv(k: usize) -> egui::Rect {
    let rows = icon::COUNT.div_ceil(6) as f32;
    let (cx, cy) = ((k % 6) as f32, (k / 6) as f32);
    egui::Rect::from_min_max(egui::pos2((cx * 96.0 + 0.5) / 576.0, (cy * 96.0 + 0.5) / (rows * 96.0)), egui::pos2(((cx + 1.0) * 96.0 - 0.5) / 576.0, ((cy + 1.0) * 96.0 - 0.5) / (rows * 96.0)))
}

/// An `egui::Image` of atlas tile `k` at `size` points (for buttons).
pub fn icon_image(ctx: &Context, k: usize, size: f32) -> Option<egui::Image<'static>> {
    let tex = icon_atlas(ctx)?;
    Some(egui::Image::new((tex.id(), egui::Vec2::splat(size))).uv(icon_uv(k)))
}

/// Draws atlas tile `k` into `r` (square, centred), tinted by `tint` (white = its own gold).
pub fn paint_icon(p: &egui::Painter, r: egui::Rect, k: usize, tint: egui::Color32) -> bool {
    let Some(tex) = icon_atlas(p.ctx()) else { return false };
    let s = r.width().min(r.height());
    let sq = egui::Rect::from_center_size(r.center(), egui::Vec2::splat(s));
    // A half-texel inset keeps neighbours from bleeding in.
    p.image(tex.id(), sq, icon_uv(k), tint);
    true
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
