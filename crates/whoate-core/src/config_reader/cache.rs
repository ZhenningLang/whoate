use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    time::SystemTime,
};

#[derive(Debug, PartialEq)]
struct Stamp {
    modified: SystemTime,
    len: u64,
    device: u64,
    inode: u64,
    mode: u32,
}
#[derive(Default)]
pub(super) struct Files {
    entries: BTreeMap<PathBuf, (Stamp, Vec<u8>)>,
    pub reads: usize,
}
impl std::fmt::Debug for Files {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConfigFileCache")
            .field("entry_count", &self.entries.len())
            .field("reads", &self.reads)
            .finish_non_exhaustive()
    }
}
impl Files {
    pub fn read(&mut self, path: &Path) -> Result<String, &'static str> {
        String::from_utf8(self.read_bytes(path)?).map_err(|_| "read_failed")
    }
    pub fn read_bytes(&mut self, path: &Path) -> Result<Vec<u8>, &'static str> {
        let path = path.canonicalize().map_err(|_| "read_failed")?;
        let meta = fs::metadata(&path).map_err(|_| "read_failed")?;
        let stamp = Stamp {
            modified: meta.modified().map_err(|_| "config_stat_failed")?,
            len: meta.len(),
            device: meta.dev(),
            inode: meta.ino(),
            mode: meta.mode(),
        };
        if let Some((old, data)) = self.entries.get(&path)
            && *old == stamp
        {
            return Ok(data.clone());
        }
        // Invalidate before reading: a failed update must not resurrect old data.
        self.entries.remove(&path);
        self.reads += 1;
        let data = fs::read(&path).map_err(|_| "read_failed")?;
        self.entries.insert(path, (stamp, data.clone()));
        Ok(data)
    }
}
