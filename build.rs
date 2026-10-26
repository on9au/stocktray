//! Embeds the icon and a version resource (from Cargo.toml) in the exe, so Explorer, the Start menu
//! and Installed apps show them. Regenerate the icon with `cargo run --example make_icon`.

fn main() {
    println!("cargo:rerun-if-changed=assets/stocktray.ico");
    #[cfg(windows)]
    winresource::WindowsResource::new().set_icon("assets/stocktray.ico").compile().expect("embedding resources");
}
