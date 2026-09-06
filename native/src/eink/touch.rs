//! evdev touchscreen reader. Multi-touch protocol B: `Down` and `Up` flush at
//! `SYN_REPORT`; a move between them updates `cur_x/cur_y`. Events are 16 raw
//! bytes. `pick_from_devices` scores `/proc/bus/input/devices` nodes.

use std::fs::{File, OpenOptions};
use std::io::Read;
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};

use crate::orientation::Orientation;

// evdev type/code constants (linux/input-event-codes.h). Stable.
const EV_SYN: u16 = 0x00;
const SYN_REPORT: u16 = 0x00;
const EV_ABS: u16 = 0x03;
const ABS_MT_POSITION_X: u16 = 0x35;
const ABS_MT_POSITION_Y: u16 = 0x36;
const ABS_MT_TRACKING_ID: u16 = 0x39;
// Protocol-B contact selector: subsequent ABS_MT_* events address this slot.
// Sticky — the kernel only emits it when the active contact changes.
const ABS_MT_SLOT: u16 = 0x2f;
// Capability bits identifying a touchscreen in /proc/bus/input/devices.
// EV_ABS in the `B: EV=` bitmap → reports absolute axes; INPUT_PROP_DIRECT in
// `B: PROP=` → finger maps 1:1 to a screen point (touchscreen, not touchpad).
const EV_ABS_BIT: u32 = 3;
const INPUT_PROP_DIRECT: u32 = 1;

const EVENT_BYTES: usize = 16;

/// Side of the screenshot gesture's corner zones, in user-visible pixels.
const SCREENSHOT_CORNER_PX: u32 = 180;

/// Boundary touch events reaching the main loop: `Down` on a landing contact,
/// `Up` on the lift. A move between the two updates `cur_x/cur_y` silently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TouchEvent {
    Down {
        x: u32,
        y: u32,
    },
    Up {
        x: u32,
        y: u32,
    },
    /// Two contacts in opposite screen corners, either diagonal: the Kindle
    /// screenshot gesture, recognized here past an `EVIOCGRAB`ped framework.
    /// Carries no coords.
    Screenshot,
}

/// Horizontal-swipe direction over one stroke's start→end vector, from
/// [`classify_swipe`]. [`crate::app::App`] maps these to page turns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwipeDir {
    /// Right-to-left drag → next page (the current page slides off to the left).
    Next,
    /// Left-to-right drag → previous page.
    Prev,
}

// _IOW('E', 0x90, int) = 0x40044590. Call sites cast with `as _`:
// `libc::ioctl`'s request arg is `c_int` on armv7 Linux and `c_ulong` on the
// host, and the value fits both.
const EVIOCGRAB: libc::c_int = 0x40044590;

pub struct Touch {
    file: File,
    cur_x: i32,
    cur_y: i32,
    /// Set on `ABS_MT_TRACKING_ID >= 0`, cleared as a `Down` flushes.
    down_pending: bool,
    up_pending: bool,
    /// `ABS_MT_SLOT`: slot 0 drives `Down`/`Up`, slot 1 is the secondary.
    cur_slot: usize,
    slot0_active: bool,
    slot1_active: bool,
    slot1_x: i32,
    slot1_y: i32,
    /// Latches the gesture to one firing per two-contact episode.
    screenshot_latched: bool,
    /// After a screenshot fires, swallow the trailing slot-0 `Up`.
    suppress_next_up: bool,
    /// Once grabbed, no other reader sees events from this device.
    grabbed: bool,
    /// The orientation the framebuffer was opened with.
    orientation: Orientation,
    fb_xres: u32,
    fb_yres: u32,
}

