//! Windows per-process audio capture via WASAPI Process Loopback.
//!
//! Uses `ActivateAudioInterfaceAsync` with `AUDIOCLIENT_ACTIVATION_PARAMS`
//! configured for `PROCESS_LOOPBACK` mode and `INCLUDE_PROCESS_TREE` so that
//! a browser parent PID captures all of its renderer/utility children.
//! Min OS: Windows 10 21H2 (build 22000+) for reliable behaviour.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc as std_mpsc;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crossbeam_channel::Sender;
use windows::core::{implement, ComInterface, Interface, GUID, HRESULT, PCWSTR};
use windows::Win32::Foundation::{E_FAIL, S_OK, WAIT_OBJECT_0};
use windows::Win32::Media::Audio::{
    eRender, ActivateAudioInterfaceAsync, AudioCategory_Communications, IActivateAudioInterfaceAsyncOperation,
    IActivateAudioInterfaceCompletionHandler, IActivateAudioInterfaceCompletionHandler_Impl,
    IAudioCaptureClient, IAudioClient, AUDCLNT_E_DEVICE_IN_USE, AUDCLNT_SHAREMODE_SHARED,
    AUDCLNT_STREAMFLAGS_LOOPBACK, AUDIOCLIENT_ACTIVATION_PARAMS, AUDIOCLIENT_ACTIVATION_PARAMS_0,
    AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK, AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS,
    PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE, VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
    WAVEFORMATEX,
};
use windows::Win32::System::Com::{
    CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED, STGM_READ,
};
use windows::Win32::System::Threading::{CreateEventW, SetEvent, WaitForSingleObject, INFINITE};

use crate::capture::{AudioCapture, AudioChunk, AudioFormat, CaptureError, Result};

const WAVE_FORMAT_IEEE_FLOAT: u16 = 3;

const VAD_PROCESS_LOOPBACK_W: &[u16] = &[
    0x0056, 0x0041, 0x0044, 0x005C, 0x0050, 0x0072, 0x006F, 0x0063, 0x0065, 0x0073, 0x0073, 0x005F,
    0x004C, 0x006F, 0x006F, 0x0070, 0x0062, 0x0061, 0x0063, 0x006B, 0x0000,
];

pub struct WindowsCapture {
    stop: Arc<AtomicBool>,
    join: Option<thread::JoinHandle<()>>,
    format: AudioFormat,
}

impl WindowsCapture {
    pub fn start(pid: u32, tx: Sender<AudioChunk>) -> Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = stop.clone();

        let (ready_tx, ready_rx) = std_mpsc::channel::<std::result::Result<(), String>>();

        let join = thread::Builder::new()
            .name(format!("wasapi-loopback-{pid}"))
            .spawn(move || {
                let outcome = unsafe { run_capture(pid, tx, stop_thread, &ready_tx) };
                if let Err(e) = outcome {
                    let _ = ready_tx.send(Err(format!("{e}")));
                }
            })
            .map_err(|e| CaptureError::Platform(format!("spawn capture thread: {e}")))?;

        match ready_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => Ok(Self {
                stop,
                join: Some(join),
                format: AudioFormat::stereo_48k(),
            }),
            Ok(Err(msg)) => Err(if msg.contains("DEVICE_IN_USE") {
                CaptureError::DeviceBusy(msg)
            } else {
                CaptureError::Platform(msg)
            }),
            Err(_) => {
                stop.store(true, Ordering::SeqCst);
                Err(CaptureError::Platform("capture activation timeout".into()))
            }
        }
    }
}

impl AudioCapture for WindowsCapture {
    fn format(&self) -> AudioFormat {
        self.format
    }

    fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

impl Drop for WindowsCapture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

#[implement(IActivateAudioInterfaceCompletionHandler)]
struct CompletionHandler {
    event: windows::Win32::Foundation::HANDLE,
}

impl IActivateAudioInterfaceCompletionHandler_Impl for CompletionHandler {
    fn ActivateCompleted(
        &self,
        _operation: Option<&IActivateAudioInterfaceAsyncOperation>,
    ) -> windows::core::Result<()> {
        unsafe {
            let _ = SetEvent(self.event);
        }
        Ok(())
    }
}

unsafe fn run_capture(
    pid: u32,
    tx: Sender<AudioChunk>,
    stop: Arc<AtomicBool>,
    ready_tx: &std_mpsc::Sender<std::result::Result<(), String>>,
) -> std::result::Result<(), String> {
    CoInitializeEx(None, COINIT_MULTITHREADED).map_err(|e| e.to_string())?;
    let _co_guard = CoUninitGuard;

    let mut wave = WAVEFORMATEX {
        wFormatTag: WAVE_FORMAT_IEEE_FLOAT as u16,
        nChannels: 2,
        nSamplesPerSec: 48_000,
        wBitsPerSample: 32,
        nBlockAlign: 8,
        nAvgBytesPerSec: 48_000 * 8,
        cbSize: 0,
    };

    let mut params = AUDIOCLIENT_ACTIVATION_PARAMS {
        ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
        Anonymous: AUDIOCLIENT_ACTIVATION_PARAMS_0 {
            ProcessLoopbackParams: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS {
                TargetProcessId: pid,
                ProcessLoopbackMode: PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE,
            },
        },
    };

    let prop = windows::Win32::System::Com::StructuredStorage::PROPVARIANT::default();
    let mut prop_with_blob = prop;
    let blob_ptr = &mut params as *mut _ as *mut c_void;
    let blob_size = std::mem::size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>() as u32;

    // Build PROPVARIANT containing a BLOB pointing to params.
    let mut blob = windows::Win32::System::Com::StructuredStorage::PROPVARIANT::default();
    set_propvariant_blob(&mut blob, blob_ptr, blob_size);

    let event = CreateEventW(None, false, false, PCWSTR::null())
        .map_err(|e| format!("CreateEventW: {e}"))?;
    let handler = CompletionHandler { event }.into();
    let handler_iface: IActivateAudioInterfaceCompletionHandler = handler;

    let op = ActivateAudioInterfaceAsync(
        PCWSTR(VAD_PROCESS_LOOPBACK_W.as_ptr()),
        &IAudioClient::IID,
        Some(&blob as *const _),
        &handler_iface,
    )
    .map_err(|e| format!("ActivateAudioInterfaceAsync: {e}"))?;

    if WaitForSingleObject(event, 5000) != WAIT_OBJECT_0 {
        return Err("activation wait timed out".into());
    }
    let _ = windows::Win32::Foundation::CloseHandle(event);
    let mut activated_iface: Option<windows::core::IUnknown> = None;
    let mut activate_hr = HRESULT(0);
    op.GetActivateResult(&mut activate_hr, &mut activated_iface)
        .map_err(|e| format!("GetActivateResult: {e}"))?;

    if activate_hr.0 == AUDCLNT_E_DEVICE_IN_USE.0 {
        return Err("AUDCLNT_E_DEVICE_IN_USE".into());
    }
    if activate_hr.0 != S_OK.0 {
        return Err(format!("activate hr=0x{:08x}", activate_hr.0));
    }

    let audio_client: IAudioClient = activated_iface
        .ok_or_else(|| "no IUnknown returned".to_string())?
        .cast()
        .map_err(|e| format!("cast IAudioClient: {e}"))?;

    audio_client
        .Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            AUDCLNT_STREAMFLAGS_LOOPBACK,
            10_000_000,
            0,
            &wave,
            None,
        )
        .map_err(|e| format!("IAudioClient::Initialize: {e}"))?;

    let capture_client: IAudioCaptureClient = audio_client
        .GetService()
        .map_err(|e| format!("GetService(IAudioCaptureClient): {e}"))?;

    audio_client
        .Start()
        .map_err(|e| format!("IAudioClient::Start: {e}"))?;

    let _ = ready_tx.send(Ok(()));

    while !stop.load(Ordering::SeqCst) {
        let mut packet_size = capture_client
            .GetNextPacketSize()
            .map_err(|e| format!("GetNextPacketSize: {e}"))?;
        if packet_size == 0 {
            std::thread::sleep(Duration::from_millis(5));
            continue;
        }
        while packet_size > 0 {
            let mut data_ptr: *mut u8 = std::ptr::null_mut();
            let mut frames: u32 = 0;
            let mut flags: u32 = 0;
            capture_client
                .GetBuffer(&mut data_ptr, &mut frames, &mut flags, None, None)
                .map_err(|e| format!("GetBuffer: {e}"))?;

            if frames > 0 && !data_ptr.is_null() {
                let total_floats = frames as usize * wave.nChannels as usize;
                let slice = std::slice::from_raw_parts(data_ptr as *const f32, total_floats);
                let _ = tx.try_send(AudioChunk {
                    samples: Arc::new(slice.to_vec()),
                });
            }

            capture_client
                .ReleaseBuffer(frames)
                .map_err(|e| format!("ReleaseBuffer: {e}"))?;
            packet_size = capture_client
                .GetNextPacketSize()
                .map_err(|e| format!("GetNextPacketSize: {e}"))?;
        }
    }

    let _ = audio_client.Stop();

    let _ = (prop_with_blob, blob_size, INFINITE, _co_guard);
    Ok(())
}

unsafe fn set_propvariant_blob(
    pv: &mut windows::Win32::System::Com::StructuredStorage::PROPVARIANT,
    ptr: *mut c_void,
    size: u32,
) {
    use windows::Win32::System::Variant::VT_BLOB;
    let inner = &mut *pv.Anonymous.Anonymous;
    inner.vt = VT_BLOB;
    inner.Anonymous.blob.cbSize = size;
    inner.Anonymous.blob.pBlobData = ptr as *mut u8;
}

struct CoUninitGuard;
impl Drop for CoUninitGuard {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}
