# 🎙️ InterviewBuddy

> **A real-time, stealthy AI interview & meeting copilot for macOS and Windows.**
> Direct system audio capture, local offline GPU-accelerated speech-to-text (Whisper), and intelligent live answer generation powered by Google Gemini.

---

## 🌟 Overview

**InterviewBuddy** is a lightweight, low-latency desktop assistant designed to listen to live system audio (interviews, lectures, Zoom/Teams/Meet calls, quizzes, or media playback), transcribe the spoken audio in real-time on-device, and provide instant AI-generated answers, explanations, and context-aware insights.

### Key Highlights
- **Direct System Audio Capture**: Captures internal macOS audio directly via Apple's native `ScreenCaptureKit`—**no virtual audio drivers (BlackHole/Loopback) required**.
- **100% Local & Fast STT**: Powered by `whisper.cpp` (`whisper-rs`) with **Apple Silicon Metal GPU acceleration** and custom Voice Activity Detection (VAD).
- **Intelligent Gemini Copilot**: Real-time question detection, multiple-choice quiz solving, interview coaching, and manual on-demand snippets via Google Gemini.
- **Stealth Overlay Window**: Frameless, translucent glassmorphic UI, always-on-top, draggable from any background point, and **invisible to screen-sharing and recordings** (`NSWindowSharingNone`).

---

## 🏗️ Architecture & Tech Stack

```mermaid
flowchart LR
    subgraph AudioEngine [macOS Native Layer]
        SCK[ScreenCaptureKit / CoreAudio] --> Resample[16kHz Mono Stream]
        Resample --> FFI[Objective-C Bridge]
    end

    subgraph RustCore [Rust Core Engine - Tauri Backend]
        FFI --> VAD[Voice Activity Detection]
        VAD --> Whisper[Whisper.cpp + Metal GPU]
        Whisper --> Events[Tauri IPC Events]
    end

    subgraph FrontendApp [Desktop UI - Vite + TS]
        Events --> LiveUI[Live Transcript & Audio Meter]
        LiveUI --> Gemini[Gemini 2.0 / 1.5 Flash API]
        Gemini --> Answers[Instant AI Answers]
    end
```

### Technology Breakdown

