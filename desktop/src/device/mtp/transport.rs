//! [`Transport`] over MTP through `mtp-rs`, whose calls are async and are run
//! here with `block_on`.

use std::sync::Mutex;

use anyhow::{Context, Result, anyhow};
use futures::executor::block_on;
use mtp_rs::mtp::{MtpDevice, Storage};
use mtp_rs::ptp::{ObjectHandle, ObjectInfo};

use crate::device::transport::{ChildFiles, TEntry, TPath, Transport};

pub struct MtpTransport {
    /// The storage the session is bound to. It holds the session and the
    /// claimed USB interface open for as long as the transport lives, and the
    /// lock keeps two calls from interleaving on it.
    storage: Mutex<Storage>,
    /// The firmware's version off `system/version.txt`, `None` where the
    /// Kindle does not show that file over MTP.
    firmware: Option<String>,
}

impl MtpTransport {
    /// Open the MTP device at `location_id` and bind its first storage.
    pub fn open(location_id: u64) -> Result<Self> {
        let (storage, firmware) = block_on(async {
            let device = MtpDevice::open_by_location(location_id)
                .await
                .map_err(map_mtp_err)
                .context("open MTP device")?;
            let storage = device
                .storages()
                .await
                .map_err(map_mtp_err)
                .context("list MTP storages")?
                .into_iter()
                .next()
                .ok_or_else(|| anyhow!("Kindle reports no MTP storage — try reconnecting"))?;
            let firmware = read_firmware(&storage).await;
            Ok::<_, anyhow::Error>((storage, firmware))
        })?;
        Ok(Self {
            storage: Mutex::new(storage),
            firmware,
        })
    }
}

/// The firmware's version off `system/version.txt` on the storage.
async fn read_firmware(storage: &Storage) -> Option<String> {
    let path = TPath::parse(crate::device::VERSION_TXT_REL);
    let handle = resolve(storage, &path).await.ok().flatten()?;
    let bytes = storage.download(handle).await.ok()?;
    crate::device::parse_firmware(&String::from_utf8_lossy(&bytes))
}

/// The handle of `path`'s last segment, found a segment at a time from the
/// storage root. `Ok(None)` where a segment is missing, apart from an error
/// (the USB dropped, the Kindle busy).
async fn resolve(storage: &Storage, path: &TPath) -> Result<Option<ObjectHandle>> {
    let mut parent: Option<ObjectHandle> = None;
    for segment in path.segments() {
        let entries = storage.list_objects(parent).await.map_err(map_mtp_err)?;
        match entries.into_iter().find(|o| &o.filename == segment) {
            Some(obj) => parent = Some(obj.handle),
            None => return Ok(None),
        }
    }
    Ok(parent)
}

impl Transport for MtpTransport {
    fn read(&self, path: &TPath) -> Result<Vec<u8>> {
        self.read_with_progress(path, &|_, _| {})
    }

    fn read_with_progress(&self, path: &TPath, on_progress: &dyn Fn(u64, u64)) -> Result<Vec<u8>> {
        let guard = self.storage.lock().expect("storage lock poisoned");
        let storage = &*guard;
        block_on(async {
            let handle = resolve(storage, path)
                .await?
                .ok_or_else(|| anyhow!("MTP read: object not found at `{path}`"))?;
            // In bounded chunks: a Scribe stalls a multi-megabyte single
            // `GetObject`.
            let mut dl = storage.download_stream(handle).await.map_err(map_mtp_err)?;
            let total = dl.size();
            let mut buf = Vec::with_capacity(total as usize);
            while let Some(chunk) = dl.next_chunk().await {
                buf.extend_from_slice(&chunk.map_err(map_mtp_err)?);
                on_progress(buf.len() as u64, total);
            }
            Ok(buf)
        })
    }

