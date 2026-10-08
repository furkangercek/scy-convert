//! Prints the classic Explorer right-click menu ("Show more options") for a
//! file, submenus included, as the shell builds it from the registry. Shows
//! whether the registered handler appears without opening Explorer.
#[cfg(windows)]
fn main() -> windows::core::Result<()> {
    use windows::Win32::Foundation::*;
    use windows::Win32::System::Com::*;
    use windows::Win32::UI::Shell::*;
    use windows::Win32::UI::WindowsAndMessaging::*;
    use windows::core::*;

    unsafe fn print(menu: HMENU, depth: usize) {
        unsafe {
            for i in 0..GetMenuItemCount(Some(menu)).max(0) {
                let mut text = [0u16; 256];
                let mut info = MENUITEMINFOW {
                    cbSize: size_of::<MENUITEMINFOW>() as u32,
                    fMask: MIIM_STRING | MIIM_SUBMENU | MIIM_FTYPE,
                    dwTypeData: PWSTR(text.as_mut_ptr()),
                    cch: text.len() as u32,
                    ..Default::default()
                };
                if GetMenuItemInfoW(menu, i as u32, true, &mut info).is_err()
                    || info.fType.contains(MFT_SEPARATOR)
                {
                    continue;
                }
                let label = String::from_utf16_lossy(&text[..info.cch as usize]);
                println!("{}{label}", "  ".repeat(depth));
                if !info.hSubMenu.is_invalid() {
                    print(info.hSubMenu, depth + 1);
                }
            }
        }
    }

    let path = std::env::args().nth(1).expect("context-menu <file>");
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
        let item: IShellItem = SHCreateItemFromParsingName(&HSTRING::from(path), None)?;
        let menu: IContextMenu = item.BindToHandler(None, &BHID_SFUIObject)?;
        let popup = CreatePopupMenu()?;
        menu.QueryContextMenu(popup, 0, 1, 0x7FFF, CMF_NORMAL)
            .ok()?;
        // Submenus fill in when opened; ask for each, as Explorer does.
        if let Ok(menu3) = menu.cast::<IContextMenu3>() {
            for i in 0..GetMenuItemCount(Some(popup)).max(0) {
                let sub = GetSubMenu(popup, i);
                if !sub.is_invalid() {
                    let mut result = LRESULT(0);
                    let _ = menu3.HandleMenuMsg2(
                        WM_INITMENUPOPUP,
                        WPARAM(sub.0 as usize),
                        LPARAM(i as isize),
                        Some(&mut result),
                    );
                }
            }
        }
        print(popup, 0);
    }
    Ok(())
}

#[cfg(not(windows))]
fn main() {}
