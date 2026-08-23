use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use super::AudioChunk;

pub struct WindowsAudioCapture {
    _stream: Option<cpal::Stream>,
    is_running: Arc<AtomicBool>,
}

impl WindowsAudioCapture {
    pub fn start(
        target_sample_rate: u32,
        target_channels: u32,
    ) -> Result<(Self, Receiver<AudioChunk>, Receiver<(i32, String)>), String> {
        let (data_tx, data_rx) = channel::<AudioChunk>();
        let (status_tx, status_rx) = channel::<(i32, String)>();

        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| "No default output audio device found on Windows".to_string())?;

        let device_name = device
            .name()
            .unwrap_or_else(|_| "Default Audio Device".to_string());

        let config = device
            .default_output_config()
            .map_err(|e| format!("Failed to get default output config: {}", e))?;

        let sample_rate = config.sample_rate().0;
        let channels = config.channels();
        let sample_format = config.sample_format();

        let _ = status_tx.send((
            0,
            format!(
                "Capturing Windows loopback: {} ({}Hz, {}ch)",
                device_name, sample_rate, channels
            ),
        ));

        let is_running = Arc::new(AtomicBool::new(true));
        let is_running_clone = is_running.clone();
        let err_status_tx = status_tx.clone();

        let err_fn = move |err| {
            let _ = err_status_tx.send((-1, format!("Windows audio stream error: {}", err)));
        };

        let stream = match sample_format {
            cpal::SampleFormat::F32 => {
                let tx = data_tx.clone();
                device.build_input_stream(
                    &config.into(),
                    move |data: &[f32], _: &cpal::InputCallbackInfo| {
                        if !is_running_clone.load(Ordering::Relaxed) {
                            return;
                        }
                        // Downmix to mono
                        let num_frames = data.len() / (channels as usize);
                        let mut mono_samples = Vec::with_capacity(num_frames);
                        for frame in data.chunks_exact(channels as usize) {
                            let sum: f32 = frame.iter().sum();
                            mono_samples.push(sum / (channels as f32));
                        }

                        // Resample to target sample rate (e.g. 16kHz) if necessary
                        let resampled = resample_linear(&mono_samples, sample_rate, target_sample_rate);

                        let _ = tx.send(AudioChunk {
                            samples: resampled,
                            sample_rate: target_sample_rate,
                            channels: target_channels,
                        });
                    },
                    err_fn,
                    None,
                )
            }
            cpal::SampleFormat::I16 => {
                let tx = data_tx.clone();
                device.build_input_stream(
                    &config.into(),
                    move |data: &[i16], _: &cpal::InputCallbackInfo| {
                        if !is_running_clone.load(Ordering::Relaxed) {
                            return;
                        }
                        let num_frames = data.len() / (channels as usize);
                        let mut mono_samples = Vec::with_capacity(num_frames);
                        for frame in data.chunks_exact(channels as usize) {
                            let sum: f32 = frame.iter().map(|&s| s as f32 / 32768.0).sum();
                            mono_samples.push(sum / (channels as f32));
                        }

                        let resampled = resample_linear(&mono_samples, sample_rate, target_sample_rate);

                        let _ = tx.send(AudioChunk {
                            samples: resampled,
                            sample_rate: target_sample_rate,
                            channels: target_channels,
                        });
                    },
                    err_fn,
                    None,
                )
            }
            _ => {
                return Err(format!("Unsupported audio sample format: {:?}", sample_format));
            }
        }
        .map_err(|e| format!("Failed to build Windows audio loopback stream: {}", e))?;

        stream
            .play()
            .map_err(|e| format!("Failed to start audio stream: {}", e))?;

        Ok((
            Self {
                _stream: Some(stream),
                is_running,
            },
            data_rx,
            status_rx,
        ))
    }
}

impl Drop for WindowsAudioCapture {
    fn drop(&mut self) {
        self.is_running.store(false, Ordering::SeqCst);
    }
}

/// Simple linear resampler from `src_rate` to `dst_rate`
fn resample_linear(input: &[f32], src_rate: u32, dst_rate: u32) -> Vec<f32> {
    if src_rate == dst_rate || input.is_empty() {
        return input.to_vec();
    }

    let ratio = src_rate as f64 / dst_rate as f64;
    let out_len = ((input.len() as f64) / ratio).round() as usize;
    let mut output = Vec::with_capacity(out_len);

    for i in 0..out_len {
        let src_idx = (i as f64) * ratio;
        let idx0 = src_idx.floor() as usize;
        let idx1 = (idx0 + 1).min(input.len() - 1);
        let frac = (src_idx - (idx0 as f64)) as f32;

        if idx0 < input.len() {
            let s0 = input[idx0];
            let s1 = input[idx1];
            output.push(s0 + frac * (s1 - s0));
        }
    }

    output
}
