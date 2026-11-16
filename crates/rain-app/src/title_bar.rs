//! The window's title bar in the scheme's own colours: Windows 11 draws the caption and its text
//! in the colours given, and dark mode turns the buttons light. Older Windows ignores the colours
//! and keeps dark mode only. A no-op elsewhere and before the main window exists.

use std::sync::atomic::{AtomicIsize, AtomicU64, Ordering};

use rune::Color;

/// The main window, the first one shown.
static WINDOW: AtomicIsize = AtomicIsize::new(0);
/// What was last painted, so a frame that changes nothing makes no call.
static PAINTED: AtomicU64 = AtomicU64::new(u64::MAX);

/// `0x00BBGGRR` from a linear colour.
fn colorref(c: Color) -> u32 {
    let b = |v: f32| (rune::color::srgb_encode(v).clamp(0.0, 1.0) * 255.0).round() as u32;
    b(c.r) | (b(c.g) << 8) | (b(c.b) << 16)
}

#[cfg(windows)]
pub fn remember(window: &rune::WindowHandle) {
    use rune::raw_window_handle::RawWindowHandle;
    let _ = window.with_raw_handle(|handle| {
        if let RawWindowHandle::Win32(h) = handle.as_raw() {
            let _ = WINDOW.compare_exchange(0, h.hwnd.get(), Ordering::SeqCst, Ordering::SeqCst);
        }
    });
}

#[cfg(not(windows))]
pub fn remember(_: &rune::WindowHandle) {}

/// Paints the main window's title bar: `dark` mode, `caption` behind the title, `text` for it.
pub fn paint(dark: bool, caption: Color, text: Color) {
    let key = u64::from(colorref(caption)) | (u64::from(colorref(text)) << 24) | (u64::from(dark) << 48);
    let hwnd = WINDOW.load(Ordering::SeqCst);
    if hwnd == 0 || PAINTED.swap(key, Ordering::SeqCst) == key {
        return;
    }
    set(hwnd, dark, colorref(caption), colorref(text));
}

#[cfg(windows)]
fn set(hwnd: isize, dark: bool, caption: u32, text: u32) {
    use windows_sys::Win32::Graphics::Dwm::{DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute};
    let hwnd = hwnd as windows_sys::Win32::Foundation::HWND;
    let dark: i32 = dark.into();
    // SAFETY: each call reads one 4-byte value from a local for the window this process made;
    // a failure (older Windows, a closed window) is ignored.
    unsafe {
        DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE as u32, (&raw const dark).cast(), 4);
        DwmSetWindowAttribute(hwnd, DWMWA_CAPTION_COLOR as u32, (&raw const caption).cast(), 4);
        DwmSetWindowAttribute(hwnd, DWMWA_TEXT_COLOR as u32, (&raw const text).cast(), 4);
    }
}

#[cfg(not(windows))]
fn set(_: isize, _: bool, _: u32, _: u32) {}
