//! Display surface: a WM-managed fullscreen X11 window. Drawing lands in a
//! packed-RGB backing ([`CH`] bytes/pixel, white=255) and reaches the server
//! through [`Framebuffer::send_update`]. Rendering is identity.

use anyhow::{Context, Result};

use x11rb::connection::Connection;
// `maximum_request_bytes` (BIG-REQUESTS-aware) lives on this trait.
use x11rb::connection::RequestConnection as _;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    AtomEnum, ConnectionExt, CreateGCAux, CreateWindowAux, EventMask, Gcontext, ImageFormat,
    ImageOrder, PropMode, Screen, Window, WindowClass,
};
use x11rb::rust_connection::RustConnection;
// `change_property8` lives in the wrapper `ConnectionExt`.
use x11rb::wrapper::ConnectionExt as _;

// [`Framebuffer::send_update`] accepts and ignores this.
/// Direct update: fast, two-tone, for a line of text that changes.
pub const WAVEFORM_MODE_DU: u32 = 1;

pub const WAVEFORM_MODE_GC16: u32 = 2;

/// Bytes per pixel in the backing store: packed RGB, no alpha.
pub const CH: usize = 3;

/// Rec. 601 luma, the depth-8 wire collapse. The weights sum to 256, keeping
/// `>> 8` an integer multiply-shift.
#[inline]
fn luma(r: u8, g: u8, b: u8) -> u8 {
    ((r as u32 * 77 + g as u32 * 150 + b as u32 * 29) >> 8) as u8
}

/// R/G/B byte offsets within a `bpp`-wide wire pixel, from the root visual's
/// colour masks under the server image byte order. `None` on depth-8 and on an
/// unreadable visual.
fn wire_channels(conn: &RustConnection, screen: &Screen, bpp: usize) -> Option<[usize; 3]> {
    if bpp < 3 {
        return None;
    }
    let visual = screen
        .allowed_depths
        .iter()
        .flat_map(|d| d.visuals.iter())
        .find(|v| v.visual_id == screen.root_visual)?;
    if visual.red_mask == 0 || visual.green_mask == 0 || visual.blue_mask == 0 {
        return None;
    }
    let msb = conn.setup().image_byte_order == ImageOrder::MSB_FIRST;
    // A mask sits in one byte of the native-endian pixel, at index
    // `trailing_zeros / 8`. MSBFirst mirrors that index across the width.
    let offset = |mask: u32| -> usize {
        let idx = (mask.trailing_zeros() / 8) as usize;
        if msb { bpp - 1 - idx } else { idx }
    };
    Some([
        offset(visual.red_mask),
        offset(visual.green_mask),
        offset(visual.blue_mask),
    ])
}

/// Whether any `/sys/firmware/devicetree/base/*/epd/cfa_panel` exists.
/// An unreadable `/sys` answers `false`.
pub fn has_cfa() -> bool {
    let Ok(entries) = std::fs::read_dir("/sys/firmware/devicetree/base") else {
        return false;
    };
    entries.filter_map(Result::ok).any(|entry| {
        let mut path = entry.path();
        path.push("epd");
        path.push("cfa_panel");
        path.exists()
    })
}

/// A rectangle to present, in screen coords.
#[derive(Default, Debug, Clone, Copy)]
pub struct MxcfbRect {
    pub top: u32,
    #[allow(dead_code)]
    pub left: u32,
    #[allow(dead_code)]
    pub width: u32,
    pub height: u32,
}

/// Geometry, reached as `fb.var.xres` / `fb.var.yres`.
pub struct Var {
    pub xres: u32,
    pub yres: u32,
}

/// The window a [`Framebuffer`] presents through, and the wire format the
/// server takes it in.
struct Surface {
    conn: RustConnection,
    win: Window,
    gc: Gcontext,
    depth: u8,
    /// Wire bytes per pixel for `depth`, from `pixmap_formats`.
    bytes_per_pixel: usize,
    /// R, G, B offsets within a `bytes_per_pixel`-wide wire pixel.
    chan: [usize; 3],
    /// Per-`PutImage` byte budget (server max request length minus header slack).
    max_req_bytes: usize,
}

