//! Display surface: a WM-managed fullscreen X11 window. `backing` holds packed
//! RGB ([`CH`] bytes/pixel, white=255) and reaches the server at identity
//! through [`Framebuffer::send_update`].

use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use x11rb::connection::Connection;
// `maximum_request_bytes` (BIG-REQUESTS-aware) lives on this trait.
use x11rb::connection::RequestConnection as _;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ConnectionExt, CreateGCAux, CreateWindowAux, EventMask, Gcontext, ImageFormat,
    ImageOrder, KeyButMask, PropMode, Screen, Visibility, Window, WindowClass,
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

/// Rec. 601 luma, the depth-8 wire collapse. 77, 150 and 29 sum to 256, and
/// `>> 8` divides by it.
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
    // `mask` sits in one byte of the native-endian pixel, at `trailing_zeros / 8`.
    // `msb` mirrors that index across `bpp`.
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

/// `events` folded into a [`Pump`] against `covered` and the `size` drawn. The
/// last event wins, an unchanged state answers `None`, and `VisibilityNotify`
/// sets `covered` either way where a [`SCREENSAVER_MESSAGE`] sets it true alone.
fn fold(events: &[Event], screensaver: Atom, covered: bool, size: (u32, u32)) -> Pump {
    let mut pump = Pump::default();
    let mut folded = covered;
    for event in events {
        match event {
            Event::Expose(_) => pump.repaint = true,
            // `ConfigureNotify` carries the size the window is laid out at.
            Event::ConfigureNotify(ev) => {
                let laid = (u32::from(ev.width), u32::from(ev.height));
                pump.resized = (laid != size && laid.0 > 0 && laid.1 > 0).then_some(laid);
            }
            // `FULLY_OBSCURED` is the whole panel; anything less is a share of it.
            Event::VisibilityNotify(ev) => folded = ev.state == Visibility::FULLY_OBSCURED,
            // `data8[0]` of 1 covers; a 0 leaves `folded` alone.
            Event::ClientMessage(ev) if screensaver != 0 && ev.type_ == screensaver => {
                folded |= ev.data.as_data8()[0] != 0;
            }
            Event::Error(e) => {
                // A `put_image` the server rejects arrives here.
                eprintln!("x11: WARNING request failed: {e:?}");
                pump.repaint = true;
            }
            _ => {}
        }
    }
    if folded != covered {
        pump.covered = Some(folded);
    }
    pump
}

/// The keysym one keycode stands for under `state`.
///
/// The mapping is asked for a keycode at a time rather than cached, because
/// the on-screen keyboard writes a keysym the map does not carry into a
/// scratch keycode of its own just before sending it. It rotates through
/// thirty-five of them, so the answer is still the right one by the time the
/// event is read.
fn keysym_of(conn: &RustConnection, keycode: u8, state: u16) -> Option<u32> {
    let reply = conn.get_keyboard_mapping(keycode, 1).ok()?.reply().ok()?;
    // A scratch keycode carries one keysym and no shifted form, so the shifted
    // column falls back to the plain one.
    let shifted = usize::from(state & u16::from(KeyButMask::SHIFT) != 0);
    let at = shifted.min(reply.keysyms.len().saturating_sub(1));
    let keysym = match reply.keysyms.get(at).copied().unwrap_or(0) {
        0 => reply.keysyms.first().copied().unwrap_or(0),
        keysym => keysym,
    };
    (keysym != 0).then_some(keysym)
}

/// `pixel_bytes` rounded up to a multiple of `pad`, the bytes `put_image` takes
/// per ZPixmap scanline.
fn wire_stride(pixel_bytes: usize, pad: usize) -> usize {
    let pad = pad.max(1);
    pixel_bytes.div_ceil(pad) * pad
}

