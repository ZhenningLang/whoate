use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    time::SystemTime,
};

#[derive(Clone, Debug, PartialEq)]
struct Stamp {
    modified: SystemTime,
    len: u64,
    device: u64,
    inode: u64,
    mode: u32,
    changed: (i64, i64),
}
impl Stamp {
    fn read(path: &Path) -> Result<Self, &'static str> {
        let meta = fs::metadata(path).map_err(|_| "read_failed")?;
        if !meta.is_file() || meta.mode() & 0o444 == 0 {
            return Err("read_failed");
        }
        Ok(Self {
            modified: meta.modified().map_err(|_| "config_stat_failed")?,
            len: meta.len(),
            device: meta.dev(),
            inode: meta.ino(),
            mode: meta.mode(),
            changed: (meta.ctime(), meta.ctime_nsec()),
        })
    }
}
struct Entry {
    stamp: Stamp,
    bytes: Vec<u8>,
    plist: Option<plist::Value>,
}
#[derive(Default)]
pub(super) struct Files {
    entries: BTreeMap<PathBuf, Entry>,
    pub reads: usize,
    pub plist_parses: usize,
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
        Ok(self.entry(path)?.bytes.clone())
    }
    fn entry(&mut self, path: &Path) -> Result<&mut Entry, &'static str> {
        let path = path.canonicalize().map_err(|_| "read_failed")?;
        let stamp = Stamp::read(&path)?;
        if !self
            .entries
            .get(&path)
            .is_some_and(|entry| entry.stamp == stamp)
        {
            // Failed updates never resurrect an old successful parse.
            self.entries.remove(&path);
            self.reads += 1;
            let bytes = fs::read(&path).map_err(|_| "read_failed")?;
            if Stamp::read(&path)? != stamp {
                return Err("config_changed_during_read");
            }
            self.entries.insert(
                path.clone(),
                Entry {
                    stamp,
                    bytes,
                    plist: None,
                },
            );
        }
        Ok(self.entries.get_mut(&path).expect("validated cache entry"))
    }
    pub fn read_plist(&mut self, path: &Path) -> Result<plist::Value, &'static str> {
        let entry = self.entry(path)?;
        if let Some(value) = &entry.plist {
            return Ok(value.clone());
        }
        let value = plist::Value::from_reader(std::io::Cursor::new(&entry.bytes))
            .map_err(|_| "app_metadata_unavailable");
        if let Ok(value) = &value {
            entry.plist = Some(value.clone());
        }
        self.plist_parses += 1;
        value
    }
}