    fn read_files_in_children(
        &self,
        dir: &TPath,
        pick_dir: &dyn Fn(&str) -> bool,
        pick_file: &dyn Fn(&str) -> bool,
    ) -> Result<Vec<ChildFiles>> {
        let guard = self.storage.lock().expect("storage lock poisoned");
        let storage = &*guard;
        block_on(async {
            let Some(parent) = resolve(storage, dir).await? else {
                return Ok(Vec::new());
            };
            let children = storage
                .list_objects(Some(parent))
                .await
                .map_err(map_mtp_err)?;
            let mut out: Vec<ChildFiles> = Vec::new();
            for child in children {
                if !child.is_folder() || !pick_dir(&child.filename) {
                    continue;
                }
                let files: Vec<ObjectInfo> = storage
                    .list_objects(Some(child.handle))
                    .await
                    .map_err(map_mtp_err)?
                    .into_iter()
                    .filter(|o| o.is_file() && pick_file(&o.filename))
                    .collect();
                let mut got_files: Vec<(String, Vec<u8>)> = Vec::new();
                for obj in files {
                    let mut dl = storage
                        .download_stream(obj.handle)
                        .await
                        .map_err(map_mtp_err)?;
                    let mut buf = Vec::with_capacity(dl.size() as usize);
                    while let Some(chunk) = dl.next_chunk().await {
                        buf.extend_from_slice(&chunk.map_err(map_mtp_err)?);
                    }
                    if !buf.is_empty() {
                        got_files.push((obj.filename, buf));
                    }
                }
                if !got_files.is_empty() {
                    out.push((child.filename.clone(), got_files));
                }
            }
            Ok(out)
        })
    }

    fn exists(&self, path: &TPath) -> Result<bool> {
        let guard = self.storage.lock().expect("storage lock poisoned");
        let storage = &*guard;
        block_on(async { Ok(resolve(storage, path).await?.is_some()) })
    }

    fn list(&self, dir: &TPath) -> Result<Vec<TEntry>> {
        let guard = self.storage.lock().expect("storage lock poisoned");
        let storage = &*guard;
        block_on(async {
            let parent = if dir.is_empty() {
                None
            } else {
                match resolve(storage, dir).await? {
                    Some(h) => Some(h),
                    None => return Ok(Vec::new()),
                }
            };
            let entries = storage.list_objects(parent).await.map_err(map_mtp_err)?;
            Ok(entries
                .into_iter()
                .map(|o| TEntry {
                    name: o.filename.clone(),
                    is_dir: o.is_folder(),
                    size: o.is_file().then_some(o.size),
                })
                .collect())
        })
    }

    fn firmware(&self) -> Option<String> {
        self.firmware.clone()
    }

    fn display_path(&self, path: &TPath) -> String {
        format!("mtp:{path}")
    }
}

/// `err`, naming the apps that hold an MTP Kindle where another holds this one.
fn map_mtp_err(err: mtp_rs::Error) -> anyhow::Error {
    if err.is_exclusive_access() {
        anyhow!(
            "Kindle is in use by another app. Quit Image Capture, OpenMTP, \
             Android File Transfer, or Calibre and try again. (underlying: {err})"
        )
    } else {
        anyhow!(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exclusive_access_error_gets_actionable_text() {
        // The words nusb answers on macOS where another process holds the
        // device.
        let io_err = std::io::Error::other("could not be opened for exclusive access");
        let mapped = map_mtp_err(mtp_rs::Error::Io(io_err));
        let msg = format!("{mapped:#}");
        assert!(
            msg.contains("Quit Image Capture"),
            "user-actionable hint missing from busy error: {msg}"
        );
    }

    #[test]
    fn ordinary_io_error_passes_through() {
        let io_err = std::io::Error::other("bus reset");
        let mapped = map_mtp_err(mtp_rs::Error::Io(io_err));
        let msg = format!("{mapped:#}");
        assert!(
            !msg.contains("Image Capture"),
            "false positive busy hint: {msg}"
        );
        assert!(msg.contains("bus reset"));
    }
}
