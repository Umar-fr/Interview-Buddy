use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::thread;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rubato::{Resampler, FastFixedIn, PolynomialDegree};
use super::AudioChunk;

pub struct WindowsAudioCapture {
    stop_tx: Option<Sender<()>>,
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
        let (stop_tx, stop_rx) = channel::<()>();

        thread::spawn(move || {
            let err_status_tx_fn = err_status_tx.clone();
            let err_fn = move |err| {
                let _ = err_status_tx_fn.send((-1, format!("Windows audio stream error: {}", err)));
            };

        let stream_result: Result<cpal::Stream, String> = match sample_format {
            cpal::SampleFormat::F32 => {
                let tx = data_tx.clone();
                let mut buffer = Vec::new();
                let chunk_size = 1024;
                
                let resampler_res = if sample_rate != target_sample_rate {
                    match FastFixedIn::<f32>::new(
                        target_sample_rate as f64 / sample_rate as f64,
                        2.0,
                        PolynomialDegree::Cubic,
                        chunk_size,
                        1
                    ) {
                        Ok(r) => Ok(Some(r)),
                        Err(e) => Err(format!("Failed to create resampler: {}", e)),
                    }
                } else {
                    Ok(None)
                };

                match resampler_res {
                    Ok(mut resampler) => {
                        device.build_input_stream(
                            &config.into(),
                            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                                if !is_running_clone.load(Ordering::Relaxed) {
                                    return;
                                }
                                
                                // Downmix to mono
                                for frame in data.chunks_exact(channels as usize) {
                                    let sum: f32 = frame.iter().sum();
                                    buffer.push(sum / (channels as f32));
                                }

                                if let Some(resampler) = &mut resampler {
                                    while buffer.len() >= chunk_size {
                                        let chunk: Vec<f32> = buffer.drain(0..chunk_size).collect();
                                        if let Ok(mut resampled) = resampler.process(&[chunk], None) {
                                            let _ = tx.send(AudioChunk {
                                                samples: resampled.remove(0),
                                                sample_rate: target_sample_rate,
                                                channels: target_channels,
                                            });
                                        }
                                    }
                                } else {
                                    // No resampling needed
                                    if !buffer.is_empty() {
                                        let samples = std::mem::take(&mut buffer);
                                        let _ = tx.send(AudioChunk {
                                            samples,
                                            sample_rate: target_sample_rate,
                                            channels: target_channels,
                                        });
                                    }
                                }
                            },
                            err_fn,
                            None,
                        ).map_err(|e| format!("Stream build error: {}", e))
                    }
                    Err(e) => Err(e)
                }
            }
            cpal::SampleFormat::I16 => {
                let tx = data_tx.clone();
                let mut buffer = Vec::new();
                let chunk_size = 1024;

                let resampler_res = if sample_rate != target_sample_rate {
                    match FastFixedIn::<f32>::new(
                        target_sample_rate as f64 / sample_rate as f64,
                        2.0,
                        PolynomialDegree::Cubic,
                        chunk_size,
                        1
                    ) {
                        Ok(r) => Ok(Some(r)),
                        Err(e) => Err(format!("Failed to create resampler: {}", e)),
                    }
                } else {
                    Ok(None)
                };

                match resampler_res {
                    Ok(mut resampler) => {
                        device.build_input_stream(
                            &config.into(),
                            move |data: &[i16], _: &cpal::InputCallbackInfo| {
                                if !is_running_clone.load(Ordering::Relaxed) {
                                    return;
                                }

                                for frame in data.chunks_exact(channels as usize) {
                                    let sum: f32 = frame.iter().map(|&s| s as f32 / 32768.0).sum();
                                    buffer.push(sum / (channels as f32));
                                }

                                if let Some(resampler) = &mut resampler {
                                    while buffer.len() >= chunk_size {
                                        let chunk: Vec<f32> = buffer.drain(0..chunk_size).collect();
                                        if let Ok(mut resampled) = resampler.process(&[chunk], None) {
                                            let _ = tx.send(AudioChunk {
                                                samples: resampled.remove(0),
                                                sample_rate: target_sample_rate,
                                                channels: target_channels,
                                            });
                                        }
                                    }
                                } else {
                                    if !buffer.is_empty() {
                                        let samples = std::mem::take(&mut buffer);
                                        let _ = tx.send(AudioChunk {
                                            samples,
                                            sample_rate: target_sample_rate,
                                            channels: target_channels,
                                        });
                                    }
                                }
                            },
                            err_fn,
                            None,
                        ).map_err(|e| format!("Stream build error: {}", e))
                    }
                    Err(e) => Err(e)
                }
            }
            _ => {
                Err(format!("Unsupported audio sample format: {:?}", sample_format))
            }
        };

        match stream_result {
            Ok(stream) => {
                if let Err(e) = stream.play() {
                    let _ = err_status_tx.send((-1, format!("Failed to play stream: {}", e)));
                    return;
                }
                // Keep the stream alive until we receive a stop signal
                let _ = stop_rx.recv();
            }
            Err(e) => {
                let _ = err_status_tx.send((-1, e));
            }
        }
        });

        Ok((
            Self {
                stop_tx: Some(stop_tx),
            },
            data_rx,
            status_rx,
        ))
    }
}

impl Drop for WindowsAudioCapture {
    fn drop(&mut self) {
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(());
        }
    }
}
