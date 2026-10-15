//! The Kindle plugged in: a mounted one first, which costs a directory listing,
//! then one over MTP.

use crate::device::{DeviceInfo, mass_storage, mtp};

pub fn detect() -> Option<DeviceInfo> {
    if let Some(info) = mass_storage::detect::detect() {
        return Some(info);
    }
    if let Some(info) = mtp::detect::detect() {
        return Some(info);
    }
    None
}
