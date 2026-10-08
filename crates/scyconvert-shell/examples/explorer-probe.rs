//! Run on Windows against the installed DLL, without opening Explorer.
//!
//!     explorer-probe <installed-DLL> <files...>
//!     explorer-probe --registered <ignored> <files...>
//!
//! Builds each top-level menu the way Explorer does, prints it, and checks it
//! against what the installed CLI reports for the same files. With
//! SCYCONVERT_SHELL_CAPTURE set, it also picks the first entry of the first
//! menu, which hands the request to the app.
#[cfg(windows)]
fn main() -> windows::core::Result<()> {
    use std::ffi::c_void;
    use std::path::PathBuf;
    use windows::Win32::{
        System::{Com::*, LibraryLoader::*},
        UI::Shell::*,
    };
    use windows::core::*;

    const CLSIDS: [u128; 4] = [
        0xbb1183d6_e6ca_44e1_906c_a0a47845841d,
        0xb77c2bc9_b09c_4659_85cf_d4e34fc6ca15,
        0x7b070ad5_9dfe_4769_b4a3_27cc8b0fb2ca,
        0xa7791315_7700_4e6d_aee1_5df43833c4d8,
    ];
    let mut args: Vec<_> = std::env::args_os().skip(1).collect();
    let registered = args.first().is_some_and(|arg| arg == "--registered");
    if registered {
        args.remove(0);
    }
    assert!(args.len() >= 2, "explorer-probe <installed-DLL> <files...>");
    let paths: Vec<_> = args[1..].iter().map(PathBuf::from).collect();
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
        let get = if registered {
            None
        } else {
            let module = LoadLibraryW(&HSTRING::from(&args[0]))?;
            let proc = GetProcAddress(module, s!("DllGetClassObject")).expect("COM export");
            Some(std::mem::transmute::<
                unsafe extern "system" fn() -> isize,
                unsafe extern "system" fn(*const GUID, *const GUID, *mut *mut c_void) -> HRESULT,
            >(proc))
        };
        let create = |class: GUID| -> Result<IExplorerCommand> {
            match get {
                // LOCAL_SERVER tests the sparse packaged COM surrogate,
                // rather than the classic HKCU in-process registration.
                None => CoCreateInstance(&class, None, CLSCTX_LOCAL_SERVER),
                Some(get) => {
                    let mut raw = std::ptr::null_mut();
                    get(&class, &IClassFactory::IID, &mut raw).ok()?;
                    IClassFactory::from_raw(raw).CreateInstance(None)
                }
            }
        };
        let mut pidls = Vec::new();
        for path in &paths {
            let mut pidl = std::ptr::null_mut();
            SHParseDisplayName(&HSTRING::from(path.as_os_str()), None, &mut pidl, 0, None)?;
            pidls.push(pidl);
        }
        let pointers: Vec<_> = pidls.iter().map(|p| *p as *const _).collect();
        let items: IShellItemArray = SHCreateShellItemArrayFromIDLists(&pointers)?;
        for pidl in pidls {
            CoTaskMemFree(Some(pidl.cast()));
        }

        let cli = PathBuf::from(&args[0])
            .parent()
            .unwrap()
            .join("scyconvert.exe");
        let per_file: Vec<_> = paths
            .iter()
            .map(|path| {
                let output = std::process::Command::new(&cli)
                    .arg("targets")
                    .arg(path)
                    .arg("--menu")
                    .output()
                    .unwrap();
                output
                    .status
                    .success()
                    .then(|| {
                        scyconvert_shell::parse_menus(&String::from_utf8(output.stdout).unwrap())
                    })
                    .flatten()
                    .unwrap_or_default()
            })
            .collect();
        let expected = scyconvert_shell::common_menus(&per_file);

        let mut first = None;
        for (i, (id, title)) in scyconvert_shell::MENUS.iter().enumerate() {
            let root = create(GUID::from_u128(CLSIDS[i]))?;
            // GetTitle hands the selection to the command, as Explorer does.
            let shown = root.GetTitle(&items)?;
            assert_eq!(String::from_utf16_lossy(shown.as_wide()), *title);
            CoTaskMemFree(Some(shown.0.cast()));
            let state = root.GetState(&items, true)?;
            let want: Vec<String> = expected
                .iter()
                .find(|m| m.id == *id)
                .map(|m| render(&m.entries))
                .unwrap_or_default();
            let hidden = state == ECS_HIDDEN.0 as u32;
            assert_eq!(hidden, want.is_empty(), "{title}: state {state}");
            if hidden {
                println!("{title}: hidden");
                continue;
            }
            let got = entries(&root, &items)?;
            assert_eq!(got, want, "{title}");
            println!("{title} >");
            for line in &got {
                println!("  {line}");
            }
            first.get_or_insert(root);
        }

        // Optional capture fixture lives beside a COPY of the real DLL/CLI.
        // It proves Invoke passes UTF-16 paths safely without any GUI input.
        if std::env::var_os("SCYCONVERT_SHELL_CAPTURE").is_some()
            && let Some(root) = &first
            && let Some(leaf) = first_leaf(root, &items)?
        {
            leaf.Invoke(&items, None)?;
            println!("PASS Invoke dispatched {} paths", paths.len());
        }
        CoUninitialize();
    }
    Ok(())
}

