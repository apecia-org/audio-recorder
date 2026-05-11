use std::sync::Arc;

use crossbeam_channel::Sender;
use thiserror::Error;

#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "windows")]
pub mod windows;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioFormat {
    pub sample_rate: u32,
    pub channels: u16,
}

impl AudioFormat {
    pub const fn stereo_48k() -> Self {
        Self {
            sample_rate: 48_000,
            channels: 2,
        }
    }
}

#[derive(Debug, Error)]
#[allow(dead_code)]
pub enum CaptureError {
    #[error("permission denied: {0}")]
    PermissionDenied(String),
    #[error("target process {0} not found")]
    ProcessNotFound(u32),
    #[error("audio device busy (exclusive mode?): {0}")]
    DeviceBusy(String),
    #[error("platform error: {0}")]
    Platform(String),
    #[error("unsupported on this platform")]
    Unsupported,
}

pub type Result<T> = std::result::Result<T, CaptureError>;

/// A chunk of interleaved f32 stereo PCM at 48 kHz.
#[derive(Debug, Clone)]
pub struct AudioChunk {
    pub samples: Arc<Vec<f32>>,
}

/// Backend-agnostic per-process audio capture handle.
///
/// Implementations spawn their own capture thread on `start` and stop it on `stop`/drop.
pub trait AudioCapture: Send {
    fn format(&self) -> AudioFormat;
    fn stop(&mut self);
}

/// Construct a platform-specific capture handle that streams audio from `pid` into `tx`.
pub fn start_capture(pid: u32, tx: Sender<AudioChunk>) -> Result<Box<dyn AudioCapture>> {
    #[cfg(target_os = "macos")]
    {
        let cap = macos::MacCapture::start(pid, tx)?;
        Ok(Box::new(cap))
    }
    #[cfg(target_os = "windows")]
    {
        let cap = windows::WindowsCapture::start(pid, tx)?;
        Ok(Box::new(cap))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (pid, tx);
        Err(CaptureError::Unsupported)
    }
}
