//! End-to-end coverage for scriptc as a *consumer*: a TypeScript host calling a
//! native library through the `--ffi` manifest equilibrium generates.
//!
//! These tests need `scriptc` (Node.js 24+) and a C compiler. scriptc's
//! executable lane lowers through its bundled helper and links with the
//! platform linker driver, so no C compiler is needed for the host itself.

use std::path::Path;
use std::process::Command;

use equilibrium_ffi::{find_compiler, load_with_options, Language, LoadOptions};
use tempfile::tempdir;

const HEADER: &str = r#"
double c_scale(double value);
int c_add(int a, int b);
unsigned c_or(unsigned a, unsigned b);
int c_len(const uint8_t *p, size_t p_len);
bool c_flag(int a);
"#;

const SOURCE: &str = r#"
#include <stdbool.h>

double c_scale(double value) { return value * 2.0; }
int c_add(int a, int b) { return a + b; }
unsigned c_or(unsigned a, unsigned b) { return a | b; }
int c_len(const unsigned char *p, unsigned long n) { (void)p; return (int)n; }
bool c_flag(int a) { return a != 0; }
"#;

const HOST: &str = r#"
import { c_scale, c_add, c_or, c_len, c_flag } from "./bindings";

const bytes = new Uint8Array([1, 2, 3, 4]);
console.log("HOST", c_scale(21), c_add(20, 22), c_or(5, 3), c_len(bytes), c_flag(7));
"#;

fn scriptc_available() -> bool {
    find_compiler(Language::ScriptC).is_some()
}

#[test]
fn typescript_host_calls_a_native_library_through_the_generated_manifest() {
    if !scriptc_available() {
        return;
    }

    let dir = tempdir().unwrap();
    let source = dir.path().join("c_module.c");
    std::fs::write(&source, SOURCE).unwrap();
    std::fs::write(dir.path().join("c_module.h"), HEADER).unwrap();

    let out = dir.path().join("out");
    let module = load_with_options(
        &source,
        LoadOptions::default()
            .output_dir(&out)
            .link(false)
            .generate_bindings(false)
            .consumer_languages([Language::ScriptC]),
    )
    .expect("load the C module with a scriptc consumer");

    assert_eq!(module.imports.len(), 1);
    let generated = &module.imports[0];
    assert_eq!(generated.language, Language::ScriptC);
    assert!(
        generated
            .code
            .contains("export declare function c_add(a: number, b: number): number;"),
        "bindings:\n{}",
        generated.code
    );

    // Lay the host project out exactly as the generated files describe.
    let host = dir.path().join("host");
    std::fs::create_dir_all(&host).unwrap();
    std::fs::write(host.join("bindings.ts"), &generated.code).unwrap();
    let mut manifest_name = None;
    for companion in &generated.companions {
        std::fs::write(host.join(&companion.name), &companion.contents).unwrap();
        manifest_name = Some(companion.name.clone());
    }
    let manifest_name = manifest_name.expect("scriptc bindings carry their manifest");

    // The manifest must name the artifact equilibrium just compiled.
    let manifest = std::fs::read_to_string(host.join(&manifest_name)).unwrap();
    assert!(
        manifest.contains(&module.output_path.display().to_string()),
        "manifest:\n{manifest}"
    );
    assert!(
        manifest.contains("\"ffi_format\": 1"),
        "manifest:\n{manifest}"
    );

    std::fs::write(host.join("main.ts"), HOST).unwrap();
    let scriptc = find_compiler(Language::ScriptC)
        .and_then(|info| info.compiler_path)
        .expect("scriptc path");
    let build = Command::new(&scriptc)
        .args(["build", "main.ts", "--ffi", &manifest_name, "-o", "host"])
        .current_dir(&host)
        .output()
        .expect("run scriptc");
    assert!(
        build.status.success(),
        "scriptc build failed:\n{}\n{}",
        String::from_utf8_lossy(&build.stdout),
        String::from_utf8_lossy(&build.stderr)
    );

    let run = Command::new(host.join("host"))
        .output()
        .expect("run the TypeScript host");
    assert!(run.status.success(), "host exited with {:?}", run.status);
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(
        stdout.contains("HOST 42 42 7 4 true"),
        "host output:\n{stdout}"
    );
}

