# InterviewBuddy

InterviewBuddy is a desktop app for live system-audio transcription and optional Gemini-powered assistance.

## Download

| Platform | Download |
| --- | --- |
| Windows 10/11 (x64) | [Download installer](https://github.com/Umar-fr/Interview-Buddy/releases/latest/download/InterviewBuddy_0.1.2_x64-setup.exe) |
| macOS (Intel) | [Download DMG](https://github.com/Umar-fr/Interview-Buddy/releases/latest/download/InterviewBuddy_0.1.2_x64.dmg) |
| macOS (Apple Silicon) | [Download DMG](https://github.com/Umar-fr/Interview-Buddy/releases/latest/download/InterviewBuddy_0.1.2_aarch64.dmg) |

These links will work after the `v0.1.2` release is published. For earlier versions, visit [All Releases](https://github.com/Umar-fr/Interview-Buddy/releases).

### Installation notes

- **Windows:** Run the downloaded installer. It installs per-user and downloads WebView2 if it is missing. Windows SmartScreen may show a warning because the installer is not signed.
- **macOS:** Open the DMG and drag InterviewBuddy into Applications. The app is unsigned and may require Control-click → **Open** the first time. macOS 13 or later is required.
- **First launch:** Choose a multilingual Whisper model (Tiny ~75 MB, Base ~150 MB, or Small ~500 MB) and a transcription language. The selected model downloads on first use and is cached locally; an internet connection is required for the initial download.
- **Transcription languages:** Auto-detect is recommended for speech that mixes Hindi and English. Results vary with audio quality and code-switching.
- **AI answers:** Add your own Gemini API key in the app settings. Speech transcription runs locally; Gemini features send the text you choose to process to Google.

## Using the app

1. On first launch, select your Whisper model and transcription language. Later launches use the saved choices and start audio capture automatically.
2. Play audio through your computer’s default output device. Transcriptions appear in the Live Transcript panel.
3. To enable Gemini answers, open **Gemini AI**, enter your API key, detect/select a model, and save.
4. Optionally add resume/background details in settings to personalize responses.

| Shortcut | Action |
| --- | --- |
| `Ctrl + Shift + H` | Hide/show the app |
| `Ctrl + Shift + C` | Open Coding Mode |

## Troubleshooting

- **No audio detected:** Check that sound is playing through the default output device. On macOS, grant Screen & System Audio Recording permission in System Settings.
- **First transcription takes time:** The Whisper model downloads on first use; subsequent launches use the local copy.
- **Windows closes during transcription:** Install the latest release linked above. If it still closes, report your Windows version and the faulting module/exception code from Reliability Monitor.
- **Gemini answers are unavailable:** Confirm that your Gemini API key and selected model are valid. Transcription does not require a Gemini key.

## Contributing

Bug reports and pull requests are welcome. For a useful bug report, include your operating system, app version, steps to reproduce, and any relevant error details. Please do not include API keys, resumes, or private transcripts.

To build from source, install Node.js 18+, Rust, and the platform’s Tauri prerequisites (Visual Studio C++ Build Tools, LLVM, and CMake on Windows; Xcode Command Line Tools and CMake on macOS), then run:

```bash
npm ci
npm run tauri dev
```

For changes, create a focused branch, run `npm run build`, and open a pull request describing the behavior changed and how you tested it.
