use serde::Serialize;
use std::collections::HashMap;
use std::ffi::{OsString, c_void};
use std::io::Read;
use std::os::windows::{ffi::OsStringExt, process::CommandExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::LibraryLoader::*;
use windows::Win32::System::Ole::*;
use windows::Win32::System::Threading::CREATE_NO_WINDOW;
use windows::Win32::UI::Shell::*;
use windows::core::*;

use crate::{Entry, Item, MENUS, Submenu, common_menus, parse_menus};

/// One COM class per top-level menu, in `MENUS` order. The installer and
/// the sparse package register each one; the first is the original
/// "Convert with scyconvert" class.
pub const CLSIDS: [GUID; 3] = [
    GUID::from_u128(0xbb1183d6_e6ca_44e1_906c_a0a47845841d),
    GUID::from_u128(0xb77c2bc9_b09c_4659_85cf_d4e34fc6ca15),
    GUID::from_u128(0x7b070ad5_9dfe_4769_b4a3_27cc8b0fb2ca),
];
static OBJECTS: AtomicUsize = AtomicUsize::new(0);
static SERVER_LOCKS: AtomicUsize = AtomicUsize::new(0);
struct ModuleLease;
impl ModuleLease {
    fn new() -> Self {
        OBJECTS.fetch_add(1, Ordering::SeqCst);
        Self
    }
}
impl Drop for ModuleLease {
    fn drop(&mut self) {
        OBJECTS.fetch_sub(1, Ordering::SeqCst);
    }
}
const PROBE_LIMIT: Duration = Duration::from_secs(2);
const CACHE_TTL: Duration = Duration::from_secs(30);

fn error() -> Error {
    Error::from_hresult(E_FAIL)
}
fn text(value: &str) -> Result<PWSTR> {
    let wide = HSTRING::from(value);
    // Shell owns the CoTaskMem string returned by SHStrDupW.
    unsafe { SHStrDupW(&wide) }
}
fn install_dir() -> Result<PathBuf> {
    let mut module = HMODULE::default();
    let mut buffer = vec![0u16; 32768];
    // Resolve this DLL, not Explorer.exe/dllhost.exe and never PATH.
    unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            PCWSTR(DllGetClassObject as *const () as *const u16),
            &mut module,
        )?;
        let len = GetModuleFileNameW(Some(module), &mut buffer) as usize;
        if len == 0 || len >= buffer.len() {
            return Err(error());
        }
        PathBuf::from(OsString::from_wide(&buffer[..len]))
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(error)
    }
}
fn files(items: Ref<IShellItemArray>) -> Result<Vec<PathBuf>> {
    let items = items.ok()?;
    let mut paths = Vec::new();
    unsafe {
        let count = items.GetCount()?;
        if count == 0 || count > 256 {
            return Err(error());
        }
        for index in 0..count {
            let item = items.GetItemAt(index)?;
            let name = item.GetDisplayName(SIGDN_FILESYSPATH)?;
            let path = PathBuf::from(OsString::from_wide(name.as_wide()));
            CoTaskMemFree(Some(name.0.cast()));
            if !path.is_absolute() || !path.is_file() {
                return Err(error());
            }
            paths.push(path);
        }
    }
    Ok(paths)
}

// Probe once per extension, not once per file. Cache expires so installing or
// removing document support updates the menu without restarting Explorer.
type Cache = HashMap<OsString, (Instant, Vec<Submenu>)>;
static MENUS_BY_EXTENSION: OnceLock<Mutex<Cache>> = OnceLock::new();
fn menus_for(path: &Path) -> Vec<Submenu> {
    let Some(extension) = path.extension().map(|e| e.to_ascii_lowercase()) else {
        return Vec::new();
    };
    let cache = MENUS_BY_EXTENSION.get_or_init(Default::default);
    if let Ok(cache) = cache.lock()
        && let Some((when, menus)) = cache.get(&extension)
        && when.elapsed() < CACHE_TTL
    {
        return menus.clone();
    }
    let Some(menus) = probe(path) else {
        return Vec::new();
    };
    if let Ok(mut cache) = cache.lock() {
        if cache.len() >= 256 {
            cache.clear();
        }
        cache.insert(extension, (Instant::now(), menus.clone()));
    }
    menus
}

/// The entries of top-level menu `menu` for a selection.
fn selection_entries(menu: usize, paths: &[PathBuf]) -> Vec<Entry> {
    let per_file: Vec<_> = paths.iter().map(|p| menus_for(p)).collect();
    common_menus(&per_file)
        .into_iter()
        .find(|m| m.id == MENUS[menu].0)
        .map(|m| m.entries)
        .unwrap_or_default()
}

