//! File identities contain no source bytes and are never printed with agents.
use std::{
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    time::SystemTime,
};

#[derive(Clone, PartialEq, Eq)]
pub(super) struct Stamp {
    path: PathBuf,
    canonical: PathBuf,
    modified: SystemTime,
    size: u64,
    identity: (u64, u64, u32, i64, i64),
}
impl Stamp {
    pub(super) fn read(path: &Path) -> Option<Self> {
        let canonical = path.canonicalize().ok()?;
        let meta = fs::metadata(&canonical).ok()?;
        if !meta.is_file() || meta.mode() & 0o444 == 0 {
            return None;
        }
        Some(Self {
            path: path.into(),
            canonical,
            modified: meta.modified().ok()?,
            size: meta.len(),
            identity: (
                meta.dev(),
                meta.ino(),
                meta.mode(),
                meta.ctime(),
                meta.ctime_nsec(),
            ),
        })
    }
    pub(super) fn unchanged(&self, cfg: &crate::recognizers::ConfigReader) -> bool {
        cfg.allows(&self.path) && Self::read(&self.path).as_ref() == Some(self)
    }
}

pub(super) struct Memo {
    pub agent: crate::model::Agent,
    pub sources: Vec<Stamp>,
}