impl Touch {
    pub fn open(orientation: Orientation, fb_xres: u32, fb_yres: u32) -> Result<Self> {
        let path = find_touch_device()?;
        // `O_NONBLOCK` for the `poll(2)` multiplexer in `crate::eink::input`.
        // A blocking read on an fd readable mid-stroke starves the
        // bezel-button fd.
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(&path)
            .with_context(|| format!("open {}", path.display()))?;
        // The kernel treats the arg as a "non-NULL = grab, NULL = ungrab"
        // boolean (see drivers/input/evdev.c). Pass 1.
        let grab_res = unsafe { libc::ioctl(file.as_raw_fd(), EVIOCGRAB as _, 1) };
        let grabbed = grab_res == 0;
        // A failed grab leaves the device readable and non-exclusive.
        if grabbed {
            eprintln!("touch: EVIOCGRAB ok — exclusive");
        } else {
            let err = std::io::Error::last_os_error();
            eprintln!(
                "touch: WARNING EVIOCGRAB failed on {} ({err}) — input is NOT exclusive; \
                 the framework will also act on these touches",
                path.display()
            );
        }
        Ok(Self {
            file,
            cur_x: 0,
            cur_y: 0,
            down_pending: false,
            up_pending: false,
            cur_slot: 0,
            slot0_active: false,
            slot1_active: false,
            slot1_x: 0,
            slot1_y: 0,
            screenshot_latched: false,
            suppress_next_up: false,
            grabbed,
            orientation,
            fb_xres,
            fb_yres,
        })
    }

    /// Raw fd for `poll(2)` multiplexing with the button device (see
    /// [`crate::eink::input`]). The `Touch` keeps ownership; callers only poll
    /// on it, then call [`Touch::next_event`] when it's readable.
    pub fn raw_fd(&self) -> RawFd {
        self.file.as_raw_fd()
    }

    /// Sets the orientation transforming raw coords. The X server rotates the
    /// display and raw evdev coords stay panel-fixed.
    pub fn set_orientation(&mut self, orientation: Orientation) {
        self.orientation = orientation;
    }

    /// The available events drained. `Some` on a completed `Down`/`Up` boundary
    /// in orientation-corrected coords, `None` on a move-only or partial
    /// packet, which the caller re-polls. Boundary state lives on `self`.
    pub fn next_event(&mut self) -> Result<Option<TouchEvent>> {
        let mut buf = [0u8; EVENT_BYTES];
        loop {
            match self.file.read(&mut buf) {
                Ok(EVENT_BYTES) => {}
                // evdev hands back whole 16-byte records: a short read is an
                // empty buffer.
                Ok(_) => return Ok(None),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return Ok(None),
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e).context("read /dev/input/eventN"),
            }

            // Bytes 0..8 are the timestamp.
            let type_ = u16::from_ne_bytes([buf[8], buf[9]]);
            let code = u16::from_ne_bytes([buf[10], buf[11]]);
            let value = i32::from_ne_bytes([buf[12], buf[13], buf[14], buf[15]]);

            match (type_, code) {
                (EV_SYN, SYN_REPORT) => {
                    // Two contacts in opposite corners: the screenshot
                    // gesture, checked ahead of the single-touch flush and
                    // latched to one firing per two-finger episode.
                    if self.slot0_active && self.slot1_active {
                        if !self.screenshot_latched {
                            let (ax, ay) = self.transform_xy(self.cur_x, self.cur_y);
                            let (bx, by) = self.transform_xy(self.slot1_x, self.slot1_y);
                            if opposite_corners(ax, ay, bx, by, self.fb_xres, self.fb_yres) {
                                self.screenshot_latched = true;
                                // The gesture, not a tap: drop any queued slot-0
                                // boundary and swallow the eventual lift.
                                self.down_pending = false;
                                self.up_pending = false;
                                self.suppress_next_up = true;
                                return Ok(Some(TouchEvent::Screenshot));
                            }
                        }
                    } else {
                        self.screenshot_latched = false;
                    }

                    // Packet boundary, flushing the accumulated pending state.
                    // `up_pending` wins a packet carrying both.
                    if self.up_pending {
                        self.up_pending = false;
                        self.down_pending = false;
                        if self.suppress_next_up {
                            // Post-screenshot lift: drained, not surfaced.
                            self.suppress_next_up = false;
                        } else {
                            let (x, y) = self.transform_coords();
                            return Ok(Some(TouchEvent::Up { x, y }));
                        }
                    } else if self.down_pending {
                        self.down_pending = false;
                        let (x, y) = self.transform_coords();
                        return Ok(Some(TouchEvent::Down { x, y }));
                    }
                    // Move-only packet — keep draining.
                }
                (EV_ABS, ABS_MT_SLOT) => self.cur_slot = value.max(0) as usize,
                (EV_ABS, ABS_MT_TRACKING_ID) => match self.cur_slot {
                    // Slot 0 drives the single-touch Down/Up boundaries (taps,
                    // long-press). Slot 1 only feeds the two-finger gesture.
                    0 => {
                        if value >= 0 {
                            self.slot0_active = true;
                            self.down_pending = true;
                        } else if value == -1 {
                            self.slot0_active = false;
                            self.up_pending = true;
                        }
                    }
                    1 => self.slot1_active = value >= 0,
                    _ => {}
                },
                (EV_ABS, ABS_MT_POSITION_X) => match self.cur_slot {
                    0 => self.cur_x = value,
                    1 => self.slot1_x = value,
                    _ => {}
                },
                (EV_ABS, ABS_MT_POSITION_Y) => match self.cur_slot {
                    0 => self.cur_y = value,
                    1 => self.slot1_y = value,
                    _ => {}
                },
                _ => {}
            }
        }
    }

    fn transform_coords(&self) -> (u32, u32) {
        self.transform_xy(self.cur_x, self.cur_y)
    }

    /// Map raw panel coords to user-visible framebuffer coords for the current
    /// orientation. Shared by the slot-0 `Down`/`Up` path and the two-finger
    /// gesture.
    fn transform_xy(&self, x: i32, y: i32) -> (u32, u32) {
        let raw_x = x.max(0) as u32;
        let raw_y = y.max(0) as u32;
        match self.orientation {
            Orientation::Up => (raw_x, raw_y),
            // Both axes mirrored, matching the orientation-transformed fb
            // writes.
            Orientation::Down => (
                self.fb_xres.saturating_sub(1).saturating_sub(raw_x),
                self.fb_yres.saturating_sub(1).saturating_sub(raw_y),
            ),
        }
    }
}

