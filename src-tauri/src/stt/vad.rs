use std::time::{Duration, Instant};

/// Audio segment ready for Whisper processing
pub struct SpeechSegment {
    pub samples: Vec<f32>,
    pub timestamp_ms: u64,
}

/// Dynamic Voice Activity Detector and Audio Segmenter
pub struct AudioSegmenter {
    sample_rate: u32,
    threshold: f32,
    buffer: Vec<f32>,
    is_speaking: bool,
    last_speech_time: Option<Instant>,
    speech_start_time: Option<Instant>,
    silence_duration: Duration,
    max_speech_duration: Duration,
    min_speech_samples: usize,
}

impl AudioSegmenter {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            sample_rate,
            threshold: 0.010, // Sensitive RMS energy threshold
            buffer: Vec::with_capacity(sample_rate as usize * 5),
            is_speaking: false,
            last_speech_time: None,
            speech_start_time: None,
            silence_duration: Duration::from_millis(250), // Rapid flush after 250ms silence for low latency
            max_speech_duration: Duration::from_millis(1500), // Max 1.5s per chunk for much faster responses
            min_speech_samples: (sample_rate as usize * 3) / 10, // At least 0.3s audio
        }
    }

    /// Process a stream of PCM float samples and return completed speech segments
    pub fn process_samples(&mut self, samples: &[f32]) -> Option<SpeechSegment> {
        let now = Instant::now();
        let chunk_rms = crate::audio::calculate_rms(samples);

        let energy_detected = chunk_rms > self.threshold;

        if energy_detected {
            if !self.is_speaking {
                self.is_speaking = true;
                self.speech_start_time = Some(now);
            }
            self.last_speech_time = Some(now);
            self.buffer.extend_from_slice(samples);
        } else if self.is_speaking {
            // Include trailing audio
            self.buffer.extend_from_slice(samples);

            // Check if silence timeout has elapsed
            if let Some(last_speech) = self.last_speech_time {
                if now.duration_since(last_speech) >= self.silence_duration {
                    return self.flush_segment();
                }
            }
        }

        // Check if maximum continuous speech duration is reached to avoid buffering delay
        if self.is_speaking {
            if let Some(start) = self.speech_start_time {
                if now.duration_since(start) >= self.max_speech_duration {
                    return self.flush_segment();
                }
            }
        }

        None
    }

    /// Flush the accumulated audio buffer as a speech segment if it contains enough samples
    pub fn flush_segment(&mut self) -> Option<SpeechSegment> {
        self.is_speaking = false;
        self.last_speech_time = None;
        self.speech_start_time = None;

        if self.buffer.len() >= self.min_speech_samples {
            let mut segment_samples = std::mem::take(&mut self.buffer);
            
            // Pad to at least 0.8 second with slight silence for Whisper consistency
            let min_whisper_len = (self.sample_rate as usize * 8) / 10;
            if segment_samples.len() < min_whisper_len {
                segment_samples.resize(min_whisper_len, 0.0);
            }

            let timestamp_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;

            Some(SpeechSegment {
                samples: segment_samples,
                timestamp_ms,
            })
        } else {
            self.buffer.clear();
            None
        }
    }
}
