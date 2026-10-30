pub mod vad;
pub mod queue;

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperState,
};

const MODEL_TINY_URL: &str =
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.bin";
const MODEL_BASE_URL: &str =
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin";
const MODEL_SMALL_URL: &str =
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin";
const MODEL_TINY_MIN_BYTES: u64 = 70_000_000;
const MODEL_BASE_MIN_BYTES: u64 = 130_000_000;
const MODEL_SMALL_MIN_BYTES: u64 = 430_000_000;
const GGML_FILE_MAGIC: u32 = 0x67676d6c;
const MAX_WHISPER_THREADS: usize = 4;

fn whisper_thread_count(available_threads: Option<usize>) -> i32 {
    available_threads
        .unwrap_or(2)
        .clamp(1, MAX_WHISPER_THREADS) as i32
}

#[derive(Debug, Clone)]
pub struct ModelDownloadStatus {
    pub status: &'static str,
    pub message: String,
    pub downloaded_bytes: Option<u64>,
    pub total_bytes: Option<u64>,
}

impl ModelDownloadStatus {
    fn new(status: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
            downloaded_bytes: None,
            total_bytes: None,
        }
    }

    fn progress(
        model_file_name: &str,
        downloaded_bytes: u64,
        total_bytes: Option<u64>,
    ) -> Self {
        Self {
            status: "downloading",
            message: format!("Downloading Whisper model {}...", model_file_name),
            downloaded_bytes: Some(downloaded_bytes),
            total_bytes,
        }
    }
}

fn is_valid_model(path: &Path, minimum_size: u64) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if metadata.len() < minimum_size {
        return false;
    }

    let Ok(mut file) = File::open(path) else {
        return false;
    };
    let mut magic = [0; 4];
    file.read_exact(&mut magic).is_ok() && u32::from_le_bytes(magic) == GGML_FILE_MAGIC
}

fn sanitize_samples(samples: &[f32]) -> Vec<f32> {
    samples
        .iter()
        .map(|sample| {
            if sample.is_finite() {
                sample.clamp(-1.0, 1.0)
            } else {
                0.0
            }
        })
        .collect()
}

fn migrate_cached_models(data_dir: &Path, model_dir: &Path) -> Result<(), String> {
    let legacy_dirs = [
        data_dir.join("interview-buddy").join("models"),
        data_dir.join("system-transcriber").join("models"),
    ];
    let models = [
        ("ggml-tiny.bin", MODEL_TINY_MIN_BYTES),
        ("ggml-base.bin", MODEL_BASE_MIN_BYTES),
        ("ggml-small.bin", MODEL_SMALL_MIN_BYTES),
    ];

    for legacy_dir in &legacy_dirs {
        for (file_name, minimum_size) in models {
            let source = legacy_dir.join(file_name);
            let destination = model_dir.join(file_name);
            if !source.exists() {
                continue;
            }

            if let (Ok(source_path), Ok(destination_path)) =
                (fs::canonicalize(&source), fs::canonicalize(&destination))
            {
                if source_path == destination_path {
                    continue;
                }
            }

            if !is_valid_model(&source, minimum_size) {
                continue;
            }
            if !is_valid_model(&destination, minimum_size) {
                if destination.exists() {
                    fs::remove_file(&destination).map_err(|error| {
                        format!("Failed to replace invalid cached Whisper model: {error}")
                    })?;
                }
                fs::copy(&source, &destination)
                    .map_err(|error| format!("Failed to migrate Whisper model: {error}"))?;
                if !is_valid_model(&destination, minimum_size) {
                    let _ = fs::remove_file(&destination);
                    return Err(format!("Migrated Whisper model {file_name} failed validation"));
                }
            }
            fs::remove_file(&source)
                .map_err(|error| format!("Failed to remove migrated Whisper model: {error}"))?;
        }
    }

    let canonical_model_dir = fs::canonicalize(model_dir).ok();
    let canonical_model_parent = model_dir.parent().and_then(|path| fs::canonicalize(path).ok());
    for legacy_dir in &legacy_dirs {
        if let (Ok(legacy_path), Some(model_path)) =
            (fs::canonicalize(legacy_dir), canonical_model_dir.as_ref())
        {
            if &legacy_path == model_path {
                continue;
            }
        }
        if let Some(parent) = legacy_dir.parent() {
            match fs::remove_dir(legacy_dir) {
                Ok(()) => {}
                Err(error)
                    if error.kind() == std::io::ErrorKind::NotFound
                        || error.kind() == std::io::ErrorKind::DirectoryNotEmpty => {}
                Err(error) => {
                    return Err(format!("Failed to remove old model cache directory: {error}"))
                }
            }
            if let (Ok(parent_path), Some(model_parent)) =
                (fs::canonicalize(parent), canonical_model_parent.as_ref())
            {
                if &parent_path == model_parent {
                    continue;
                }
            }
            match fs::remove_dir(parent) {
                Ok(()) => {}
                Err(error)
                    if error.kind() == std::io::ErrorKind::NotFound
                        || error.kind() == std::io::ErrorKind::DirectoryNotEmpty => {}
                Err(error) => {
                    return Err(format!("Failed to remove old model cache directory: {error}"))
                }
            }
        }
    }

    Ok(())
}