pub struct Framebuffer {
    /// Where a frame is presented. `None` under [`Framebuffer::offscreen`].
    surface: Option<Surface>,
    pub var: Var,
    /// Packed RGB ([`CH`] bytes/pixel), stride `xres * CH`. Every draw writes here.
    backing: Vec<u8>,
}

impl Framebuffer {
    /// Connect to the X server (`$DISPLAY`), create + map a fullscreen window.
    pub fn open() -> Result<Self> {
        let (conn, screen_num) = x11rb::connect(None).context("connect to X ($DISPLAY)")?;
        let screen = conn.setup().roots[screen_num].clone();
        let xres = screen.width_in_pixels as u32;
        let yres = screen.height_in_pixels as u32;
        let depth = screen.root_depth;
        // Depth 8 → 1; depth 24/32 → 4, X padding 24-bit pixels to 32.
        let bytes_per_pixel = conn
            .setup()
            .pixmap_formats
            .iter()
            .find(|f| f.depth == depth)
            .map(|f| (f.bits_per_pixel as usize / 8).max(1))
            .unwrap_or(1);
        // BGRX little-endian is the lab126 depth-24 fallback layout.
        let chan = wire_channels(&conn, &screen, bytes_per_pixel).unwrap_or([2, 1, 0]);
        // stderr, which `readinglog.sh` appends to its log. `depths` names every
        // depth the server offers: a `depth` of 8 with a 24 among them is a
        // colour panel painted through a grey visual.
        let depths: Vec<String> = screen
            .allowed_depths
            .iter()
            .filter(|d| !d.visuals.is_empty())
            .map(|d| d.depth.to_string())
            .collect();
        eprintln!(
            "fb: xres={xres} yres={yres} depth={depth} bytes_per_pixel={bytes_per_pixel} \
             chan=[{},{},{}] root_visual=0x{:x} depths=[{}] colour={}",
            chan[0],
            chan[1],
            chan[2],
            screen.root_visual,
            depths.join(","),
            bytes_per_pixel >= 3,
        );

        let win = conn.generate_id().context("generate_id window")?;
        conn.create_window(
            depth,
            win,
            screen.root,
            0,
            0,
            screen.width_in_pixels,
            screen.height_in_pixels,
            0,
            WindowClass::INPUT_OUTPUT,
            screen.root_visual,
            // No `backing_store`: it costs panel refreshes on this hardware.
            &CreateWindowAux::new()
                .background_pixel(screen.white_pixel)
                .event_mask(EventMask::EXPOSURE),
        )
        .context("create_window")?;

        // The lab126 WM reads `WM_NAME` as a layout spec.
        let name = b"L:A_N:application_ID:com.readinglog.stats_PC:N_O:U";
        conn.change_property8(
            PropMode::REPLACE,
            win,
            AtomEnum::WM_NAME,
            AtomEnum::STRING,
            name,
        )
        .context("set WM_NAME")?;

        conn.map_window(win).context("map_window")?;

        let gc = conn.generate_id().context("generate_id gc")?;
        conn.create_gc(gc, win, &CreateGCAux::new())
            .context("create_gc")?;
        conn.flush().context("flush after map")?;

        // The granted geometry against the requested `xres` / `yres`.
        match conn
            .get_geometry(win)
            .map_err(|e| e.to_string())
            .and_then(|c| c.reply().map_err(|e| e.to_string()))
        {
            Ok(g) => {
                if u32::from(g.width) != xres || u32::from(g.height) != yres || g.x != 0 || g.y != 0
                {
                    eprintln!(
                        "fb: WARNING window geometry {}x{}+{}+{} != root {xres}x{yres}+0+0 \
                         — edge-anchored UI will be clipped",
                        g.width, g.height, g.x, g.y
                    );
                } else {
                    eprintln!("fb: window geometry matches root ({xres}x{yres})");
                }
            }
            Err(e) => eprintln!("fb: could not read window geometry: {e}"),
        }

        // `maximum_request_bytes` is the post-BIG-REQUESTS limit (~16 MB), past
        // `setup().maximum_request_length`. A 1860×2480 frame is 4.6 MB: one
        // request, uncapped.
        let max_req_bytes = conn.maximum_request_bytes().max(4096);
        eprintln!(
            "fb: max request {} bytes ({} rows/band at {} bpp)",
            max_req_bytes,
            max_req_bytes / (xres as usize * bytes_per_pixel).max(1),
            bytes_per_pixel
        );

        let backing = vec![0xFFu8; xres as usize * yres as usize * CH];

        Ok(Self {
            surface: Some(Surface {
                conn,
                win,
                gc,
                depth,
                bytes_per_pixel,
                chan,
                max_req_bytes,
            }),
            var: Var { xres, yres },
            backing,
        })
    }

