# InterviewBuddy

InterviewBuddy is a desktop app for live system-audio transcription and optional Gemini-powered assistance.

## Download

| Platform | Download |
| --- | --- |
| Windows 10/11 (x64) | [Download installer](https://github.com/Umar-fr/Interview-Buddy/releases/latest/download/InterviewBuddy_0.1.5_x64-setup.exe) |
| macOS (Intel) | [Download DMG](https://github.com/Umar-fr/Interview-Buddy/releases/latest/download/InterviewBuddy_0.1.5_x64.dmg) |
| macOS (Apple Silicon) | [Download DMG](https://github.com/Umar-fr/Interview-Buddy/releases/latest/download/InterviewBuddy_0.1.5_aarch64.dmg) |

These links will work after the `v0.1.5` release is published. For earlier versions, visit [All Releases](https://github.com/Umar-fr/Interview-Buddy/releases).

### Installation notes

- **Windows:** Run the downloaded installer. It installs per-user and downloads WebView2 if it is missing. Windows SmartScreen may show a warning because the installer is not signed.
- **macOS:** Open the DMG and drag InterviewBuddy into Applications. The app is unsigned and may require Control-click → **Open** the first time. macOS 13 or later is required.
- **First launch:** Choose a multilingual Whisper model (Tiny ~75 MB, Base ~150 MB, or Small ~500 MB) and a transcription language. Tiny is recommended for responsive live transcription on Windows; Windows users with the previous default Base model are moved to Tiny once. The **Downloads** button shows download progress; models are verified and cached under `%LOCALAPPDATA%\InterviewBuddy\models`. An internet connection is required for the initial download.
- **Transcription languages:** Auto-detect is recommended for speech that mixes Hindi and English. Results vary with audio quality and code-switching.
- **AI answers:** Add your own Gemini API key in the app settings. Speech transcription runs locally; Gemini features send the text you choose to process to Google.

## Using the app

1. On first launch, select your Whisper model and transcription language. Later launches use the saved choices and start audio capture automatically.
2. Play audio through your computer’s default output device. Transcriptions appear in the Live Transcript panel with capture times and processing delay.
3. To enable Gemini answers, open **Gemini AI** and click the Google AI Studio link to open its key page in your browser. Detect, test, and save a model; compatible Gemini models are ordered from most capable to fastest. Auto-Answer needs a connected Gemini key/model and responds to detected questions.
4. Optionally add resume/background details in settings to personalize responses.

| Shortcut | Action |
| --- | --- |
| `Ctrl + Shift + H` | Hide/show the app |
| `Ctrl + Shift + C` | Open Coding Mode |

## Troubleshooting

- **No audio detected:** Check that sound is playing through the default output device. On macOS, grant Screen & System Audio Recording permission in System Settings.
- **First transcription takes time:** The Whisper model downloads on first use; subsequent launches use the local copy.
- **Model download status:** Open **Downloads** to see progress, verification, or a retry option if the model download fails.
- **Slow or delayed transcription:** Open the Whisper model settings and select Tiny for the fastest CPU transcription. Continuous speech is grouped into longer segments instead of being cut every 1.5 seconds; pending old segments are replaced by newer speech when inference falls behind.
- **Audio or transcription stops unexpectedly:** Open **Logs**, select and copy the diagnostics, then share them with your bug report. The log does not include recorded audio or transcript text; review it before sharing.
- **Gemini answers are unavailable:** Confirm that your Gemini API key and selected model are valid. Transcription does not require a Gemini key.

## Contributing

Bug reports and pull requests are welcome. For a useful bug report, include your operating system, app version, steps to reproduce, and any relevant error details. Please do not include API keys, resumes, or private transcripts.

To build from source, install Node.js 18+, Rust, and the platform’s Tauri prerequisites (Visual Studio C++ Build Tools, LLVM, and CMake on Windows; Xcode Command Line Tools and CMake on macOS), then run:

```bash
npm ci
npm run tauri dev
```

To run the Rust tests on Windows with Visual Studio 2026, open its x64 Developer Command Prompt and run:

```bat
set "CMAKE_GENERATOR=Ninja"
set "CMAKE_GENERATOR_PLATFORM="
set "CMAKE_GENERATOR_TOOLSET="
set "PATH=%ProgramFiles%\Microsoft Visual Studio\18\Community\Common7\IDE\CommonExtensions\Microsoft\CMake\Ninja;%PATH%"
cargo clean --manifest-path src-tauri\Cargo.toml -p whisper-rs-sys
cargo test --manifest-path src-tauri\Cargo.toml --lib --locked
npm run tauri dev
```

The clean removes a cached CMake build if it was previously configured with a different generator.

For changes, create a focused branch, run `npm run build`, and open a pull request describing the behavior changed and how you tested it.
