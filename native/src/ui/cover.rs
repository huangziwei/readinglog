//! Book covers, decoded from the file a `BookRecord` names.
//!
//! [`Covers`] holds each decode, keyed by path and by the height asked for.

use std::collections::HashMap;

use crate::eink::fb::Framebuffer;

use super::paint::{self, LIGHT, PALE, Rect};

/// The width a cover box takes at `height`: two thirds of it.
///
/// [`Covers::draw`] centres a cover of another shape inside that box.
pub fn width_for(height: i32) -> i32 {
    height * 2 / 3
}

/// One decoded cover, at the size [`Covers::draw`] blits it.
struct Thumb {
    w: usize,
    h: usize,
    /// Three bytes a pixel, row-major, for `Framebuffer::put_pixel_rgb`.
    rgb: Vec<u8>,
}

/// One decode per (path, height).
#[derive(Default)]
pub struct Covers {
    cache: HashMap<(String, i32), Option<Thumb>>,
}

impl Covers {
    /// Give up every cover read so far. What is on disk has changed: a reset
    /// deleted the cache, or a restore put files back into it.
    pub fn forget(&mut self) {
        self.cache.clear();
    }

    /// The box the cover for `path` fills inside `area`, centred and keeping
    /// its aspect; `area` itself where it cannot be read. Anything set beside a
    /// cover aligns on this and not on `area`.
    pub fn box_in(&mut self, area: Rect, path: &str) -> Rect {
        let key = (path.to_string(), area.h);
        let thumb = self
            .cache
            .entry(key)
            .or_insert_with(|| decode(path, area.w, area.h));
        let Some(thumb) = thumb else { return area };
        let (w, h) = (thumb.w as i32, thumb.h as i32);
        Rect::new(area.x + (area.w - w) / 2, area.y + (area.h - h) / 2, w, h)
    }

    /// Draw the cover for `path` inside `area`, centred, keeping its aspect. A
    /// `path` naming nothing, or a file that will not decode, gets a plain
    /// outlined block.
    pub fn draw(&mut self, fb: &mut Framebuffer, area: Rect, path: &str) {
        let key = (path.to_string(), area.h);
        let thumb = self
            .cache
            .entry(key)
            .or_insert_with(|| decode(path, area.w, area.h));
        let Some(thumb) = thumb else {
            paint::fill(fb, area, PALE);
            paint::stroke(fb, area, LIGHT, 1);
            return;
        };
        let x0 = area.x + (area.w - thumb.w as i32) / 2;
        let y0 = area.y + (area.h - thumb.h as i32) / 2;
        for row in 0..thumb.h {
            for col in 0..thumb.w {
                let at = (row * thumb.w + col) * 3;
                fb.put_pixel_rgb(
                    x0 + col as i32,
                    y0 + row as i32,
                    [thumb.rgb[at], thumb.rgb[at + 1], thumb.rgb[at + 2]],
                );
            }
        }
        paint::stroke(
            fb,
            Rect::new(x0, y0, thumb.w as i32, thumb.h as i32),
            LIGHT,
            1,
        );
    }
}

/// Decode and box-fit a thumbnail into `max_w` × `max_h`.
fn decode(path: &str, max_w: i32, max_h: i32) -> Option<Thumb> {
    if path.is_empty() || max_w <= 0 || max_h <= 0 {
        return None;
    }
    let img = image::open(path).ok()?.into_rgb8();
    let (sw, sh) = (img.width() as i32, img.height() as i32);
    if sw == 0 || sh == 0 {
        return None;
    }
    // Whichever of `max_w`, `max_h` and the source's own size binds first.
    let w = max_w.min(sw * max_h / sh).min(sw).max(1) as usize;
    let h = max_h.min(sh * max_w / sw).min(sh).max(1) as usize;
    let mut rgb = vec![0u8; w * h * 3];
    for (row, out) in rgb.chunks_exact_mut(w * 3).enumerate() {
        let sy = (row * sh as usize / h).min(sh as usize - 1);
        for (col, px) in out.as_chunks_mut::<3>().0.iter_mut().enumerate() {
            let sx = (col * sw as usize / w).min(sw as usize - 1);
            *px = img.get_pixel(sx as u32, sy as u32).0;
        }
    }
    Some(Thumb { w, h, rgb })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_that_names_nothing_decodes_to_nothing() {
        assert!(decode("", 60, 90).is_none());
        assert!(decode("/nonexistent/cover.jpg", 60, 90).is_none());
    }

    #[test]
    fn a_cover_is_boxed_into_the_space_without_being_cropped() {
        let dir = std::env::temp_dir().join("readinglog-cover");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("tall.png");
        // 40x120, three times as tall as it is wide.
        image::RgbImage::from_pixel(40, 120, image::Rgb([20, 130, 240]))
            .save(&path)
            .expect("a written fixture");

        let thumb = decode(path.to_str().unwrap(), 60, 90).expect("a decoded cover");
        assert!(thumb.w <= 60 && thumb.h <= 90);
        // `max_h` binds; the width follows.
        assert_eq!(thumb.h, 90);
        assert_eq!(thumb.w, 30);
        assert_eq!(thumb.rgb.len(), thumb.w * thumb.h * 3);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_cover_smaller_than_its_box_is_never_sampled_up() {
        let dir = std::env::temp_dir().join("readinglog-cover-small");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("small.png");
        image::RgbImage::from_pixel(80, 120, image::Rgb([20, 130, 240]))
            .save(&path)
            .expect("a written fixture");

        let thumb = decode(path.to_str().unwrap(), 200, 300).expect("a decoded cover");
        assert_eq!((thumb.w, thumb.h), (80, 120), "drawn at its own size");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_cover_box_stands_two_thirds_as_wide_as_it_is_tall() {
        assert_eq!(width_for(300), 200);
        assert_eq!(width_for(0), 0);
    }

    #[test]
    fn a_colour_cover_keeps_its_channels() {
        let dir = std::env::temp_dir().join("readinglog-cover-colour");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("blue.png");
        image::RgbImage::from_pixel(40, 40, image::Rgb([20, 130, 240]))
            .save(&path)
            .expect("a written fixture");

        let thumb = decode(path.to_str().unwrap(), 40, 40).expect("a decoded cover");
        // Three distinct channels.
        assert!(
            thumb
                .rgb
                .as_chunks::<3>()
                .0
                .iter()
                .all(|p| *p == [20, 130, 240]),
            "a channel was collapsed"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
