//! A mounted Kindle: a volume under `/Volumes` (macOS) or `/media` (Linux)
//! holding `system/version.txt`.

use std::path::{Path, PathBuf};

use nusb::MaybeFuture;

use crate::device::{DeviceInfo, TransportKind};

#[cfg(target_os = "macos")]
const VOLUMES_ROOT: &str = "/Volumes";
#[cfg(target_os = "linux")]
const VOLUMES_ROOT_FALLBACK: &str = "/media";

/// Amazon's USB vendor ID, on every Kindle, mounted or over MTP.
const AMAZON_VID: u16 = 0x1949;
/// The USB mass-storage interface class, which tells a mounted Kindle from an
/// MTP one when both are plugged in.
const USB_CLASS_MASS_STORAGE: u8 = 0x08;

/// The first mounted Kindle.
pub fn detect() -> Option<DeviceInfo> {
    for root in candidate_roots() {
        if let Ok(entries) = std::fs::read_dir(&root) {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(info) = inspect(&path) {
                    return Some(info);
                }
            }
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn candidate_roots() -> Vec<PathBuf> {
    vec![PathBuf::from(VOLUMES_ROOT)]
}

#[cfg(target_os = "linux")]
fn candidate_roots() -> Vec<PathBuf> {
    // Where a distribution mounts a removable volume: `/media/$USER` (GNOME,
    // GVfs), `/run/media/$USER` (udisks2).
    let mut roots = Vec::new();
    if let Ok(user) = std::env::var("USER") {
        roots.push(PathBuf::from(format!("/media/{user}")));
        roots.push(PathBuf::from(format!("/run/media/{user}")));
    }
    roots.push(PathBuf::from(VOLUMES_ROOT_FALLBACK));
    roots
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn candidate_roots() -> Vec<PathBuf> {
    Vec::new()
}

/// The Kindle mounted at `mount`, where `system/version.txt` says it is one.
fn inspect(mount: &Path) -> Option<DeviceInfo> {
    let version_path = mount.join("system").join("version.txt");
    let raw = std::fs::read_to_string(&version_path).ok()?;
    // `version.txt` names the firmware and no serial: the serial is the USB
    // descriptor's.
    let serial = usb_kindle_serial().unwrap_or_else(|| anon_serial(mount));
    let model = parse_model(&raw);
    let firmware = crate::device::parse_firmware(&raw);
    Some(DeviceInfo {
        serial,
        model,
        firmware,
        transport: TransportKind::MassStorage {
            mount: mount.to_string_lossy().to_string(),
        },
    })
}

/// A name for a Kindle whose serial cannot be read: `anon-` and its volume's name.
fn anon_serial(mount: &Path) -> String {
    let name = mount
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "Kindle".to_string());
    format!("anon-{name}")
}

/// The Kindle's serial, off its USB `iSerial` descriptor: the mass-storage
/// device's where two are attached.
fn usb_kindle_serial() -> Option<String> {
    let amazon: Vec<nusb::DeviceInfo> = nusb::list_devices()
        .wait()
        .ok()?
        .filter(|d| d.vendor_id() == AMAZON_VID)
        .collect();
    let pick = match amazon.as_slice() {
        [] => return None,
        [only] => only,
        many => many
            .iter()
            .find(|d| d.interfaces().any(|i| i.class() == USB_CLASS_MASS_STORAGE))
            .unwrap_or(&many[0]),
    };
    pick.serial_number()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn parse_model(raw: &str) -> Option<String> {
    let first = raw.lines().next()?.trim();
    if first.is_empty() {
        return None;
    }
    Some(first.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::scratch;

    #[test]
    fn parses_model_from_first_line() {
        let raw = "Kindle 5.16.10.4.0\nS/N: X\n";
        assert_eq!(parse_model(raw).as_deref(), Some("Kindle 5.16.10.4.0"));
    }

    #[test]
    fn inspect_detects_kindle_without_serial_in_version_txt() {
        let tmp = scratch("mass-storage-inspect");
        let sys = tmp.join("system");
        std::fs::create_dir_all(&sys).unwrap();
        std::fs::write(sys.join("version.txt"), "Kindle 5.16.2.1.1 (409745 002)\n").unwrap();

        let info = inspect(&tmp).expect("a Kindle without a serial in version.txt");
        // The serial is the USB descriptor's on a real Kindle, an `anon-` name
        // here; never empty.
        assert!(!info.serial.is_empty());
        assert_eq!(
            info.model.as_deref(),
            Some("Kindle 5.16.2.1.1 (409745 002)")
        );
        assert_eq!(info.firmware.as_deref(), Some("5.16.2.1.1"));
        match info.transport {
            TransportKind::MassStorage { mount } => {
                assert_eq!(mount, tmp.to_string_lossy());
            }
            other => panic!("expected MassStorage, got {other:?}"),
        }
        assert!(inspect(&scratch("mass-storage-stick")).is_none());
    }
}
