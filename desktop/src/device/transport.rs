//! Reading a Kindle's files, whichever way it is plugged in.

use anyhow::Result;

/// A path on the Kindle, e.g. `extensions/readinglog/sessions.tsv`, held as its
/// segments so each transport maps them to its own namespace: a path on disk,
/// or a chain of MTP objects.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct TPath {
    segments: Vec<String>,
}

impl TPath {
    /// The storage root.
    pub fn new() -> Self {
        Self::default()
    }

    /// `s`'s slash-separated segments, empty ones dropped.
    pub fn parse(s: &str) -> Self {
        Self {
            segments: s
                .split('/')
                .filter(|seg| !seg.is_empty())
                .map(String::from)
                .collect(),
        }
    }

    pub fn segments(&self) -> &[String] {
        &self.segments
    }

    pub fn name(&self) -> Option<&str> {
        self.segments.last().map(|s| s.as_str())
    }

    pub fn parent(&self) -> Option<TPath> {
        if self.segments.is_empty() {
            None
        } else {
            Some(TPath {
                segments: self.segments[..self.segments.len() - 1].to_vec(),
            })
        }
    }

    pub fn join(&self, name: &str) -> TPath {
        let mut segments = self.segments.clone();
        segments.push(name.to_string());
        TPath { segments }
    }

    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }
}

impl std::fmt::Display for TPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.segments.join("/"))
    }
}

/// One entry of a folder's listing.
#[derive(Debug, Clone)]
pub struct TEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: Option<u64>,
}

/// One folder [`Transport::read_files_in_children`] matched: its name, and the
/// `(filename, bytes)` of each file picked in it.
pub type ChildFiles = (String, Vec<(String, Vec<u8>)>);

/// The calls a Kindle's files are read through.
pub trait Transport: Send + Sync {
    fn read(&self, path: &TPath) -> Result<Vec<u8>>;

    /// [`read`](Self::read), calling `on_progress(done, total)` in bytes as
    /// the read advances.
    fn read_with_progress(&self, path: &TPath, on_progress: &dyn Fn(u64, u64)) -> Result<Vec<u8>> {
        let bytes = self.read(path)?;
        let n = bytes.len() as u64;
        on_progress(n, n);
        Ok(bytes)
    }

    /// For each folder in `dir` that `pick_dir` takes, every file in it that
    /// `pick_file` takes. `dir` and each folder are found once, so MTP stays
    /// linear.
    fn read_files_in_children(
        &self,
        dir: &TPath,
        pick_dir: &dyn Fn(&str) -> bool,
        pick_file: &dyn Fn(&str) -> bool,
    ) -> Result<Vec<ChildFiles>> {
        let mut out = Vec::new();
        for entry in self.list(dir)? {
            if !entry.is_dir || !pick_dir(&entry.name) {
                continue;
            }
            let child = dir.join(&entry.name);
            let mut files = Vec::new();
            for f in self.list(&child).unwrap_or_default() {
                if !f.is_dir && pick_file(&f.name) {
                    match self.read(&child.join(&f.name)) {
                        Ok(bytes) if !bytes.is_empty() => files.push((f.name, bytes)),
                        _ => {}
                    }
                }
            }
            if !files.is_empty() {
                out.push((entry.name, files));
            }
        }
        Ok(out)
    }

    fn exists(&self, path: &TPath) -> Result<bool>;

    /// What `dir` holds, one level down. Empty where `dir` is absent.
    fn list(&self, dir: &TPath) -> Result<Vec<TEntry>>;

    /// The firmware's version, where the transport read it on opening.
    fn firmware(&self) -> Option<String> {
        None
    }

    /// `path` as a person reads it: a path on disk, or `mtp:` and the path.
    fn display_path(&self, path: &TPath) -> String;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tpath_parse_drops_empty_segments() {
        let p = TPath::parse("/documents//Sidle/foo.kfx/");
        assert_eq!(
            p.segments(),
            &[
                "documents".to_string(),
                "Sidle".to_string(),
                "foo.kfx".to_string()
            ]
        );
    }

    #[test]
    fn tpath_parent_and_name() {
        let p = TPath::parse("documents/Sidle/foo.kfx");
        assert_eq!(p.name(), Some("foo.kfx"));
        let parent = p.parent().expect("has parent");
        assert_eq!(parent.name(), Some("Sidle"));
        assert_eq!(format!("{parent}"), "documents/Sidle");
    }

    #[test]
    fn tpath_join_appends_segment() {
        let p = TPath::parse("documents/Sidle").join("foo.kfx");
        assert_eq!(format!("{p}"), "documents/Sidle/foo.kfx");
    }

    #[test]
    fn tpath_empty_round_trip() {
        let empty = TPath::new();
        assert!(empty.is_empty());
        assert_eq!(empty.parent(), None);
        assert_eq!(empty.name(), None);
        assert_eq!(format!("{empty}"), "");
    }
}
