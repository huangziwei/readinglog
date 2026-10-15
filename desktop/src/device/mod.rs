//! A Kindle plugged in over USB: [`detect`] finds it, mounted or over MTP, and
//! names it by its serial; a [`Transport`] reads its files, the same calls on
//! either. Nothing here writes to a Kindle.

use std::path::PathBuf;

use anyhow::Result;

pub mod detect;
pub mod mass_storage;
pub mod mtp;
pub mod transport;

pub use transport::{TEntry, TPath, Transport};

/// What is known of a Kindle plugged in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    /// The USB `iSerial` descriptor, else an `anon-` name for the place it is
    /// plugged in.
    pub serial: String,
    pub model: Option<String>,
    /// The firmware's version, e.g. `5.16.2.1.1`, off `system/version.txt`:
    /// read at detection on a mounted Kindle, at a session's opening over MTP.
    pub firmware: Option<String>,
    pub transport: TransportKind,
}

/// How a Kindle is reached.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportKind {
    MassStorage {
        /// Where the volume is mounted, e.g. `/Volumes/Kindle`.
        mount: String,
    },
    Mtp {
        /// The USB location, which holds while the Kindle stays in one port:
        /// the key a session opens by.
        location_id: u64,
        /// The USB product ID, which tells one Kindle model from another
        /// without a session.
        product_id: u16,
    },
}

impl DeviceInfo {
    /// A transport over this Kindle: the mount path on a mounted one, a USB
    /// session over MTP, held until it is dropped.
    pub fn open_transport(&self) -> Result<Box<dyn Transport>> {
        match &self.transport {
            TransportKind::MassStorage { mount } => Ok(Box::new(
                mass_storage::transport::MassStorageTransport::new(PathBuf::from(mount)),
            )),
            TransportKind::Mtp { location_id, .. } => {
                Ok(Box::new(mtp::transport::MtpTransport::open(*location_id)?))
            }
        }
    }

    /// Where a mounted Kindle's volume is; `None` over MTP.
    pub fn mass_storage_mount(&self) -> Option<PathBuf> {
        match &self.transport {
            TransportKind::MassStorage { mount } => Some(PathBuf::from(mount)),
            TransportKind::Mtp { .. } => None,
        }
    }
}

/// The firmware's own version line, under the volume or storage root.
pub const VERSION_TXT_REL: &str = "system/version.txt";

/// The firmware's version off `system/version.txt`'s first line, which reads
/// `Kindle <firmware> [(build)]` — e.g. `Kindle 5.19.4.0.1 (476724 003)`: its
/// first dotted number. `None` where it carries none.
pub(crate) fn parse_firmware(raw: &str) -> Option<String> {
    let first = raw.lines().next()?;
    first
        .split_whitespace()
        .find(|tok| {
            tok.starts_with(|c: char| c.is_ascii_digit())
                && tok.contains('.')
                && tok.chars().all(|c| c.is_ascii_digit() || c == '.')
        })
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::parse_firmware;

    #[test]
    fn parses_firmware_with_and_without_build() {
        assert_eq!(
            parse_firmware("Kindle 5.16.10.4.0\n").as_deref(),
            Some("5.16.10.4.0")
        );
        assert_eq!(
            parse_firmware("Kindle 5.19.4.0.1 (476724 003)\n").as_deref(),
            Some("5.19.4.0.1")
        );
        assert_eq!(parse_firmware("Kindle\n"), None);
    }
}