    /// A white `xres` by `yres` surface with no server behind it. Everything
    /// draws into it as into a window and [`Framebuffer::capture_png`] takes
    /// the frame off it; [`Framebuffer::send_update`] presents nothing.
    pub fn offscreen(xres: u32, yres: u32) -> Self {
        Self {
            surface: None,
            var: Var { xres, yres },
            backing: vec![0xFFu8; xres as usize * yres as usize * CH],
        }
    }

    /// A gray pixel (0=black, 255=white) stored as `(v,v,v)`. Out-of-range no-ops.
    #[inline]
    pub fn put_pixel(&mut self, x: i32, y: i32, value: u8) {
        self.put_pixel_rgb(x, y, [value, value, value]);
    }

    /// An `[r, g, b]` pixel, for cover art. Out-of-range no-ops.
    #[inline]
    pub fn put_pixel_rgb(&mut self, x: i32, y: i32, rgb: [u8; 3]) {
        if x < 0 || y < 0 || x >= self.var.xres as i32 || y >= self.var.yres as i32 {
            return;
        }
        let idx = (y as usize * self.var.xres as usize + x as usize) * CH;
        if idx + CH <= self.backing.len() {
            self.backing[idx..idx + CH].copy_from_slice(&rgb);
        }
    }

    /// `pixels`, `width` wide in packed RGB, with its top left at `(x, y)`.
    ///
    /// A row at a time, clipped to the surface.
    pub fn blit_rgb(&mut self, x: i32, y: i32, width: u32, pixels: &[u8]) {
        let stride = self.var.xres as usize * CH;
        let src_stride = width as usize * CH;
        if src_stride == 0 {
            return;
        }
        for (row, line) in pixels.chunks_exact(src_stride).enumerate() {
            let at_y = y + row as i32;
            if at_y < 0 || at_y >= self.var.yres as i32 {
                continue;
            }
            // `from` and `till` clip the run to the surface's own columns.
            let from = x.max(0);
            let till = (x + width as i32).min(self.var.xres as i32);
            if till <= from {
                continue;
            }
            let head = (from - x) as usize * CH;
            let take = (till - from) as usize * CH;
            let at = at_y as usize * stride + from as usize * CH;
            self.backing[at..at + take].copy_from_slice(&line[head..head + take]);
        }
    }

    /// A rectangle of `rgb`, three bytes a pixel across the span.
    pub fn fill_rect_rgb(&mut self, top: u32, left: u32, width: u32, height: u32, rgb: [u8; 3]) {
        if left >= self.var.xres {
            return;
        }
        let stride = self.var.xres as usize * CH;
        let max_y = top.saturating_add(height).min(self.var.yres);
        let max_x = left.saturating_add(width).min(self.var.xres);
        for y in top..max_y {
            let row = y as usize * stride;
            let s = row + left as usize * CH;
            let e = row + max_x as usize * CH;
            if e <= self.backing.len() {
                for px in self.backing[s..e].as_chunks_mut::<CH>().0 {
                    px[..3].copy_from_slice(&rgb);
                }
            }
        }
    }

    /// A rectangle of gray `value`: every backing byte in the span is `value`.
    pub fn fill_rect(&mut self, top: u32, left: u32, width: u32, height: u32, value: u8) {
        if left >= self.var.xres {
            return;
        }
        let stride = self.var.xres as usize * CH;
        let max_y = top.saturating_add(height).min(self.var.yres);
        let max_x = left.saturating_add(width).min(self.var.xres);
        for y in top..max_y {
            let row = y as usize * stride;
            let s = row + left as usize * CH;
            let e = row + max_x as usize * CH;
            if e <= self.backing.len() {
                self.backing[s..e].fill(value);
            }
        }
    }

