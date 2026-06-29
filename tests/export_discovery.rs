use equilibrium_ffi::{
    discover_exports_with_options, find_compiler, load_with_options, ExportOptions, ExportSource,
    Language, LoadOptions,
};
use tempfile::tempdir;

#[test]
fn rust_explicit_exports_use_ffi_attribute() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("math.rs");
    std::fs::write(
        &source,
        r#"
#[ffi]
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

pub fn helper(a: i32) -> i32 {
    a
}
"#,
    )
    .unwrap();

    let discovery =
        discover_exports_with_options(&source, Language::Rust, &ExportOptions::default()).unwrap();

    assert_eq!(discovery.exports, vec!["add"]);
    assert_eq!(discovery.source, ExportSource::ExplicitMarkers);
}

#[test]
fn zig_explicit_exports_use_pub_export_functions() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("math.zig");
    std::fs::write(
        &source,
        r#"
pub export fn add(a: i32, b: i32) i32 {
    return a + b;
}

fn helper(a: i32) i32 {
    return a;
}
"#,
    )
    .unwrap();

    let discovery =
        discover_exports_with_options(&source, Language::Zig, &ExportOptions::default()).unwrap();

    assert_eq!(discovery.exports, vec!["add"]);
    assert_eq!(discovery.source, ExportSource::ExplicitMarkers);
}

#[test]
fn nim_explicit_exports_use_export_pragmas() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("math.nim");
    std::fs::write(
        &source,
        r#"
proc add*(a, b: cint): cint {.exportc, cdecl.} =
  return a + b

proc helper(a: cint): cint =
  return a
"#,
    )
    .unwrap();

    let discovery =
        discover_exports_with_options(&source, Language::Nim, &ExportOptions::default()).unwrap();

    assert_eq!(discovery.exports, vec!["add"]);
    assert_eq!(discovery.source, ExportSource::ExplicitMarkers);
}

#[test]
fn d_explicit_exports_use_extern_c_export() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("math.d");
    std::fs::write(
        &source,
        r#"
extern(C) export int add(int a, int b)
{
    return a + b;
}

int helper(int a)
{
    return a;
}
"#,
    )
    .unwrap();

    let discovery =
        discover_exports_with_options(&source, Language::D, &ExportOptions::default()).unwrap();

    assert_eq!(discovery.exports, vec!["add"]);
    assert_eq!(discovery.source, ExportSource::ExplicitMarkers);
}

#[test]
fn c_declarations_are_explicit_exports() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("math.h");
    std::fs::write(
        &source,
        r#"
int add(int a, int b);
static int helper(int a);
"#,
    )
    .unwrap();

    let discovery =
        discover_exports_with_options(&source, Language::C, &ExportOptions::default()).unwrap();

    assert_eq!(discovery.exports, vec!["add"]);
    assert_eq!(discovery.source, ExportSource::ExplicitMarkers);
}

#[test]
fn unmarked_file_falls_back_to_all_top_level_functions() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("math.rs");
    std::fs::write(
        &source,
        r#"
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

pub fn multiply(a: i32, b: i32) -> i32 {
    a * b
}
"#,
    )
    .unwrap();

    let discovery =
        discover_exports_with_options(&source, Language::Rust, &ExportOptions::default()).unwrap();

    assert_eq!(discovery.exports, vec!["add", "multiply"]);
    assert_eq!(discovery.source, ExportSource::AllFunctions);
}

#[test]
fn api_exports_override_config_and_fallback() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("math.rs");
    std::fs::write(
        &source,
        r#"
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

pub fn multiply(a: i32, b: i32) -> i32 {
    a * b
}
"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("equilibrium.toml"),
        r#"
[target.math]
language = "rust"
sources = ["math.rs"]
exports = ["multiply"]
"#,
    )
    .unwrap();

    let options = ExportOptions::default().exports(["add"]);
    let discovery = discover_exports_with_options(&source, Language::Rust, &options).unwrap();

    assert_eq!(discovery.exports, vec!["add"]);
    assert_eq!(discovery.source, ExportSource::Requested);
}

#[test]
fn config_exports_override_fallback() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("math.rs");
    std::fs::write(
        &source,
        r#"
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

pub fn multiply(a: i32, b: i32) -> i32 {
    a * b
}
"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("equilibrium.toml"),
        r#"
[target.math]
language = "rust"
sources = ["math.rs"]
exports = ["multiply"]
"#,
    )
    .unwrap();

    let discovery =
        discover_exports_with_options(&source, Language::Rust, &ExportOptions::default()).unwrap();

    assert_eq!(discovery.exports, vec!["multiply"]);
    assert_eq!(discovery.source, ExportSource::Config);
}

#[test]
fn unsupported_fallback_signatures_are_reported_as_warnings() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("math.rs");
    std::fs::write(
        &source,
        r#"
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

pub fn label(name: String) -> String {
    name
}
"#,
    )
    .unwrap();

    let discovery =
        discover_exports_with_options(&source, Language::Rust, &ExportOptions::default()).unwrap();

    assert_eq!(discovery.exports, vec!["add"]);
    assert!(discovery
        .warnings
        .iter()
        .any(|warning| warning.contains("label")));
}

#[test]
fn load_with_options_populates_requested_exports_without_requiring_bindings() {
    if find_compiler(Language::C).is_none() {
        return;
    }

    let dir = tempdir().unwrap();
    let source = dir.path().join("math.c");
    let output = dir.path().join("out");
    std::fs::write(
        &source,
        r#"
int add(int a, int b) {
    return a + b;
}

int multiply(int a, int b) {
    return a * b;
}
"#,
    )
    .unwrap();

    let module = load_with_options(
        &source,
        LoadOptions::default()
            .exports(["multiply"])
            .output_dir(&output)
            .generate_bindings(false),
    )
    .unwrap();

    assert_eq!(module.exports, vec!["multiply"]);
    assert_eq!(module.export_source, ExportSource::Requested);
    assert!(module.warnings.is_empty());
}
