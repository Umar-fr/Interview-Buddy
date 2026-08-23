#import "macos_audio_capture.h"

#import <Foundation/Foundation.h>
#import <ScreenCaptureKit/ScreenCaptureKit.h>
#import <CoreMedia/CoreMedia.h>
#import <CoreAudio/CoreAudio.h>
#import <AudioToolbox/AudioToolbox.h>

@interface SCKAudioCaptureSession : NSObject <SCStreamOutput, SCStreamDelegate>

@property (nonatomic, strong) SCStream *stream;
@property (nonatomic, strong) dispatch_queue_t audioQueue;
@property (nonatomic, assign) AudioDataCallback dataCallback;
@property (nonatomic, assign) AudioStatusCallback statusCallback;
@property (nonatomic, assign) void *userData;
@property (nonatomic, assign) uint32_t targetSampleRate;
@property (nonatomic, assign) uint32_t targetChannels;
@property (nonatomic, assign) BOOL isRunning;

- (instancetype)initWithSampleRate:(uint32_t)sampleRate
                          channels:(uint32_t)channels
                      dataCallback:(AudioDataCallback)dataCb
                    statusCallback:(AudioStatusCallback)statusCb
                          userData:(void *)userData;

- (void)start;
- (void)stop;

@end

@implementation SCKAudioCaptureSession

- (instancetype)initWithSampleRate:(uint32_t)sampleRate
                          channels:(uint32_t)channels
                      dataCallback:(AudioDataCallback)dataCb
                    statusCallback:(AudioStatusCallback)statusCb
                          userData:(void *)userData {
    self = [super init];
    if (self) {
        _targetSampleRate = sampleRate > 0 ? sampleRate : 16000;
        _targetChannels = channels > 0 ? channels : 1;
        _dataCallback = dataCb;
        _statusCallback = statusCb;
        _userData = userData;
        _isRunning = NO;
        _audioQueue = dispatch_queue_create("com.interviewbuddy.audioqueue", DISPATCH_QUEUE_SERIAL);
    }
    return self;
}

- (void)start {
    // Query shareable content to obtain the primary display for the content filter.
    [SCShareableContent getShareableContentWithCompletionHandler:^(SCShareableContent * _Nullable shareableContent, NSError * _Nullable error) {
        if (error) {
            NSString *errStr = [NSString stringWithFormat:@"Failed to get shareable content: %@", error.localizedDescription];
            if (self.statusCallback) {
                self.statusCallback(-1, errStr.UTF8String, self.userData);
            }
            return;
        }

        SCDisplay *display = shareableContent.displays.firstObject;
        if (!display) {
            if (self.statusCallback) {
                self.statusCallback(-2, "No displays found for audio capture", self.userData);
            }
            return;
        }

        // Create content filter with the display.
        // We exclude nothing so we capture all system audio output, but exclude our own app below.
        SCContentFilter *filter = [[SCContentFilter alloc] initWithDisplay:display excludingWindows:@[]];

        // Configure stream settings.
        SCStreamConfiguration *config = [[SCStreamConfiguration alloc] init];
        
        // System audio capture settings:
        config.capturesAudio = YES;
        
        // Exclude audio produced by this application itself to prevent audio feedback.
        if (@available(macOS 13.0, *)) {
            config.excludesCurrentProcessAudio = YES;
        }
        
        config.sampleRate = self.targetSampleRate;
        config.channelCount = self.targetChannels;
        
        // Minimal video settings to keep video capture overhead effectively zero:
        config.width = 2;
        config.height = 2;
        config.minimumFrameInterval = CMTimeMake(1, 1);
        config.showsCursor = NO;

        // Initialize SCStream
        self.stream = [[SCStream alloc] initWithFilter:filter configuration:config delegate:self];
        
        NSError *addOutputError = nil;
        BOOL added = [self.stream addStreamOutput:self
                                             type:SCStreamOutputTypeAudio
                              sampleHandlerQueue:self.audioQueue
                                           error:&addOutputError];
        if (!added || addOutputError) {
            NSString *errStr = [NSString stringWithFormat:@"Failed to add audio stream output: %@", addOutputError.localizedDescription];
            if (self.statusCallback) {
                self.statusCallback(-3, errStr.UTF8String, self.userData);
            }
            return;
        }

        // Start capturing
        [self.stream startCaptureWithCompletionHandler:^(NSError * _Nullable startError) {
            if (startError) {
                NSString *errStr = [NSString stringWithFormat:@"Failed to start capture: %@", startError.localizedDescription];
                if (self.statusCallback) {
                    self.statusCallback(-4, errStr.UTF8String, self.userData);
                }
            } else {
                self.isRunning = YES;
                if (self.statusCallback) {
                    self.statusCallback(0, "System audio capture active", self.userData);
                }
            }
        }];
    }];
}

