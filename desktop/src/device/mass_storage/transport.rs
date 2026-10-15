//! [`Transport`] over a mounted Kindle's volume.

use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::device::transport::{TEntry, TPath, Transport};

pub struct MassStorageTransport {
    mount: PathBuf,
}

impl MassStorageTransport {
    pub fn new(mount: PathBuf) -> Self {
        Self { mount }
    }

    fn resolve(&self, path: &TPath) -> PathBuf {
        let mut out = self.mount.clone();
        for seg in path.segments() {
            out.push(seg);
        }
        out
    }
}

impl Transport for MassStorageTransport {
    fn read(&self, path: &TPath) -> Result<Vec<u8>> {
        let p = self.resolve(path);
        std::fs::read(&p).with_context(|| format!("read {}", p.display()))
    }

    fn exists(&self, path: &TPath) -> Result<bool> {
        Ok(self.resolve(path).exists())
    }

    fn list(&self, dir: &TPath) -> Result<Vec<TEntry>> {
        let p = self.resolve(dir);
        let read_dir = match std::fs::read_dir(&p) {
            Ok(r) => r,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(anyhow::anyhow!("read_dir {}: {}", p.display(), e)),
        };
        let mut out = Vec::new();
        for entry in read_dir.flatten() {
            let meta = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            let name = entry.file_name().to_string_lossy().into_owned();
            out.push(TEntry {
                name,
                is_dir: meta.is_dir(),
                size: meta.is_file().then_some(meta.len()),
            });
        }
        Ok(out)
    }

    fn display_path(&self, path: &TPath) -> String {
        self.resolve(path).to_string_lossy().into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::scratch;

    #[test]
    fn a_file_on_the_volume_reads_back() {
        let tmp = scratch("mass-storage-read");
        std::fs::create_dir_all(tmp.join("extensions/readinglog")).unwrap();
        std::fs::write(tmp.join("extensions/readinglog/sessions.tsv"), b"hello").unwrap();
        let t = MassStorageTransport::new(tmp);
        let path = TPath::parse("extensions/readinglog/sessions.tsv");
        assert!(t.exists(&path).unwrap());
        assert_eq!(t.read(&path).unwrap(), b"hello");
    }

    #[test]
    fn list_missing_dir_is_empty() {
        let t = MassStorageTransport::new(scratch("mass-storage-list"));
        let entries = t.list(&TPath::parse("does/not/exist")).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn display_path_renders_full_filesystem_path() {
        let tmp = scratch("mass-storage-display");
        let t = MassStorageTransport::new(tmp.clone());
        let p = TPath::parse("documents/Sidle/foo.kfx");
        let expected = tmp
            .join("documents/Sidle/foo.kfx")
            .to_string_lossy()
            .into_owned();
        assert_eq!(t.display_path(&p), expected);
    }
}
