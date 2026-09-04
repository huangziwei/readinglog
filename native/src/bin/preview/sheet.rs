//! Several frames on one sheet, each under its own name.

use readinglog_native::eink::fb::Framebuffer;
use readinglog_native::ui::text::TextRenderer;

/// The ground the frames are laid on, dark enough to edge a white page.
const GROUND: [u8; 3] = [0x24, 0x26, 0x2A];

/// How wide a sheet runs before it wraps onto another row.
const WIDEST: i32 = 2800;

/// A frame and what to call it.
pub struct Tile {
    pub name: String,
    pub width: u32,
    pub height: u32,
    /// Packed RGB, `width * 3` to a row.
    pub pixels: Vec<u8>,
}

impl Tile {
    /// The frame `fb` holds.
    pub fn of(name: String, fb: &Framebuffer) -> Self {
        Self {
            name,
            width: fb.var.xres,
            height: fb.var.yres,
            pixels: fb.backing_snapshot(),
        }
    }
}

/// `tiles` at `scale` percent, captioned, laid left to right and wrapped.
pub fn compose(tiles: &[Tile], scale: u32, text: &mut TextRenderer, px: f32) -> Framebuffer {
    let small: Vec<Tile> = tiles.iter().map(|t| shrink(t, scale)).collect();

    text.set_px(px);
    let caption = text.line_height() as i32;
    let gap = (px as i32).max(8);
    let cell_w = small.iter().map(|t| t.width as i32).max().unwrap_or(1);
    let cell_h = small.iter().map(|t| t.height as i32).max().unwrap_or(1);
    let columns = (((WIDEST - gap) / (cell_w + gap)).max(1) as usize).min(small.len().max(1));
    let rows = small.len().div_ceil(columns).max(1);

    let width = gap + columns as i32 * (cell_w + gap);
    let height = gap + rows as i32 * (caption + gap / 2 + cell_h + gap);
    let mut fb = Framebuffer::offscreen(width as u32, height as u32);
    fb.fill_rect_rgb(0, 0, width as u32, height as u32, GROUND);

    for (at, tile) in small.iter().enumerate() {
        let column = (at % columns) as i32;
        let row = (at / columns) as i32;
        let x = gap + column * (cell_w + gap);
        let y = gap + row * (caption + gap / 2 + cell_h + gap);
        text.set_px(px);
        // `true` sets the name in white, over the ground.
        text.draw(&mut fb, x, y + caption * 3 / 4, &tile.name, true);
        fb.blit_rgb(x, y + caption + gap / 2, tile.width, &tile.pixels);
    }
    fb
}

/// `tile` at `scale` percent, each pixel the mean of the block it stands for.
fn shrink(tile: &Tile, scale: u32) -> Tile {
    let (w, h) = (tile.width.max(1), tile.height.max(1));
    let out_w = (w * scale / 100).max(1);
    let out_h = (h * scale / 100).max(1);
    let mut pixels = vec![0u8; out_w as usize * out_h as usize * 3];
    for row in 0..out_h {
        let from_y = row * h / out_h;
        let till_y = (((row + 1) * h).div_ceil(out_h)).min(h).max(from_y + 1);
        for column in 0..out_w {
            let from_x = column * w / out_w;
            let till_x = (((column + 1) * w).div_ceil(out_w)).min(w).max(from_x + 1);
            let mut sum = [0u32; 3];
            let mut count = 0u32;
            for y in from_y..till_y {
                let line = y as usize * w as usize * 3;
                for x in from_x..till_x {
                    let at = line + x as usize * 3;
                    sum[0] += tile.pixels[at] as u32;
                    sum[1] += tile.pixels[at + 1] as u32;
                    sum[2] += tile.pixels[at + 2] as u32;
                    count += 1;
                }
            }
            let at = (row as usize * out_w as usize + column as usize) * 3;
            for band in 0..3 {
                pixels[at + band] = (sum[band] / count.max(1)) as u8;
            }
        }
    }
    Tile {
        name: tile.name.clone(),
        width: out_w,
        height: out_h,
        pixels,
    }
}

/// A rectangle of a tile, as `--crop` states it.
#[derive(Clone, Copy)]
pub struct Crop {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl Crop {
    /// `WxH+X+Y`, the geometry ImageMagick and X take.
    pub fn read(spec: &str) -> Option<Self> {
        let (size, at) = spec.split_once('+')?;
        let (w, h) = size.split_once('x')?;
        let (x, y) = at.split_once('+')?;
        Some(Self {
            x: x.parse().ok()?,
            y: y.parse().ok()?,
            w: w.parse().ok()?,
            h: h.parse().ok()?,
        })
    }
}

impl Tile {
    /// The part of this tile `crop` names, clipped to what it holds.
    pub fn cropped(&self, crop: Crop) -> Tile {
        let x = crop.x.min(self.width);
        let y = crop.y.min(self.height);
        let w = crop.w.min(self.width - x).max(1);
        let h = crop.h.min(self.height - y).max(1);
        let mut pixels = Vec::with_capacity(w as usize * h as usize * 3);
        for row in y..y + h {
            let at = (row as usize * self.width as usize + x as usize) * 3;
            pixels.extend_from_slice(&self.pixels[at..at + w as usize * 3]);
        }
        Tile {
            name: self.name.clone(),
            width: w,
            height: h,
            pixels,
        }
    }

    /// This tile as a PNG at `path`.
    pub fn save(&self, path: &std::path::Path) -> anyhow::Result<()> {
        image::RgbImage::from_raw(self.width, self.height, self.pixels.clone())
            .ok_or_else(|| anyhow::anyhow!("a frame the size it says"))?
            .save(path)
            .map_err(Into::into)
    }
}
