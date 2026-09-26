//! End-to-end coverage for scriptc (TypeScript → native) library mode.
//!
//! These tests need the `scriptc` compiler and a C compiler for its library
//! lane (scriptc's `clang` driver, or `zig` which equilibrium selects
//! automatically). They are skipped when `scriptc` is not installed.

use equilibrium_ffi::{
    discover_exports_with_options, find_compiler, load_with_options, ExportOptions, ExportSource,
    Language, LoadOptions,
};
use tempfile::tempdir;

const MODULE: &str = r#"
export function add(a: number, b: number): number {
  return a + b;
}

export function greet(
  who: string,
  loud: boolean,
): string {
  return loud ? who.toUpperCase() : who;
}

export function size(data: Uint8Array): number {
  return data.length;
}

function helper(a: number): number {
  return a;
}
"#;

fn scriptc_available() -> bool {
    find_compiler(Language::ScriptC).is_some()
}

#[test]
fn load_builds_a_scriptc_library_archive_with_matching_bindings() {
    if !scriptc_available() {
        return;
    }

    let dir = tempdir().unwrap();
    let source = dir.path().join("math.ts");
    let out = dir.path().join("out");
    std::fs::write(&source, MODULE).unwrap();

    let module = load_with_options(&source, LoadOptions::default().output_dir(&out).link(false))
        .expect("load scriptc module");

    assert_eq!(module.language, Language::ScriptC);
    assert_eq!(
        module.exports,
        vec!["add", "greet", "size"],
        "only exported functions belong to the ABI"
    );
    assert_eq!(module.export_source, ExportSource::ExplicitMarkers);

    // scriptc names its archives `<stem>.lib.a`; equilibrium keeps the `lib`
    // prefix so rustc's `-l` lookup finds it.
    assert_eq!(
        module
            .output_path
            .file_name()
            .and_then(|name| name.to_str()),
        Some("libmath.a")
    );
    assert!(module.output_path.is_file(), "archive was not produced");
    let header = module.header_path.as_deref().expect("generated header");
    assert_eq!(
        header.file_name().and_then(|name| name.to_str()),
        Some("math.h")
    );
    assert!(header.is_file(), "generated header is missing");
    assert!(
        out.join("math.profile.json").is_file(),
        "library profile was not written"
    );

    let code = module.bindings_code().expect("bindings");
    assert!(
        code.contains("pub fn math_add(a: c_double, b: c_double) -> c_double;"),
        "bindings:\n{code}"
    );
    assert!(code.contains("pub fn math_init();"), "bindings:\n{code}");
    assert!(
        code.contains("pub fn math_set_panic_sink(fn_ptr: *mut c_void, ctx: *mut c_void);"),
        "bindings:\n{code}"
    );
    assert!(
        code.contains("pub fn math_size(data_ptr: *const u8, data_len: usize) -> c_double;"),
        "bindings:\n{code}"
    );
    // `string` results come back through scriptc's out parameters.
    assert!(
        code.contains(
            "pub fn math_greet(who_ptr: *const u8, who_len: usize, loud: u8, out: *mut *const u8, out_len: *mut usize);"
        ),
        "bindings:\n{code}"
    );
    // The module's private helper is not part of the ABI.
    assert!(!code.contains("helper"), "bindings:\n{code}");
}

#[test]
fn profile_declares_the_discovered_exports() {
    if !scriptc_available() {
        return;
    }

    let dir = tempdir().unwrap();
    let source = dir.path().join("math.ts");
    let out = dir.path().join("out");
    std::fs::write(&source, MODULE).unwrap();

    load_with_options(&source, LoadOptions::default().output_dir(&out).link(false))
        .expect("load scriptc module");

    let profile = std::fs::read_to_string(out.join("math.profile.json")).unwrap();
    assert!(profile.contains("\"prefix\": \"math_\""));
    assert!(profile.contains("\"localize_runtime\": true"));
    assert!(profile.contains("\"entry\":"));
    assert!(profile.contains(
        "{ \"export\": \"add\", \"symbol\": \"math_add\", \"params\": [\"f64\", \"f64\"], \"returns\": \"f64\" }"
    ));
    assert!(profile.contains(
        "{ \"export\": \"greet\", \"symbol\": \"math_greet\", \"params\": [\"string\", \"bool\"], \"returns\": \"string\" }"
    ));
    assert!(profile.contains(
        "{ \"export\": \"size\", \"symbol\": \"math_size\", \"params\": [\"bytes\"], \"returns\": \"f64\" }"
    ));
}

#[test]
fn exports_outside_the_c_abi_are_reported_and_left_out() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("shapes.ts");
    std::fs::write(
        &source,
        r#"
export function ok(a: number): number {
  return a;
}

export function unsupported(when: Date): number {
  return 0;
}
"#,
    )
    .unwrap();

    let discovery =
        discover_exports_with_options(&source, Language::ScriptC, &ExportOptions::default())
            .unwrap();

    assert_eq!(discovery.exports, vec!["ok"]);
    assert!(discovery
        .warnings
        .iter()
        .any(|warning| warning.contains("unsupported") && warning.contains("Date")));
}

