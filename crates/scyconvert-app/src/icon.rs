//! The app icon on Windows windows. GPUI registers only the large icon on its
//! window class, so Windows shrinks the 32 px icon into the title bar
//! instead of using the .ico's own 16 px size.

use gpui_kit::Window;

/// Gives `window` the icon sizes its title bar and Alt-Tab ask for.
pub fn apply(window: &Window) {
    imp::apply(window);
}

#[cfg(windows)]
pub use imp::load;

#[cfg(windows)]
mod imp {
    use std::sync::Mutex;

    use gpui_kit::Window;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        HICON, ICON_BIG, ICON_SMALL, IMAGE_ICON, LoadImageW, SM_CXICON, SM_CXSMICON, SendMessageW,
        WM_SETICON,
    };

    /// Loaded icons by pixel size. Windows and the tray share them for the
    /// life of the process.
    static LOADED: Mutex<Vec<(i32, isize)>> = Mutex::new(Vec::new());

    /// Icon resource 1 (build.rs) at `size` px, picked from the .ico's sizes.
    pub fn load(size: i32) -> HICON {
        let mut loaded = LOADED.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(&(_, icon)) = loaded.iter().find(|(s, _)| *s == size) {
            return icon as HICON;
        }
        // SAFETY: resource 1 is an icon embedded in this executable; the
        // integer resource id is passed the MAKEINTRESOURCE way.
        let icon = unsafe {
            LoadImageW(
                GetModuleHandleW(std::ptr::null()),
                std::ptr::without_provenance(1),
                IMAGE_ICON,
                size,
                size,
                0,
            )
        };
        if !icon.is_null() {
            loaded.push((size, icon as isize));
        }
        icon as HICON
    }

    pub fn apply(window: &Window) {
        let Ok(handle) = HasWindowHandle::window_handle(window) else {
            return;
        };
        let RawWindowHandle::Win32(handle) = handle.as_raw() else {
            return;
        };
        let hwnd = handle.hwnd.get() as HWND;
        // SAFETY: `hwnd` is the live window GPUI just handed out.
        unsafe {
            let dpi = GetDpiForWindow(hwnd);
            for (which, metric) in [(ICON_SMALL, SM_CXSMICON), (ICON_BIG, SM_CXICON)] {
                let icon = load(GetSystemMetricsForDpi(metric, dpi));
                if !icon.is_null() {
                    SendMessageW(hwnd, WM_SETICON, which as usize, icon as isize);
                }
            }
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn apply(_: &gpui_kit::Window) {}
}
