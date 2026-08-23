fn main() {
    #[cfg(target_os = "macos")]
    {
        cc::Build::new()
            .file("native/macos_audio_capture.m")
            .flag("-fmodules")
            .flag("-fcxx-modules")
            .flag("-fobjc-arc")
            .compile("macos_audio_capture");

        println!("cargo:rustc-link-lib=framework=ScreenCaptureKit");
        println!("cargo:rustc-link-lib=framework=CoreMedia");
        println!("cargo:rustc-link-lib=framework=CoreAudio");
        println!("cargo:rustc-link-lib=framework=AudioToolbox");
        println!("cargo:rustc-link-lib=framework=AVFoundation");
        println!("cargo:rustc-link-lib=framework=Foundation");
        println!("cargo:rerun-if-changed=native/macos_audio_capture.h");
        println!("cargo:rerun-if-changed=native/macos_audio_capture.m");
    }

    tauri_build::build();
}