    /// Drains the X event queue, returning whether the server asked for a redraw.
    /// `EXPOSURE` events and `put_image` errors both arrive here.
    pub fn pump_events(&mut self) -> bool {
        let Some(surface) = self.surface.as_mut() else {
            return false;
        };
        let mut needs_repaint = false;
        while let Ok(Some(event)) = surface.conn.poll_for_event() {
            match event {
                Event::Expose(_) => needs_repaint = true,
                Event::Error(e) => {
                    // An error counts as damage: the panel holds a stale frame.
                    eprintln!("x11: WARNING request failed: {e:?}");
                    needs_repaint = true;
                }
                _ => {}
            }
        }
        needs_repaint
    }

    /// Presents the rows of `rect`, converting the backing to the wire pixel
    /// format per band. `waveform` is ignored.
    pub fn send_update(&mut self, rect: MxcfbRect, _waveform: u32) -> Result<u32> {
        let Some(surface) = self.surface.as_mut() else {
            return Ok(0);
        };
        let bpp = surface.bytes_per_pixel;
        let xres = self.var.xres as usize;
        let bk_stride = xres * CH; // backing bytes per scanline (RGB)
        let wire_stride = xres * bpp; // wire bytes per scanline
        let width = self.var.xres as u16;
        let top = rect.top.min(self.var.yres);
        let bottom = rect.top.saturating_add(rect.height).min(self.var.yres);
        let max_rows = (surface.max_req_bytes.saturating_sub(64) / wire_stride.max(1)).max(1);
        let [rb, gb, bb] = surface.chan;

        // Reused across bands. Pad bytes stay at the 0xFF fill.
        let mut wire: Vec<u8> = Vec::new();

        let mut y = top;
        while y < bottom {
            let h = ((bottom - y) as usize).min(max_rows);
            let s = y as usize * bk_stride;
            let e = s + h * bk_stride;
            let band = &self.backing[s..e];
            let px = h * xres;

            wire.clear();
            wire.resize(px * bpp, 0xFF);
            // `bpp == 1` collapses to one luma byte; wider scatters R/G/B to
            // [`Framebuffer::chan`].
            let (triples, _) = band.as_chunks::<CH>();
            let pairs = wire.chunks_exact_mut(bpp).zip(triples);
            if bpp == 1 {
                for (w, src) in pairs {
                    w[0] = luma(src[0], src[1], src[2]);
                }
            } else {
                for (w, src) in pairs {
                    w[rb] = src[0];
                    w[gb] = src[1];
                    w[bb] = src[2];
                }
            }

            surface
                .conn
                .put_image(
                    ImageFormat::Z_PIXMAP,
                    surface.win,
                    surface.gc,
                    width,
                    h as u16,
                    0,
                    y as i16,
                    0,
                    surface.depth,
                    &wire,
                )
                .context("put_image")?;
            y += h as u32;
        }
        // A round-trip, past `flush`: the reply marks the batch processed and
        // delivers any error it raised.
        surface
            .conn
            .get_input_focus()
            .context("sync round-trip")?
            .reply()
            .context("sync reply")?;
        Ok(0)
    }

    /// `backing`, cloned.
    pub fn backing_snapshot(&self) -> Vec<u8> {
        self.backing.clone()
    }

    /// `snap` into `backing`, when the two are the same length.
    /// [`Framebuffer::send_update`] presents it.
    pub fn restore_backing(&mut self, snap: Vec<u8>) {
        if snap.len() == self.backing.len() {
            self.backing = snap;
        }
    }

    /// `backing` encoded as a PNG at `path`, unrotated.
    pub fn capture_png(&self, path: &std::path::Path) -> Result<()> {
        let img = image::RgbImage::from_raw(self.var.xres, self.var.yres, self.backing.clone())
            .context("backing buffer size != xres*yres*CH")?;
        img.save(path)
            .with_context(|| format!("write screenshot {}", path.display()))?;
        Ok(())
    }
}

impl Drop for Framebuffer {
    fn drop(&mut self) {
        // The WM recomposites the screen under a destroyed window.
        let Some(surface) = &self.surface else {
            return;
        };
        let _ = surface.conn.destroy_window(surface.win);
        let _ = surface.conn.flush();
    }
}
