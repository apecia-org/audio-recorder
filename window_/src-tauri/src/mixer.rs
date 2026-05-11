use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};

use crate::capture::AudioChunk;

#[derive(Debug, Clone, Copy)]
pub struct MixerConfig {
    pub mic_enabled: bool,
    pub mic_gain: f32,
}

impl Default for MixerConfig {
    fn default() -> Self {
        Self {
            mic_enabled: false,
            mic_gain: 0.8,
        }
    }
}

pub fn run(
    app_rx: Receiver<AudioChunk>,
    mic_rx: Option<Receiver<AudioChunk>>,
    out_tx: Sender<AudioChunk>,
    cfg: MixerConfig,
    stop: Arc<AtomicBool>,
    level_tx: Sender<f32>,
) {
    let mut mic_buf: VecDeque<f32> = VecDeque::with_capacity(48_000 * 2);
    let mut last_level_emit = Instant::now();
    let level_interval = Duration::from_millis(33);

    while !stop.load(Ordering::SeqCst) {
        match app_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(chunk) => {
                let mut samples = (*chunk.samples).clone();

                if cfg.mic_enabled {
                    if let Some(rx) = mic_rx.as_ref() {
                        drain_mic_into(rx, &mut mic_buf, samples.len());
                        for sample in samples.iter_mut() {
                            let mic_sample = mic_buf.pop_front().unwrap_or(0.0) * cfg.mic_gain;
                            let summed = *sample + mic_sample;
                            *sample = summed.tanh();
                        }
                    }
                }

                for sample in samples.iter_mut() {
                    if sample.abs() < 1e-30 {
                        *sample = 0.0;
                    }
                }

                if last_level_emit.elapsed() >= level_interval {
                    let rms = compute_rms(&samples);
                    let _ = level_tx.try_send(rms);
                    last_level_emit = Instant::now();
                }

                if out_tx
                    .send(AudioChunk {
                        samples: Arc::new(samples),
                    })
                    .is_err()
                {
                    break;
                }
            }
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
}

fn drain_mic_into(rx: &Receiver<AudioChunk>, buf: &mut VecDeque<f32>, target_len: usize) {
    while buf.len() < target_len {
        match rx.try_recv() {
            Ok(chunk) => buf.extend(chunk.samples.iter().copied()),
            Err(_) => break,
        }
    }
}

fn compute_rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f32 = samples.iter().map(|s| s * s).sum();
    (sum_sq / samples.len() as f32).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rms_zero_for_silence() {
        let s = vec![0.0; 1024];
        assert_eq!(compute_rms(&s), 0.0);
    }

    #[test]
    fn rms_full_scale() {
        let s = vec![1.0; 1024];
        assert!((compute_rms(&s) - 1.0).abs() < 1e-6);
    }
}