- (void)stop {
    if (!self.isRunning || !self.stream) {
        return;
    }
    
    self.isRunning = NO;
    SCStream *streamToStop = self.stream;
    self.stream = nil;

    [streamToStop stopCaptureWithCompletionHandler:^(NSError * _Nullable error) {
        if (error && self.statusCallback) {
            NSString *errStr = [NSString stringWithFormat:@"Error stopping capture: %@", error.localizedDescription];
            self.statusCallback(-5, errStr.UTF8String, self.userData);
        } else if (self.statusCallback) {
            self.statusCallback(1, "System audio capture stopped", self.userData);
        }
    }];
}

#pragma mark - SCStreamOutput Protocol

- (void)stream:(SCStream *)stream didOutputSampleBuffer:(CMSampleBufferRef)sampleBuffer ofType:(SCStreamOutputType)type {
    if (type != SCStreamOutputTypeAudio) {
        return;
    }
    if (!CMSampleBufferIsValid(sampleBuffer) || !CMSampleBufferDataIsReady(sampleBuffer)) {
        return;
    }

    CMFormatDescriptionRef formatDesc = CMSampleBufferGetFormatDescription(sampleBuffer);
    if (!formatDesc) {
        return;
    }

    const AudioStreamBasicDescription *asbd = CMAudioFormatDescriptionGetStreamBasicDescription(formatDesc);
    if (!asbd) {
        return;
    }

    CMItemCount numSamples = CMSampleBufferGetNumSamples(sampleBuffer);
    if (numSamples <= 0) {
        return;
    }

    // Determine required size for AudioBufferList
    size_t bufferListSizeNeeded = 0;
    OSStatus status = CMSampleBufferGetAudioBufferListWithRetainedBlockBuffer(
        sampleBuffer,
        &bufferListSizeNeeded,
        NULL,
        0,
        NULL,
        NULL,
        kCMSampleBufferFlag_AudioBufferList_Assure16ByteAlignment,
        NULL
    );

    if (status != noErr || bufferListSizeNeeded == 0) {
        return;
    }

    AudioBufferList *audioBufferList = (AudioBufferList *)malloc(bufferListSizeNeeded);
    if (!audioBufferList) {
        return;
    }

    CMBlockBufferRef blockBuffer = NULL;
    status = CMSampleBufferGetAudioBufferListWithRetainedBlockBuffer(
        sampleBuffer,
        NULL,
        audioBufferList,
        bufferListSizeNeeded,
        NULL,
        NULL,
        kCMSampleBufferFlag_AudioBufferList_Assure16ByteAlignment,
        &blockBuffer
    );

    if (status != noErr) {
        free(audioBufferList);
        if (blockBuffer) {
            CFRelease(blockBuffer);
        }
        return;
    }

    uint32_t numChannels = asbd->mChannelsPerFrame;
    uint32_t sampleRate = (uint32_t)asbd->mSampleRate;
    BOOL isFloat = (asbd->mFormatFlags & kAudioFormatFlagIsFloat) != 0;
    BOOL isNonInterleaved = (asbd->mFormatFlags & kAudioFormatFlagIsNonInterleaved) != 0;

    // Buffer to hold standard mono float32 output
    float *monoSamples = (float *)malloc(sizeof(float) * numSamples);
    if (!monoSamples) {
        free(audioBufferList);
        if (blockBuffer) {
            CFRelease(blockBuffer);
        }
        return;
    }

    if (isFloat) {
        if (isNonInterleaved && audioBufferList->mNumberBuffers > 1) {
            // Non-interleaved multi-channel (e.g. separate buffers for L and R)
            float *left = (float *)audioBufferList->mBuffers[0].mData;
            float *right = (float *)audioBufferList->mBuffers[1].mData;
            for (CMItemCount i = 0; i < numSamples; i++) {
                monoSamples[i] = (left[i] + right[i]) * 0.5f;
            }
        } else if (numChannels == 1 || audioBufferList->mNumberBuffers == 1) {
            float *source = (float *)audioBufferList->mBuffers[0].mData;
            if (numChannels == 1) {
                memcpy(monoSamples, source, sizeof(float) * numSamples);
            } else {
                // Interleaved multi-channel
                for (CMItemCount i = 0; i < numSamples; i++) {
                    float sum = 0.0f;
                    for (uint32_t ch = 0; ch < numChannels; ch++) {
                        sum += source[i * numChannels + ch];
                    }
                    monoSamples[i] = sum / (float)numChannels;
                }
            }
        }
    } else {
        // 16-bit signed integer PCM
        if (isNonInterleaved && audioBufferList->mNumberBuffers > 1) {
            int16_t *left = (int16_t *)audioBufferList->mBuffers[0].mData;
            int16_t *right = (int16_t *)audioBufferList->mBuffers[1].mData;
            for (CMItemCount i = 0; i < numSamples; i++) {
                monoSamples[i] = ((float)left[i] + (float)right[i]) / (2.0f * 32768.0f);
            }
        } else {
            int16_t *source = (int16_t *)audioBufferList->mBuffers[0].mData;
            for (CMItemCount i = 0; i < numSamples; i++) {
                float sum = 0.0f;
                for (uint32_t ch = 0; ch < numChannels; ch++) {
                    sum += (float)source[i * numChannels + ch] / 32768.0f;
                }
                monoSamples[i] = sum / (float)numChannels;
            }
        }
    }

    // Deliver mono float32 audio samples to callback
    if (self.dataCallback) {
        self.dataCallback(monoSamples, (uint32_t)numSamples, sampleRate, 1, self.userData);
    }

    free(monoSamples);
    free(audioBufferList);

    if (blockBuffer) {
        CFRelease(blockBuffer);
    }
}