#[derive(Serialize)]
struct ExplorerRequest<'a> {
    files: &'a [PathBuf],
    show_progress: bool,
    /// Say when it's done with a notification, since no window opens.
    notify: bool,
    to: Option<&'a str>,
    action: Option<&'a str>,
    preset: Option<&'a str>,
    source: &'static str,
}

fn request_dir() -> PathBuf {
    std::env::var_os("SCYCONVERT_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("scyconvert"))
}

fn handoff(paths: &[PathBuf], to: Option<&str>, action: Option<&str>) -> Result<()> {
    static REQUEST_ID: AtomicUsize = AtomicUsize::new(0);
    let dir = request_dir();
    std::fs::create_dir_all(&dir).map_err(|_| error())?;
    let id = format!(
        "{}-{}",
        std::process::id(),
        REQUEST_ID.fetch_add(1, Ordering::Relaxed)
    );
    let request = ExplorerRequest {
        files: paths,
        show_progress: false,
        notify: true,
        to,
        action,
        preset: None,
        source: "Cli",
    };
    let bytes = serde_json::to_vec(&request).map_err(|_| error())?;
    let temp = dir.join(format!("request-{id}.tmp"));
    let path = dir.join(format!("request-{id}.json"));
    std::fs::write(&temp, bytes).map_err(|_| error())?;
    std::fs::rename(temp, path).map_err(|_| error())?;
    Ok(())
}

fn app_running() -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenMutexW, SYNCHRONIZATION_SYNCHRONIZE};
    let name: Vec<u16> = std::ffi::OsStr::new("Local\\scyconvert-instance")
        .encode_wide()
        .chain(Some(0))
        .collect();
    // SAFETY: valid NUL-terminated mutex name; the handle is closed below.
    let Ok(handle) =
        (unsafe { OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, false, PCWSTR(name.as_ptr())) })
    else {
        return false;
    };
    // SAFETY: the handle came from OpenMutexW above and is closed once.
    let _ = unsafe { CloseHandle(handle) };
    true
}
fn probe(path: &Path) -> Option<Vec<Submenu>> {
    let mut child = Command::new(install_dir().ok()?.join("scyconvert.exe"))
        .arg("targets")
        .arg(path)
        .arg("--menu")
        .creation_flags(CREATE_NO_WINDOW.0)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.take(16385).read_to_end(&mut bytes).ok()?;
        (bytes.len() <= 16384).then_some(bytes)
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if start.elapsed() < PROBE_LIMIT => {
                std::thread::sleep(Duration::from_millis(10))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let bytes = reader.join().ok()??;
    if !status?.success() {
        return None;
    }
    parse_menus(&String::from_utf8(bytes).ok()?)
}

/// What a command in the menu is.
#[derive(Clone)]
enum Kind {
    /// A top-level menu, by index in `MENUS`, holding everything else.
    Root(usize),
    /// Greyed text over the entries below it.
    Heading(String),
    Format(Item),
    Action(Item),
    Separator,
}

#[implement(IExplorerCommand, IObjectWithSite)]
struct ExplorerCommand {
    _lease: ModuleLease,
    kind: Kind,
    site: Mutex<Option<IUnknown>>,
    selection: Mutex<Vec<PathBuf>>,
}
impl ExplorerCommand {
    fn new(kind: Kind, selection: Vec<PathBuf>) -> Self {
        Self {
            _lease: ModuleLease::new(),
            kind,
            site: Mutex::new(None),
            selection: Mutex::new(selection),
        }
    }
    fn root(menu: usize) -> Self {
        Self::new(Kind::Root(menu), vec![])
    }
}
impl IObjectWithSite_Impl for ExplorerCommand_Impl {
    fn SetSite(&self, site: Ref<IUnknown>) -> Result<()> {
        *self.site.lock().map_err(|_| error())? = site.cloned();
        Ok(())
    }
    fn GetSite(&self, iid: *const GUID, out: *mut *mut c_void) -> Result<()> {
        if iid.is_null() || out.is_null() {
            return Err(Error::from_hresult(E_POINTER));
        }
        unsafe {
            *out = std::ptr::null_mut();
        }
        let site = self.site.lock().map_err(|_| error())?;
        unsafe { site.as_ref().ok_or_else(error)?.query(iid, out).ok() }
    }
}
impl ExplorerCommand_Impl {
    fn selected_paths(&self) -> Result<Vec<PathBuf>> {
        let paths = self.selection.lock().map_err(|_| error())?.clone();
        if !paths.is_empty() {
            return Ok(paths);
        }
        // Some shell hosts enumerate before passing an item array to GetTitle
        // or GetState. Query the site's current view only during construction;
        // Invoke always uses the array supplied by the shell or this snapshot.
        let site = self
            .site
            .lock()
            .map_err(|_| error())?
            .clone()
            .ok_or_else(error)?;
        let services: IServiceProvider = site.cast()?;
        unsafe {
            let browser: IShellBrowser = services.QueryService(&SID_STopLevelBrowser)?;
            let view = browser.QueryActiveShellView()?;
            let items: IShellItemArray = view.GetItemObject(SVGIO_SELECTION)?;
            let paths = files((&items).into())?;
            *self.selection.lock().map_err(|_| error())? = paths.clone();
            Ok(paths)
        }
    }

    /// The commands under this one.
    fn children(&self) -> Result<Vec<Kind>> {
        let Kind::Root(menu) = self.kind else {
            return Ok(Vec::new());
        };
        Ok(selection_entries(menu, &self.selected_paths()?)
            .into_iter()
            .map(|entry| match entry {
                Entry::Heading(text) => Kind::Heading(text),
                Entry::Format(item) => Kind::Format(item),
                Entry::Action(item) => Kind::Action(item),
                Entry::Separator => Kind::Separator,
            })
            .collect())
    }
}
impl IExplorerCommand_Impl for ExplorerCommand_Impl {
    fn GetTitle(&self, items: Ref<IShellItemArray>) -> Result<PWSTR> {
        match &self.kind {
            Kind::Root(menu) => {
                if let Ok(paths) = files(items) {
                    *self.selection.lock().map_err(|_| error())? = paths;
                }
                text(MENUS[*menu].1)
            }
            Kind::Heading(heading) => text(heading),
            Kind::Format(item) | Kind::Action(item) => text(&item.label),
            Kind::Separator => text(""),
        }
    }
    fn GetIcon(&self, _: Ref<IShellItemArray>) -> Result<PWSTR> {
        if !matches!(self.kind, Kind::Root(_)) {
            return Err(Error::from_hresult(E_NOTIMPL));
        }
        text(&format!(
            "{},0",
            install_dir()?.join("scyconvert-app.exe").display()
        ))
    }
    fn GetToolTip(&self, _: Ref<IShellItemArray>) -> Result<PWSTR> {
        if !matches!(self.kind, Kind::Root(_)) {
            return Err(Error::from_hresult(E_NOTIMPL));
        }
        text("Runs on this computer")
    }
    fn GetCanonicalName(&self) -> Result<GUID> {
        let key = match &self.kind {
            Kind::Root(menu) => return Ok(CLSIDS[*menu]),
            Kind::Heading(h) => format!("heading:{h}"),
            Kind::Format(i) => format!("format:{}", i.id),
            Kind::Action(i) => format!("action:{}", i.id),
            Kind::Separator => "separator".into(),
        };
        // Stable distinct canonical names for each entry, independent of menu order.
        let mut id = CLSIDS[0];
        id.data1 ^= key
            .bytes()
            .fold(2166136261u32, |h, b| (h ^ b as u32).wrapping_mul(16777619));
        Ok(id)
    }
    // Answers even when `slow` is false: the classic menu reads E_PENDING as
    // hidden, and the probe is ~50 ms once per extension (then cached).
    fn GetState(&self, items: Ref<IShellItemArray>, _slow: BOOL) -> Result<u32> {
        let menu = match self.kind {
            Kind::Root(menu) => menu,
            Kind::Heading(_) => return Ok(ECS_DISABLED.0 as u32),
            _ => return Ok(ECS_ENABLED.0 as u32),
        };
        let Ok(paths) = files(items) else {
            return Ok(ECS_HIDDEN.0 as u32);
        };
        let entries = selection_entries(menu, &paths);
        *self.selection.lock().map_err(|_| error())? = paths;
        Ok(if entries.is_empty() {
            ECS_HIDDEN
        } else {
            ECS_ENABLED
        }
        .0 as u32)
    }
    fn Invoke(&self, items: Ref<IShellItemArray>, _: Ref<IBindCtx>) -> Result<()> {
        let (to, action) = match &self.kind {
            Kind::Format(item) => (Some(item.id.as_str()), None),
            Kind::Action(item) => (None, Some(item.id.as_str())),
            _ => return Err(error()),
        };
        let paths = if items.is_some() {
            files(items)?
        } else {
            self.selection.lock().map_err(|_| error())?.clone()
        };
        if paths.is_empty() {
            return Err(error());
        }
        handoff(&paths, to, action)?;
        if !app_running() {
            // Into the tray: the conversion says when it's done.
            Command::new(install_dir()?.join("scyconvert-app.exe"))
                .arg("--minimized")
                .creation_flags(CREATE_NO_WINDOW.0)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|_| error())?;
        }
        Ok(())
    }
    fn GetFlags(&self) -> Result<u32> {
        Ok(match self.kind {
            Kind::Root(_) => ECF_HASSUBCOMMANDS.0 as u32,
            Kind::Separator => ECF_ISSEPARATOR.0 as u32,
            _ => 0,
        })
    }
    fn EnumSubCommands(&self) -> Result<IEnumExplorerCommand> {
        let paths = self.selected_paths()?;
        let commands = self
            .children()?
            .into_iter()
            .map(|kind| ExplorerCommand::new(kind, paths.clone()).into())
            .collect();
        Ok(Enumerator {
            _lease: ModuleLease::new(),
            commands,
            cursor: Mutex::new(0),
        }
        .into())
    }
}

