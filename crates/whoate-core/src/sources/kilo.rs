//! Kilo shares the upstream SQLite schema; its path and tool identity stay distinct.
use crate::{model::Agent, snapshot::Proc};
use std::path::Path;
pub fn read(path: &Path, proc: &Proc) -> Result<Option<Agent>, &'static str> {
    super::opencode::read(path, proc, "kilo")
}
