//! A Kindle over MTP, found by enumerating USB: no session is opened.

use crate::device::{DeviceInfo, TransportKind};

/// Amazon's USB vendor ID, on every Kindle.
const AMAZON_VID: u16 = 0x1949;

/// The first Amazon MTP device.
pub fn detect() -> Option<DeviceInfo> {
    let devices = match mtp_rs::MtpDevice::list_devices() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[readinglog/mtp] list_devices failed: {e}");
            return None;
        }
    };

    let dev = devices.into_iter().find(|d| d.vendor_id == AMAZON_VID)?;

    // The USB serial holds from port to port; the location only while the
    // Kindle stays in one.
    let serial = dev
        .serial_number
        .clone()
        .unwrap_or_else(|| format!("anon-mtp-{:x}", dev.location_id));

    // The USB product descriptor names the model without a session.
    let model = match (&dev.manufacturer, &dev.product) {
        (Some(m), Some(p)) => Some(format!("{m} {p}")),
        (None, Some(p)) => Some(p.clone()),
        (Some(m), None) => Some(m.clone()),
        (None, None) => None,
    };

    Some(DeviceInfo {
        serial,
        model,
        // `system/version.txt` takes a session, which detection opens none of.
        firmware: None,
        transport: TransportKind::Mtp {
            location_id: dev.location_id,
            product_id: dev.product_id,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// With no Kindle plugged into the machine the tests run on, `detect`
    /// answers `None` and does not panic.
    #[test]
    fn detect_returns_none_when_no_amazon_device() {
        match detect() {
            None => {}
            Some(d) => {
                assert!(matches!(d.transport, TransportKind::Mtp { .. }));
                assert!(!d.serial.is_empty());
            }
        }
    }
}
