//! End-to-end: a consumer crate's `build.rs` calls `load()` and links the artifact through the
//! `cargo:rustc-link-*` directives it emits, then the binary calls into it. This is the contract
//! "auto-linking" promises, so it is checked by building and running a real crate.
//! Unix only: the Windows import-library/DLL path is not exercised here.
#![cfg(unix)]

use equilibrium_ffi::{find_compiler, Language};
use std::path::Path;
use std::process::Command;

fn write(path: &Path, content: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

/// Build and run a crate whose `build.rs` loads `source` (with a sibling `header`) and whose
/// `main.rs` prints `add(20, 22)` through the generated bindings.
fn build_and_run(stem: &str, source_ext: &str, source: &str, header: &str) {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        &root.join("Cargo.toml"),
        &format!(
            r#"[package]
name = "link-e2e-{stem}"
version = "0.0.0"
edition = "2021"

[build-dependencies]
equilibrium-ffi = {{ path = {repo:?} }}

[workspace]
"#
        ),
    );
    write(&root.join(format!("native/{stem}.{source_ext}")), source);
    write(&root.join(format!("native/{stem}.h")), header);
    write(
        &root.join("build.rs"),
        &format!(
            r#"fn main() {{
    let lib = equilibrium_ffi::load("native/{stem}.{source_ext}").expect("load");
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("bindings.rs");
    std::fs::write(out, lib.bindings_code().expect("bindings")).unwrap();
}}
"#
        ),
    );
    write(
        &root.join("src/main.rs"),
        &format!(
            r#"include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
fn main() {{
    println!("{{}}", unsafe {{ {stem}_add(20, 22) }});
}}
"#
        ),
    );

    // Share one target dir across runs so the equilibrium-ffi build dependency is reused.
    let target = repo.join("target").join("link-e2e");
    let output = Command::new(env!("CARGO"))
        .args(["run", "--quiet", "-j2"])
        .current_dir(root)
        .env("CARGO_TARGET_DIR", &target)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success() && stdout.trim() == "42",
        "{stem}: load() + auto-link did not produce a working binary\nstdout: {stdout}\nstderr: {stderr}"
    );
}

#[test]
fn nim_static_library_links_through_load() {
    if find_compiler(Language::Nim).is_none() {
        eprintln!("skipping: nim not installed");
        return;
    }
    build_and_run(
        "nmath",
        "nim",
        "proc nmath_add*(a, b: cint): cint {.exportc, cdecl.} = a + b\n",
        "int nmath_add(int a, int b);\n",
    );
}

#[test]
fn rust_library_links_through_load() {
    if find_compiler(Language::Rust).is_none() {
        eprintln!("skipping: rustc not installed");
        return;
    }
    build_and_run(
        "rmath",
        "rs",
        "#[no_mangle]\npub extern \"C\" fn rmath_add(a: i32, b: i32) -> i32 { a + b }\n",
        "int rmath_add(int a, int b);\n",
    );
}
