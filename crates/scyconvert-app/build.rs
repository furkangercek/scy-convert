//! Embeds the app icon on Windows. GPUI loads icon resource 1 from the
//! executable for the window, taskbar and Alt-Tab.

fn main() {
    let icon = "../../packaging/icon/icon.ico";
    println!("cargo:rerun-if-changed={icon}");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon_with_id(icon, "1")
            .compile()
            .expect("embedding the Windows icon");
    }
}
