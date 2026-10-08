//! Windows and the pieces they share. The main window lists activity; Quick
//! convert opens for files sent without a target; Settings is its own
//! window; the menu bar popover belongs to the tray.

mod main_window;
mod pack;
mod popover;
mod quick;
mod settings_window;
#[cfg(test)]
mod tests;
pub mod theme;

use std::path::{Path, PathBuf};

use convt_core::Preset;
use gpui_kit::*;

pub use main_window::MainView;
pub use popover::PopoverView;
pub use quick::QuickView;
pub use settings_window::{SettingsTab, SettingsView};

use crate::model;
use crate::request::Request;
use theme::Palette;

/// The icons the windows draw: checkmarks, dropdown chevrons and the drop
/// bar's arrow. Register it with `Application::with_assets`; without it
/// every icon draws empty.
pub fn assets() -> gpui_kit::assets::Assets {
    gpui_kit::assets::Assets
}

/// An open window of one kind, if any.
struct Open<V: 'static>(AnyWindowHandle, WeakEntity<V>);

impl<V: 'static> Global for Open<V> {}

impl<V: 'static> Open<V> {
    /// The window and its view, if it is still open.
    fn get(cx: &App) -> Option<(AnyWindowHandle, Entity<V>)> {
        let open = cx.try_global::<Self>()?;
        Some((open.0, open.1.upgrade()?))
    }
}

/// Brings an open window of kind `V` forward, or opens one with `build`.
fn show<V: Render>(
    size: Size<Pixels>,
    title: &str,
    cx: &mut App,
    build: impl FnOnce(&mut Window, &mut App) -> Entity<V>,
) -> Option<(AnyWindowHandle, Entity<V>)> {
    if let Some((handle, view)) = Open::<V>::get(cx)
        && handle
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
    {
        return Some((handle, view));
    }
    let opened = gpui_kit::open_window(window_options(size, title, cx), cx, build);
    cx.activate(true);
    match opened {
        Ok((handle, view)) => {
            cx.set_global(Open(handle, view.downgrade()));
            Some((handle, view))
        }
        Err(e) => {
            tracing::error!(error = %e, title, "could not open a window");
            None
        }
    }
}

/// Sends a request where it belongs:
///
/// - Files with a target, from the command line or the Finder menu, convert
///   in place with no window. Links never do: any web page can open one.
/// - Other files open Quick convert, and no files open the main window.
pub fn route(request: Request, cx: &mut App) {
    let app = model::shared(cx);
    if request.files.is_empty() {
        show_main(cx);
    } else if request.auto_start() {
        // Silent conversions never download: a document that needs the pack
        // fails here and opens Quick convert, which offers it.
        app.update(cx, |s, cx| s.refresh_pack(cx));
        if request.show_progress {
            open_main(cx);
        }
        let silent = app.update(cx, |s, cx| s.convert_silently(&request, cx));
        if let Err(e) = silent {
            tracing::info!(reason = %e, "opening Quick convert instead of converting in place");
            open_quick(request, cx);
        }
    } else {
        open_quick(request, cx);
    }
}

/// Opens the main window.
pub fn show_main(cx: &mut App) {
    open_main(cx);
}

fn open_main(cx: &mut App) -> Option<(AnyWindowHandle, Entity<MainView>)> {
    let app = model::shared(cx);
    let opened = show(size(px(1040.), px(640.)), "convt", cx, |window, cx| {
        cx.new(|cx| MainView::new(app, window, cx))
    });
    if let Some((handle, view)) = &opened {
        let _ = handle.update(cx, |_, window, _| window.activate_window());
        view.update(cx, |view, cx| {
            view.set_page(main_window::Page::Activity, cx)
        });
    }
    opened
}

/// Opens Settings on `tab`.
pub fn show_settings(tab: SettingsTab, cx: &mut App) {
    let app = model::shared(cx);
    app.update(cx, |s, cx| s.refresh_pack(cx));
    if let Some((handle, view)) = show(size(px(620.), px(600.)), "Settings", cx, |window, cx| {
        cx.new(|cx| SettingsView::new(app, window, cx))
    }) {
        let _ = handle.update(cx, |_, _, cx| view.update(cx, |v, cx| v.set_tab(tab, cx)));
    }
}

pub fn open_quick(request: Request, cx: &mut App) {
    let app = model::shared(cx);
    // Status is read offline; it may have changed through the CLI.
    app.update(cx, |s, cx| s.refresh_pack(cx));
    let options = window_options(size(px(600.), px(560.)), "Convert", cx);
    match gpui_kit::open_window(options, cx, |window, cx| {
        cx.new(|cx| QuickView::new(app, request, window, cx))
    }) {
        // Each request gets its own window; the global tracks the newest.
        Ok((handle, view)) => cx.set_global(Open(handle, view.downgrade())),
        Err(e) => tracing::error!(error = %e, "could not open Quick convert"),
    }
    cx.activate(true);
}