| Component | Technology | Description |
| :--- | :--- | :--- |
| **Desktop Shell** | [Tauri v2](https://v2.tauri.app/) | Secure, ultra-lightweight Rust desktop runtime with native windowing. |
| **Frontend** | TypeScript, Vite, Vanilla CSS | Zero-overhead, high-performance UI with responsive audio visualizers. |
| **System Audio Capture** | Objective-C + ScreenCaptureKit | Native macOS 13+ audio stream loopback with zero virtual cables. |
| **Audio Processing** | Rust + `cpal` | RMS/peak metering, chunking, and rolling buffer segmentation. |
| **Speech-to-Text (STT)** | `whisper-rs` + `whisper.cpp` | Offline automatic speech recognition with Apple Silicon Metal acceleration. |
| **VAD** | Custom Silence/Energy VAD | Filters silence, groups speech bursts, and dispatches to Whisper inference. |
| **LLM & Copilot** | Google Gemini REST API | Live model detection, structured prompt engineering, and auto-answering. |

---

## 📂 Project Structure

```text
interview-buddy/
├── index.html                   # Main application UI layout & modals
├── package.json                 # Node dependencies and build scripts
├── tsconfig.json                # TypeScript compiler configuration
├── vite.config.ts               # Vite configuration (port 1420)
├── src/                         # Frontend application source
│   ├── main.ts                  # UI event handlers, Gemini client, IPC listeners
│   ├── styles.css               # Glassmorphic dark theme styles & animations
│   └── assets/                  # Frontend static assets
└── src-tauri/                   # Rust backend & native code
    ├── Cargo.toml               # Rust crates and target dependencies
    ├── build.rs                 # Native Objective-C compilation bridge
    ├── tauri.conf.json          # Tauri application and window configuration
    ├── native/                  # Native macOS audio capture
    │   ├── macos_audio_capture.h # Objective-C header file
    │   └── macos_audio_capture.m # ScreenCaptureKit stream delegate & resampler
    └── src/
        ├── main.rs              # Tauri entry point
        ├── lib.rs               # Application setup, IPC commands, and background loop
        ├── audio/
        │   ├── mod.rs           # Audio state, RMS & peak calculations
        │   ├── macos.rs         # Safe Rust FFI wrapper for ScreenCaptureKit
        │   └── windows.rs       # Windows audio loopback placeholder
        └── stt/
            ├── mod.rs           # Whisper engine initialization & model management
            └── vad.rs           # Voice Activity Detection (VAD) segmenter
```

---

## 📋 Prerequisites

Before running or developing the project, ensure you have the following installed:

1. **macOS**: macOS 13.0 (Ventura) or later *(macOS 14+ recommended for ScreenCaptureKit audio APIs)*.
2. **Node.js**: `v18.0.0` or later (LTS recommended).
3. **Rust Toolchain**: Rust 1.77+ with `cargo`:
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```
4. **Xcode Command Line Tools**:
   ```bash
   xcode-select --install
   ```
5. **CMake**: Required for compiling `whisper.cpp`:
   ```bash
   brew install cmake
   ```

---

## 🚀 Step-by-Step Setup Guide

### 1. Clone and Install Dependencies

```bash
# Clone the repository
git clone https://github.com/your-username/interview-buddy.git
cd interview-buddy

# Install Node.js frontend dependencies
npm install
```

### 2. Grant macOS Permissions

Because InterviewBuddy captures system audio using Apple's `ScreenCaptureKit`, macOS requires **Screen & System Audio Recording** permission:

1. On first launch, macOS will prompt you to grant screen recording permissions.
2. If not prompted, go to:
   **System Settings** ➔ **Privacy & Security** ➔ **Screen & System Audio Recording**
3. Ensure your terminal or **InterviewBuddy** is toggled **ON**.

*(Note: InterviewBuddy only captures system audio streams; it does not record video).*

---

### 3. Run in Development Mode

Run the following command in the root directory:

```bash
npm run tauri dev
```

What happens on startup:
1. Vite spins up the frontend at `http://localhost:1420`.
2. Cargo builds the native Objective-C bridge and Rust backend.
3. On first run, the app automatically downloads the `ggml-tiny.en.bin` Whisper model (~75MB) to your local application cache.
4. The transparent overlay window appears on your screen and starts listening to system audio automatically.

---

## 🎯 How to Use

### 1. Configure Gemini AI Key
1. Click the **✦ Set API Key** badge in the titlebar.
2. Paste your Google Gemini API Key ([Get a free key from Google AI Studio](https://aistudio.google.com/app/apikey)).
3. Click **⟳ Detect Models** to fetch all available models for your key (e.g., `gemini-2.0-flash`, `gemini-1.5-flash`).
4. Select your preferred model and click **Save & Connect**. Check *"Remember API key"* if you wish to persist it across restarts.

### 2. Live Audio & Transcription
- Play any audio on your machine (YouTube video, Zoom meeting, Google Meet, audio file, etc.).
- The **audio meter** in the top bar will animate with the audio amplitude.
- Real-time transcriptions will stream into the **Live Transcript** panel.

### 3. Real-Time AI Copilot & Answers
- **Auto-Answer Mode**: When enabled (default), the copilot automatically detects question patterns or multiple-choice formats from the speech and queries Gemini with context.
- **Manual Ask AI**: Click the **✦ Ask AI** button on any individual transcript bubble to generate a targeted answer for that specific text.
- **Copy Transcript**: Click **Copy** in the toolbar to copy the entire timestamped conversation transcript to your clipboard.

### 4. Stealth & Overlay Features
- **Drag Anywhere**: Click and drag any background area of the window to reposition it.
- **Screen Share Protection**: The window uses `setSharingType: 0` (`NSWindowSharingNone`), making it invisible to screen capture tools, meeting recordings, and screen sharing sessions.

---

## 🛠️ Building for Production

To build a standalone macOS application (`.dmg` or `.app` bundle):

```bash
npm run tauri build
```

The compiled binary and installer will be located in:
```text
src-tauri/target/release/bundle/dmg/InterviewBuddy_0.1.0_aarch64.dmg
```

---

## 🔍 Troubleshooting & FAQ

### 1. `failed to read plugin permissions... No such file or directory`
If you renamed the project folder, stale Cargo target paths can cause permission read errors. Run:
```bash
cd src-tauri && cargo clean && cd ..
npm run tauri dev
```

### 2. No audio captured or meter stays at 0%
- Check macOS permissions: **System Settings** > **Privacy & Security** > **Screen & System Audio Recording**.
- If permissions were previously revoked, reset them with:
  ```bash
  tccutil reset ScreenCapture
  ```
- Ensure audio is playing through the default system output device.

### 3. Whisper model download issues
The Whisper model is cached in:
- macOS: `~/Library/Application Support/com.qambar.interviewbuddy/models/ggml-tiny.en.bin`
- If the download was interrupted, delete the partial file in that folder and restart the app.

---

## 🤝 Contributing

Contributions are welcome! If you want to improve InterviewBuddy:
1. Fork the Project.
2. Create your Feature Branch (`git checkout -b feature/AmazingFeature`).
3. Commit your Changes (`git commit -m 'Add some AmazingFeature'`).
4. Push to the Branch (`git push origin feature/AmazingFeature`).
5. Open a Pull Request.

---

## 📄 License

Distributed under the MIT License. See `LICENSE` for more information.
