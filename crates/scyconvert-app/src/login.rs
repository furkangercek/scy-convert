//! Starting the app minimized when the user signs in. The operating system
//! holds the state, so the switch in Settings and the installer's checkbox
//! always agree: a per-user `Run` registry value on Windows, a LaunchAgent
//! on macOS.

use std::io;

/// The argument the login entry passes; see `request::parse_args`.
pub const ARG: &str = "--minimized";

/// Whether the app is set to start at sign-in. `None` where this platform
/// has no login item support.
pub fn enabled() -> Option<bool> {
    imp::enabled()
}

/// Adds or removes the login entry for the running executable.
pub fn set(on: bool) -> io::Result<()> {
    imp::set(on)
}

#[cfg(windows)]
mod imp {
    use std::io;
    use std::os::windows::ffi::OsStrExt;

    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{
        HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
    };

    const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    /// The installer writes the same value (packaging/windows/scyconvert.iss).
    const VALUE: &str = "scyconvert";

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    pub fn enabled() -> Option<bool> {
        let (key, value) = (wide(RUN), wide(VALUE));
        // SAFETY: valid NUL-terminated strings; no output buffer requested.
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                value.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        Some(status == ERROR_SUCCESS)
    }

    pub fn set(on: bool) -> io::Result<()> {
        let (key, value) = (wide(RUN), wide(VALUE));
        let status = if on {
            let exe = std::env::current_exe()?;
            let mut command: Vec<u16> = "\"".encode_utf16().collect();
            command.extend(exe.as_os_str().encode_wide());
            command.extend(format!("\" {}", super::ARG).encode_utf16());
            command.push(0);
            // SAFETY: valid NUL-terminated strings; the data length is in bytes.
            unsafe {
                RegSetKeyValueW(
                    HKEY_CURRENT_USER,
                    key.as_ptr(),
                    value.as_ptr(),
                    REG_SZ,
                    command.as_ptr().cast(),
                    (command.len() * 2) as u32,
                )
            }
        } else {
            // SAFETY: valid NUL-terminated strings.
            match unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), value.as_ptr()) } {
                ERROR_FILE_NOT_FOUND => ERROR_SUCCESS,
                status => status,
            }
        };
        if status == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(io::Error::from_raw_os_error(status as i32))
        }
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::io;
    use std::path::PathBuf;

    const LABEL: &str = "io.github.furkangercek.scyconvert";

    fn plist() -> Option<PathBuf> {
        dirs::home_dir().map(|h| h.join(format!("Library/LaunchAgents/{LABEL}.plist")))
    }

    pub fn enabled() -> Option<bool> {
        plist().map(|p| p.is_file())
    }

    pub fn set(on: bool) -> io::Result<()> {
        let path = plist().ok_or_else(|| io::Error::other("no home folder"))?;
        if !on {
            return match std::fs::remove_file(&path) {
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
                result => result,
            };
        }
        let exe = std::env::current_exe()?;
        let exe = escape(&exe.to_string_lossy());
        let arg = super::ARG;
        let body = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>Label</key>
	<string>{LABEL}</string>
	<key>ProgramArguments</key>
	<array>
		<string>{exe}</string>
		<string>{arg}</string>
	</array>
	<key>RunAtLoad</key>
	<true/>
	<key>ProcessType</key>
	<string>Interactive</string>
</dict>
</plist>
"#
        );
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, body)
    }

    fn escape(s: &str) -> String {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod imp {
    use std::io;

    pub fn enabled() -> Option<bool> {
        None
    }

    pub fn set(_: bool) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }
}