pub struct WhisperEngine {
    state: WhisperState,
    _ctx: WhisperContext,
    language: Option<String>,
    n_threads: i32,
    model_path: PathBuf,
    using_gpu: bool,
    backend: String,
}

impl WhisperEngine {
    /// Initialize Whisper context and persistent state from the model path
    pub fn new(model_path: &Path, language: Option<String>) -> Result<Self, String> {
        let model_path = model_path.to_path_buf();
        let detected_gpu = detected_gpu_backend();
        let (ctx, state, using_gpu, backend) = if let Some(backend) = detected_gpu {
            match create_whisper_context(&model_path, true) {
                Ok((ctx, state)) => {
                    crate::diagnostics::record(
                        "INFO",
                        format!("Whisper GPU acceleration enabled; backend={backend}"),
                    );
                    (ctx, state, true, backend)
                }
                Err(gpu_error) => {
                    crate::diagnostics::record(
                        "WARN",
                        format!("Whisper GPU initialization failed; falling back to CPU: {gpu_error}"),
                    );
                    let (ctx, state) = create_whisper_context(&model_path, false)?;
                    (ctx, state, false, "CPU (GPU initialization fallback)".to_string())
                }
            }
        } else {
            crate::diagnostics::record(
                "INFO",
                "No supported Whisper GPU detected; using CPU inference",
            );
            let (ctx, state) = create_whisper_context(&model_path, false)?;
            (ctx, state, false, "CPU".to_string())
        };

        Ok(Self {
            state,
            _ctx: ctx,
            language,
            n_threads: whisper_thread_count(
                std::thread::available_parallelism()
                    .ok()
                    .map(|parallelism| parallelism.get()),
            ),
            model_path,
            using_gpu,
            backend,
        })
    }

    pub fn n_threads(&self) -> i32 {
        self.n_threads
    }

    pub fn backend(&self) -> &str {
        &self.backend
    }

    /// Transcribe 16kHz mono PCM float samples rapidly
    pub fn transcribe(&mut self, samples: &[f32]) -> Result<String, String> {
        let samples = sanitize_samples(samples);
        match run_transcription(&mut self.state, &samples, self.language.as_deref(), self.n_threads)
        {
            Ok(text) => Ok(text),
            Err(gpu_error) if self.using_gpu => {
                crate::diagnostics::record(
                    "WARN",
                    format!("Whisper GPU inference failed; retrying on CPU: {gpu_error}"),
                );
                let (ctx, state) = create_whisper_context(&self.model_path, false).map_err(
                    |cpu_error| {
                        format!(
                            "Whisper GPU inference failed ({gpu_error}); CPU fallback initialization failed: {cpu_error}"
                        )
                    },
                )?;
                self._ctx = ctx;
                self.state = state;
                self.using_gpu = false;
                self.backend = "CPU (GPU inference fallback)".to_string();
                crate::diagnostics::record(
                    "INFO",
                    "Whisper switched to CPU inference after a GPU inference error",
                );
                run_transcription(
                    &mut self.state,
                    &samples,
                    self.language.as_deref(),
                    self.n_threads,
                )
                .map_err(|cpu_error| {
                    format!(
                        "Whisper GPU inference failed ({gpu_error}); CPU fallback inference failed: {cpu_error}"
                    )
                })
            }
            Err(error) => Err(error),
        }
    }
}

fn create_whisper_context(
    model_path: &Path,
    use_gpu: bool,
) -> Result<(WhisperContext, WhisperState), String> {
    let path_str = model_path
        .to_str()
        .ok_or_else(|| "Invalid model path unicode".to_string())?;
    let mut params = WhisperContextParameters::default();
    params.use_gpu(use_gpu);
    let ctx = WhisperContext::new_with_params(path_str, params)
        .map_err(|error| format!("Failed to load Whisper model: {error:?}"))?;
    let state = ctx
        .create_state()
        .map_err(|error| format!("Failed to create persistent Whisper state: {error:?}"))?;
    Ok((ctx, state))
}

