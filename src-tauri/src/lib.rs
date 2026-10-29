#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

pub mod audio;
pub mod stt;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptSegmentEvent {
    pub id: String,
    pub text: String,
    pub is_final: bool,
    pub timestamp: String,
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
}

impl Default for AppAudioState {
    fn default() -> Self {
        Self {
            #[cfg(target_os = "macos")]
            session_mac: Mutex::new(None),
            #[cfg(target_os = "windows")]
            session_win: Mutex::new(None),
            is_running: Arc::new(AtomicBool::new(false)),
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
    #[cfg(target_os = "macos")]
    let (data_rx, status_rx) = {
        let mut session_guard = state.session_mac.lock().map_err(|e| e.to_string())?;
        if session_guard.is_some() {
            return Ok("System audio capture is already running".to_string());
        }

        // Start 16,000 Hz Mono system audio capture
        let (capture, data_rx, status_rx) = audio::macos::MacOSAudioCapture::start(16000, 1)?;
        *session_guard = Some(capture);
        (data_rx, status_rx)
    };

    #[cfg(target_os = "windows")]
    let (data_rx, status_rx) = {
        let mut session_guard = state.session_win.lock().map_err(|e| e.to_string())?;
        if session_guard.is_some() {
            return Ok("System audio capture is already running".to_string());
        }

        let (capture, data_rx, status_rx) = audio::windows::WindowsAudioCapture::start(16000, 1)?;
        *session_guard = Some(capture);
        (data_rx, status_rx)
    };

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    return Err("Audio capture not yet supported on this OS".to_string());

    state.is_running.store(true, Ordering::SeqCst);
    let is_running_flag = state.is_running.clone();
    let app_handle = app.clone();

    // Spawn audio processor and transcription background thread
    thread::spawn(move || {
        let mut resampler = audio::AudioResampler::new(16000, 16000);
        let mut segmenter = stt::vad::AudioSegmenter::new(16000);
        let mut last_level_emit = Instant::now();
        let emit_interval = Duration::from_millis(50);

        // Load or download Whisper model
        let _ = app_handle.emit(
            "model-status",
            ModelStatusEvent {
                status: "loading".to_string(),
                message: "Initializing Whisper speech recognition model...".to_string(),
            },
        );

        let app_handle_download = app_handle.clone();
        let model_choice = model_name.as_deref();
        let model_path = match stt::ensure_model_available(model_choice, move |msg| {
            println!("[Whisper Model]: {}", msg);
            let _ = app_handle_download.emit(
                "model-status",
                ModelStatusEvent {
                    status: "downloading".to_string(),
                    message: msg.to_string(),
                },
            );
        }) {
            Ok(path) => path,
            Err(err) => {
                eprintln!("[Whisper Error]: Model load error: {}", err);
                let _ = app_handle.emit(
                    "model-status",
                    ModelStatusEvent {
                        status: "error".to_string(),
                        message: format!("Model load error: {}", err),
                    },
                );
                return;
            }
        };

        let language = match language.as_deref() {
            Some("en") => Some("en".to_string()),
            Some("hi") => Some("hi".to_string()),
            None | Some("auto") => None,
            Some(other) => {
                let message = format!("Unsupported transcription language: {}", other);
                let _ = app_handle.emit(
                    "model-status",
                    ModelStatusEvent {
                        status: "error".to_string(),
                        message: message.clone(),
                    },
                );
                eprintln!("[Whisper Error]: {}", message);
                return;
            }
        };

        let mut whisper_engine = match stt::WhisperEngine::new(&model_path, language) {
            Ok(engine) => {
                println!("[Whisper]: Engine initialized successfully");
                let _ = app_handle.emit(
                    "model-status",
                    ModelStatusEvent {
                        status: "ready".to_string(),
                        message: "Whisper STT model ready (offline)".to_string(),
                    },
                );
                Some(engine)
            }
            Err(err) => {
                eprintln!("[Whisper Error]: Engine init error: {}", err);
                let _ = app_handle.emit(
                    "model-status",
                    ModelStatusEvent {
                        status: "error".to_string(),
                        message: format!("Whisper init error: {}", err),
                    },
                );
                None
            }
        };

        // Main processing loop
        while is_running_flag.load(Ordering::SeqCst) {
            // Forward audio status messages
            if let Ok((code, msg)) = status_rx.try_recv() {
                println!("[Audio Status] Code {}: {}", code, msg);
                let _ = app_handle.emit(
                    "audio-status",
                    audio::AudioStatusEvent {
                        status: if code >= 0 { "active".to_string() } else { "error".to_string() },
                        message: msg,
                        is_active: code >= 0,
                    },
                );
            }

            // Receive audio sample chunks
            if let Ok(chunk) = data_rx.recv_timeout(Duration::from_millis(100)) {
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

                // Process audio with Voice Activity Detection (VAD)
                if let Some(segment) = segmenter.process_samples(&samples_16k) {
                    if let Some(ref mut engine) = whisper_engine {
                        match engine.transcribe(&segment.samples) {
                            Ok(text) => {
                                if !text.is_empty() {
                                    let now = chrono_format_time();
                                    println!("[Transcribed] [{}] {}", now, text);

                                    let _ = app_handle.emit(
                                        "transcription-event",
                                        TranscriptSegmentEvent {
                                            id: uuid::Uuid::new_v4().to_string(),
                                            text,
                                            is_final: true,
                                            timestamp: now,
                                        },
                                    );
                                }
                            }
                            Err(e) => {
                                eprintln!("[Whisper Inference Error]: {}", e);
                            }
                        }
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
fn stop_system_audio_capture(app: AppHandle, state: State<'_, AppAudioState>) -> Result<String, String> {
    state.is_running.store(false, Ordering::SeqCst);

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

fn chrono_format_time() -> String {
    use std::time::SystemTime;
    let duration = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let total_secs = duration.as_secs();
    let secs = total_secs % 60;
    let mins = (total_secs / 60) % 60;
    let hours = (total_secs / 3600) % 24;
    format!("{:02}:{:02}:{:02}", hours, mins, secs)
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
    #[cfg(target_os = "windows")]
    {
        // Disable WebView2 autofill and spellcheck suggestions that can trigger OS-level candidate popups during screen sharing
        std::env::set_var(
            "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
            "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection,AutofillServerCommunication --disable-autofill --disable-spell-checking",
        );
    }

    tauri::Builder::default()
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
            stop_system_audio_capture,
            close_app,
            minimize_app,
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