#pragma mark - SCStreamDelegate Protocol

- (void)stream:(SCStream *)stream didStopWithError:(NSError *)error {
    self.isRunning = NO;
    if (error && self.statusCallback) {
        NSString *errStr = [NSString stringWithFormat:@"Stream stopped with error: %@", error.localizedDescription];
        self.statusCallback(-6, errStr.UTF8String, self.userData);
    }
}

@end

#pragma mark - C-ABI Exports

MacOSAudioCaptureHandle macos_audio_capture_start(
    uint32_t sample_rate,
    uint32_t channels,
    AudioDataCallback data_cb,
    AudioStatusCallback status_cb,
    void* user_data
) {
    SCKAudioCaptureSession *session = [[SCKAudioCaptureSession alloc] initWithSampleRate:sample_rate
                                                                                channels:channels
                                                                            dataCallback:data_cb
                                                                          statusCallback:status_cb
                                                                                userData:user_data];
    if (!session) {
        return NULL;
    }

    [session start];
    
    // Retain session to pass back as opaque pointer
    return (__bridge_retained void *)session;
}

void macos_audio_capture_stop(MacOSAudioCaptureHandle handle) {
    if (!handle) {
        return;
    }

    SCKAudioCaptureSession *session = (__bridge_transfer SCKAudioCaptureSession *)handle;
    [session stop];
}