/// True when two contacts occupy opposite corners — either diagonal
/// (top-right+bottom-left or top-left+bottom-right), in either finger order.
/// A corner is a [`SCREENSHOT_CORNER_PX`]-square box; coords are user-visible.
fn opposite_corners(ax: u32, ay: u32, bx: u32, by: u32, w: u32, h: u32) -> bool {
    let right = w.saturating_sub(SCREENSHOT_CORNER_PX);
    let bottom = h.saturating_sub(SCREENSHOT_CORNER_PX);
    let tl = |x: u32, y: u32| x < SCREENSHOT_CORNER_PX && y < SCREENSHOT_CORNER_PX;
    let tr = |x: u32, y: u32| x >= right && y < SCREENSHOT_CORNER_PX;
    let bl = |x: u32, y: u32| x < SCREENSHOT_CORNER_PX && y >= bottom;
    let br = |x: u32, y: u32| x >= right && y >= bottom;
    (tr(ax, ay) && bl(bx, by))
        || (bl(ax, ay) && tr(bx, by))
        || (tl(ax, ay) && br(bx, by))
        || (br(ax, ay) && tl(bx, by))
}

/// A stroke `(x0, y0)`→`(x1, y1)` as a horizontal page-flip swipe: at least
/// `xres / 5` of horizontal travel, twice as horizontal as vertical, within
/// ≈27° of horizontal. A tap or a vertical drift answers `None`.
pub fn classify_swipe(x0: u32, y0: u32, x1: u32, y1: u32, xres: u32) -> Option<SwipeDir> {
    let dx = x1 as i32 - x0 as i32;
    let dy = y1 as i32 - y0 as i32;
    let min_dx = (xres / 5).max(120) as i32;
    if dx.abs() < min_dx || dx.abs() < dy.abs() * 2 {
        return None;
    }
    Some(if dx < 0 {
        SwipeDir::Next
    } else {
        SwipeDir::Prev
    })
}

impl Drop for Touch {
    fn drop(&mut self) {
        if self.grabbed {
            unsafe {
                libc::ioctl(self.file.as_raw_fd(), EVIOCGRAB as _, 0);
            }
        }
    }
}

