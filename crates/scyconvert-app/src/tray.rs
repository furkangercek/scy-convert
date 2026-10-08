//! The menu bar (tray) icon. While conversions run, the icon would show a
//! small passive spinner: no percentage and no highlight (`indicator`).
//!
//! GPUI has no tray or status-item API, so Windows draws the icon itself
//! through `Shell_NotifyIcon`: clicking it opens the main window, and its
//! right-click menu offers Settings and Quit. While it shows, closing the
//! last window keeps the app running there. macOS draws nothing yet.

use futures::StreamExt;
use futures::channel::mpsc::unbounded;
use gpui_kit::{App, Global};

use crate::model::{self, AppState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Indicator {
    /// The plain scyconvert icon.
    Idle,
    /// The icon with a passive spinner.
    Busy,
}

/// The icon to show, or `None` when the user turned the menu bar icon off.
#[cfg_attr(not(test), allow(dead_code))]
pub fn indicator(state: &AppState) -> Option<Indicator> {
    if !state.settings.menu_bar_icon {
        return None;
    }
    Some(if state.queue.active() > 0 {
        Indicator::Busy
    } else {
        Indicator::Idle
    })
}

/// What the user picked on the tray icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    Open,
    Settings,
    Quit,
}

/// The tray icon, while the app has one.
#[cfg_attr(not(windows), allow(dead_code))]
struct Installed(imp::Tray);

impl Global for Installed {}

/// Whether the app keeps running with no windows: the menu bar icon is on
/// and this platform can show it, or (macOS) the Dock keeps the app.
pub fn keeps_app_running(menu_bar_icon: bool, cx: &App) -> bool {
    menu_bar_icon && (cfg!(target_os = "macos") || cx.has_global::<Installed>())
}

/// Puts the icon in the tray and keeps it in step with the setting.
pub fn install(cx: &mut App) {
    let (tx, mut rx) = unbounded();
    let Some(tray) = imp::Tray::new(tx) else {
        return;
    };
    let state = model::shared(cx);
    tray.set_visible(state.read(cx).settings.menu_bar_icon);
    cx.set_global(Installed(tray));
    cx.observe(&state, |state, cx| {
        let on = state.read(cx).settings.menu_bar_icon;
        cx.global::<Installed>().0.set_visible(on);
    })
    .detach();
    cx.on_app_quit(|cx| {
        cx.remove_global::<Installed>();
        async {}
    })
    .detach();
    cx.spawn(async move |cx| {
        while let Some(event) = rx.next().await {
            cx.update(|cx| match event {
                Event::Open => crate::ui::show_main(cx),
                Event::Settings => crate::ui::show_settings(crate::ui::SettingsTab::General, cx),
                Event::Quit => cx.quit(),
            });
        }
    })
    .detach();
}