/// Opens the menu bar popover. Only the tray icon should call this, when it
/// is clicked or a file is dropped on it. GPUI has no tray yet, so nothing
/// calls it outside tests; see `tray.rs`.
#[cfg_attr(not(test), allow(dead_code))]
pub fn open_popover(cx: &mut App) -> Option<(AnyWindowHandle, Entity<PopoverView>)> {
    let app = model::shared(cx);
    show(size(px(340.), px(520.)), "convt", cx, |window, cx| {
        cx.new(|cx| PopoverView::new(app, window, cx))
    })
}

fn window_options(size: Size<Pixels>, title: &str, cx: &App) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size, cx))),
        titlebar: Some(TitlebarOptions {
            title: Some(SharedString::from(title.to_string())),
            appears_transparent: theme::transparent_titlebar(),
            traffic_light_position: theme::transparent_titlebar().then(|| point(px(16.), px(16.))),
        }),
        app_id: Some("convt".into()),
        ..Default::default()
    }
}

/// "to webp, quality 80, max 2048 px".
fn describe(preset: &Preset) -> String {
    let o = &preset.options;
    let mut parts = Vec::new();
    match &preset.to {
        Some(to) => parts.push(format!("to {to}")),
        None => parts.push("any format".into()),
    }
    if let Some(q) = o.quality {
        parts.push(format!("quality {q}"));
    }
    if let Some(m) = o.max_size {
        parts.push(format!("max {m} px"));
    }
    if let Some(h) = o.video_height {
        parts.push(format!("{h}p video"));
    }
    if let Some(b) = o.audio_bitrate {
        parts.push(format!("{b} kbit/s audio"));
    }
    if o.pages.is_some() {
        parts.push("some pages".into());
    }
    if let Some(d) = o.dpi {
        parts.push(format!("{d} dpi"));
    }
    if let Some(codec) = o.video_codec {
        parts.push(codec.name().into());
    }
    if o.strip_audio {
        parts.push("no audio".into());
    }
    if let Some(background) = o.background {
        parts.push(format!("{} background", background.name().to_lowercase()));
    }
    parts.join(", ")
}

/// A path with the home folder shortened to `~`. Windows has no `~`, so
/// there the path stays whole, even when a Unix shell set `HOME`.
pub(super) fn tilde(path: &Path) -> String {
    if !cfg!(windows)
        && let Some(home) = std::env::var_os("HOME").map(PathBuf::from)
        && let Ok(rest) = path.strip_prefix(&home)
    {
        return if rest.as_os_str().is_empty() {
            "~".into()
        } else {
            format!("~/{}", rest.display())
        };
    }
    path.display().to_string()
}

/// An error line that tests can read by the id `error`.
fn error_text(message: impl Into<SharedString>, p: &Palette) -> impl IntoElement {
    let message = message.into();
    div()
        .id("error")
        .test_support()
        .aria_label(message.clone())
        .font_family(theme::SANS)
        .text_size(px(12.))
        .line_height(px(16.))
        .text_color(p.error)
        .child(message)
}

/// "1.9 MB", "214 KB".
pub(crate) fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if bytes < 1000 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / 1000.;
    let mut unit = 0;
    while value >= 1000. && unit < UNITS.len() - 1 {
        value /= 1000.;
        unit += 1;
    }
    if value < 10. && unit > 0 {
        format!("{value:.1} {}", UNITS[unit])
    } else {
        format!("{value:.0} {}", UNITS[unit])
    }
}

fn file_size(path: &Path) -> Option<u64> {
    std::fs::metadata(path).ok().map(|m| m.len())
}

/// "1 min left", "under a minute left".
fn time_left(left: std::time::Duration) -> String {
    match left.as_secs() {
        0..60 => "under a minute left".into(),
        s => format!("{} min left", s.div_ceil(60)),
    }
}

/// Opens a folder in the file manager, creating it first.
fn open_folder(dir: &Path, cx: &mut App) {
    let _ = std::fs::create_dir_all(dir);
    cx.open_with_system(dir);
}

#[cfg(test)]
mod unit {
    use super::{human_size, time_left};

    #[test]
    fn sizes_and_times() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(1_900_000), "1.9 MB");
        assert_eq!(human_size(214_000), "214 KB");
        assert_eq!(human_size(38_400_000), "38 MB");
        assert_eq!(
            time_left(std::time::Duration::from_secs(20)),
            "under a minute left"
        );
        assert_eq!(time_left(std::time::Duration::from_secs(61)), "2 min left");
    }
}
