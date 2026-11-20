//! `frayed-app`: Android entry point — Rust domain + Slint UI + thin JNI glue.

#[cfg(target_os = "android")]
mod android;
mod state;

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub extern "C" fn android_main(app: slint::android::AndroidApp) {
    android::init(&app);
    slint::android::init(app).unwrap();
    let _ = frayed_ui::register_fonts();
    let window = frayed_ui::create_window();
    let _state = state::App::new(window.clone_strong());
    window.run().unwrap();
}

#[cfg(target_os = "android")]
use slint::ComponentHandle as _;

#[cfg(not(target_os = "android"))]
pub fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Desktop preview of the same UI/state wiring (handy without an Android SDK).
#[cfg(not(target_os = "android"))]
pub fn run() -> Result<(), slint::PlatformError> {
    use slint::ComponentHandle as _;
    let _ = frayed_ui::register_fonts();
    let window = frayed_ui::create_window();
    let _state = state::App::new(window.clone_strong());
    window.run()
}
