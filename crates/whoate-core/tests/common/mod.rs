use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

pub struct Temp(pub PathBuf);
impl Temp {
    pub fn new() -> Self {
        static SERIAL: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "whoate-test-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
