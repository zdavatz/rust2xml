//! Regenerate `assets/icon.ico` from `assets/icon.png`.
//!
//! Run after the PNG changes:
//!     cargo run --release --example make_ico
//!
//! The .ico is committed alongside the PNG so build.rs can embed it
//! into the Windows .exe via `winresource` without pulling extra
//! dependencies into every release build.

use std::fs;
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let png_path = root.join("assets/icon.png");
    let ico_path = root.join("assets/icon.ico");

    let png = fs::read(&png_path)?;
    let img = image::load_from_memory(&png)?;
    println!(
        "Source: {} ({} × {})",
        png_path.display(),
        img.width(),
        img.height()
    );

    let mut dir = ico::IconDir::new(ico::ResourceType::Icon);
    for size in [16u32, 32, 48, 64, 128, 256] {
        let resized = img.resize_exact(size, size, image::imageops::FilterType::Lanczos3);
        let rgba = resized.to_rgba8();
        let entry = ico::IconImage::from_rgba_data(size, size, rgba.into_raw());
        dir.add_entry(ico::IconDirEntry::encode(&entry)?);
    }

    let f = fs::File::create(&ico_path)?;
    dir.write(f)?;
    println!("Wrote: {}", ico_path.display());
    Ok(())
}
