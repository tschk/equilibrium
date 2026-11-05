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

/// Whether `language`'s compiler is installed. A missing compiler skips the test, unless CI lists
/// the language in `EQ_REQUIRE_COMPILERS` (comma-separated, e.g. `zig,nim,d`): there a skip would
/// report `ok` for a test that never ran, so it fails instead.
fn available(language: Language) -> bool {
    if find_compiler(language).is_some() {
        return true;
    }
    let name = language.cli_name();
    let required = std::env::var("EQ_REQUIRE_COMPILERS").unwrap_or_default();
    assert!(
        !required.split(',').any(|item| item.trim() == name),
        "{name} is listed in EQ_REQUIRE_COMPILERS but its compiler was not found"
    );
    eprintln!("skipping: {name} compiler not installed");
    false
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
    if !available(Language::Nim) {
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
    if !available(Language::Rust) {
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
    if !available(Language::C) {
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
    if !available(Language::Cpp) {
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
fn synthesized_headers_follow_compiler_branches_and_includes() {
    use equilibrium_ffi::compile_to_c_with_extra;

    for (language, extension, linkage) in [
        (Language::C, "c", ""),
        (Language::Cpp, "cpp", "extern \"C\" "),
    ] {
        if !available(language) {
            continue;
        }
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join(format!("branch.{extension}"));
        write(
            &dir.path().join("included.inc"),
            &format!("{linkage}int included(int x) {{ return x; }}\n"),
        );
        write(&source, &format!("#include \"included.inc\"\n#if PICK_INT\n{linkage}int choose(int x) {{ return x; }}\n#else\n{linkage}double choose(double x) {{ return x; }}\n#endif\n"));
        let out = dir.path().join("out");
        for (flag, expected, absent) in [
            (
                "-DPICK_INT=0",
                "double choose(double x);",
                "int choose(int x);",
            ),
            (
                "-DPICK_INT=1",
                "int choose(int x);",
                "double choose(double x);",
            ),
        ] {
            let result = compile_to_c_with_extra(&source, &out, &[flag.to_string()], &[]).unwrap();
            let header = std::fs::read_to_string(result.header_path.unwrap()).unwrap();
            assert!(header.contains(expected), "{header}");
            assert!(!header.contains(absent), "{header}");
            assert!(header.contains("int included(int x);"), "{header}");
            assert!(!result.warnings.is_empty());
        }
    }
}

#[test]
fn rebuilds_do_not_reuse_or_overwrite_headers() {
    use equilibrium_ffi::compile_to_c;
    if !available(Language::C) {
        return;
    }
    for same_dir in [true, false] {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("sample.c");
        let out = if same_dir {
            dir.path().to_path_buf()
        } else {
            dir.path().join("out")
        };
        write(&source, "int sample(int x) { return x; }\n");
        let first = compile_to_c(&source, &out).unwrap();
        let first_path = first.header_path.unwrap();
        assert!(std::fs::read_to_string(&first_path)
            .unwrap()
            .contains("int sample(int x);"));
        if !same_dir {
            write(&out.join("sample.h"), "/* caller-owned output header */\n");
        }
        write(&source, "double sample(double x) { return x; }\n");
        let second = compile_to_c(&source, &out).unwrap();
        let second_path = second.header_path.unwrap();
        assert_eq!(first_path, second_path);
        let header = std::fs::read_to_string(second_path).unwrap();
        assert!(header.contains("double sample(double x);"), "{header}");
        assert!(!header.contains("int sample(int x);"), "{header}");
        if !same_dir {
            assert_eq!(
                std::fs::read_to_string(out.join("sample.h")).unwrap(),
                "/* caller-owned output header */\n"
            );
        }
        let sibling = source.with_extension("h");
        write(&sibling, "double sample(double x); /* handwritten */\n");
        let third = compile_to_c(&source, &out).unwrap();
        assert_eq!(third.header_path.unwrap(), sibling.canonicalize().unwrap());
        assert_eq!(
            std::fs::read_to_string(sibling).unwrap(),
            "double sample(double x); /* handwritten */\n"
        );
    }
}

#[test]
fn rust_rebuild_refreshes_generated_header() {
    if !available(Language::Rust) {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("sample.rs");
    let out = dir.path().join("out");
    write(
        &source,
        "#[no_mangle]\npub extern \"C\" fn sample(x: i32) -> i32 { x }\n",
    );
    let first = equilibrium_ffi::compile_to_c(&source, &out).unwrap();
    assert!(std::fs::read_to_string(first.header_path.unwrap())
        .unwrap()
        .contains("int32_t sample(int32_t x);"));
    write(
        &source,
        "#[no_mangle]\npub extern \"C\" fn sample(x: f64) -> f64 { x }\n",
    );
    let second = equilibrium_ffi::compile_to_c(&source, &out).unwrap();
    let header = std::fs::read_to_string(second.header_path.unwrap()).unwrap();
    assert!(header.contains("double sample(double x);"), "{header}");
    assert!(!header.contains("int32_t sample(int32_t x);"), "{header}");
}

#[test]
fn managed_header_refuses_modified_files_and_locked_builds() {
    if !available(Language::C) {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("sample.c");
    let out = dir.path().join("out");
    write(&source, "int sample(int x) { return x; }\n");
    let result = equilibrium_ffi::compile_to_c(&source, &out).unwrap();
    let header = result.header_path.unwrap();
    write(&header, "caller changed this header\n");
    assert!(equilibrium_ffi::compile_to_c(&source, &out).is_err());
    assert_eq!(
        std::fs::read_to_string(&header).unwrap(),
        "caller changed this header\n"
    );
    let lock = header.parent().unwrap().join("lock");
    write(&lock, "caller lock\n");
    assert!(equilibrium_ffi::compile_to_c(&source, &out).is_err());
    assert_eq!(std::fs::read_to_string(lock).unwrap(), "caller lock\n");
}

#[test]
fn abandoned_stages_do_not_block_rebuild_or_get_deleted() {
    if !available(Language::C) {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("sample.c");
    let out = dir.path().join("out");
    write(&source, "int sample(int x) { return x; }\n");
    let first = equilibrium_ffi::compile_to_c(&source, &out).unwrap();
    let header = first.header_path.unwrap();
    let managed = header.parent().unwrap();
    let legacy = managed.join("stage").join("sample.h");
    let abandoned = managed.join("stage-abandoned").join("sample.h");
    write(&legacy, "legacy caller data\n");
    write(&abandoned, "abandoned caller data\n");
    write(&source, "double sample(double x) { return x; }\n");
    let rebuilt = equilibrium_ffi::compile_to_c(&source, &out).unwrap();
    assert_eq!(rebuilt.header_path.as_deref(), Some(header.as_path()));
    assert!(std::fs::read_to_string(&header)
        .unwrap()
        .contains("double sample(double x);"));
    assert_eq!(
        std::fs::read_to_string(legacy).unwrap(),
        "legacy caller data\n"
    );
    assert_eq!(
        std::fs::read_to_string(abandoned).unwrap(),
        "abandoned caller data\n"
    );
    let stages: Vec<_> = std::fs::read_dir(managed)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .filter(|name| name.to_string_lossy().starts_with("stage-"))
        .collect();
    assert_eq!(stages.len(), 1, "normal builds must clean their own stage");
}

#[test]
fn abandoned_initialization_does_not_block_or_delete_caller_files() {
    if !available(Language::C) {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("sample.c");
    let out = dir.path().join("out");
    write(&source, "int sample(int x) { return x; }\n");
    let incomplete = out.join(".equilibrium-init-abandoned").join("owner");
    write(&incomplete, "partial initialization\n");
    let result = equilibrium_ffi::compile_to_c(&source, &out).unwrap();
    assert!(result.header_path.unwrap().is_file());
    assert_eq!(
        std::fs::read_to_string(incomplete).unwrap(),
        "partial initialization\n"
    );

    let other = dir.path().join("other.c");
    write(&other, "int other(int x) { return x; }\n");
    let caller = out.join(".equilibrium-header-other");
    std::fs::create_dir(&caller).unwrap();
    assert!(equilibrium_ffi::compile_to_c(&other, &out).is_err());
    assert!(caller.is_dir());
    assert_eq!(std::fs::read_dir(caller).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn concurrent_builds_leave_complete_managed_headers() {
    if !available(Language::C) {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("sample.c");
    let out = dir.path().join("out");
    write(&source, "int sample(int x) { return x; }\n");
    let path = equilibrium_ffi::compile_to_c(&source, &out)
        .unwrap()
        .header_path
        .unwrap();
    let original = std::fs::read(&path).unwrap();
    std::thread::scope(|scope| {
        let reader = scope.spawn(|| {
            for _ in 0..10000 {
                assert_eq!(std::fs::read(&path).unwrap(), original);
            }
        });
        let builds: Vec<_> = (0..4)
            .map(|_| scope.spawn(|| equilibrium_ffi::compile_to_c(&source, &out)))
            .collect();
        let results: Vec<_> = builds
            .into_iter()
            .map(|build| build.join().unwrap())
            .collect();
        assert!(results.iter().any(Result::is_ok));
        reader.join().unwrap();
    });
    assert_eq!(std::fs::read(path).unwrap(), original);
}

#[cfg(unix)]
#[test]
fn managed_header_refuses_symlink_collision() {
    if !available(Language::C) {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("sample.c");
    let out = dir.path().join("out");
    let caller = dir.path().join("caller.h");
    write(&source, "int sample(int x) { return x; }\n");
    write(&caller, "caller contents\n");
    let header = equilibrium_ffi::compile_to_c(&source, &out)
        .unwrap()
        .header_path
        .unwrap();
    std::fs::remove_file(&header).unwrap();
    std::os::unix::fs::symlink(&caller, &header).unwrap();
    assert!(equilibrium_ffi::compile_to_c(&source, &out).is_err());
    assert_eq!(
        std::fs::read_to_string(caller).unwrap(),
        "caller contents\n"
    );
}

#[test]
fn d_rebuild_and_handwritten_sibling_header() {
    if !available(Language::D) {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("sample.d");
    let out = dir.path().join("out");
    write(&source, "extern(C) int sample(int x) { return x; }\n");
    let first = equilibrium_ffi::compile_to_c(&source, &out).unwrap();
    let header = std::fs::read_to_string(first.header_path.unwrap()).unwrap();
    assert!(header.contains("int32_t sample(int32_t x);"), "{header}");
    write(&source, "extern(C) double sample(double x) { return x; }\n");
    let second = equilibrium_ffi::compile_to_c(&source, &out).unwrap();
    let header = std::fs::read_to_string(second.header_path.unwrap()).unwrap();
    assert!(header.contains("double sample(double x);"), "{header}");
    let sibling = source.with_extension("h");
    write(&sibling, "double sample(double x); /* handwritten */\n");
    let third = equilibrium_ffi::compile_to_c(&source, &out).unwrap();
    assert_eq!(third.header_path.unwrap(), sibling.canonicalize().unwrap());
    assert_eq!(
        std::fs::read_to_string(&sibling).unwrap(),
        "double sample(double x); /* handwritten */\n"
    );
}

#[test]
fn synthesized_header_with_standard_headers_compiles() {
    use equilibrium_ffi::{compile_to_c, generate_bindings, BindingOptions};
    if !available(Language::C) {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("sample.c");
    write(&dir.path().join("nested.inc"), "static inline uint32_t helper(uint32_t n) { return n; }\nuint32_t included(uint32_t n) { return helper(n); }\n");
    write(&dir.path().join("outer.inc"), "#include \"nested.inc\"\n");
    write(&source, "#include <stdint.h>\n#include <stdio.h>\n#include \"outer.inc\"\nuint32_t sample(uint32_t n) { return included(n) + 1; }\n");
    let result = compile_to_c(&source, &dir.path().join("out")).unwrap();
    let header = result.header_path.unwrap();
    let contents = std::fs::read_to_string(&header).unwrap();
    assert!(
        !contents.contains("printf("),
        "unexpected system symbol in {contents}"
    );
    assert!(
        !contents.contains("helper("),
        "static inline exposed in {contents}"
    );
    assert!(
        !contents.contains("__builtin_"),
        "builtin exposed in {contents}"
    );
    assert!(
        contents.contains("uint32_t included(uint32_t n);"),
        "{contents}"
    );
    assert!(
        contents.contains("uint32_t sample(uint32_t n);"),
        "{contents}"
    );
    let cc = find_compiler(Language::C).unwrap().compiler_path.unwrap();
    let output = Command::new(cc)
        .args(["-x", "c", "-fsyntax-only"])
        .arg(&header)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "header failed: {}\n{contents}",
        String::from_utf8_lossy(&output.stderr)
    );
    let bindings = generate_bindings(&header, &BindingOptions::default()).unwrap();
    assert!(bindings.code.contains("fn sample("), "{}", bindings.code);
    assert!(bindings.code.contains("fn included("), "{}", bindings.code);
    assert!(!bindings.code.contains("fn helper("), "{}", bindings.code);
    let rust = dir.path().join("bindings.rs");
    write(&rust, &bindings.code);
    let output = Command::new("rustc")
        .args(["--crate-type", "lib"])
        .arg(&rust)
        .arg("-o")
        .arg(dir.path().join("bindings.rlib"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "bindings failed: {}\n{}",
        String::from_utf8_lossy(&output.stderr),
        bindings.code
    );
}

#[test]
fn system_typedef_in_generated_header_and_bindings_compiles() {
    if !available(Language::C) {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("sample.c");
    write(
        &source,
        "#if 0\n#include <absent_inactive.h>\n#endif\n#include <sys/types.h>\nssize_t sample(ssize_t n) { return n; }\n",
    );
    let result = equilibrium_ffi::compile_to_c(&source, &dir.path().join("out")).unwrap();
    let header = result.header_path.unwrap();
    let contents = std::fs::read_to_string(&header).unwrap();
    assert!(contents.contains("#include <sys/types.h>"), "{contents}");
    assert!(!contents.contains("absent_inactive"), "{contents}");
    assert!(
        contents.contains("ssize_t sample(ssize_t n);"),
        "{contents}"
    );
    let compiler = find_compiler(Language::C).unwrap().compiler_path.unwrap();
    let output = Command::new(compiler)
        .args(["-x", "c", "-fsyntax-only"])
        .arg(&header)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let bindings =
        equilibrium_ffi::generate_bindings(&header, &equilibrium_ffi::BindingOptions::default())
            .unwrap();
    assert!(
        bindings.code.contains("pub fn sample(n: isize) -> isize;"),
        "{}",
        bindings.code
    );
    let rust = dir.path().join("bindings.rs");
    write(&rust, &bindings.code);
    let output = Command::new("rustc")
        .args(["--crate-type", "lib"])
        .arg(&rust)
        .arg("-o")
        .arg(dir.path().join("bindings.rlib"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stderr),
        bindings.code
    );
}

#[test]
fn source_feature_macro_before_include_is_refused_with_reason() {
    if !available(Language::C) {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("feature.c");
    write(&source, "#define _GNU_SOURCE\n#include <pthread.h>\npthread_rwlock_t *choose(pthread_rwlock_t *p) { return p; }\n");
    let compiler = find_compiler(Language::C).unwrap().compiler_path.unwrap();
    let probe = Command::new(compiler)
        .args(["-std=c11", "-fsyntax-only"])
        .arg(&source)
        .output()
        .unwrap();
    assert!(
        probe.status.success(),
        "source did not compile: {}",
        String::from_utf8_lossy(&probe.stderr)
    );
    let out = dir.path().join("out");
    let error =
        equilibrium_ffi::compile_to_c_with_extra(&source, &out, &["-std=c11".to_string()], &[])
            .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("source-defined macro directives preceding an active angle-bracket include"),
        "{error}"
    );
    assert!(!out.join(".equilibrium-header-feature/feature.h").exists());
}

#[test]
fn d_source_without_a_header_links_through_load() {
    if !available(Language::D) {
        return;
    }
    let info = find_compiler(Language::D).unwrap();
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
    if !available(Language::Nim) {
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
    if !available(Language::Rust) {
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

#[test]
fn zig_source_without_a_header_links_through_load() {
    if !available(Language::Zig) {
        return;
    }
    build_and_run(
        "zmath",
        "zig",
        "fn twice(v: c_int) c_int { return v * 2; }\n\
         export fn zmath_add(a: c_int, b: c_int) c_int { return @divTrunc(twice(a + b), 2); }\n",
        None,
    );
}

#[test]
fn odin_source_without_a_header_links_through_load() {
    if !available(Language::Odin) {
        return;
    }
    build_and_run(
        "omath",
        "odin",
        "package omath\nimport \"core:c\"\n\n\
         twice :: proc \"c\" (v: c.int) -> c.int { return v * 2 }\n\n\
         @(export)\nomath_add :: proc \"c\" (a, b: c.int) -> c.int { return twice(a + b) / 2 }\n",
        None,
    );
}

#[test]
fn v_source_without_a_header_links_through_load() {
    if !available(Language::V) {
        return;
    }
    build_and_run(
        "vmath",
        "v",
        "module main\n\nfn twice(v int) int { return v * 2 }\n\n\
         @[export: 'vmath_add']\npub fn vmath_add(a int, b int) int { return twice(a + b) / 2 }\n\n\
         fn main() {}\n",
        None,
    );
}

#[test]
fn hare_source_without_a_header_links_through_load() {
    if !available(Language::Hare) {
        return;
    }
    build_and_run(
        "hmath",
        "ha",
        "fn twice(v: int) int = v * 2;\n\nexport fn hmath_add(a: int, b: int) int = twice(a + b) / 2;\n",
        None,
    );
}
