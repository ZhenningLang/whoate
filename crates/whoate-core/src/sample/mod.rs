//! Platform sampling, without attribution.
use crate::snapshot::Snapshot;

pub type SampleResult<T> = Result<T, SampleError>;

#[derive(Clone, Debug, PartialEq)]
pub struct SampleError {
    pub stage: &'static str,
    pub code: String,
}
impl SampleError {
    pub fn new(stage: &'static str, code: impl Into<String>) -> Self {
        Self {
            stage,
            code: code.into(),
        }
    }
    pub fn os(stage: &'static str, errno: i32) -> Self {
        Self::new(stage, format!("errno={errno}"))
    }
}
impl std::fmt::Display for SampleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.stage, self.code)
    }
}
impl std::error::Error for SampleError {}

pub trait Sampler {
    fn sample(&mut self) -> SampleResult<Snapshot>;
}

pub mod cache;
pub mod command;
pub mod fixture;
pub mod libproc;
pub mod ports;
pub mod sysctl;
pub mod tmux;
pub mod top;

pub use libproc::MacSampler;
