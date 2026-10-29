use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rubato::{Resampler, FastFixedIn, PolynomialDegree};
use super::AudioChunk;

pub struct WindowsAudioCapture {
    stop_tx: Option<Sender<()>>,
    is_running: Arc<AtomicBool>,
}

impl WindowsAudioCapture {
    pub fn start(
        target_sample_rate: u32,
        target_channels: u32,
    ) -> Result<(Self, Receiver<AudioChunk>, Receiver<(i32, String)>), String> {
        let (data_tx, data_rx) = super::audio_chunk_channel();
        let (status_tx, status_rx) = channel::<(i32, String)>();

        let is_running = Arc::new(AtomicBool::new(true));
        let is_running_clone = is_running.clone();
        let (stop_tx, stop_rx) = channel::<()>();

        thread::spawn(move || {
            let host = cpal::default_host();

            while is_running_clone.load(Ordering::Relaxed) {
                // Attempt to find default output device for WASAPI loopback
                let device = match host.default_output_device() {
                    Some(d) => d,
                    None => {
                        let _ = status_tx.send((-1, "No default output audio device found. Retrying...".to_string()));
                        thread::sleep(Duration::from_millis(1500));
                        continue;
                    }
                };

                let device_name = device.name().unwrap_or_else(|_| "Default Audio Device".to_string());
                let config = match device.default_output_config() {
                    Ok(c) => c,
                    Err(e) => {
                        let _ = status_tx.send((-1, format!("Failed to get output config: {}. Retrying...", e)));
                        thread::sleep(Duration::from_millis(1500));
                        continue;
                    }
                };

                let sample_rate = config.sample_rate().0;
                let channels = config.channels();
                let sample_format = config.sample_format();
                if channels == 0 {
                    let _ = status_tx.send((-1, "Default audio device reported zero channels. Retrying...".to_string()));
                    thread::sleep(Duration::from_millis(1500));
                    continue;
                }

                let _ = status_tx.send((
                    0,
                    format!("Capturing Windows loopback: {} ({}Hz, {}ch)", device_name, sample_rate, channels),
                ));

                let stream_err_flag = Arc::new(AtomicBool::new(false));
                let stream_err_flag_clone = stream_err_flag.clone();
                let err_status_tx = status_tx.clone();

                let err_fn = move |err| {
                    eprintln!("[Windows Audio Error]: {}", err);
                    let _ = err_status_tx.send((-1, format!("Windows audio stream error: {}", err)));
                    stream_err_flag_clone.store(true, Ordering::SeqCst);
                };

                let chunk_size = 1024;
                let resampler_res = if sample_rate != target_sample_rate && sample_rate > 0 {
                    FastFixedIn::<f32>::new(
                        target_sample_rate as f64 / sample_rate as f64,
                        2.0,
                        PolynomialDegree::Cubic,
                        chunk_size,
                        1,
                    ).ok()
                } else {
                    None
                };

                let tx = data_tx.clone();
                let is_running_stream = is_running_clone.clone();

                let stream_result = match sample_format {
                    cpal::SampleFormat::F32 => {
                        let mut resampler = resampler_res;
                        let mut buffer: Vec<f32> = Vec::new();
                        device.build_input_stream(
                            &config.into(),
                            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                                if !is_running_stream.load(Ordering::Relaxed) {
                                    return;
                                }
                                for frame in data.chunks_exact(channels as usize) {
                                    let sum: f32 = frame.iter().sum();
                                    buffer.push(sum / (channels as f32));
                                }

                                if let Some(ref mut r) = resampler {
                                    while buffer.len() >= chunk_size {
                                        let chunk: Vec<f32> = buffer.drain(0..chunk_size).collect();
                                        if let Ok(mut resampled) = r.process(&[chunk], None) {
                                            if !resampled.is_empty() {
                                                super::try_send_audio_chunk(&tx, AudioChunk {
                                                    samples: resampled.remove(0),
                                                    sample_rate: target_sample_rate,
                                                    channels: target_channels,
                                                });
                                            }
                                        }
                                    }
                                } else if !buffer.is_empty() {
                                    let samples = std::mem::take(&mut buffer);
                                    super::try_send_audio_chunk(&tx, AudioChunk {
                                        samples,
                                        sample_rate: target_sample_rate,
                                        channels: target_channels,
                                    });
                                }
                            },
                            err_fn,
                            None,
                        )
                    }
                    cpal::SampleFormat::I16 => {
                        let mut resampler = resampler_res;
                        let mut buffer: Vec<f32> = Vec::new();
                        device.build_input_stream(
                            &config.into(),
                            move |data: &[i16], _: &cpal::InputCallbackInfo| {
                                if !is_running_stream.load(Ordering::Relaxed) {
                                    return;
                                }
                                for frame in data.chunks_exact(channels as usize) {
                                    let sum: f32 = frame.iter().map(|&s| s as f32 / 32768.0).sum();
                                    buffer.push(sum / (channels as f32));
                                }

                                if let Some(ref mut r) = resampler {
                                    while buffer.len() >= chunk_size {
                                        let chunk: Vec<f32> = buffer.drain(0..chunk_size).collect();
                                        if let Ok(mut resampled) = r.process(&[chunk], None) {
                                            if !resampled.is_empty() {
                                                super::try_send_audio_chunk(&tx, AudioChunk {
                                                    samples: resampled.remove(0),
                                                    sample_rate: target_sample_rate,
                                                    channels: target_channels,
                                                });
                                            }
                                        }
                                    }
                                } else if !buffer.is_empty() {
                                    let samples = std::mem::take(&mut buffer);
                                    super::try_send_audio_chunk(&tx, AudioChunk {
                                        samples,
                                        sample_rate: target_sample_rate,
                                        channels: target_channels,
                                    });
                                }
                            },
                            err_fn,
                            None,
                        )
                    }
                    cpal::SampleFormat::I32 => {
                        let mut resampler = resampler_res;
                        let mut buffer: Vec<f32> = Vec::new();
                        device.build_input_stream(
                            &config.into(),
                            move |data: &[i32], _: &cpal::InputCallbackInfo| {
                                if !is_running_stream.load(Ordering::Relaxed) {
                                    return;
                                }
                                for frame in data.chunks_exact(channels as usize) {
                                    let sum: f32 = frame.iter().map(|&s| s as f32 / 2147483648.0).sum();
                                    buffer.push(sum / (channels as f32));
                                }

                                if let Some(ref mut r) = resampler {
                                    while buffer.len() >= chunk_size {
                                        let chunk: Vec<f32> = buffer.drain(0..chunk_size).collect();
                                        if let Ok(mut resampled) = r.process(&[chunk], None) {
                                            if !resampled.is_empty() {
                                                super::try_send_audio_chunk(&tx, AudioChunk {
                                                    samples: resampled.remove(0),
                                                    sample_rate: target_sample_rate,
                                                    channels: target_channels,
                                                });
                                            }
                                        }
                                    }
                                } else if !buffer.is_empty() {
                                    let samples = std::mem::take(&mut buffer);
                                    super::try_send_audio_chunk(&tx, AudioChunk {
                                        samples,
                                        sample_rate: target_sample_rate,
                                        channels: target_channels,
                                    });
                                }
                            },
                            err_fn,
                            None,
                        )
                    }
                    _ => {
                        let _ = status_tx.send((-1, format!("Unsupported sample format: {:?}", sample_format)));
                        thread::sleep(Duration::from_millis(2000));
                        continue;
                    }
                };

                match stream_result {
                    Ok(stream) => {
                        if let Err(e) = stream.play() {
                            let _ = status_tx.send((-1, format!("Failed to play stream: {}. Retrying...", e)));
                            thread::sleep(Duration::from_millis(1500));
                            continue;
                        }

                        // Loop while stream is active and not stopped
                        while is_running_clone.load(Ordering::Relaxed) && !stream_err_flag.load(Ordering::Relaxed) {
                            if stop_rx.try_recv().is_ok() {
                                return;
                            }
                            thread::sleep(Duration::from_millis(100));
                        }
                    }
                    Err(e) => {
                        let _ = status_tx.send((-1, format!("Failed to build input stream: {}. Retrying...", e)));
                        thread::sleep(Duration::from_millis(1500));
                    }
                }
            }
        });

        Ok((
            Self {
                stop_tx: Some(stop_tx),
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
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(());
        }
    }
}
