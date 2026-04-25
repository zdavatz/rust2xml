//! Build script — Windows-only step that embeds `assets/icon.ico`
//! into the resulting `rust2xml-gui.exe` (and any other Windows .exe
//! built from this crate) via `winresource`.  No-op on Linux / macOS.

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=build.rs");

    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        if let Err(e) = res.compile() {
            // Don't fail the whole build over a cosmetic icon — print
            // a warning and carry on so CI on a misconfigured runner
            // still produces a usable binary.
            println!("cargo:warning=winresource icon embed failed: {e}");
        }
    }
}
