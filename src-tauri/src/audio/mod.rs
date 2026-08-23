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