#[cfg(windows)]
mod imp {
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicBool, Ordering};

    use futures::channel::mpsc::UnboundedSender;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::HiDpi::{GetDpiForSystem, GetSystemMetricsForDpi};
    use windows_sys::Win32::UI::Shell::{
        NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW, Shell_NotifyIconW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
        GetCursorPos, MF_SEPARATOR, MF_STRING, PostMessageW, RegisterClassW,
        RegisterWindowMessageW, SM_CXSMICON, SetForegroundWindow, SetMenuDefaultItem, TPM_NONOTIFY,
        TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, WM_APP, WM_LBUTTONUP, WM_NULL,
        WM_RBUTTONUP, WNDCLASSW,
    };

    use super::Event;

    const WM_TRAY: u32 = WM_APP + 1;
    const MENU: [(usize, &str, Event); 3] = [
        (1, "Open scyconvert", Event::Open),
        (2, "Settings", Event::Settings),
        (3, "Quit scyconvert", Event::Quit),
    ];

    static EVENTS: OnceLock<UnboundedSender<Event>> = OnceLock::new();
    /// Sent to every top-level window when Explorer restarts; the icon has
    /// to be added again.
    static TASKBAR_CREATED: OnceLock<u32> = OnceLock::new();
    static SHOWN: AtomicBool = AtomicBool::new(false);

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    /// A hidden top-level window receives the icon's clicks. It is never
    /// shown, so it has no taskbar button.
    pub struct Tray {
        hwnd: HWND,
    }

    impl Tray {
        pub fn new(events: UnboundedSender<Event>) -> Option<Self> {
            EVENTS.set(events).ok()?;
            let class = wide("scyconvert-tray");
            // SAFETY: valid NUL-terminated strings that outlive the calls;
            // the class is registered once, before the window is created.
            let hwnd = unsafe {
                TASKBAR_CREATED
                    .get_or_init(|| RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()));
                let instance = GetModuleHandleW(std::ptr::null());
                let mut wc: WNDCLASSW = std::mem::zeroed();
                wc.lpfnWndProc = Some(window_proc);
                wc.hInstance = instance;
                wc.lpszClassName = class.as_ptr();
                RegisterClassW(&wc);
                CreateWindowExW(
                    0,
                    class.as_ptr(),
                    class.as_ptr(),
                    0,
                    0,
                    0,
                    0,
                    0,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    instance,
                    std::ptr::null(),
                )
            };
            if hwnd.is_null() {
                tracing::warn!("could not create the tray window");
                return None;
            }
            Some(Self { hwnd })
        }

        pub fn set_visible(&self, on: bool) {
            if SHOWN.swap(on, Ordering::SeqCst) == on {
                return;
            }
            if on {
                add(self.hwnd);
            } else {
                remove(self.hwnd);
            }
        }
    }

    impl Drop for Tray {
        fn drop(&mut self) {
            self.set_visible(false);
            // SAFETY: the window was created by `Tray::new` on this thread.
            unsafe { DestroyWindow(self.hwnd) };
        }
    }

    fn data(hwnd: HWND) -> NOTIFYICONDATAW {
        // SAFETY: NOTIFYICONDATAW is plain data; all-zero is a valid value.
        let mut data: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
        data.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
        data.hWnd = hwnd;
        data.uID = 1;
        data
    }

    fn add(hwnd: HWND) {
        let mut data = data(hwnd);
        data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        data.uCallbackMessage = WM_TRAY;
        // SAFETY: plain metric queries.
        data.hIcon =
            crate::icon::load(unsafe { GetSystemMetricsForDpi(SM_CXSMICON, GetDpiForSystem()) });
        for (dst, src) in data.szTip.iter_mut().zip("scyconvert".encode_utf16()) {
            *dst = src;
        }
        // SAFETY: `data` is fully initialized for NIM_ADD.
        if unsafe { Shell_NotifyIconW(NIM_ADD, &data) } == 0 {
            tracing::warn!("could not add the tray icon");
        }
    }

    fn remove(hwnd: HWND) {
        // SAFETY: deleting needs only the window and id.
        unsafe { Shell_NotifyIconW(NIM_DELETE, &data(hwnd)) };
    }

    fn send(event: Event) {
        if let Some(events) = EVENTS.get() {
            drop(events.unbounded_send(event));
        }
    }

    /// Shows the right-click menu at the cursor and returns the pick.
    unsafe fn menu(hwnd: HWND) -> Option<Event> {
        let labels: Vec<_> = MENU.iter().map(|(_, label, _)| wide(label)).collect();
        // SAFETY: the labels outlive the menu; TrackPopupMenu needs the
        // owner in the foreground, and the WM_NULL lets the menu close when
        // the user clicks elsewhere.
        unsafe {
            let menu = CreatePopupMenu();
            for (i, (id, ..)) in MENU.iter().enumerate() {
                if i == MENU.len() - 1 {
                    AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
                }
                AppendMenuW(menu, MF_STRING, *id, labels[i].as_ptr());
            }
            SetMenuDefaultItem(menu, MENU[0].0 as u32, 0);
            let mut at = POINT { x: 0, y: 0 };
            GetCursorPos(&mut at);
            SetForegroundWindow(hwnd);
            let picked = TrackPopupMenu(
                menu,
                TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY,
                at.x,
                at.y,
                0,
                hwnd,
                std::ptr::null(),
            );
            PostMessageW(hwnd, WM_NULL, 0, 0);
            DestroyMenu(menu);
            MENU.iter()
                .find(|(id, ..)| *id == picked as usize)
                .map(|(.., event)| *event)
        }
    }

    unsafe extern "system" fn window_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if msg == WM_TRAY {
            match (lparam & 0xFFFF) as u32 {
                WM_LBUTTONUP => send(Event::Open),
                // SAFETY: called on the window's own thread.
                WM_RBUTTONUP => {
                    if let Some(event) = unsafe { menu(hwnd) } {
                        send(event);
                    }
                }
                _ => {}
            }
            return 0;
        }
        if TASKBAR_CREATED.get() == Some(&msg) {
            if SHOWN.load(Ordering::SeqCst) {
                add(hwnd);
            }
            return 0;
        }
        // SAFETY: forwarding the message unchanged.
        unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
    }
}

#[cfg(not(windows))]
mod imp {
    use futures::channel::mpsc::UnboundedSender;

    use super::Event;

    pub struct Tray;

    impl Tray {
        pub fn new(_: UnboundedSender<Event>) -> Option<Self> {
            None
        }

        pub fn set_visible(&self, _: bool) {}
    }
}
