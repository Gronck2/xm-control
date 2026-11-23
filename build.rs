//! Embeds the icon and `app.manifest` (comctl32 v6 look, per-monitor v2 DPI) listed in `app.rc` into the Windows exe.
//! MSVC: `embed-resource` (rc.exe). GNU: `windres` (mingw) compiles the same script into an object that is linked in.
use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=app.manifest");
    println!("cargo:rerun-if-changed=app.rc");
    println!("cargo:rerun-if-changed=assets");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    // absolute paths, so the resource compiler does not depend on its working directory
    let dir = env::var("CARGO_MANIFEST_DIR").unwrap().replace(std::path::MAIN_SEPARATOR, "/");
    let rc =
        fs::read_to_string("app.rc").unwrap().replace("\"assets/", &format!("\"{dir}/assets/")).replace("\"app.manifest", &format!("\"{dir}/app.manifest"));
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let rc_path = out_dir.join("app.rc");
    fs::write(&rc_path, rc).unwrap();
    if env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        embed_resource::compile(&rc_path, embed_resource::NONE).manifest_required().unwrap();
    } else {
        // embed-resource's library would add a second manifest next to gcc's default-manifest.o ("multiple non-default manifests").
        // A file of that name in a -B dir replaces gcc's, so ours is the only one.
        let obj = out_dir.join("default-manifest.o");
        let status = Command::new("windres").arg(&rc_path).args(["-O", "coff", "-o"]).arg(&obj).status().expect("windres (mingw) not found");
        assert!(status.success(), "windres failed");
        println!("cargo:rustc-link-arg-bins=-B{}", out_dir.display());
    }
}
