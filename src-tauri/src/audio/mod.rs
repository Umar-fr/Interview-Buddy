#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(target_os = "windows")]
pub mod windows;

use serde::{Deserialize, Serialize};

/// Audio sample data packet delivered by the capture backend.
#[derive(Debug, Clone)]
pub struct AudioChunk {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub channels: u32,
}

/// Capture status payload emitted to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioStatusEvent {
    pub status: String,
    pub message: String,
    pub is_active: bool,
}

/// Real-time audio activity meter level emitted to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioLevelEvent {
    /// Root Mean Square level (0.0 to 1.0)
    pub rms: f32,
    /// Peak amplitude level (0.0 to 1.0)
    pub peak: f32,
}

use rubato::{FastFixedIn, PolynomialDegree, Resampler};

/// Stream resampler converting any input sample rate to target rate (16kHz mono for Whisper).
pub struct AudioResampler {
    resampler: Option<FastFixedIn<f32>>,
    source_rate: u32,
    target_rate: u32,
    buffer: Vec<f32>,
    chunk_size: usize,
}

impl AudioResampler {
    pub fn new(source_rate: u32, target_rate: u32) -> Self {
        let chunk_size = 1024;
        let resampler = if source_rate != target_rate && source_rate > 0 && target_rate > 0 {
            FastFixedIn::<f32>::new(
                target_rate as f64 / source_rate as f64,
                2.0,
                PolynomialDegree::Cubic,
                chunk_size,
                1,
            ).ok()
        } else {
            None
        };

        Self {
            resampler,
            source_rate,
            target_rate,
            buffer: Vec::new(),
            chunk_size,
        }
    }

    pub fn process(&mut self, input_samples: &[f32], current_rate: u32) -> Vec<f32> {
        if current_rate != self.source_rate {
            *self = Self::new(current_rate, self.target_rate);
        }

        if let Some(ref mut resampler) = self.resampler {
            self.buffer.extend_from_slice(input_samples);
            let mut output = Vec::new();

            while self.buffer.len() >= self.chunk_size {
                let chunk: Vec<f32> = self.buffer.drain(0..self.chunk_size).collect();
                if let Ok(mut resampled) = resampler.process(&[chunk], None) {
                    if !resampled.is_empty() {
                        output.append(&mut resampled.remove(0));
                    }
                }
            }

            output
        } else {
            input_samples.to_vec()
        }
    }
}

/// Calculate RMS (Root Mean Square) volume level from PCM float samples.
pub fn calculate_rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_squares: f32 = samples.iter().map(|&s| s * s).sum();
    (sum_squares / samples.len() as f32).sqrt()
}

/// Calculate Peak volume level from PCM float samples.
pub fn calculate_peak(samples: &[f32]) -> f32 {
    samples.iter().copied().map(f32::abs).fold(0.0f32, f32::max)
}
