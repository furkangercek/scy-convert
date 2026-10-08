//! Embeds the app icon in the menu DLL as resource 101. The Windows 11 menu
//! draws a command's icon only from a DLL resource ("menu.dll,-101"); an
//! .exe index or a loose .ico left it blank.

fn main() {
    let icon = "../../packaging/icon/icon.ico";
    println!("cargo:rerun-if-changed={icon}");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon_with_id(icon, "101")
            .compile()
            .expect("embedding the menu icon");
    }
}