/// Names that are never a finger panel.
const PEN_NAMES: [&str; 4] = ["wacom", "digitizer", "stylus", "pen"];

/// Finger-panel names across the fleet. `pt_mt` is the Scribe's Parade
/// multitouch, sitting beside a Wacom pen node.
const TOUCH_NAMES: [&str; 9] = [
    "touch",
    "cyttsp",
    "zforce",
    "atmel",
    "fts",
    "focaltech",
    "goodix",
    "elan",
    "pt_mt",
];

/// The firmware's own alias, which outranks [`pick_from_devices`].
const TOUCH_ALIAS: &str = "/dev/input/touch";

fn find_touch_device() -> Result<PathBuf> {
    if std::fs::metadata(TOUCH_ALIAS).is_ok() {
        eprintln!("touch: using {TOUCH_ALIAS} (firmware alias)");
        return Ok(PathBuf::from(TOUCH_ALIAS));
    }
    find_touch_device_by_scan()
}

/// Rank the `eventN` nodes when no firmware alias exists (KOA2, Colorsoft).
fn find_touch_device_by_scan() -> Result<PathBuf> {
    let raw = std::fs::read_to_string("/proc/bus/input/devices")
        .context("read /proc/bus/input/devices")?;
    match pick_from_devices(&raw) {
        Some(node) => Ok(PathBuf::from(format!("/dev/input/{node}"))),
        None => bail!("no touchscreen entry in /proc/bus/input/devices"),
    }
}

/// The scan's decision, split out from the I/O. Returns the winning
/// `eventN`.
fn pick_from_devices(raw: &str) -> Option<String> {
    // Word width of the kernel's bitmap longs, needed to index `B: ABS=`.
    // Derived once from the whole file (see `bitmap_word_bits`).
    let word_bits = bitmap_word_bits(raw);

    let mut best: Option<(i32, String, String)> = None; // (score, event node, name)
    // A qualifying pen-named node, held as a last resort: `pen` is a substring
    // some panel name carries.
    let mut pen_fallback: Option<(String, String)> = None;
    for block in raw.split("\n\n") {
        let name = block
            .lines()
            .find_map(|l| l.strip_prefix("N: Name="))
            .unwrap_or("")
            .to_lowercase();
        let Some(node) = block
            .lines()
            .find_map(|l| l.strip_prefix("H: Handlers="))
            .and_then(|rest| rest.split_whitespace().find(|w| w.starts_with("event")))
        else {
            continue;
        };

        // The power key (no EV_ABS) and accelerometers (no INPUT_PROP_DIRECT)
        // fail this, which is what keeps them out of the running.
        let has_abs = first_hex_word(block, "B: EV=") & (1 << EV_ABS_BIT) != 0;
        let is_direct = first_hex_word(block, "B: PROP=") & (1 << INPUT_PROP_DIRECT) != 0;
        let name_match = TOUCH_NAMES.iter().any(|needle| name.contains(needle));
        if !(name_match || (has_abs && is_direct)) {
            continue;
        }

        if PEN_NAMES.iter().any(|needle| name.contains(needle)) {
            eprintln!("touch: deferring /dev/input/{node} (name={name:?}) — looks like a pen");
            pen_fallback.get_or_insert((node.to_string(), name));
            continue;
        }

        // The multitouch position axes this parser reads. An unreadable
        // bitmap earns no points.
        let has_mt = has_bitmap_bit(block, "B: ABS=", ABS_MT_POSITION_X as u32, word_bits);

        let score = i32::from(name_match) * 4 + i32::from(has_mt) * 2 + i32::from(is_direct);
        eprintln!(
            "touch: candidate /dev/input/{node} (name={name:?}) \
             score={score} mt={has_mt} direct={is_direct} abs={has_abs}"
        );
        if best.as_ref().is_none_or(|(b, _, _)| score > *b) {
            best = Some((score, node.to_string(), name));
        }
    }

    if let Some((score, node, name)) = best {
        // stderr, which `readinglog.sh` appends to its log.
        eprintln!("touch: using /dev/input/{node} (name={name:?}, score={score})");
        return Some(node);
    }
    // Nothing else qualified: a pen-named node is taken last.
    let (node, name) = pen_fallback?;
    eprintln!("touch: using /dev/input/{node} (name={name:?}) — pen-named, but the only candidate");
    Some(node)
}

