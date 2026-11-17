//! Converts Pikachu's art into Rust. `art/custom/` wins over `art/default/`
//! when it holds a `sheet.toml`.

#[path = "build/convert.rs"]
mod convert;

use std::{env, fs, path::PathBuf, process};

fn main() {
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    println!("cargo:rerun-if-changed=build/convert.rs");
    println!("cargo:rerun-if-changed=art/default");
    println!("cargo:rerun-if-changed=art/custom");
    let custom = root.join("art/custom");
    let dir = if custom.join("sheet.toml").exists() { custom } else { root.join("art/default") };
    let read = |name: &str| {
        fs::read(dir.join(name)).unwrap_or_else(|e| fail(&format!("{}: {e}", dir.join(name).display())))
    };
    let toml = String::from_utf8(read("sheet.toml")).unwrap_or_else(|_| fail("sheet.toml is not UTF-8"));
    let sheet = convert::convert(&toml, &read("sheet.png"))
        .unwrap_or_else(|e| fail(&format!("{}: {e}", dir.display())));
    fs::write(out.join("pikachu_pixels.bin"), &sheet.pixels).unwrap();
    fs::write(out.join("pikachu_sheet.rs"), convert::to_rust(&sheet, "pikachu_pixels.bin")).unwrap();
}

fn fail(message: &str) -> ! {
    eprintln!("pikachu art: {message}");
    process::exit(1);
}