#[test]
fn target_config_refines_the_abi_and_the_emission() {
    if !scriptc_available() {
        return;
    }

    let dir = tempdir().unwrap();
    let source = dir.path().join("mix.ts");
    std::fs::write(
        &source,
        r#"
export function mix(tag: number, idx: number): number {
  return tag * 1000 + idx;
}

export function scale(seq: number, factor: number): number {
  return seq * factor;
}

// scriptc only accepts an integer return whose value it can prove whole and
// in range, which is what the ordered comparisons establish here.
export function truncate(value: number): number {
  if (value > -9007199254740991 && value < 9007199254740991) {
    return Math.trunc(value);
  }
  return 0;
}
"#,
    )
    .unwrap();
    let config = dir.path().join("equilibrium.toml");
    std::fs::write(
        &config,
        r#"
[target.mix]
language = "scriptc"
sources = ["mix.ts"]
emission = "c"

[target.mix.signatures]
mix = { params = ["u32", "u32"], returns = "f64" }
scale = { params = ["u64", "f64"], returns = "f64" }
truncate = { returns = "i64" }
"#,
    )
    .unwrap();

    let out = dir.path().join("out");
    let module = load_with_options(&source, LoadOptions::default().output_dir(&out).link(false))
        .expect("load configured scriptc module");
    assert!(module.output_path.is_file(), "archive was not produced");

    let profile = std::fs::read_to_string(out.join("mix.profile.json")).unwrap();
    assert!(
        profile.contains("\"emission\": \"c\""),
        "profile:\n{profile}"
    );
    assert!(
        profile.contains(
            "{ \"export\": \"mix\", \"symbol\": \"mix_mix\", \"params\": [\"u32\", \"u32\"], \"returns\": \"f64\" }"
        ),
        "profile:\n{profile}"
    );
    assert!(
        profile.contains(
            "{ \"export\": \"scale\", \"symbol\": \"mix_scale\", \"params\": [\"u64\", \"f64\"], \"returns\": \"f64\" }"
        ),
        "profile:\n{profile}"
    );
    assert!(
        profile.contains(
            "{ \"export\": \"truncate\", \"symbol\": \"mix_truncate\", \"params\": [\"f64\"], \"returns\": \"i64\" }"
        ),
        "profile:\n{profile}"
    );

    // The bindings follow the configured classes, so the host hands over
    // integers in their C-native widths.
    let code = module.bindings_code().expect("bindings");
    assert!(
        code.contains("pub fn mix_mix(tag: u32, idx: u32) -> c_double;"),
        "bindings:\n{code}"
    );
    assert!(
        code.contains("pub fn mix_scale(seq: u64, factor: c_double) -> c_double;"),
        "bindings:\n{code}"
    );
    assert!(
        code.contains("pub fn mix_truncate(value: c_double) -> i64;"),
        "bindings:\n{code}"
    );
}

#[test]
fn unusable_signature_override_fails_the_build() {
    if !scriptc_available() {
        return;
    }

    let dir = tempdir().unwrap();
    let source = dir.path().join("mix.ts");
    std::fs::write(
        &source,
        "export function mix(tag: number): number {\n  return tag;\n}\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("equilibrium.toml"),
        r#"
[target.mix]
language = "scriptc"
sources = ["mix.ts"]

[target.mix.signatures]
mix = { params = ["string"], returns = "f64" }
"#,
    )
    .unwrap();

    let out = dir.path().join("out");
    let error = load_with_options(&source, LoadOptions::default().output_dir(&out).link(false))
        .expect_err("the override cannot describe the annotation");
    let message = error.to_string();
    assert!(
        message.contains("cannot describe the TypeScript parameter `tag: number`"),
        "error: {message}"
    );
}

#[test]
fn target_config_exports_allowlist_scopes_discovery_not_the_archive() {
    if !scriptc_available() {
        return;
    }

    let dir = tempdir().unwrap();
    let source = dir.path().join("math.ts");
    std::fs::write(
        &source,
        r#"
export function add(a: number, b: number): number {
  return a + b;
}

export function mul(a: number, b: number): number {
  return a * b;
}
"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("equilibrium.toml"),
        r#"
[target.math]
language = "scriptc"
sources = ["math.ts"]
exports = ["mul"]
"#,
    )
    .unwrap();

    let out = dir.path().join("out");
    let module = load_with_options(&source, LoadOptions::default().output_dir(&out).link(false))
        .expect("load scriptc module");

    // The allowlist scopes discovery (and the consumer wrappers built from it);
    // the archive and the header still carry the whole module, so the Rust
    // bindings do too.
    let profile = std::fs::read_to_string(out.join("math.profile.json")).unwrap();
    assert!(
        profile.contains("\"symbol\": \"math_add\""),
        "profile:\n{profile}"
    );
    assert!(
        profile.contains("\"symbol\": \"math_mul\""),
        "profile:\n{profile}"
    );

    assert_eq!(module.exports, vec!["mul"]);
    assert_eq!(module.export_source, ExportSource::Config);
}

#[test]
fn empty_module_still_builds_a_callable_archive() {
    if !scriptc_available() {
        return;
    }

    let dir = tempdir().unwrap();
    let source = dir.path().join("nothing.ts");
    let out = dir.path().join("out");
    std::fs::write(&source, "// no exports\nconst answer = 42;\n").unwrap();

    let module = load_with_options(&source, LoadOptions::default().output_dir(&out).link(false))
        .expect("load empty scriptc module");

    assert!(module.exports.is_empty());
    assert!(out.join("libnothing.a").is_file());
    let code = module.bindings_code().expect("bindings");
    assert!(code.contains("pub fn nothing_init();"), "bindings:\n{code}");
    assert!(!code.contains("pub fn nothing_add"), "bindings:\n{code}");
}
