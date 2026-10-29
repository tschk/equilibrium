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

/// Build and run a crate whose `build.rs` loads `source` and whose `main.rs` prints
/// `add(20, 22)` through the generated bindings. With `header: None` there is no hand-written
/// header, so the bindings must come from one equilibrium synthesizes.
fn build_and_run(stem: &str, source_ext: &str, source: &str, header: Option<&str>) {
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
    if let Some(header) = header {
        write(&root.join(format!("native/{stem}.h")), header);
    }
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
        Some("int nmath_add(int a, int b);\n"),
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
        Some("int rmath_add(int a, int b);\n"),
    );
}

#[test]
fn c_source_without_a_header_links_through_load() {
    if find_compiler(Language::C).is_none() {
        eprintln!("skipping: no C compiler");
        return;
    }
    build_and_run(
        "cmath",
        "c",
        "static int twice(int v) { return v * 2; }\nint cmath_add(int a, int b) { return twice(a + b) / 2; }\n",
        None,
    );
}

#[test]
fn cpp_source_without_a_header_links_through_load() {
    if find_compiler(Language::Cpp).is_none() {
        eprintln!("skipping: no C++ compiler");
        return;
    }
    build_and_run(
        "cppmath",
        "cpp",
        "struct Acc { int v; int add(int x) { return v + x; } };\n\
         extern \"C\" int cppmath_add(int a, int b) { Acc acc{a}; return acc.add(b); }\n",
        None,
    );
}

#[test]
fn d_source_without_a_header_links_through_load() {
    let Some(info) = find_compiler(Language::D) else {
        eprintln!("skipping: no D compiler");
        return;
    };
    if info.compiler.as_deref() == Some("gdc") {
        eprintln!("skipping: gdc has no -HC header output");
        return;
    }
    build_and_run(
        "dmath",
        "d",
        "module dmath;\nint helper(int a) { return a; }\n\
         extern(C) int dmath_add(int a, int b) { return helper(a) + b; }\n",
        None,
    );
}

#[test]
fn nim_source_without_a_header_links_through_load() {
    if find_compiler(Language::Nim).is_none() {
        eprintln!("skipping: nim not installed");
        return;
    }
    build_and_run(
        "nimgen",
        "nim",
        "proc twice(v: cint): cint = v * 2\n\
         proc nimgen_add*(a, b: cint): cint {.exportc, cdecl.} = twice(a + b) div 2\n",
        None,
    );
}

#[test]
fn rust_source_without_a_header_links_through_load() {
    if find_compiler(Language::Rust).is_none() {
        eprintln!("skipping: rustc not installed");
        return;
    }
    build_and_run(
        "rgen",
        "rs",
        "fn twice(v: i32) -> i32 { v * 2 }\n\
         #[no_mangle]\npub extern \"C\" fn rgen_add(a: i32, b: i32) -> i32 { twice(a + b) / 2 }\n",
        None,
    );
}