#[implement(IEnumExplorerCommand)]
struct Enumerator {
    _lease: ModuleLease,
    commands: Vec<IExplorerCommand>,
    cursor: Mutex<usize>,
}
impl IEnumExplorerCommand_Impl for Enumerator_Impl {
    fn Next(
        &self,
        count: u32,
        commands: *mut Option<IExplorerCommand>,
        fetched: *mut u32,
    ) -> HRESULT {
        if commands.is_null() || (fetched.is_null() && count != 1) {
            return E_POINTER;
        }
        let Ok(mut cursor) = self.cursor.lock() else {
            return E_FAIL;
        };
        let take = (count as usize).min(self.commands.len() - *cursor);
        // COM caller provides count output slots. Each receives a new reference.
        unsafe {
            if !fetched.is_null() {
                *fetched = take as u32;
            }
            for i in 0..count as usize {
                commands.add(i).write(if i < take {
                    Some(self.commands[*cursor + i].clone())
                } else {
                    None
                });
            }
        }
        *cursor += take;
        if take == count as usize {
            S_OK
        } else {
            S_FALSE
        }
    }
    fn Skip(&self, count: u32) -> Result<()> {
        let mut cursor = self.cursor.lock().map_err(|_| error())?;
        let remaining = self.commands.len() - *cursor;
        *cursor += (count as usize).min(remaining);
        if count as usize > remaining {
            // The projection uses Result even though COM permits S_FALSE here.
            Err(Error::from_hresult(S_FALSE))
        } else {
            Ok(())
        }
    }
    fn Reset(&self) -> Result<()> {
        *self.cursor.lock().map_err(|_| error())? = 0;
        Ok(())
    }
    fn Clone(&self) -> Result<IEnumExplorerCommand> {
        Ok(Enumerator {
            _lease: ModuleLease::new(),
            commands: self.commands.clone(),
            cursor: Mutex::new(*self.cursor.lock().map_err(|_| error())?),
        }
        .into())
    }
}