/// `band`'s packed-RGB rows, `xres` wide, into `wire`: one `bpp`-byte wire
/// pixel each, rows `wire_stride` apart with the pad left at 0xFF. `bpp == 1`
/// collapses to one luma byte; wider scatters R/G/B to `chan`.
fn pack_band(
    wire: &mut Vec<u8>,
    band: &[u8],
    xres: usize,
    bpp: usize,
    wire_stride: usize,
    chan: [usize; 3],
) {
    wire.clear();
    let bk_stride = xres * CH;
    if bk_stride == 0 || wire_stride == 0 {
        return;
    }
    let pixel_bytes = xres * bpp;
    let [rb, gb, bb] = chan;
    wire.resize(band.len() / bk_stride * wire_stride, 0xFF);
    for (out, row) in wire
        .chunks_exact_mut(wire_stride)
        .zip(band.chunks_exact(bk_stride))
    {
        let (triples, _) = row.as_chunks::<CH>();
        let pairs = out[..pixel_bytes].chunks_exact_mut(bpp).zip(triples);
        if bpp == 1 {
            for (w, rgb) in pairs {
                w[0] = luma(rgb[0], rgb[1], rgb[2]);
            }
        } else {
            for (w, rgb) in pairs {
                w[rb] = rgb[0];
                w[gb] = rgb[1];
                w[bb] = rgb[2];
            }
        }
    }
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
    /// Wire bytes per scanline, from [`wire_stride`] and `scanline_pad`.
    wire_stride: usize,
    /// The format's `scanline_pad`, in bytes: [`wire_stride`]'s second half.
    scanline_pad: usize,
    /// R, G, B offsets within a `bytes_per_pixel`-wide wire pixel.
    chan: [usize; 3],
    /// Per-`PutImage` byte budget (server max request length minus header slack).
    max_req_bytes: usize,
    /// The interned [`SCREENSAVER_MESSAGE`] atom, or 0.
    screensaver: Atom,
    /// Whether another window covers this one. [`fold`] reports its changes.
    covered: bool,
}

/// What [`Framebuffer::pump_events`] answers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Pump {
    /// Set by `Expose` and by `Event::Error`.
    pub repaint: bool,
    /// `Some(true)` covered, `Some(false)` uncovered, `None` unchanged.
    pub covered: Option<bool>,
    /// A `ConfigureNotify` size differing from the one being drawn.
    pub resized: Option<(u32, u32)>,
    /// What was typed into this window, in order, as keysyms.
    pub typed: Vec<u32>,
}

/// How long [`Framebuffer::open`] waits for `MapNotify`.
const LAYOUT_WAIT: Duration = Duration::from_millis(500);

/// Atom naming the message a `CMS~E:ss` `WM_NAME` subscribes to.
const SCREENSAVER_MESSAGE: &[u8] = b"lab126_screen_saver";

pub struct Framebuffer {
    /// Where a frame is presented. `None` under [`Framebuffer::offscreen`].
    surface: Option<Surface>,
    pub var: Var,
    /// Packed RGB ([`CH`] bytes/pixel), stride `xres * CH`.
    backing: Vec<u8>,
}

