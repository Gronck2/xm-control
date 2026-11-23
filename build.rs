//! Embeds `app.manifest` (comctl32 v6 look, per-monitor v2 DPI) without a build dependency:
//! MSVC's linker embeds the manifest itself, for the GNU target `windres` (mingw) compiles `app.rc` into an object that is linked in.
use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=app.manifest");
    println!("cargo:rerun-if-changed=app.rc");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    if env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg-bins=/MANIFESTINPUT:{}", dir.join("app.manifest").display());
    } else {
        let obj = PathBuf::from(env::var("OUT_DIR").unwrap()).join("default-manifest.o");
        let status = Command::new("windres").current_dir(&dir).args(["app.rc", "-O", "coff", "-o"]).arg(&obj).status().expect("windres (mingw) not found");
        assert!(status.success(), "windres failed");
        // gcc links its own default-manifest.o (a second manifest); a file of that name in a -B dir replaces it, so ours is the only one.
        println!("cargo:rustc-link-arg-bins=-B{}", obj.parent().unwrap().display());
    }
}
