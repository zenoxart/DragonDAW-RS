//! Animated GIFs generated with Higgsfield AI (the splash logo, the level-up animation), decoded into
//! textures and played back by time. The files are looked up in `assets/` next to the executable, in the
//! working directory and in the source tree, so a release can ship them beside the binary.

use egui::TextureHandle;
use std::path::PathBuf;
use std::time::Duration;

/// A decoded animation: textures with their display durations.
pub struct GifAnim {
    pub frames: Vec<(TextureHandle, Duration)>,
    pub total: Duration,
    /// Pixel size of the frames.
    #[allow(dead_code)]
    pub size: [usize; 2],
}

/// Where an asset called `name` may live.
pub fn candidates(name: &str) -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(d) = exe.parent() {
            v.push(d.join("assets").join(name));
        }
    }
    v.push(PathBuf::from("assets").join(name));
    v.push(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets").join(name));
    v
}

/// Decodes GIF bytes into RGBA frames with their delays (at most `max` frames).
pub fn decode(bytes: &[u8], max: usize) -> Option<Vec<(egui::ColorImage, Duration)>> {
    use image::AnimationDecoder;
    let dec = image::codecs::gif::GifDecoder::new(std::io::Cursor::new(bytes)).ok()?;
    let frames = dec.into_frames().collect_frames().ok()?;
    let out: Vec<_> = frames
        .into_iter()
        .take(max)
        .map(|fr| {
            let (num, den) = fr.delay().numer_denom_ms();
            let ms = num.checked_div(den).unwrap_or(80).max(20);
            let buf = fr.into_buffer();
            (egui::ColorImage::from_rgba_unmultiplied([buf.width() as usize, buf.height() as usize], buf.as_raw()), Duration::from_millis(ms as u64))
        })
        .collect();
    (!out.is_empty()).then_some(out)
}

impl GifAnim {
    /// Loads the first readable `name` from [`candidates`].
    pub fn load(ctx: &egui::Context, name: &str) -> Option<GifAnim> {
        for p in candidates(name) {
            let Ok(bytes) = std::fs::read(&p) else { continue };
            if let Some(a) = Self::from_bytes(ctx, name, &bytes) {
                tracing::info!("animation {name}: {} ({} frames)", p.display(), a.frames.len());
                return Some(a);
            }
        }
        None
    }

    pub fn from_bytes(ctx: &egui::Context, name: &str, bytes: &[u8]) -> Option<GifAnim> {
        let frames = decode(bytes, 240)?;
        let size = frames[0].0.size;
        let frames: Vec<_> = frames.into_iter().enumerate().map(|(i, (img, d))| (ctx.load_texture(format!("{name}#{i}"), img, egui::TextureOptions::LINEAR), d)).collect();
        let total = frames.iter().map(|f| f.1).sum();
        Some(GifAnim { frames, total, size })
    }

    /// The frame shown `t` seconds after the start; `looped` wraps around, otherwise holds the last frame.
    pub fn frame_at(&self, t: f32, looped: bool) -> &TextureHandle {
        let mut at = Duration::from_secs_f32(t.max(0.0));
        if looped && !self.total.is_zero() {
            at = Duration::from_nanos((at.as_nanos() % self.total.as_nanos()) as u64);
        }
        for (tex, d) in &self.frames {
            if at < *d {
                return tex;
            }
            at -= *d;
        }
        &self.frames[self.frames.len() - 1].0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A two-frame GIF decodes and plays frame by frame, looping or holding the last frame.
    #[test]
    fn decodes_and_plays_an_animated_gif() {
        let mut bytes = Vec::new();
        {
            let mut enc = image::codecs::gif::GifEncoder::new(&mut bytes);
            enc.set_repeat(image::codecs::gif::Repeat::Infinite).unwrap();
            for c in [[240u8, 138, 75, 255], [75, 192, 240, 255]] {
                let img = image::RgbaImage::from_pixel(8, 8, image::Rgba(c));
                enc.encode_frame(image::Frame::from_parts(img, 0, 0, image::Delay::from_numer_denom_ms(100, 1))).unwrap();
            }
        }
        let ctx = egui::Context::default();
        let a = GifAnim::from_bytes(&ctx, "test.gif", &bytes).expect("decodes");
        assert_eq!((a.frames.len(), a.size), (2, [8, 8]));
        assert_eq!(a.total, Duration::from_millis(200));
        assert_eq!(a.frame_at(0.05, true).id(), a.frames[0].0.id());
        assert_eq!(a.frame_at(0.15, true).id(), a.frames[1].0.id());
        assert_eq!(a.frame_at(0.25, true).id(), a.frames[0].0.id(), "loops");
        assert_eq!(a.frame_at(5.0, false).id(), a.frames[1].0.id(), "holds the last frame");
    }
}