/// Bit width of the kernel's `unsigned long`, from the longest hex word in
/// `/proc/bus/input/devices`. The kernel prints each word `%lx` and elides
/// leading empty ones; a word past 8 hex digits came from a 64-bit long.
fn bitmap_word_bits(raw: &str) -> u32 {
    let widest = raw
        .lines()
        .filter(|l| l.starts_with("B: "))
        .flat_map(|l| l.split_whitespace())
        .filter(|w| w.chars().all(|c| c.is_ascii_hexdigit()))
        .map(|w| w.len())
        .max()
        .unwrap_or(0);
    if widest > 8 { 64 } else { 32 }
}

/// One bit of a `/proc/bus/input/devices` bitmap line. Words print
/// most-significant first with the lowest last, and indexing from the end holds
/// across any number of elided words.
fn has_bitmap_bit(block: &str, prefix: &str, bit: u32, word_bits: u32) -> bool {
    let Some(rest) = block.lines().find_map(|l| l.strip_prefix(prefix)) else {
        return false;
    };
    let words: Vec<&str> = rest.split_whitespace().collect();
    let from_end = (bit / word_bits) as usize;
    if from_end >= words.len() {
        return false;
    }
    u64::from_str_radix(words[words.len() - 1 - from_end], 16)
        .map(|w| (w >> (bit % word_bits)) & 1 == 1)
        .unwrap_or(false)
}