fn detected_gpu_backend() -> Option<String> {
    #[cfg(windows)]
    {
        let devices = whisper_rs::vulkan::list_devices();
        if !devices.is_empty() {
            return Some(format!(
                "Vulkan ({})",
                devices
                    .iter()
                    .map(|device| device.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }

    #[cfg(target_os = "macos")]
    {
        return Some("Metal".to_string());
    }

    None
}

fn run_transcription(
    state: &mut WhisperState,
    samples: &[f32],
    language: Option<&str>,
    n_threads: i32,
) -> Result<String, String> {
    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_language(language);
    params.set_translate(false);
    params.set_no_context(true);
    params.set_single_segment(true);
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    params.set_suppress_blank(true);
    params.set_n_threads(n_threads);
    params.set_entropy_thold(2.4);
    params.set_logprob_thold(-1.0);
    params.set_no_speech_thold(0.6);
    params.set_initial_prompt(
        "The speaker may switch between Hindi and English; preserve English technical terms.",
    );

    state
        .full(params, samples)
        .map_err(|error| format!("Whisper inference error: {error:?}"))?;

    let num_segments = state
        .full_n_segments()
        .map_err(|error| format!("Failed to count segments: {error:?}"))?;
    let mut full_text = String::new();
    for i in 0..num_segments {
        if let Ok(segment) = state.full_get_segment_text(i) {
            full_text.push_str(&segment);
        }
    }
    Ok(full_text.trim().to_string())
}

/// Ensure Whisper model is available locally, checking bundled resources or downloading if not present.
pub fn ensure_model_available<F>(model_choice: Option<&str>, progress_callback: F) -> Result<PathBuf, String>
where
    F: Fn(ModelDownloadStatus),
{
    let (model_file_name, download_url, approx_size, minimum_size) = match model_choice.unwrap_or("tiny") {
        "tiny" | "tiny.en" => ("ggml-tiny.bin", MODEL_TINY_URL, "~75 MB", MODEL_TINY_MIN_BYTES),
        "base" | "base.en" => ("ggml-base.bin", MODEL_BASE_URL, "~150 MB", MODEL_BASE_MIN_BYTES),
        "small" => ("ggml-small.bin", MODEL_SMALL_URL, "~500 MB", MODEL_SMALL_MIN_BYTES),
        model => return Err(format!("Unsupported Whisper model: {}", model)),
    };

    // 1. Check bundled application resources first (for pre-packaged distributions)
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let candidates = [
                exe_dir.join("resources").join(model_file_name),
                exe_dir.join("../Resources").join(model_file_name),
                exe_dir.join("models").join(model_file_name),
                exe_dir.join(model_file_name),
            ];
            for candidate in candidates {
                if candidate.exists() {
                    if is_valid_model(&candidate, minimum_size) {
                        progress_callback(ModelDownloadStatus::new(
                            "cached",
                            format!(
                                "Using bundled Whisper model at {}.",
                                candidate.display()
                            ),
                        ));
                        return Ok(candidate);
                    }
                }
            }
        }
    }

    let data_dir = dirs::data_local_dir()
        .or_else(dirs::cache_dir)
        .unwrap_or_else(|| PathBuf::from("."));

    let model_dir = data_dir.join("InterviewBuddy").join("models");
    fs::create_dir_all(&model_dir)
        .map_err(|e| format!("Failed to create model directory: {}", e))?;
    migrate_cached_models(&data_dir, &model_dir)?;

    let model_path = model_dir.join(model_file_name);

    if is_valid_model(&model_path, minimum_size) {
        progress_callback(ModelDownloadStatus::new(
            "cached",
            format!("Using downloaded Whisper model at {}.", model_path.display()),
        ));
        return Ok(model_path);
    }
    if model_path.exists() {
        progress_callback(ModelDownloadStatus::new(
            "verifying",
            "Removing an incomplete or invalid cached model before redownloading.",
        ));
        fs::remove_file(&model_path)
            .map_err(|e| format!("Failed to remove invalid cached model: {}", e))?;
    }

    progress_callback(ModelDownloadStatus::new(
        "downloading",
        format!("Connecting to download Whisper {} model ({}).", model_file_name, approx_size),
    ));

    let response = ureq::get(download_url)
        .call()
        .map_err(|e| format!("Failed to request Whisper model: {}", e))?;

    if response.status() != 200 {
        return Err(format!(
            "Failed to download model: HTTP {}",
            response.status()
        ));
    }

    let total_bytes = response
        .header("Content-Length")
        .and_then(|length| length.parse::<u64>().ok());
    if total_bytes.map_or(false, |size| size < minimum_size) {
        return Err(format!(
            "Whisper model response is too small ({} bytes); expected at least {} bytes.",
            total_bytes.unwrap_or_default(),
            minimum_size
        ));
    }

    let temp_path = model_dir.join(format!("{}.tmp", model_file_name));
    if temp_path.exists() {
        fs::remove_file(&temp_path)
            .map_err(|e| format!("Failed to remove stale model download: {}", e))?;
    }
    let mut temp_file =
        File::create(&temp_path).map_err(|e| format!("Failed to create temp model file: {}", e))?;
    let mut reader = response.into_reader();
    let mut downloaded_bytes = 0;
    let mut last_progress = Instant::now();
    let mut buffer = [0; 64 * 1024];
    let download_result = (|| -> Result<(), String> {
        loop {
            let bytes_read = reader
                .read(&mut buffer)
                .map_err(|e| format!("Failed to read model download: {}", e))?;
            if bytes_read == 0 {
                break;
            }
            temp_file
                .write_all(&buffer[..bytes_read])
                .map_err(|e| format!("Failed to write model file: {}", e))?;
            downloaded_bytes += bytes_read as u64;
            if last_progress.elapsed() >= Duration::from_millis(250) {
                progress_callback(ModelDownloadStatus::progress(
                    model_file_name,
                    downloaded_bytes,
                    total_bytes,
                ));
                last_progress = Instant::now();
            }
        }

        temp_file
            .flush()
            .map_err(|e| format!("Failed to flush model file: {}", e))?;
        temp_file
            .sync_all()
            .map_err(|e| format!("Failed to sync model file: {}", e))?;

        if total_bytes.map_or(false, |expected| downloaded_bytes != expected) {
            return Err(format!(
                "Whisper model download was incomplete (received {} of {} bytes).",
                downloaded_bytes,
                total_bytes.unwrap_or_default()
            ));
        }
        if downloaded_bytes < minimum_size {
            return Err(format!(
                "Whisper model download is incomplete (received {} bytes; expected at least {}).",
                downloaded_bytes, minimum_size
            ));
        }
        if !is_valid_model(&temp_path, minimum_size) {
            return Err("Downloaded file is not a valid Whisper model.".to_string());
        }
        Ok(())
    })();

    drop(temp_file);
    if let Err(error) = download_result {
        let _ = fs::remove_file(&temp_path);
        return Err(error);
    }

    progress_callback(ModelDownloadStatus::new(
        "verifying",
        "Download complete. Verifying model file...",
    ));
    fs::rename(&temp_path, &model_path)
        .map_err(|e| format!("Failed to finalize model file: {}", e))?;
    progress_callback(ModelDownloadStatus {
        status: "downloaded",
        message: format!(
            "Whisper model {} downloaded and verified at {}.",
            model_file_name,
            model_path.display()
        ),
        downloaded_bytes: Some(downloaded_bytes),
        total_bytes: Some(downloaded_bytes),
    });
    Ok(model_path)
}

#[cfg(test)]
mod tests {
    use super::{
        is_valid_model, sanitize_samples, whisper_thread_count, GGML_FILE_MAGIC, MODEL_BASE_URL,
        MODEL_SMALL_URL, MODEL_TINY_URL,
    };
    use std::fs;

    #[test]
    fn sanitizes_non_finite_and_out_of_range_audio() {
        let samples = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1.5, 0.5, 1.5];

        assert_eq!(
            sanitize_samples(&samples),
            vec![0.0, 0.0, 0.0, -1.0, 0.5, 1.0]
        );
    }

    #[test]
    fn caps_whisper_threads_to_avoid_logical_core_oversubscription() {
        assert_eq!(whisper_thread_count(None), 2);
        assert_eq!(whisper_thread_count(Some(0)), 1);
        assert_eq!(whisper_thread_count(Some(4)), 4);
        assert_eq!(whisper_thread_count(Some(8)), 4);
    }

    #[test]
    fn selects_multilingual_whisper_models() {
        assert_eq!(
            super::ensure_model_available(Some("invalid"), |_| {}).unwrap_err(),
            "Unsupported Whisper model: invalid"
        );
        assert!(MODEL_TINY_URL.ends_with("ggml-tiny.bin"));
        assert!(MODEL_BASE_URL.ends_with("ggml-base.bin"));
        assert!(MODEL_SMALL_URL.ends_with("ggml-small.bin"));
    }

    #[test]
    fn rejects_truncated_and_non_model_files() {
        let path = std::env::temp_dir().join(format!(
            "interview-buddy-invalid-model-{}.bin",
            std::process::id()
        ));
        fs::write(&path, [0; 32]).unwrap();
        assert!(!is_valid_model(&path, 1));

        let valid_header = GGML_FILE_MAGIC.to_le_bytes().to_vec();
        fs::write(&path, valid_header).unwrap();
        assert!(is_valid_model(&path, 4));
        fs::remove_file(path).unwrap();
    }
}