/// A menu's entries as lines: separators as `---`, headings as `[text]`.
#[cfg(windows)]
fn entries(
    command: &windows::Win32::UI::Shell::IExplorerCommand,
    items: &windows::Win32::UI::Shell::IShellItemArray,
) -> windows::core::Result<Vec<String>> {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{ECF_ISSEPARATOR, ECS_DISABLED};
    use windows::core::HRESULT;
    let mut lines = Vec::new();
    let children = unsafe { command.EnumSubCommands()? };
    loop {
        let mut next = [None];
        let status = unsafe { children.Next(&mut next, None) };
        if status == HRESULT(1) {
            break;
        }
        status.ok()?;
        let child = next[0].take().expect("one command");
        if unsafe { child.GetFlags()? } & ECF_ISSEPARATOR.0 as u32 != 0 {
            lines.push("---".into());
            continue;
        }
        let title = unsafe { child.GetTitle(items)? };
        let text = String::from_utf16_lossy(unsafe { title.as_wide() });
        unsafe { CoTaskMemFree(Some(title.0.cast())) };
        let disabled = unsafe { child.GetState(items, true)? } & ECS_DISABLED.0 as u32 != 0;
        lines.push(if disabled { format!("[{text}]") } else { text });
    }
    Ok(lines)
}

#[cfg(windows)]
fn render(entries: &[scyconvert_shell::Entry]) -> Vec<String> {
    use scyconvert_shell::Entry;
    entries
        .iter()
        .map(|e| match e {
            Entry::Heading(text) => format!("[{text}]"),
            Entry::Format(item) | Entry::Action(item) => item.label.clone(),
            Entry::Separator => "---".into(),
        })
        .collect()
}

/// The first entry that does something when picked.
#[cfg(windows)]
fn first_leaf(
    command: &windows::Win32::UI::Shell::IExplorerCommand,
    items: &windows::Win32::UI::Shell::IShellItemArray,
) -> windows::core::Result<Option<windows::Win32::UI::Shell::IExplorerCommand>> {
    use windows::Win32::UI::Shell::{ECF_ISSEPARATOR, ECS_DISABLED};
    let children = unsafe { command.EnumSubCommands()? };
    loop {
        let mut next = [None];
        if unsafe { children.Next(&mut next, None) } != windows::core::HRESULT(0) {
            return Ok(None);
        }
        let child = next[0].take().expect("one command");
        let separator = unsafe { child.GetFlags()? } & ECF_ISSEPARATOR.0 as u32 != 0;
        let heading = unsafe { child.GetState(items, true)? } & ECS_DISABLED.0 as u32 != 0;
        if !separator && !heading {
            return Ok(Some(child));
        }
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Run explorer-probe on Windows");
}