#[test]
fn scriptc_bindings_report_shapes_the_outbound_abi_cannot_carry() {
    if !scriptc_available() {
        return;
    }

    let dir = tempdir().unwrap();
    let source = dir.path().join("shapes.c");
    std::fs::write(
        &source,
        r#"
double shapes_ok(double value) { return value; }
long long shapes_big(long long value) { return value; }
int shapes_label(const char *name) { (void)name; return 0; }
"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("shapes.h"),
        r#"
double shapes_ok(double value);
long long shapes_big(long long value);
int shapes_label(const char *name);
"#,
    )
    .unwrap();

    let out = dir.path().join("out");
    let module = load_with_options(
        &source,
        LoadOptions::default()
            .output_dir(&out)
            .link(false)
            .generate_bindings(false)
            .consumer_languages([Language::ScriptC]),
    )
    .expect("load the C module");

    let generated = &module.imports[0];
    assert!(generated.code.contains("shapes_ok"));
    assert!(!generated.code.contains("shapes_big"));
    assert!(!generated.code.contains("shapes_label"));
    assert!(generated
        .warnings
        .iter()
        .any(|warning| warning.contains("shapes_big") && warning.contains("i32")));
    assert!(generated
        .warnings
        .iter()
        .any(|warning| warning.contains("shapes_label") && warning.contains("cstring")));
}

/// The generated manifest is JSON that scriptc accepts verbatim.
#[test]
fn generated_manifest_is_accepted_by_scriptc_coverage() {
    if !scriptc_available() {
        return;
    }

    let dir = tempdir().unwrap();
    let source = dir.path().join("c_module.c");
    std::fs::write(&source, SOURCE).unwrap();
    std::fs::write(dir.path().join("c_module.h"), HEADER).unwrap();

    let module = load_with_options(
        &source,
        LoadOptions::default()
            .output_dir(dir.path().join("out"))
            .link(false)
            .generate_bindings(false)
            .consumer_languages([Language::ScriptC]),
    )
    .unwrap();
    let generated = &module.imports[0];

    let host = dir.path().join("host");
    std::fs::create_dir_all(&host).unwrap();
    std::fs::write(host.join("bindings.ts"), &generated.code).unwrap();
    for companion in &generated.companions {
        std::fs::write(host.join(&companion.name), &companion.contents).unwrap();
    }
    std::fs::write(
        host.join("main.ts"),
        "import { c_add } from \"./bindings\";\nconsole.log(c_add(1, 2));\n",
    )
    .unwrap();

    // `coverage` parses the manifest without linking, so a rejected manifest
    // shows up here rather than as a link error.
    let scriptc = find_compiler(Language::ScriptC)
        .and_then(|info| info.compiler_path)
        .expect("scriptc path");
    let coverage = Command::new(&scriptc)
        .args(["coverage", "main.ts", "--ffi", "c_module.ffi.json"])
        .current_dir(&host)
        .output()
        .expect("run scriptc coverage");
    let stderr = String::from_utf8_lossy(&coverage.stderr);
    assert!(
        !stderr.contains("SC5"),
        "scriptc rejected the manifest:\n{stderr}"
    );
}

#[test]
fn relative_libraries_are_anchored_when_the_manifest_is_written() {
    let dir = tempdir().unwrap();
    let header = dir.path().join("lib.h");
    std::fs::write(&header, "int lib_add(int a, int b);\n").unwrap();

    let generated = equilibrium_ffi::generate_imports(
        &header,
        Language::ScriptC,
        &equilibrium_ffi::ImportOptions::default().native_libraries(["build/liblib.a"]),
    )
    .expect("scriptc imports");

    let manifest = &generated.companions[0].contents;
    let anchored = std::env::current_dir().unwrap().join("build/liblib.a");
    assert!(
        manifest.contains(&format!("\"libraries\": [\"{}\"]", anchored.display())),
        "manifest:\n{manifest}"
    );
    assert!(!manifest.contains("size_t"));
}

#[test]
fn generated_bindings_do_not_reference_the_source_header_path() {
    let dir = tempdir().unwrap();
    let header = dir.path().join("lib.h");
    std::fs::write(&header, "int lib_add(int a, int b);\n").unwrap();

    let generated = equilibrium_ffi::generate_imports(
        &header,
        Language::ScriptC,
        &equilibrium_ffi::ImportOptions::default(),
    )
    .unwrap();

    let expected_name = format!(
        "{}.ffi.json",
        Path::new(&header).file_stem().unwrap().to_str().unwrap()
    );
    assert_eq!(generated.companions[0].name, expected_name);
    assert!(generated.code.contains(&expected_name));
}
