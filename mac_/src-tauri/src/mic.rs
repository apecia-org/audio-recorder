//! Optional microphone capture using cpal.
//!
//! cpal's `Stream` is neither `Send` nor `Sync` on macOS, so the stream is
//! built and owned on a dedicated worker thread. The struct returned to
//! callers only holds a stop flag and the join handle, both of which are
//! `Send`/`Sync`. Audio samples are forwarded via a `crossbeam_channel`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc as std_mpsc;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleFormat, Stream, StreamConfig};
use crossbeam_channel::Sender;

use crate::capture::AudioChunk;

pub struct MicCapture {
    stop: Arc<AtomicBool>,
    join: Option<thread::JoinHandle<()>>,
}

impl MicCapture {
    pub fn start(tx: Sender<AudioChunk>) -> Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = stop.clone();

        let (ready_tx, ready_rx) = std_mpsc::channel::<std::result::Result<(), String>>();

        let join = thread::Builder::new()
            .name("mic-capture".into())
            .spawn(move || {
                if let Err(e) = run_stream(tx, stop_thread, &ready_tx) {
                    let _ = ready_tx.send(Err(e.to_string()));
                }
            })
            .context("spawn mic thread")?;

        match ready_rx.recv_timeout(Duration::from_secs(3)) {
            Ok(Ok(())) => Ok(Self {
                stop,
                join: Some(join),
            }),
            Ok(Err(msg)) => Err(anyhow!("{msg}")),
            Err(_) => {
                stop.store(true, Ordering::SeqCst);
                Err(anyhow!("mic startup timeout"))
            }
        }
    }
}

impl Drop for MicCapture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

fn run_stream(
    tx: Sender<AudioChunk>,
    stop: Arc<AtomicBool>,
    ready_tx: &std_mpsc::Sender<std::result::Result<(), String>>,
) -> Result<()> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| anyhow!("no default input device"))?;
    let supported = device
        .default_input_config()
        .context("query default input config")?;

    let sample_format = supported.sample_format();
    let config: StreamConfig = supported.into();
    let input_channels = config.channels;

    let stream = build_stream(&device, &config, sample_format, input_channels, tx)?;
    stream.play().context("play mic stream")?;

    let _ = ready_tx.send(Ok(()));

    while !stop.load(Ordering::SeqCst) {
        thread::sleep(Duration::from_millis(50));
    }

    drop(stream);
    Ok(())
}

fn build_stream(
    device: &Device,
    config: &StreamConfig,
    fmt: SampleFormat,
    input_channels: u16,
    tx: Sender<AudioChunk>,
) -> Result<Stream> {
    let err = |e| eprintln!("mic stream error: {e}");

    let stream = match fmt {
        SampleFormat::F32 => device.build_input_stream(
            config,
            move |data: &[f32], _| {
                push_interleaved_f32(data, input_channels, &tx);
            },
            err,
            None,
        ),
        SampleFormat::I16 => device.build_input_stream(
            config,
            move |data: &[i16], _| {
                let f: Vec<f32> = data.iter().map(|s| *s as f32 / i16::MAX as f32).collect();
                push_interleaved_f32(&f, input_channels, &tx);
            },
            err,
            None,
        ),
        SampleFormat::U16 => device.build_input_stream(
            config,
            move |data: &[u16], _| {
                let f: Vec<f32> = data
                    .iter()
                    .map(|s| (*s as f32 - 32768.0) / 32768.0)
                    .collect();
                push_interleaved_f32(&f, input_channels, &tx);
            },
            err,
            None,
        ),
        other => return Err(anyhow!("unsupported mic sample format: {other:?}")),
    }
    .context("build mic stream")?;

    Ok(stream)
}

fn push_interleaved_f32(data: &[f32], input_channels: u16, tx: &Sender<AudioChunk>) {
    let stereo: Vec<f32> = match input_channels {
        1 => {
            let mut out = Vec::with_capacity(data.len() * 2);
            for s in data {
                out.push(*s);
                out.push(*s);
            }
            out
        }
        2 => data.to_vec(),
        n => {
            let frames = data.len() / n as usize;
            let mut out = Vec::with_capacity(frames * 2);
            for f in 0..frames {
                let base = f * n as usize;
                out.push(data[base]);
                out.push(data[base + 1]);
            }
            out
        }
    };

    let _ = tx.try_send(AudioChunk {
        samples: Arc::new(stereo),
    });
}
