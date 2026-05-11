//! macOS per-application audio capture via ScreenCaptureKit.
//!
//! ScreenCaptureKit requires a display in the content filter even when only
//! audio is desired; the video track is configured to a 2x2 placeholder and
//! discarded. The audio track is delivered as `CMSampleBuffer`s which we
//! convert to interleaved f32 stereo and forward downstream.

use std::sync::Arc;

use crossbeam_channel::Sender;
use screencapturekit::output::CMSampleBuffer;
use screencapturekit::shareable_content::SCShareableContent;
use screencapturekit::stream::configuration::SCStreamConfiguration;
use screencapturekit::stream::content_filter::SCContentFilter;
use screencapturekit::stream::output_trait::SCStreamOutputTrait;
use screencapturekit::stream::output_type::SCStreamOutputType;
use screencapturekit::stream::SCStream;

use crate::capture::{AudioCapture, AudioChunk, AudioFormat, CaptureError, Result};

pub struct MacCapture {
    stream: SCStream,
    format: AudioFormat,
}

#[derive(Clone)]
struct AudioOutput {
    tx: Sender<AudioChunk>,
}

impl SCStreamOutputTrait for AudioOutput {
    fn did_output_sample_buffer(&self, sample: CMSampleBuffer, of_type: SCStreamOutputType) {
        if of_type != SCStreamOutputType::Audio {
            return;
        }
        let Some(samples) = decode_audio(&sample) else {
            return;
        };
        let _ = self.tx.try_send(AudioChunk {
            samples: Arc::new(samples),
        });
    }
}

fn decode_audio(buffer: &CMSampleBuffer) -> Option<Vec<f32>> {
    let abl = buffer.get_audio_buffer_list().ok()?;
    let buffers = abl.buffers();
    if buffers.is_empty() {
        return None;
    }

    if buffers.len() == 1 {
        let raw = buffers[0].data();
        if raw.is_empty() {
            return None;
        }
        let frames = raw.len() / 4;
        let floats = unsafe { std::slice::from_raw_parts(raw.as_ptr() as *const f32, frames) };
        if buffers[0].number_channels == 1 {
            let mut interleaved = Vec::with_capacity(frames * 2);
            for s in floats {
                interleaved.push(*s);
                interleaved.push(*s);
            }
            Some(interleaved)
        } else {
            Some(floats.to_vec())
        }
    } else {
        let left = buffers[0].data();
        let right = buffers[1].data();
        let frames = left.len().min(right.len()) / 4;
        let l = unsafe { std::slice::from_raw_parts(left.as_ptr() as *const f32, frames) };
        let r = unsafe { std::slice::from_raw_parts(right.as_ptr() as *const f32, frames) };
        let mut interleaved = Vec::with_capacity(frames * 2);
        for i in 0..frames {
            interleaved.push(l[i]);
            interleaved.push(r[i]);
        }
        Some(interleaved)
    }
}

impl MacCapture {
    pub fn start(pid: u32, tx: Sender<AudioChunk>) -> Result<Self> {
        let content = SCShareableContent::get()
            .map_err(|e| permission_or_platform(format!("SCShareableContent: {e:?}")))?;

        let displays = content.displays();
        let display = displays
            .first()
            .ok_or_else(|| CaptureError::Platform("no displays available".into()))?;

        let apps = content.applications();
        let target_app = apps
            .iter()
            .find(|a| a.process_id() as u32 == pid)
            .ok_or(CaptureError::ProcessNotFound(pid))?;

        let filter = SCContentFilter::new()
            .with_display_including_application_excepting_windows(
                display,
                &[target_app],
                &[],
            );

        let config = SCStreamConfiguration::new()
            .set_captures_audio(true)
            .map_err(|e| CaptureError::Platform(format!("set_captures_audio: {e:?}")))?
            .set_excludes_current_process_audio(true)
            .map_err(|e| {
                CaptureError::Platform(format!("set_excludes_current_process_audio: {e:?}"))
            })?
            .set_sample_rate(48_000)
            .map_err(|e| CaptureError::Platform(format!("set_sample_rate: {e:?}")))?
            .set_channel_count(2)
            .map_err(|e| CaptureError::Platform(format!("set_channel_count: {e:?}")))?
            .set_width(2)
            .map_err(|e| CaptureError::Platform(format!("set_width: {e:?}")))?
            .set_height(2)
            .map_err(|e| CaptureError::Platform(format!("set_height: {e:?}")))?;

        let mut stream = SCStream::new(&filter, &config);
        stream.add_output_handler(AudioOutput { tx }, SCStreamOutputType::Audio);

        stream
            .start_capture()
            .map_err(|e| permission_or_platform(format!("start_capture: {e:?}")))?;

        Ok(Self {
            stream,
            format: AudioFormat::stereo_48k(),
        })
    }
}

impl AudioCapture for MacCapture {
    fn format(&self) -> AudioFormat {
        self.format
    }

    fn stop(&mut self) {
        let _ = self.stream.stop_capture();
    }
}

impl Drop for MacCapture {
    fn drop(&mut self) {
        let _ = self.stream.stop_capture();
    }
}

fn permission_or_platform(msg: String) -> CaptureError {
    let lower = msg.to_lowercase();
    if lower.contains("permission") || lower.contains("declined") || lower.contains("not authorized") {
        CaptureError::PermissionDenied(msg)
    } else {
        CaptureError::Platform(msg)
    }
}

pub fn screen_recording_authorized() -> bool {
    SCShareableContent::get().is_ok()
}
