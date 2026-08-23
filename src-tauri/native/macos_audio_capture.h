#ifndef MACOS_AUDIO_CAPTURE_H
#define MACOS_AUDIO_CAPTURE_H

#include <stdint.h>
#include <stdbool.h>

#ifdef __cplusplus
extern "C" {
#endif

// Callback for delivering audio PCM samples (mono or interleaved float32).
typedef void (*AudioDataCallback)(
    const float* samples,
    uint32_t num_samples,
    uint32_t sample_rate,
    uint32_t channels,
    void* user_data
);

// Callback for reporting capture status or errors.
typedef void (*AudioStatusCallback)(
    int32_t status_code,
    const char* message,
    void* user_data
);

// Opaque capture handle.
typedef void* MacOSAudioCaptureHandle;

/**
 * Start capturing system audio using ScreenCaptureKit.
 * 
 * @param sample_rate Requested sample rate (e.g. 16000 for speech recognition).
 * @param channels Number of channels (e.g. 1 for mono).
 * @param data_cb Callback invoked when new audio samples arrive.
 * @param status_cb Callback invoked on state changes or errors.
 * @param user_data Context pointer passed back into callbacks.
 * @return Opaque handle to the capture session, or NULL on immediate failure.
 */
MacOSAudioCaptureHandle macos_audio_capture_start(
    uint32_t sample_rate,
    uint32_t channels,
    AudioDataCallback data_cb,
    AudioStatusCallback status_cb,
    void* user_data
);

/**
 * Stop an active system audio capture session and release all resources.
 * 
 * @param handle The capture handle returned by macos_audio_capture_start.
 */
void macos_audio_capture_stop(MacOSAudioCaptureHandle handle);

#ifdef __cplusplus
}
#endif

#endif // MACOS_AUDIO_CAPTURE_H
