//! The Kindle two-corner screenshot gesture, recognized in [`super::touch`] and
//! captured here: an exclusive `EVIOCGRAB` starves the stock recognizer.
//! `Framebuffer`'s backing holds the screen, leaving an encode and a flash.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};

use super::fb::{Framebuffer, MxcfbRect, WAVEFORM_MODE_GC16};

/// Where screenshots land — the same directory stock Kindle screenshots use.
const SCREENSHOT_DIR: &str = "/mnt/us/screenshots";

/// How long the white flash is held.
const FLASH_MS: u64 = 120;

/// The screen to a timestamped PNG, a white flash, then the screen restored.
/// Returns the written path, unrotated — see [`Framebuffer::capture_png`]. The
/// flash and restore run past a failed encode.
pub fn capture(fb: &mut Framebuffer) -> Result<PathBuf> {
    let dir = Path::new(SCREENSHOT_DIR);
    std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let path = dir.join(format!("screenshot_{secs}.png"));

    // Taken before anything touches the backing: `capture_png` reads the live
    // screen, and this exact buffer restores after the flash.
    let snap = fb.backing_snapshot();
    let cap = fb.capture_png(&path);

    // White flash → brief hold → restore. send_update widens to full rows, and
    // a full-screen rect repaints everything. GC16 is the clean full refresh.
    let (w, h) = (fb.var.xres, fb.var.yres);
    let full = MxcfbRect {
        top: 0,
        left: 0,
        width: w,
        height: h,
    };
    fb.fill_rect(0, 0, w, h, 0xFF);
    let _ = fb.send_update(full, WAVEFORM_MODE_GC16);
    std::thread::sleep(Duration::from_millis(FLASH_MS));
    fb.restore_backing(snap);
    let _ = fb.send_update(full, WAVEFORM_MODE_GC16);

    cap.map(|()| path)
}
