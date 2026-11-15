//! The window's title-bar and taskbar icon: the exe's embedded icon (resource 1), which winit
//! does not pick up by itself. A no-op where the running exe has no icon (tests, examples).

#[cfg(windows)]
pub fn apply(window: &rune::WindowHandle) {
    use rune::raw_window_handle::RawWindowHandle;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, ICON_BIG, ICON_SMALL, IMAGE_ICON, LR_DEFAULTCOLOR, LoadImageW, SM_CXICON, SM_CXSMICON, SM_CYICON, SM_CYSMICON,
        SendMessageW, WM_SETICON,
    };

    let _ = window.with_raw_handle(|handle| {
        let RawWindowHandle::Win32(h) = handle.as_raw() else {
            return;
        };
        let hwnd = h.hwnd.get() as windows_sys::Win32::Foundation::HWND;
        // SAFETY: plain Win32 calls on the live window this callback borrows; resource 1 is
        // read-only data in our own module, and a failed load returns null, which we skip.
        unsafe {
            let module = GetModuleHandleW(std::ptr::null());
            for (kind, cx, cy) in [(ICON_BIG, SM_CXICON, SM_CYICON), (ICON_SMALL, SM_CXSMICON, SM_CYSMICON)] {
                let icon = LoadImageW(
                    module,
                    std::ptr::without_provenance(1),
                    IMAGE_ICON,
                    GetSystemMetrics(cx),
                    GetSystemMetrics(cy),
                    LR_DEFAULTCOLOR,
                );
                if !icon.is_null() {
                    SendMessageW(hwnd, WM_SETICON, kind as usize, icon as isize);
                }
            }
        }
    });
}

#[cfg(not(windows))]
pub fn apply(_: &rune::WindowHandle) {}