/// First hex word of the `prefix` line in a `/proc/bus/input/devices` block
/// (`B: EV=b` → `0xb`), `0` on an absent or unparseable line. EV and PROP fit
/// one word.
fn first_hex_word(block: &str, prefix: &str) -> u64 {
    block
        .lines()
        .find_map(|l| l.strip_prefix(prefix))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|w| u64::from_str_radix(w, 16).ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verbatim `/proc/bus/input/devices` from a Kindle Scribe on 5.19.4.0.1.
    /// The pen digitizer enumerates ahead of the finger panel, as `EV_ABS` +
    /// `INPUT_PROP_DIRECT` as it is.
    const SCRIBE_DEVICES: &str = "\
I: Bus=0019 Vendor=0001 Product=0001 Version=0100
N: Name=\"bd71828-pwrkey\"
P: Phys=gpio-keys/input0
H: Handlers=event0 perfmgr
B: PROP=0
B: EV=3
B: KEY=100000 0 0 0

I: Bus=0018 Vendor=003d Product=0000 Version=0000
N: Name=\"kx132-accel\"
P: Phys=
H: Handlers=event1 perfmgr
B: PROP=0
B: EV=9
B: ABS=1000007

I: Bus=0018 Vendor=2d1f Product=0158 Version=1827
N: Name=\"WacomDigitizer\"
P: Phys=
H: Handlers=event2 perfmgr
B: PROP=2
B: EV=b
B: KEY=1c03 0 0 0 0 0 0 0 0 0 0
B: ABS=f000003

I: Bus=0000 Vendor=0000 Product=0000 Version=0000
N: Name=\"pt_mt\"
P: Phys=2-0024/input0
H: Handlers=event3 perfmgr
B: PROP=2
B: EV=f
B: KEY=0
B: REL=0
B: ABS=ee18000 0

I: Bus=0018 Vendor=2d1f Product=0158 Version=1827
N: Name=\"stylus-custom\"
P: Phys=
H: Handlers=event4 perfmgr
B: PROP=0
B: EV=b
B: KEY=1c03 0 0 0 0 0 0 0 0 0 0
B: ABS=f000003
";

    #[test]
    fn scribe_picks_the_finger_panel_not_the_pen() {
        assert_eq!(
            pick_from_devices(SCRIBE_DEVICES).as_deref(),
            Some("event3"),
            "must pick pt_mt; picking the Wacom node freezes the device"
        );
    }

    /// The discriminator holds on capability alone: a panel name varies.
    #[test]
    fn the_pen_loses_on_capability_even_without_its_name() {
        let anonymised = SCRIBE_DEVICES
            .replace("WacomDigitizer", "acme-input-a")
            .replace("stylus-custom", "acme-input-b")
            .replace("pt_mt", "acme-input-c");
        assert_eq!(
            pick_from_devices(&anonymised).as_deref(),
            Some("event3"),
            "ABS_MT axes alone should carry the decision"
        );
    }

    /// The Scribe kernel is 32-bit: `B: ABS=ee18000 0` is two words,
    /// most-significant first. `ABS_MT_POSITION_X` (0x35, bit 53) sits in the
    /// high word, and the Wacom node's single word carries no bit 53.
    #[test]
    fn abs_mt_bit_is_read_from_the_right_word() {
        let word_bits = bitmap_word_bits(SCRIBE_DEVICES);
        assert_eq!(word_bits, 32);

        let finger = "B: ABS=ee18000 0";
        let pen = "B: ABS=f000003";
        assert!(has_bitmap_bit(
            finger,
            "B: ABS=",
            ABS_MT_POSITION_X as u32,
            word_bits
        ));
        assert!(!has_bitmap_bit(
            pen,
            "B: ABS=",
            ABS_MT_POSITION_X as u32,
            word_bits
        ));
        // ABS_MT_SLOT (0x2f) and ABS_MT_TRACKING_ID (0x39) are the other axes
        // this parser depends on; both are in the same high word.
        assert!(has_bitmap_bit(finger, "B: ABS=", 0x2f, word_bits));
        assert!(has_bitmap_bit(
            finger,
            "B: ABS=",
            ABS_MT_TRACKING_ID as u32,
            word_bits
        ));
    }

    /// The pen exclusion leaves no device without input. `pen` is a substring
    /// some panel name carries.
    #[test]
    fn a_pen_named_node_is_used_when_it_is_the_only_candidate() {
        let only_pen = "\
I: Bus=0018 Vendor=2d1f Product=0158 Version=1827
N: Name=\"acme-pen-touch\"
P: Phys=
H: Handlers=event2 perfmgr
B: PROP=2
B: EV=f
B: ABS=ee18000 0
";
        assert_eq!(pick_from_devices(only_pen).as_deref(), Some("event2"));
    }

    /// …but it stays a last resort: with a real panel present, the pen loses.
    #[test]
    fn a_pen_named_node_still_loses_to_a_real_panel() {
        assert_eq!(
            pick_from_devices(SCRIBE_DEVICES).as_deref(),
            Some("event3"),
            "the fallback must not promote the pen when something better exists"
        );
    }

    /// A device with no touch node at all must report that, not pick the power
    /// key or the accelerometer.
    #[test]
    fn no_touch_node_yields_none() {
        let only_pwrkey = SCRIBE_DEVICES
            .split("\n\n")
            .next()
            .unwrap_or_default()
            .to_string();
        assert_eq!(pick_from_devices(&only_pwrkey), None);
    }

    // KOA2 / Colorsoft portrait width.
    const XRES: u32 = 1264;

    #[test]
    fn left_drag_is_next() {
        // Right→left across a third of the screen, near-horizontal.
        assert_eq!(
            classify_swipe(900, 800, 300, 820, XRES),
            Some(SwipeDir::Next)
        );
    }

    #[test]
    fn right_drag_is_prev() {
        assert_eq!(
            classify_swipe(300, 800, 900, 790, XRES),
            Some(SwipeDir::Prev)
        );
    }

    #[test]
    fn short_drag_is_not_a_swipe() {
        // A tap with a little drift — well under the xres/5 floor.
        assert_eq!(classify_swipe(600, 800, 660, 810, XRES), None);
    }

    #[test]
    fn vertical_drag_is_not_a_swipe() {
        // Long but mostly vertical — not a page flip.
        assert_eq!(classify_swipe(600, 300, 540, 900, XRES), None);
    }

    #[test]
    fn shallow_diagonal_still_counts() {
        // dx=400, dy=150 → 400 >= 2·150, horizontal enough to flip.
        assert_eq!(
            classify_swipe(900, 400, 500, 550, XRES),
            Some(SwipeDir::Next)
        );
    }
}