#[implement(IClassFactory)]
struct Factory {
    _lease: ModuleLease,
    /// Which top-level menu this class makes, by index in `MENUS`.
    menu: usize,
}
impl IClassFactory_Impl for Factory_Impl {
    fn CreateInstance(
        &self,
        outer: Ref<IUnknown>,
        iid: *const GUID,
        out: *mut *mut c_void,
    ) -> Result<()> {
        if out.is_null() || iid.is_null() {
            return Err(Error::from_hresult(E_POINTER));
        }
        unsafe {
            *out = std::ptr::null_mut();
        }
        if outer.is_some() {
            return Err(Error::from_hresult(CLASS_E_NOAGGREGATION));
        }
        let command: IExplorerCommand = ExplorerCommand::root(self.menu).into();
        unsafe { command.query(iid, out).ok() }
    }
    fn LockServer(&self, lock: BOOL) -> Result<()> {
        if lock.as_bool() {
            SERVER_LOCKS.fetch_add(1, Ordering::SeqCst);
        } else {
            let _ = SERVER_LOCKS.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |count| {
                count.checked_sub(1)
            });
        }
        Ok(())
    }
}

/// COM export; Windows supplies valid GUIDs and an output pointer.
#[unsafe(no_mangle)]
unsafe extern "system" fn DllGetClassObject(
    class: *const GUID,
    iid: *const GUID,
    out: *mut *mut c_void,
) -> HRESULT {
    if out.is_null() || class.is_null() || iid.is_null() {
        return E_POINTER;
    }
    unsafe {
        *out = std::ptr::null_mut();
        let Some(menu) = CLSIDS.iter().position(|c| *c == *class) else {
            return CLASS_E_CLASSNOTAVAILABLE;
        };
        let factory: IClassFactory = Factory {
            _lease: ModuleLease::new(),
            menu,
        }
        .into();
        factory.query(iid, out)
    }
}
// COM may release the DLL only after every object and server lock is gone.
// Probe workers join before the owning command returns.
#[unsafe(no_mangle)]
extern "system" fn DllCanUnloadNow() -> HRESULT {
    if OBJECTS.load(Ordering::SeqCst) == 0 && SERVER_LOCKS.load(Ordering::SeqCst) == 0 {
        S_OK
    } else {
        S_FALSE
    }
}