impl Framebuffer {
    /// Connect to the X server (`$DISPLAY`), create + map a fullscreen window.
    pub fn open() -> Result<Self> {
        let (conn, screen_num) = x11rb::connect(None).context("connect to X ($DISPLAY)")?;
        let screen = conn.setup().roots[screen_num].clone();
        // The root size, until `get_geometry` answers below.
        let mut xres = screen.width_in_pixels as u32;
        let mut yres = screen.height_in_pixels as u32;
        let depth = screen.root_depth;
        let format = conn
            .setup()
            .pixmap_formats
            .iter()
            .find(|f| f.depth == depth);
        // Depth 8 → 1; depth 24 and 32 → 4.
        let bytes_per_pixel = format
            .map(|f| (f.bits_per_pixel as usize / 8).max(1))
            .unwrap_or(1);
        // `scanline_pad` is 32 bits on every standard format.
        let scanline_pad = format.map(|f| f.scanline_pad as usize / 8).unwrap_or(4);
        // `[2, 1, 0]` is BGRX little-endian, the lab126 depth-24 layout.
        let chan = wire_channels(&conn, &screen, bytes_per_pixel).unwrap_or([2, 1, 0]);
        // `depths` names every depth the server offers: a `depth` of 8 with a 24
        // among them is a colour panel painted through a grey visual.
        let depths: Vec<String> = screen
            .allowed_depths
            .iter()
            .filter(|d| !d.visuals.is_empty())
            .map(|d| d.depth.to_string())
            .collect();
        eprintln!(
            "fb: root {xres}x{yres} depth={depth} bytes_per_pixel={bytes_per_pixel} \
             scanline_pad={scanline_pad} \
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
            // `CreateWindowAux` sets no `backing_store`.
            &CreateWindowAux::new()
                .background_pixel(screen.white_pixel)
                // `VISIBILITY_CHANGE` reports a window put over this one, and
                // `STRUCTURE_NOTIFY` `MapNotify` and `ConfigureNotify`.
                //
                // `KEY_PRESS` is what the on-screen keyboard types into: it
                // sends a synthetic `KeyPress` to whichever window holds the
                // input focus, and a window selecting no key mask never gets
                // one. `KEY_RELEASE` follows each press and is dropped.
                .event_mask(
                    EventMask::EXPOSURE
                        | EventMask::VISIBILITY_CHANGE
                        | EventMask::STRUCTURE_NOTIFY
                        | EventMask::KEY_PRESS
                        | EventMask::KEY_RELEASE,
                ),
        )
        .context("create_window")?;

        // `WM_NAME` carries the lab126 WM's layout spec. `CMS~E:ss` subscribes
        // to [`SCREENSAVER_MESSAGE`].
        let name = b"L:A_N:application_ID:com.readinglog.stats_PC:N_O:U_CMS~E:ss";
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

        // `MapNotify` marks the layout done.
        let deadline = Instant::now() + LAYOUT_WAIT;
        let mut mapped = false;
        while !mapped && Instant::now() < deadline {
            while let Ok(Some(event)) = conn.poll_for_event() {
                mapped |= matches!(event, Event::MapNotify(_));
            }
            if !mapped {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        eprintln!("fb: mapped={mapped}");

        // `get_geometry` against the requested `xres` / `yres`.
        match conn
            .get_geometry(win)
            .map_err(|e| e.to_string())
            .and_then(|c| c.reply().map_err(|e| e.to_string()))
        {
            Ok(g) if u32::from(g.width) != xres || u32::from(g.height) != yres => {
                // `get_geometry` outranks the root read above.
                eprintln!(
                    "fb: window is {}x{}, root read {xres}x{yres} — drawing {}x{}",
                    g.width, g.height, g.width, g.height
                );
                (xres, yres) = (u32::from(g.width), u32::from(g.height));
            }
            Ok(_) => eprintln!("fb: window geometry matches root ({xres}x{yres})"),
            Err(e) => eprintln!("fb: could not read window geometry: {e}"),
        }

        // `wire_stride` follows `xres`, which `get_geometry` above sets.
        let wire_stride = wire_stride(xres as usize * bytes_per_pixel, scanline_pad);
        eprintln!("fb: drawing {xres}x{yres}, wire_stride={wire_stride}");

        // `maximum_request_bytes` is the post-BIG-REQUESTS limit (~16 MB), past
        // `setup().maximum_request_length`. A 1860×2480 frame is 4.6 MB: one
        // request, uncapped.
        let max_req_bytes = conn.maximum_request_bytes().max(4096);
        eprintln!(
            "fb: max request {} bytes ({} rows/band at {} bpp)",
            max_req_bytes,
            max_req_bytes / wire_stride.max(1),
            bytes_per_pixel
        );

        // `only_if_exists` false creates the atom. [`fold`] matches no
        // `ClientMessage` against the 0 an unanswered reply leaves.
        let screensaver = conn
            .intern_atom(false, SCREENSAVER_MESSAGE)
            .map_err(|e| e.to_string())
            .and_then(|c| c.reply().map_err(|e| e.to_string()))
            .map(|r| r.atom)
            .unwrap_or_else(|e| {
                eprintln!("fb: could not intern lab126_screen_saver: {e}");
                0
            });

        let backing = vec![0xFFu8; xres as usize * yres as usize * CH];

        Ok(Self {
            surface: Some(Surface {
                conn,
                win,
                gc,
                depth,
                bytes_per_pixel,
                wire_stride,
                scanline_pad,
                chan,
                max_req_bytes,
                screensaver,
                covered: false,
            }),
            var: Var { xres, yres },
            backing,
        })
    }

    /// Whether a server stands behind this framebuffer. `false` under
    /// [`Framebuffer::offscreen`], where nothing is presented and no other X
    /// client can be spoken to either.
    pub fn on_screen(&self) -> bool {
        self.surface.is_some()
    }

    /// The X connection's own descriptor, for a `poll(2)` beside the input
    /// devices. Readable means an event is on the wire, which
    /// [`Framebuffer::pump_events`] then drains.
    pub fn raw_fd(&self) -> Option<std::os::fd::RawFd> {
        use std::os::fd::AsRawFd;
        self.surface
            .as_ref()
            .map(|surface| surface.conn.stream().as_raw_fd())
    }

    /// A white `xres` by `yres` surface with no server behind it.
    /// [`Framebuffer::capture_png`] takes the frame off it and
    /// [`Framebuffer::send_update`] presents nothing.
    pub fn offscreen(xres: u32, yres: u32) -> Self {
        Self {
            surface: None,
            var: Var { xres, yres },
            backing: vec![0xFFu8; xres as usize * yres as usize * CH],
        }
    }

    /// A gray pixel (0=black, 255=white) stored as `(v,v,v)`, no-op out of range.
    #[inline]
    pub fn put_pixel(&mut self, x: i32, y: i32, value: u8) {
        self.put_pixel_rgb(x, y, [value, value, value]);
    }

    /// An `[r, g, b]` pixel, for cover art, no-op out of range.
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

    /// Drains the X event queue through [`fold`], keeping `Surface::covered` and
    /// resizing on a `Pump::resized`.
    pub fn pump_events(&mut self) -> Pump {
        let size = (self.var.xres, self.var.yres);
        let Some(surface) = self.surface.as_mut() else {
            return Pump::default();
        };
        let mut events = Vec::new();
        while let Ok(Some(event)) = surface.conn.poll_for_event() {
            events.push(event);
        }
        let mut pump = fold(&events, surface.screensaver, surface.covered, size);
        for event in &events {
            if let Event::KeyPress(ev) = event
                && let Some(keysym) = keysym_of(&surface.conn, ev.detail, ev.state.into())
            {
                pump.typed.push(keysym);
            }
        }
        if let Some(covered) = pump.covered {
            surface.covered = covered;
        }
        if let Some((w, h)) = pump.resized {
            self.resize(w, h);
        }
        pump
    }

    /// Draws `w` by `h`: `var`, `backing` and `Surface::wire_stride` follow, and
    /// `backing` is white.
    fn resize(&mut self, w: u32, h: u32) {
        eprintln!(
            "fb: laid out {w}x{h}, was {}x{}",
            self.var.xres, self.var.yres
        );
        self.var = Var { xres: w, yres: h };
        self.backing = vec![0xFFu8; w as usize * h as usize * CH];
        if let Some(surface) = self.surface.as_mut() {
            surface.wire_stride =
                wire_stride(w as usize * surface.bytes_per_pixel, surface.scanline_pad);
        }
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
        let wire_stride = surface.wire_stride; // wire bytes per scanline, padded
        let width = self.var.xres as u16;
        let top = rect.top.min(self.var.yres);
        let bottom = rect.top.saturating_add(rect.height).min(self.var.yres);
        let max_rows = (surface.max_req_bytes.saturating_sub(64) / wire_stride.max(1)).max(1);

        // `wire` is reused across bands.
        let mut wire: Vec<u8> = Vec::new();

        let mut y = top;
        while y < bottom {
            let h = ((bottom - y) as usize).min(max_rows);
            let s = y as usize * bk_stride;
            let e = s + h * bk_stride;
            pack_band(
                &mut wire,
                &self.backing[s..e],
                xres,
                bpp,
                wire_stride,
                surface.chan,
            );

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
        // `get_input_focus` round-trips past `flush`; its `reply` marks the batch
        // processed and delivers any error the batch raised.
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
        // `destroy_window` hands the screen back to the WM.
        let Some(surface) = &self.surface else {
            return;
        };
        let _ = surface.conn.destroy_window(surface.win);
        let _ = surface.conn.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::{CH, Framebuffer, Pump, fold, pack_band, wire_stride};
    use x11rb::protocol::Event;
    use x11rb::protocol::xproto::{
        ClientMessageData, ClientMessageEvent, ConfigureNotifyEvent, ExposeEvent, Visibility,
        VisibilityNotifyEvent,
    };

    /// The atom under test. Any non-zero value stands for `lab126_screen_saver`.
    const SS: u32 = 42;

    /// The size being drawn, as `pump_events` passes it.
    const SIZE: (u32, u32) = (1264, 1680);

    /// A `ConfigureNotify` laying the window out at `w` by `h`.
    fn configure(w: u16, h: u16) -> Event {
        Event::ConfigureNotify(ConfigureNotifyEvent {
            response_type: 22,
            sequence: 0,
            event: 1,
            window: 1,
            above_sibling: 0,
            x: 0,
            y: 0,
            width: w,
            height: h,
            border_width: 0,
            override_redirect: false,
        })
    }

    fn expose() -> Event {
        Event::Expose(ExposeEvent {
            response_type: 12,
            sequence: 0,
            window: 1,
            x: 0,
            y: 0,
            width: 1,
            height: 1,
            count: 0,
        })
    }

    fn visibility(state: Visibility) -> Event {
        Event::VisibilityNotify(VisibilityNotifyEvent {
            response_type: 15,
            sequence: 0,
            window: 1,
            state,
        })
    }

    /// A `ClientMessage` under `type_`, carrying `up` in `data8[0]`.
    fn message(type_: u32, up: u8) -> Event {
        let mut data = [0u8; 20];
        data[0] = up;
        Event::ClientMessage(ClientMessageEvent {
            response_type: 33,
            format: 8,
            sequence: 0,
            window: 1,
            type_,
            data: ClientMessageData::from(data),
        })
    }

    /// An empty queue, and an `Expose` alone, leave `Pump::covered` at `None`.
    #[test]
    fn nothing_drained_changes_nothing() {
        assert_eq!(fold(&[], SS, false, SIZE), Pump::default());
        assert_eq!(fold(&[expose()], SS, false, SIZE).covered, None);
    }

    /// `visibility` and `message` each set `Pump::covered` on their own.
    #[test]
    fn either_signal_covers_the_window() {
        for events in [
            vec![visibility(Visibility::FULLY_OBSCURED)],
            vec![message(SS, 1)],
        ] {
            assert_eq!(fold(&events, SS, false, SIZE).covered, Some(true));
        }
    }

    /// `UNOBSCURED` and `PARTIALLY_OBSCURED` both fold to a `covered` of false.
    #[test]
    fn a_partly_covered_window_keeps_the_screen() {
        for state in [Visibility::UNOBSCURED, Visibility::PARTIALLY_OBSCURED] {
            assert_eq!(
                fold(&[visibility(state)], SS, true, SIZE).covered,
                Some(false)
            );
            assert_eq!(fold(&[visibility(state)], SS, false, SIZE).covered, None);
        }
    }

    /// An event repeating the `covered` passed in folds to `None`.
    #[test]
    fn an_unchanged_state_is_not_reported() {
        let covered = [visibility(Visibility::FULLY_OBSCURED), message(SS, 1)];
        assert_eq!(fold(&covered, SS, true, SIZE).covered, None);
        assert_eq!(fold(&[message(SS, 0)], SS, false, SIZE).covered, None);
    }

    /// The last event of `events` sets `Pump::covered`.
    #[test]
    fn the_last_event_of_a_drain_wins() {
        let events = [
            visibility(Visibility::FULLY_OBSCURED),
            expose(),
            visibility(Visibility::UNOBSCURED),
        ];
        let pump = fold(&events, SS, false, SIZE);
        assert_eq!(pump.covered, None, "it ended where it started");
        assert!(pump.repaint);

        let events = [message(SS, 1), message(SS, 0), message(SS, 1)];
        assert_eq!(fold(&events, SS, false, SIZE).covered, Some(true));
    }

    /// A `type_` other than `screensaver`, and a `screensaver` of 0, fold to `None`.
    #[test]
    fn a_message_under_another_atom_is_ignored() {
        assert_eq!(fold(&[message(SS + 1, 1)], SS, false, SIZE).covered, None);
        assert_eq!(fold(&[message(0, 1)], 0, false, SIZE).covered, None);
    }

    /// `backing` holds `xres * yres * CH` across a resize. `send_update` slices
    /// it by `var`, and a short `backing` panics there.
    #[test]
    fn a_resize_keeps_the_backing_in_step() {
        let mut fb = Framebuffer::offscreen(1680, 1264);
        for (w, h) in [(1264u32, 1680u32), (1680, 1264), (600, 800)] {
            fb.resize(w, h);
            assert_eq!((fb.var.xres, fb.var.yres), (w, h));
            assert_eq!(fb.backing.len(), w as usize * h as usize * CH);
        }
    }

    /// A layout differing from `size` is reported, and one matching it is not.
    #[test]
    fn only_a_new_layout_is_reported() {
        assert_eq!(
            fold(&[configure(1680, 1264)], SS, false, SIZE).resized,
            Some((1680, 1264))
        );
        assert_eq!(
            fold(&[configure(1264, 1680)], SS, false, SIZE).resized,
            None
        );
        assert_eq!(fold(&[configure(0, 0)], SS, false, SIZE).resized, None);
    }

    /// The last `ConfigureNotify` of a drain sets `Pump::resized`, and a drain
    /// ending back at `size` reports nothing.
    #[test]
    fn the_last_layout_of_a_drain_wins() {
        let events = [configure(1680, 1264), configure(1264, 1680)];
        assert_eq!(fold(&events, SS, false, SIZE).resized, None);
        let events = [configure(1264, 1680), configure(1680, 1264)];
        assert_eq!(fold(&events, SS, false, SIZE).resized, Some((1680, 1264)));
    }

    /// A `data8[0]` of 0 leaves a covered window covered: `UNOBSCURED` alone
    /// uncovers it.
    #[test]
    fn only_visibility_uncovers() {
        assert_eq!(fold(&[message(SS, 0)], SS, true, SIZE).covered, None);
        let events = [message(SS, 1), message(SS, 0)];
        assert_eq!(fold(&events, SS, false, SIZE).covered, Some(true));
        let events = [message(SS, 0), visibility(Visibility::UNOBSCURED)];
        assert_eq!(fold(&events, SS, true, SIZE).covered, Some(false));
    }

    /// `wire_stride` over the shipped panel widths at both `bytes_per_pixel` a
    /// Kindle X server offers, under a 4-byte pad.
    #[test]
    fn a_scanline_reaches_the_pad() {
        // 758 is the one width that is not itself a multiple of the pad.
        for (width, bpp, stride) in [
            (600, 1, 600),
            (758, 1, 760),
            (1072, 1, 1072),
            (1236, 1, 1236),
            (1264, 1, 1264),
            (1860, 1, 1860),
            (758, 4, 3032),
            (1272, 4, 5088),
            (1860, 4, 7440),
        ] {
            assert_eq!(wire_stride(width * bpp, 4), stride, "{width} at {bpp} bpp");
            assert_eq!(wire_stride(width * bpp, 4) % 4, 0);
        }
    }

    /// `pad` of 0 answers `pixel_bytes`.
    #[test]
    fn a_pad_of_zero_is_one_byte() {
        assert_eq!(wire_stride(758, 0), 758);
    }

    /// `pack_band` writes four bytes a row for three pixels at 1 bpp, the
    /// fourth left at the 0xFF pad.
    #[test]
    fn a_short_row_keeps_its_pad() {
        #[rustfmt::skip]
        let band: Vec<u8> = vec![
            0, 0, 0,  255, 255, 255,  128, 128, 128,
            255, 0, 0,  0, 255, 0,  0, 0, 255,
        ];
        let mut wire = Vec::new();
        pack_band(&mut wire, &band, 3, 1, 4, [2, 1, 0]);
        assert_eq!(wire, [0, 255, 128, 0xFF, 76, 149, 28, 0xFF]);
    }

    /// `pack_band` scatters to `chan` at 4 bpp, leaving the fourth byte at 0xFF.
    #[test]
    fn a_wide_pixel_scatters_to_its_channels() {
        let mut wire = Vec::new();
        pack_band(&mut wire, &[10, 20, 30], 1, 4, 4, [2, 1, 0]);
        assert_eq!(wire, [30, 20, 10, 0xFF]);
    }

    /// `pack_band` answers an empty `wire` for an `xres` of 0.
    #[test]
    fn no_columns_packs_nothing() {
        let mut wire = vec![1, 2, 3];
        pack_band(&mut wire, &[], 0, 1, 4, [2, 1, 0]);
        assert!(wire.is_empty());
    }
}
