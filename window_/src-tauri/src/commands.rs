use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Instant;

use crossbeam_channel::{bounded, Receiver, Sender};
use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::capture::{self, AudioCapture, AudioChunk};
use crate::encoder::{self, EncoderMetadata};
use crate::mic::MicCapture;
use crate::mixer::{self, MixerConfig};
use crate::process_list::{self, ProcessInfo};

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum RecordingStatus {
    Idle,
    Recording {
        pid: u32,
        source_app: String,
        output_path: String,
        started_at_iso: String,
        mic_mix: bool,
    },
}

#[derive(Debug, Serialize, Clone)]
pub struct RecordingMeta {
    pub output_path: String,
    pub duration_seconds: f64,
    pub source_app: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct PermissionStatus {
    pub screen_recording: bool,
    pub microphone: bool,
}

#[derive(Default)]
pub enum RecorderInner {
    #[default]
    Idle,
    Recording(Box<RunningRecording>),
}

pub struct RunningRecording {
    pub pid: u32,
    pub source_app: String,
    pub output_path: PathBuf,
    pub started_at: Instant,
    pub started_at_iso: String,
    pub mic_mix: bool,
    pub stop_flag: Arc<AtomicBool>,
    pub capture: Box<dyn AudioCapture>,
    pub _mic: Option<MicCapture>,
    pub mixer_join: Option<JoinHandle<()>>,
    pub encoder_join: Option<JoinHandle<anyhow::Result<()>>>,
    pub level_drain_join: Option<JoinHandle<()>>,
}

pub struct RecorderState {
    pub inner: Arc<Mutex<RecorderInner>>,
    pub app_handle: AppHandle,
}

impl RecorderState {
    pub fn new(app_handle: AppHandle) -> Self {
        Self {
            inner: Arc::new(Mutex::new(RecorderInner::Idle)),
            app_handle,
        }
    }

    fn lock(&self) -> parking_lot::MutexGuard<'_, RecorderInner> {
        self.inner.lock()
    }
}

#[tauri::command]
pub fn list_recordable_processes() -> Vec<ProcessInfo> {
    process_list::list_processes()
}

#[derive(serde::Deserialize)]
pub struct StartArgs {
    pub pid: u32,
    pub mic_mix: bool,
    pub output_path: Option<String>,
    pub source_app: Option<String>,
}

#[tauri::command]
pub fn start_recording(
    state: State<'_, RecorderState>,
    args: StartArgs,
) -> std::result::Result<RecordingStatus, String> {
    let mut guard = state.lock();
    if matches!(*guard, RecorderInner::Recording(_)) {
        return Err("a recording is already in progress".into());
    }

    let resolved_pid = process_list::resolve_top_level_pid(args.pid);
    let source_app = args.source_app.unwrap_or_else(|| format!("pid-{resolved_pid}"));
    let output_path = match args.output_path {
        Some(p) => PathBuf::from(p),
        None => default_output_path(&source_app),
    };
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create output dir: {e}"))?;
    }

    let stop_flag = Arc::new(AtomicBool::new(false));

    let (cap_tx, cap_rx): (Sender<AudioChunk>, Receiver<AudioChunk>) = bounded(64);
    let capture = capture::start_capture(resolved_pid, cap_tx)
        .map_err(|e| format!("capture start: {e}"))?;

    let (mic_capture, mic_rx) = if args.mic_mix {
        let (mic_tx, mic_rx) = bounded::<AudioChunk>(64);
        match MicCapture::start(mic_tx) {
            Ok(m) => (Some(m), Some(mic_rx)),
            Err(e) => {
                tracing::warn!("mic capture failed, continuing app-only: {e}");
                let _ = state
                    .app_handle
                    .emit("recording-warning", format!("microphone unavailable: {e}"));
                (None, None)
            }
        }
    } else {
        (None, None)
    };

    let (mix_tx, mix_rx) = bounded::<AudioChunk>(128);
    let (level_tx, level_rx) = bounded::<f32>(64);
    let mixer_cfg = MixerConfig {
        mic_enabled: mic_rx.is_some(),
        mic_gain: 0.8,
    };
    let mixer_stop = stop_flag.clone();
    let mixer_join = thread::Builder::new()
        .name("audio-mixer".into())
        .spawn(move || mixer::run(cap_rx, mic_rx, mix_tx, mixer_cfg, mixer_stop, level_tx))
        .map_err(|e| format!("spawn mixer: {e}"))?;

    let started_at_iso = chrono::Local::now().to_rfc3339();
    let enc_meta = EncoderMetadata {
        source_app: source_app.clone(),
        started_at_iso: started_at_iso.clone(),
    };
    let enc_stop = stop_flag.clone();
    let enc_path = output_path.clone();
    let encoder_join = thread::Builder::new()
        .name("audio-encoder".into())
        .spawn(move || encoder::run(mix_rx, enc_path, enc_meta, enc_stop))
        .map_err(|e| format!("spawn encoder: {e}"))?;

    let app_handle = state.app_handle.clone();
    let level_stop = stop_flag.clone();
    let level_drain_join = thread::Builder::new()
        .name("audio-level-relay".into())
        .spawn(move || {
            while !level_stop.load(Ordering::SeqCst) {
                match level_rx.recv_timeout(std::time::Duration::from_millis(200)) {
                    Ok(level) => {
                        let _ = app_handle.emit("audio-level", level);
                    }
                    Err(_) => continue,
                }
            }
        })
        .map_err(|e| format!("spawn level relay: {e}"))?;

    let started_at = Instant::now();
    let running = Box::new(RunningRecording {
        pid: resolved_pid,
        source_app: source_app.clone(),
        output_path: output_path.clone(),
        started_at,
        started_at_iso: started_at_iso.clone(),
        mic_mix: mic_capture.is_some(),
        stop_flag,
        capture,
        _mic: mic_capture,
        mixer_join: Some(mixer_join),
        encoder_join: Some(encoder_join),
        level_drain_join: Some(level_drain_join),
    });

    let status = RecordingStatus::Recording {
        pid: resolved_pid,
        source_app,
        output_path: output_path.to_string_lossy().to_string(),
        started_at_iso,
        mic_mix: running.mic_mix,
    };
    *guard = RecorderInner::Recording(running);
    Ok(status)
}

