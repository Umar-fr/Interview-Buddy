#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

pub mod audio;
pub mod diagnostics;
pub mod stt;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

#[cfg(target_os = "macos")]
#[allow(deprecated)]
use cocoa::base::id;

#[cfg(target_os = "macos")]
use objc::{msg_send, sel, sel_impl};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelStatusEvent {
    pub status: String,
    pub message: String,
    pub progress: Option<f64>,
    pub downloaded_bytes: Option<u64>,
    pub total_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptSegmentEvent {
    pub id: String,
    pub text: String,
    pub is_final: bool,
    pub timestamp_ms: u64,
    pub inference_duration_ms: u64,
}

struct LoadedWhisperModel {
    generation: u64,
    model_name: String,
    engine: stt::WhisperEngine,
}

fn emit_model_status(
    app: &AppHandle,
    status: &str,
    message: String,
    downloaded_bytes: Option<u64>,
    total_bytes: Option<u64>,
) {
    let progress = total_bytes
        .filter(|total| *total > 0)
        .zip(downloaded_bytes)
        .map(|(total, downloaded)| (downloaded as f64 / total as f64 * 100.0).min(100.0));
    let _ = app.emit(
        "model-status",
        ModelStatusEvent {
            status: status.to_string(),
            message,
            progress,
            downloaded_bytes,
            total_bytes,
        },
    );
}

fn stop_capture_after_model_error(app: &AppHandle, is_running: &AtomicBool, message: &str) {
    diagnostics::record("ERROR", message);
    is_running.store(false, Ordering::SeqCst);
    let state = app.state::<AppAudioState>();
    state.model_switch_generation.fetch_add(1, Ordering::SeqCst);
    if let Ok(mut switch_tx) = state.model_switch_tx.lock() {
        *switch_tx = None;
    }
    #[cfg(target_os = "macos")]
    if let Ok(mut session) = state.session_mac.lock() {
        *session = None;
    }
    #[cfg(target_os = "windows")]
    if let Ok(mut session) = state.session_win.lock() {
        *session = None;
    }
    let _ = app.emit(
        "audio-status",
        audio::AudioStatusEvent {
            status: "error".to_string(),
            message: message.to_string(),
            is_active: false,
        },
    );
}

struct AudioWorkerPanicGuard {
    app: AppHandle,
    is_running: Arc<AtomicBool>,
    message: &'static str,
}

impl Drop for AudioWorkerPanicGuard {
    fn drop(&mut self) {
        if std::thread::panicking() {
            stop_capture_after_model_error(&self.app, &self.is_running, self.message);
        }
    }
}

#[cfg(target_os = "macos")]
fn configure_macos_window(window: &WebviewWindow) {
    #[allow(deprecated)]
    let ns_window = window
        .ns_window()
        .expect("Failed to get NSWindow") as id;

    unsafe {
        // Transparent native window.
        let _: () = msg_send![ns_window, setOpaque: false];
        let _: () = msg_send![ns_window, setAlphaValue: 0.72f64];

        // Exclude from screen capture (recording / screen sharing).
        let _: () = msg_send![ns_window, setSharingType: 0i64];

        // Allow the borderless window to be moved by dragging its background.
        let _: () = msg_send![ns_window, setMovable: true];

        // Allow movement by clicking anywhere in the window background.
        let _: () = msg_send![
            ns_window,
            setMovableByWindowBackground: true
        ];
    }
}

#[cfg(target_os = "windows")]
fn configure_windows_window(window: &WebviewWindow) {
    if let Ok(hwnd_val) = window.hwnd() {
        let hwnd_isize = hwnd_val.0 as isize;
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::{SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE};
        unsafe {
            let _ = SetWindowDisplayAffinity(HWND(hwnd_isize as _), WDA_EXCLUDEFROMCAPTURE);
        }
    }
    let _ = window.set_skip_taskbar(true);
    let _ = window.set_title("Calculator");
}

/// Global audio capture state managed by Tauri.
pub struct AppAudioState {
    #[cfg(target_os = "macos")]
    session_mac: Mutex<Option<audio::macos::MacOSAudioCapture>>,
    #[cfg(target_os = "windows")]
    session_win: Mutex<Option<audio::windows::WindowsAudioCapture>>,
    is_running: Arc<AtomicBool>,
    model_switch_tx: Mutex<Option<Sender<LoadedWhisperModel>>>,
    model_switch_generation: Arc<AtomicU64>,
    model_load_lock: Arc<Mutex<()>>,
}

impl Default for AppAudioState {
    fn default() -> Self {
        Self {
            #[cfg(target_os = "macos")]
            session_mac: Mutex::new(None),
            #[cfg(target_os = "windows")]
            session_win: Mutex::new(None),
            is_running: Arc::new(AtomicBool::new(false)),
            model_switch_tx: Mutex::new(None),
            model_switch_generation: Arc::new(AtomicU64::new(0)),
            model_load_lock: Arc::new(Mutex::new(())),
        }
    }
}

fn apply_ready_model_switches(
    receiver: &Receiver<LoadedWhisperModel>,
    generation: &AtomicU64,
    app: &AppHandle,
    engine: &mut stt::WhisperEngine,
) {
    loop {
        match receiver.try_recv() {
            Ok(model) if model.generation == generation.load(Ordering::SeqCst) => {
                let model_name = model.model_name;
                *engine = model.engine;
                diagnostics::record(
                    "INFO",
                    format!("Switched active Whisper model to {model_name}"),
                );
                emit_model_status(
                    app,
                    "ready",
                    format!("Whisper {model_name} model is active."),
                    None,
                    None,
                );
            }
            Ok(_) => diagnostics::record(
                "INFO",
                "Ignored a Whisper model switch superseded by a newer selection",
            ),
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
        }
    }
}

#[tauri::command]
fn start_system_audio_capture(
    app: AppHandle,
    state: State<'_, AppAudioState>,
    model_name: Option<String>,
    language: Option<String>,
) -> Result<String, String> {
    diagnostics::record(
        "INFO",
        format!(
            "Audio capture requested; model={}, language={}",
            model_name.as_deref().unwrap_or("tiny"),
            language.as_deref().unwrap_or("auto")
        ),
    );

    #[cfg(target_os = "macos")]
    let (data_rx, status_rx) = {
        let mut session_guard = state.session_mac.lock().map_err(|e| e.to_string())?;
        if session_guard.is_some() {
            return Ok("System audio capture is already running".to_string());
        }

        // Start 16,000 Hz Mono system audio capture
        let (capture, data_rx, status_rx) =
            audio::macos::MacOSAudioCapture::start(16000, 1).map_err(|error| {
                diagnostics::record("ERROR", format!("Could not start macOS audio capture: {error}"));
                error
            })?;
        *session_guard = Some(capture);
        (data_rx, status_rx)
    };

    #[cfg(target_os = "windows")]
    let (data_rx, status_rx) = {
        let mut session_guard = state.session_win.lock().map_err(|e| e.to_string())?;
        if session_guard.is_some() {
            return Ok("System audio capture is already running".to_string());
        }

        let (capture, data_rx, status_rx) =
            audio::windows::WindowsAudioCapture::start(16000, 1).map_err(|error| {
                diagnostics::record("ERROR", format!("Could not start Windows audio capture: {error}"));
                error
            })?;
        *session_guard = Some(capture);
        (data_rx, status_rx)
    };

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    return Err("Audio capture not yet supported on this OS".to_string());

    state.is_running.store(true, Ordering::SeqCst);
    let initial_model_generation = state
        .model_switch_generation
        .fetch_add(1, Ordering::SeqCst)
        .saturating_add(1);
    let model_switch_generation = state.model_switch_generation.clone();
    let model_load_lock = state.model_load_lock.clone();
    let (model_switch_tx, model_switch_rx) = channel();
    *state
        .model_switch_tx
        .lock()
        .map_err(|error| error.to_string())? = Some(model_switch_tx);
    let is_running_flag = state.is_running.clone();
    let app_handle = app.clone();
    diagnostics::record("INFO", "Audio capture backend started");

    // Spawn audio processor and transcription background thread
    thread::spawn(move || {
        let _panic_guard = AudioWorkerPanicGuard {
            app: app_handle.clone(),
            is_running: is_running_flag.clone(),
            message: "Audio processing stopped unexpectedly. Open Logs for diagnostics.",
        };
        let mut resampler = audio::AudioResampler::new(16000, 16000);
        let mut segmenter = stt::vad::AudioSegmenter::new(16000);
        let mut last_level_emit = Instant::now();
        let emit_interval = Duration::from_millis(50);
        let mut audio_chunk_count = 0u64;

        diagnostics::record("INFO", "Audio processing worker started");
        emit_model_status(
            &app_handle,
            "loading",
            "Checking Whisper model...".to_string(),
            None,
            None,
        );

        let _model_load_guard = model_load_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let app_handle_download = app_handle.clone();
        let initial_status_generation = model_switch_generation.clone();
        let model_choice = model_name.as_deref();
        let model_path = match stt::ensure_model_available(model_choice, move |status| {
            println!("[Whisper Model]: {}", status.message);
            if status.status != "downloading"
                && initial_status_generation.load(Ordering::SeqCst) == initial_model_generation
            {
                diagnostics::record("INFO", format!("Whisper model status: {}", status.status));
            }
            if initial_status_generation.load(Ordering::SeqCst) == initial_model_generation {
                emit_model_status(
                    &app_handle_download,
                    status.status,
                    status.message,
                    status.downloaded_bytes,
                    status.total_bytes,
                );
            }
        }) {
            Ok(path) => path,
            Err(err) => {
                eprintln!("[Whisper Error]: Model load error: {}", err);
                diagnostics::record("ERROR", format!("Whisper model load error: {err}"));
                stop_capture_after_model_error(
                    &app_handle,
                    &is_running_flag,
                    "Speech model download failed. Open Downloads to retry.",
                );
                emit_model_status(
                    &app_handle,
                    "error",
                    format!("Model download error: {}", err),
                    None,
                    None,
                );
                return;
            }
        };

        if model_switch_generation.load(Ordering::SeqCst) == initial_model_generation {
            emit_model_status(
                &app_handle,
                "loading",
                "Loading Whisper model into memory...".to_string(),
                None,
                None,
            );
        }
        let language = match language.as_deref() {
            Some("en") => Some("en".to_string()),
            Some("hi") => Some("hi".to_string()),
            None | Some("auto") => None,
            Some(other) => {
                let message = format!("Unsupported transcription language: {}", other);
                eprintln!("[Whisper Error]: {}", message);
                stop_capture_after_model_error(
                    &app_handle,
                    &is_running_flag,
                    "Unsupported transcription language; audio capture stopped.",
                );
                emit_model_status(&app_handle, "error", message, None, None);
                return;
            }
        };

        let whisper_engine = match stt::WhisperEngine::new(&model_path, language) {
            Ok(engine) => {
                println!("[Whisper]: Engine initialized successfully");
                diagnostics::record("INFO", "Whisper engine initialized successfully");
                if model_switch_generation.load(Ordering::SeqCst) == initial_model_generation {
                    emit_model_status(
                        &app_handle,
                        "ready",
                        "Whisper model ready (offline).".to_string(),
                        None,
                        None,
                    );
                }
                engine
            }
            Err(err) => {
                eprintln!("[Whisper Error]: Engine init error: {}", err);
                diagnostics::record("ERROR", format!("Whisper engine initialization failed: {err}"));
                stop_capture_after_model_error(
                    &app_handle,
                    &is_running_flag,
                    "Whisper model could not be loaded. Open Downloads to retry.",
                );
                emit_model_status(
                    &app_handle,
                    "error",
                    format!("Could not load Whisper model: {}", err),
                    None,
                    None,
                );
                return;
            }
        };
        drop(_model_load_guard);

        let transcription_queue = Arc::new(stt::queue::LatestSpeechQueue::default());
        let transcription_sender = transcription_queue.clone();
        let transcription_running = is_running_flag.clone();
        let transcription_app = app_handle.clone();
        let transcription_generation = model_switch_generation.clone();
        thread::spawn(move || {
            let _panic_guard = AudioWorkerPanicGuard {
                app: transcription_app.clone(),
                is_running: transcription_running.clone(),
                message: "Speech transcription stopped unexpectedly. Open Logs for diagnostics.",
            };
            let mut whisper_engine = whisper_engine;
            diagnostics::record("INFO", "Whisper transcription worker started");
            while transcription_running.load(Ordering::SeqCst) {
                apply_ready_model_switches(
                    &model_switch_rx,
                    &transcription_generation,
                    &transcription_app,
                    &mut whisper_engine,
                );
                if let Some(segment) = transcription_queue.recv_timeout(Duration::from_millis(100)) {
                    apply_ready_model_switches(
                        &model_switch_rx,
                        &transcription_generation,
                        &transcription_app,
                        &mut whisper_engine,
                    );
                    let capture_delay_ms = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis()
                        .saturating_sub(segment.timestamp_ms as u128) as u64;
                    let inference_started = Instant::now();
                    diagnostics::record(
                        "INFO",
                        format!(
                            "Whisper inference started; samples={}; capture_delay_ms={capture_delay_ms}",
                            segment.samples.len()
                        ),
                    );
                    match whisper_engine.transcribe(&segment.samples) {
                        Ok(text) if !text.is_empty() => {
                            let inference_duration_ms = inference_started.elapsed().as_millis() as u64;
                            diagnostics::record(
                                "INFO",
                                format!(
                                    "Whisper inference completed; duration_ms={inference_duration_ms}; capture_delay_ms={capture_delay_ms}; characters={}",
                                    text.chars().count()
                                ),
                            );
                            let _ = transcription_app.emit(
                                "transcription-event",
                                TranscriptSegmentEvent {
                                    id: uuid::Uuid::new_v4().to_string(),
                                    text,
                                    is_final: true,
                                    timestamp_ms: segment.timestamp_ms,
                                    inference_duration_ms,
                                },
                            );
                        }
                        Ok(_) => {
                            diagnostics::record(
                                "INFO",
                                format!(
                                    "Whisper returned no text; duration_ms={}",
                                    inference_started.elapsed().as_millis()
                                ),
                            );
                        }
                        Err(error) => {
                            eprintln!("[Whisper Inference Error]: {}", error);
                            diagnostics::record("ERROR", format!("Whisper inference failed: {error}"));
                        }
                    }
                    apply_ready_model_switches(
                        &model_switch_rx,
                        &transcription_generation,
                        &transcription_app,
                        &mut whisper_engine,
                    );
                }
            }
        });

        // Main processing loop
        let mut last_overload_notice = None;
        while is_running_flag.load(Ordering::SeqCst) {
            // Forward audio status messages
            loop {
                match status_rx.try_recv() {
                    Ok((code, msg)) => {
                        println!("[Audio Status] Code {}: {}", code, msg);
                        diagnostics::record(
                            if code >= 0 { "INFO" } else { "ERROR" },
                            if code >= 0 {
                                format!("Audio status {code}")
                            } else {
                                format!("Audio status {code}: {msg}")
                            },
                        );
                        if code == audio::AUDIO_STATUS_FATAL {
                            stop_capture_after_model_error(
                                &app_handle,
                                &is_running_flag,
                                &msg,
                            );
                        }
                        let _ = app_handle.emit(
                            "audio-status",
                            audio::AudioStatusEvent {
                                status: if code >= 0 { "active".to_string() } else { "error".to_string() },
                                message: msg,
                                is_active: code >= 0,
                            },
                        );
                    }
                    Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
                }
            }

            // Receive audio sample chunks
            if let Ok(chunk) = data_rx.recv_timeout(Duration::from_millis(100)) {
                audio_chunk_count += 1;
                if audio_chunk_count == 1 || audio_chunk_count % 500 == 0 {
                    diagnostics::record(
                        "INFO",
                        format!(
                            "Audio chunks received={audio_chunk_count}; sample_rate={}; samples={}",
                            chunk.sample_rate,
                            chunk.samples.len()
                        ),
                    );
                }
                // Ensure audio is strictly 16kHz mono via resampler
                let samples_16k = resampler.process(&chunk.samples, chunk.sample_rate);

                let rms = audio::calculate_rms(&samples_16k);
                let peak = audio::calculate_peak(&samples_16k);

                // Emit audio metering events for UI visualizer
                if last_level_emit.elapsed() >= emit_interval {
                    let _ = app_handle.emit(
                        "audio-level",
                        audio::AudioLevelEvent { rms, peak },
                    );
                    last_level_emit = Instant::now();
                }

                // Keep audio capture independent from slow Whisper inference.
                if let Some(segment) = segmenter.process_samples(&samples_16k) {
                    let segment_samples = segment.samples.len();
                    let replaced_stale = transcription_sender.push(segment);
                    if replaced_stale {
                        diagnostics::record(
                            "WARN",
                            "Whisper is behind real time; replaced an older queued segment with newer speech",
                        );
                        if last_overload_notice
                            .map_or(true, |last: Instant| last.elapsed() >= Duration::from_secs(5))
                        {
                            let _ = app_handle.emit(
                                "audio-status",
                                audio::AudioStatusEvent {
                                    status: "active".to_string(),
                                    message: "Speech recognition is busy; skipping an audio segment to keep the app responsive.".to_string(),
                                    is_active: true,
                                },
                            );
                            last_overload_notice = Some(Instant::now());
                        }
                    } else {
                        diagnostics::record(
                            "INFO",
                            format!("Speech segment queued; samples={segment_samples}"),
                        );
                    }
                }
            }
        }
    });

    let _ = app.emit(
        "audio-status",
        audio::AudioStatusEvent {
            status: "starting".to_string(),
            message: "System audio capture active".to_string(),
            is_active: true,
        },
    );

    Ok("System audio capture and transcription started".to_string())
}

#[tauri::command]
fn change_whisper_model(
    app: AppHandle,
    state: State<'_, AppAudioState>,
    model_name: String,
    language: String,
) -> Result<String, String> {
    if !matches!(model_name.as_str(), "tiny" | "base" | "small") {
        return Err(format!("Unsupported Whisper model: {model_name}"));
    }
    if !matches!(language.as_str(), "auto" | "en" | "hi") {
        return Err(format!("Unsupported transcription language: {language}"));
    }

    let switch_tx = state
        .model_switch_tx
        .lock()
        .map_err(|error| error.to_string())?
        .clone()
        .ok_or_else(|| "Audio transcription is not running".to_string())?;
    let generation = state
        .model_switch_generation
        .fetch_add(1, Ordering::SeqCst)
        .saturating_add(1);
    let current_generation = state.model_switch_generation.clone();
    let model_load_lock = state.model_load_lock.clone();
    let app_handle = app.clone();
    let selected_model = model_name.clone();

    diagnostics::record(
        "INFO",
        format!("Live Whisper model change requested; model={model_name}; language={language}"),
    );
    emit_model_status(
        &app,
        "downloading",
        format!("Preparing Whisper {model_name} model while audio capture continues..."),
        None,
        None,
    );

    thread::spawn(move || {
        let _model_load_guard = model_load_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if current_generation.load(Ordering::SeqCst) != generation {
            return;
        }

        let model_path = match stt::ensure_model_available(Some(&selected_model), |status| {
            if current_generation.load(Ordering::SeqCst) == generation {
                emit_model_status(
                    &app_handle,
                    status.status,
                    status.message,
                    status.downloaded_bytes,
                    status.total_bytes,
                );
            }
        }) {
            Ok(path) => path,
            Err(error) => {
                if current_generation.load(Ordering::SeqCst) == generation {
                    diagnostics::record(
                        "ERROR",
                        format!("Live Whisper model download failed: {error}"),
                    );
                    emit_model_status(
                        &app_handle,
                        "error",
                        format!(
                            "Could not prepare Whisper {selected_model}; the current model remains active. {error}"
                        ),
                        None,
                        None,
                    );
                }
                return;
            }
        };
        if current_generation.load(Ordering::SeqCst) != generation {
            return;
        }

        emit_model_status(
            &app_handle,
            "loading",
            format!("Loading Whisper {selected_model}; audio capture continues..."),
            None,
            None,
        );
        let language = match language.as_str() {
            "en" => Some("en".to_string()),
            "hi" => Some("hi".to_string()),
            "auto" => None,
            _ => unreachable!("language was validated before the worker started"),
        };
        let engine = match stt::WhisperEngine::new(&model_path, language) {
            Ok(engine) => engine,
            Err(error) => {
                if current_generation.load(Ordering::SeqCst) == generation {
                    diagnostics::record(
                        "ERROR",
                        format!("Live Whisper model initialization failed: {error}"),
                    );
                    emit_model_status(
                        &app_handle,
                        "error",
                        format!(
                            "Could not load Whisper {selected_model}; the current model remains active. {error}"
                        ),
                        None,
                        None,
                    );
                }
                return;
            }
        };
        if current_generation.load(Ordering::SeqCst) != generation {
            return;
        }
        if switch_tx
            .send(LoadedWhisperModel {
                generation,
                model_name: selected_model.clone(),
                engine,
            })
            .is_err()
        {
            diagnostics::record("ERROR", "Could not apply live Whisper model: transcription worker stopped");
            emit_model_status(
                &app_handle,
                "error",
                "Could not apply the selected Whisper model; restart audio capture and try again."
                    .to_string(),
                None,
                None,
            );
        } else {
            diagnostics::record(
                "INFO",
                format!("Whisper {selected_model} loaded; waiting for the current inference to finish"),
            );
            emit_model_status(
                &app_handle,
                "loading",
                format!("Whisper {selected_model} is ready and will activate after current transcription."),
                None,
                None,
            );
        }
    });

    Ok(format!("Preparing Whisper {model_name} in the background"))
}

#[tauri::command]
fn stop_system_audio_capture(app: AppHandle, state: State<'_, AppAudioState>) -> Result<String, String> {
    state.is_running.store(false, Ordering::SeqCst);
    state.model_switch_generation.fetch_add(1, Ordering::SeqCst);
    *state
        .model_switch_tx
        .lock()
        .map_err(|error| error.to_string())? = None;

    #[cfg(target_os = "macos")]
    {
        let mut session_guard = state.session_mac.lock().map_err(|e| e.to_string())?;
        *session_guard = None;
    }

    #[cfg(target_os = "windows")]
    {
        let mut session_guard = state.session_win.lock().map_err(|e| e.to_string())?;
        *session_guard = None;
    }

    let _ = app.emit(
        "audio-status",
        audio::AudioStatusEvent {
            status: "stopped".to_string(),
            message: "System audio capture stopped".to_string(),
            is_active: false,
        },
    );

    Ok("System audio capture stopped".to_string())
}

#[tauri::command]
fn close_app(app: AppHandle) {
    app.exit(0);
}

#[tauri::command]
fn minimize_app(window: WebviewWindow) {
    let _ = window.minimize();
}

#[tauri::command]
fn get_diagnostics() -> Result<diagnostics::DiagnosticLog, String> {
    diagnostics::read()
}

#[tauri::command]
fn record_diagnostic(level: String, message: String) -> Result<(), String> {
    if !matches!(level.as_str(), "INFO" | "WARN" | "ERROR") {
        return Err("Invalid diagnostic level".to_string());
    }
    let message = message
        .chars()
        .take(512)
        .collect::<String>()
        .replace('\r', " ")
        .replace('\n', " ");
    diagnostics::record(&level, message);
    Ok(())
}

#[tauri::command]
fn take_screenshot(app: AppHandle) -> Result<String, String> {
    let screens = screenshots::Screen::all().map_err(|e| format!("Failed to get screens: {}", e))?;
    if screens.is_empty() {
        return Err("No screens found".to_string());
    }

    // Capture the monitor where the window is currently located, or fallback to primary monitor
    let target_screen = if let Some(win) = app.get_webview_window("main") {
        if let Ok(pos) = win.outer_position() {
            screens
                .iter()
                .find(|s| {
                    let info = &s.display_info;
                    pos.x >= info.x
                        && pos.x < (info.x + info.width as i32)
                        && pos.y >= info.y
                        && pos.y < (info.y + info.height as i32)
                })
                .unwrap_or_else(|| {
                    screens
                        .iter()
                        .find(|s| s.display_info.is_primary)
                        .unwrap_or(&screens[0])
                })
        } else {
            screens
                .iter()
                .find(|s| s.display_info.is_primary)
                .unwrap_or(&screens[0])
        }
    } else {
        screens
            .iter()
            .find(|s| s.display_info.is_primary)
            .unwrap_or(&screens[0])
    };

    let image = target_screen.capture().map_err(|e| format!("Failed to capture: {}", e))?;
    
    let mut cursor = std::io::Cursor::new(Vec::new());
    image.write_to(&mut cursor, screenshots::image::ImageFormat::Png).map_err(|e| format!("Failed to encode png: {}", e))?;
    let buffer = cursor.into_inner();
    
    use base64::{Engine as _, engine::general_purpose};
    let b64 = general_purpose::STANDARD.encode(buffer);
    Ok(b64)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    if let Err(error) = diagnostics::initialize() {
        eprintln!("Could not initialize diagnostics log: {error}");
    }
    diagnostics::record(
        "INFO",
        format!("Application started; version={}", env!("CARGO_PKG_VERSION")),
    );
    let default_panic_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        diagnostics::record("PANIC", panic_info.to_string());
        default_panic_hook(panic_info);
    }));

    #[cfg(target_os = "windows")]
    {
        // Disable WebView2 autofill and spellcheck suggestions that can trigger OS-level candidate popups during screen sharing
        std::env::set_var(
            "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
            "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection,AutofillServerCommunication --disable-autofill --disable-spell-checking",
        );
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    if event.state == ShortcutState::Pressed {
                        let is_boss_key = shortcut.matches(Modifiers::ALT, Code::KeyR)
                            || shortcut.matches(Modifiers::CONTROL | Modifiers::SHIFT, Code::KeyH)
                            || shortcut.matches(Modifiers::SUPER | Modifiers::SHIFT, Code::KeyH);

                        let is_coding_key = shortcut.matches(Modifiers::CONTROL | Modifiers::SHIFT, Code::KeyC)
                            || shortcut.matches(Modifiers::SUPER | Modifiers::SHIFT, Code::KeyC);

                        if is_boss_key {
                            if let Some(win) = app.get_webview_window("main") {
                                if win.is_visible().unwrap_or(true) {
                                    let _ = win.hide();
                                } else {
                                    let _ = win.show();
                                    let _ = win.set_focus();
                                }
                            }
                        } else if is_coding_key {
                            if let Some(win) = app.get_webview_window("main") {
                                let _ = win.show();
                                let _ = win.set_focus();
                                let _ = app.emit("open-coding-modal", ());
                            }
                        }
                    }
                })
                .build(),
        )
        .manage(AppAudioState::default())
        .invoke_handler(tauri::generate_handler![
            start_system_audio_capture,
            change_whisper_model,
            stop_system_audio_capture,
            close_app,
            minimize_app,
            get_diagnostics,
            record_diagnostic,
            take_screenshot
        ])
        .setup(|app| {
            let window = app
                .get_webview_window("main")
                .expect("main window not found");

            #[cfg(target_os = "macos")]
            configure_macos_window(&window);

            #[cfg(target_os = "windows")]
            configure_windows_window(&window);

            // Register global shortcuts: Boss keys and Coding mode
            let shortcuts = [
                Shortcut::new(Some(Modifiers::ALT), Code::KeyR),
                Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyH),
                Shortcut::new(Some(Modifiers::SUPER | Modifiers::SHIFT), Code::KeyH),
                Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyC),
                Shortcut::new(Some(Modifiers::SUPER | Modifiers::SHIFT), Code::KeyC),
            ];

            for s in shortcuts {
                let _ = app.handle().global_shortcut().register(s);
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running application");
}