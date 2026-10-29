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
    silence_samples: usize,
    max_speech_samples: usize,
    trailing_silence_samples: usize,
    min_speech_samples: usize,
}

impl AudioSegmenter {
    pub fn new(sample_rate: u32) -> Self {
        let sample_rate = sample_rate as usize;
        Self {
            sample_rate: sample_rate as u32,
            threshold: 0.010, // Sensitive RMS energy threshold
            buffer: Vec::with_capacity(sample_rate * 5),
            is_speaking: false,
            silence_samples: 0,
            max_speech_samples: sample_rate * 3 / 2,
            trailing_silence_samples: sample_rate / 4,
            min_speech_samples: sample_rate * 3 / 10,
        }
    }

    /// Process a stream of PCM float samples and return completed speech segments
    pub fn process_samples(&mut self, samples: &[f32]) -> Option<SpeechSegment> {
        let chunk_rms = crate::audio::calculate_rms(samples);

        let energy_detected = chunk_rms > self.threshold;

        if energy_detected {
            if !self.is_speaking {
                self.is_speaking = true;
            }
            self.silence_samples = 0;
            self.buffer.extend_from_slice(samples);
        } else if self.is_speaking {
            self.buffer.extend_from_slice(samples);
            self.silence_samples += samples.len();
            if self.silence_samples >= self.trailing_silence_samples {
                return self.flush_segment();
            }
        }

        if self.is_speaking && self.buffer.len() >= self.max_speech_samples {
            return self.flush_segment();
        }

        None
    }

    /// Flush the accumulated audio buffer as a speech segment if it contains enough samples
    pub fn flush_segment(&mut self) -> Option<SpeechSegment> {
        self.is_speaking = false;
        self.silence_samples = 0;

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

#[cfg(test)]
mod tests {
    use super::AudioSegmenter;

    #[test]
    fn flushes_after_captured_silence_duration() {
        let mut segmenter = AudioSegmenter::new(16_000);
        let speech = vec![0.1; 16_000 / 10];
        let silence = vec![0.0; 16_000 / 10];

        for _ in 0..10 {
            assert!(segmenter.process_samples(&speech).is_none());
        }
        assert!(segmenter.process_samples(&silence).is_none());
        assert!(segmenter.process_samples(&silence).is_none());
        let segment = segmenter.process_samples(&silence).unwrap();

        assert!(segment.samples.len() >= 16_000 / 10 * 10);
    }

    #[test]
    fn splits_long_speech_using_audio_duration() {
        let mut segmenter = AudioSegmenter::new(16_000);
        let speech = vec![0.1; 16_000 / 10];
        let mut segment = None;

        for _ in 0..15 {
            segment = segmenter.process_samples(&speech);
        }

        assert!(segment.is_some());
    }
}