#[tauri::command]
pub fn stop_recording(state: State<'_, RecorderState>) -> std::result::Result<RecordingMeta, String> {
    let mut guard = state.lock();
    if !matches!(*guard, RecorderInner::Recording(_)) {
        return Err("not currently recording".into());
    }
    let RecorderInner::Recording(mut rec) =
        std::mem::replace(&mut *guard, RecorderInner::Idle)
    else {
        unreachable!()
    };

    rec.stop_flag.store(true, Ordering::SeqCst);
    rec.capture.stop();

    if let Some(j) = rec.mixer_join.take() {
        let _ = j.join();
    }
    if let Some(j) = rec.encoder_join.take() {
        match j.join() {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                let _ = state.app_handle.emit("recording-error", format!("{e}"));
            }
            Err(_) => {
                let _ = state.app_handle.emit("recording-error", "encoder thread panicked".to_string());
            }
        }
    }
    if let Some(j) = rec.level_drain_join.take() {
        let _ = j.join();
    }

    let duration = rec.started_at.elapsed().as_secs_f64();
    Ok(RecordingMeta {
        output_path: rec.output_path.to_string_lossy().to_string(),
        duration_seconds: duration,
        source_app: rec.source_app,
    })
}

#[tauri::command]
pub fn get_recording_state(state: State<'_, RecorderState>) -> RecordingStatus {
    let guard = state.lock();
    match &*guard {
        RecorderInner::Idle => RecordingStatus::Idle,
        RecorderInner::Recording(r) => RecordingStatus::Recording {
            pid: r.pid,
            source_app: r.source_app.clone(),
            output_path: r.output_path.to_string_lossy().to_string(),
            started_at_iso: r.started_at_iso.clone(),
            mic_mix: r.mic_mix,
        },
    }
}

#[tauri::command]
pub fn check_permissions() -> PermissionStatus {
    #[cfg(target_os = "macos")]
    {
        PermissionStatus {
            screen_recording: crate::capture::macos::screen_recording_authorized(),
            microphone: true,
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        PermissionStatus {
            screen_recording: true,
            microphone: true,
        }
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SettingsPane {
    ScreenRecording,
    Microphone,
}

#[tauri::command]
pub fn open_system_settings(pane: SettingsPane) -> std::result::Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let url = match pane {
            SettingsPane::ScreenRecording => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture"
            }
            SettingsPane::Microphone => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone"
            }
        };
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = pane;
        Ok(())
    }
}

#[tauri::command]
pub fn default_recordings_dir() -> String {
    default_dir().to_string_lossy().to_string()
}

fn default_dir() -> PathBuf {
    let home = dirs_home();
    home.join("Documents").join("AudioRecorder")
}

fn dirs_home() -> PathBuf {
    if let Some(h) = std::env::var_os("HOME") {
        return PathBuf::from(h);
    }
    if let Some(p) = std::env::var_os("USERPROFILE") {
        return PathBuf::from(p);
    }
    PathBuf::from(".")
}

fn default_output_path(source_app: &str) -> PathBuf {
    let safe: String = source_app
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    default_dir().join(format!("{safe}-{stamp}.mp3"))
}
