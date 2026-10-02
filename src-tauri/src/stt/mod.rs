pub mod vad;

use std::fs::{self, File};
use std::io::{copy, BufReader};
use std::path::{Path, PathBuf};
use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperState,
};

const MODEL_DOWNLOAD_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin";
const MODEL_FILE_NAME: &str = "ggml-base.en.bin";

pub struct WhisperEngine {
    _ctx: WhisperContext,
    state: WhisperState,
}

// WhisperEngine is safe to send to background worker threads
unsafe impl Send for WhisperEngine {}

impl WhisperEngine {
    /// Initialize Whisper context and persistent state from the model path
    pub fn new(model_path: &Path) -> Result<Self, String> {
        let path_str = model_path
            .to_str()
            .ok_or_else(|| "Invalid model path unicode".to_string())?;

        let mut params = WhisperContextParameters::default();
        params.use_gpu(true);

        let ctx = WhisperContext::new_with_params(path_str, params)
            .map_err(|e| format!("Failed to load Whisper model: {:?}", e))?;

        let state = ctx
            .create_state()
            .map_err(|e| format!("Failed to create persistent Whisper state: {:?}", e))?;

        Ok(Self { _ctx: ctx, state })
    }

    /// Transcribe 16kHz mono PCM float samples rapidly
    pub fn transcribe(&mut self, samples: &[f32]) -> Result<String, String> {
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(Some("en"));
        params.set_translate(false);
        params.set_no_context(true);
        params.set_single_segment(true);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_suppress_blank(true);

        let n_threads = std::thread::available_parallelism()
            .map(|p| (p.get() as i32).clamp(2, 8))
            .unwrap_or(4);
        params.set_n_threads(n_threads);

        // Anti-hallucination filters
        params.set_entropy_thold(2.4);
        params.set_logprob_thold(-1.0);
        params.set_no_speech_thold(0.6);

        self.state
            .full(params, samples)
            .map_err(|e| format!("Whisper inference error: {:?}", e))?;

        let num_segments = self
            .state
            .full_n_segments()
            .map_err(|e| format!("Failed to count segments: {:?}", e))?;

        let mut full_text = String::new();
        for i in 0..num_segments {
            if let Ok(segment) = self.state.full_get_segment_text(i) {
                full_text.push_str(&segment);
            }
        }

        Ok(full_text.trim().to_string())
    }
}

/// Ensure Whisper model is available locally, downloading it if not present
pub fn ensure_model_available<F>(progress_callback: F) -> Result<PathBuf, String>
where
    F: Fn(&str),
{
    let base_dir = dirs::data_local_dir()
        .or_else(dirs::cache_dir)
        .unwrap_or_else(|| PathBuf::from("."));
    
    let model_dir = base_dir.join("interview-buddy").join("models");
    fs::create_dir_all(&model_dir).map_err(|e| format!("Failed to create model directory: {}", e))?;

    let model_path = model_dir.join(MODEL_FILE_NAME);

    // If model does not exist in new dir, check if legacy system-transcriber directory has it
    if !model_path.exists() {
        let legacy_model = base_dir.join("system-transcriber").join("models").join(MODEL_FILE_NAME);
        if legacy_model.exists() {
            let _ = fs::copy(&legacy_model, &model_path);
        }
    }

    if model_path.exists() {
        if let Ok(metadata) = fs::metadata(&model_path) {
            if metadata.len() > 10_000_000 {
                progress_callback("Whisper model loaded from local cache");
                return Ok(model_path);
            }
        }
    }

    progress_callback("Downloading Whisper base.en model (~140MB)...");

    let response = reqwest::blocking::get(MODEL_DOWNLOAD_URL)
        .map_err(|e| format!("Failed to request Whisper model: {}", e))?;

    if !response.status().is_success() {
        return Err(format!(
            "Failed to download model: HTTP {}",
            response.status()
        ));
    }

    let temp_path = model_dir.join(format!("{}.tmp", MODEL_FILE_NAME));
    let mut temp_file = File::create(&temp_path)
        .map_err(|e| format!("Failed to create temp model file: {}", e))?;

    let mut reader = BufReader::new(response);
    copy(&mut reader, &mut temp_file)
        .map_err(|e| format!("Failed to write model file: {}", e))?;

    fs::rename(&temp_path, &model_path)
        .map_err(|e| format!("Failed to finalize model file: {}", e))?;

    progress_callback("Whisper model downloaded successfully");
    Ok(model_path)
}
