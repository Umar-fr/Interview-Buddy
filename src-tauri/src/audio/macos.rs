use std::ffi::{c_char, c_void, CStr};
use std::sync::mpsc::{channel, Receiver, Sender};
use super::AudioChunk;

// FFI Declarations matching native/macos_audio_capture.h
type AudioDataCallback = unsafe extern "C" fn(
    samples: *const f32,
    num_samples: u32,
    sample_rate: u32,
    channels: u32,
    user_data: *mut c_void,
);

type AudioStatusCallback = unsafe extern "C" fn(
    status_code: i32,
    message: *const c_char,
    user_data: *mut c_void,
);

type MacOSAudioCaptureHandle = *mut c_void;

extern "C" {
    fn macos_audio_capture_start(
        sample_rate: u32,
        channels: u32,
        data_cb: AudioDataCallback,
        status_cb: AudioStatusCallback,
        user_data: *mut c_void,
    ) -> MacOSAudioCaptureHandle;

    fn macos_audio_capture_stop(handle: MacOSAudioCaptureHandle);
}

struct AudioCaptureContext {
    data_sender: Sender<AudioChunk>,
    status_sender: Sender<(i32, String)>,
}

unsafe extern "C" fn on_audio_data(
    samples: *const f32,
    num_samples: u32,
    sample_rate: u32,
    channels: u32,
    user_data: *mut c_void,
) {
    if samples.is_null() || user_data.is_null() || num_samples == 0 {
        return;
    }
    let context = &*(user_data as *const AudioCaptureContext);
    let sample_slice = std::slice::from_raw_parts(samples, num_samples as usize);
    let chunk = AudioChunk {
        samples: sample_slice.to_vec(),
        sample_rate,
        channels,
    };
    let _ = context.data_sender.send(chunk);
}

unsafe extern "C" fn on_audio_status(
    status_code: i32,
    message: *const c_char,
    user_data: *mut c_void,
) {
    if user_data.is_null() {
        return;
    }
    let context = &*(user_data as *const AudioCaptureContext);
    let msg_str = if !message.is_null() {
        CStr::from_ptr(message).to_string_lossy().into_owned()
    } else {
        String::new()
    };
    let _ = context.status_sender.send((status_code, msg_str));
}

/// Safe Rust wrapper managing a macOS ScreenCaptureKit system audio capture session.
pub struct MacOSAudioCapture {
    handle: MacOSAudioCaptureHandle,
    _context: Box<AudioCaptureContext>,
}

// The handle and context pointers are safely managed and can be sent across threads.
unsafe impl Send for MacOSAudioCapture {}

impl MacOSAudioCapture {
    /// Start a new system audio capture session.
    ///
    /// - `sample_rate`: Sample rate (e.g. 16000 for speech-to-text engines).
    /// - `channels`: Channels (1 for mono).
    ///
    /// Returns the session instance, the audio chunks receiver, and the status receiver.
    pub fn start(
        sample_rate: u32,
        channels: u32,
    ) -> Result<(Self, Receiver<AudioChunk>, Receiver<(i32, String)>), String> {
        let (data_tx, data_rx) = channel::<AudioChunk>();
        let (status_tx, status_rx) = channel::<(i32, String)>();

        let context = Box::new(AudioCaptureContext {
            data_sender: data_tx,
            status_sender: status_tx,
        });

        let context_ptr = &*context as *const AudioCaptureContext as *mut c_void;

        let handle = unsafe {
            macos_audio_capture_start(
                sample_rate,
                channels,
                on_audio_data,
                on_audio_status,
                context_ptr,
            )
        };

        if handle.is_null() {
            return Err("Failed to initialize ScreenCaptureKit audio session".to_string());
        }

        let session = Self {
            handle,
            _context: context,
        };

        Ok((session, data_rx, status_rx))
    }
}

impl Drop for MacOSAudioCapture {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            unsafe {
                macos_audio_capture_stop(self.handle);
            }
            self.handle = std::ptr::null_mut();
        }
    }
}
