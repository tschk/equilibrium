use equilibrium_ffi::{
    find_compiler, generate_imports, load_with_options, GeneratedImport, ImportOptions, Language,
    LoadOptions,
};
use tempfile::tempdir;

fn write_header() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempdir().unwrap();
    let header = dir.path().join("math.h");
    std::fs::write(
        &header,
        r#"
int add(int a, int b);
int multiply(int a, int b);
const char *label(const char *name);
void set_value(int *value);
typedef struct Pair {
    int left;
    int right;
} Pair;
Pair unsupported_pair(void);
"#,
    )
    .unwrap();
    (dir, header)
}

#[test]
fn generates_imports_for_all_detected_languages() {
    let (_dir, header) = write_header();
    let cases = [
        (Language::Rust, "extern \"C\"", "pub fn add"),
        (Language::Zig, "@cImport", "pub const add = c.add"),
        (Language::C, "#include \"math.h\"", "eq_math_add"),
        (Language::Cpp, "extern \"C\"", "eq_math_add"),
        (
            Language::CSharp,
            "DllImport",
            "public static extern int add",
        ),
        (Language::D, "extern(C)", "int add"),
        (Language::Nim, "{.importc: \"add\", cdecl.}", "proc add"),
        (Language::Odin, "foreign import", "add :: proc"),
        (Language::Hare, "@symbol(\"add\")", "fn add"),
        (Language::V, "#flag -I", "fn C.add"),
    ];

    for (language, required, function) in cases {
        let generated = generate_imports(&header, language, &ImportOptions::default()).unwrap();
        assert_contains(&generated, required);
        assert_contains(&generated, function);
        assert_eq!(generated.language, language);
        assert_eq!(generated.source_header, header);
    }
}

#[test]
fn import_generation_skips_unsupported_return_types_with_warnings() {
    // A by-value struct return cannot be remapped into a mapping-based consumer (Nim here), so it
    // is skipped with a warning. (Header-including consumers like Zig/C bind it fine via the
    // header and are covered by header_including_consumers_keep_functions_using_declared_types.)
    let (_dir, header) = write_header();
    let generated = generate_imports(&header, Language::Nim, &ImportOptions::default()).unwrap();

    assert!(!generated.code.contains("unsupported_pair"));
    assert!(generated
        .warnings
        .iter()
        .any(|warning| warning.contains("unsupported_pair")));
}

#[test]
fn import_generation_respects_allowlist() {
    let (_dir, header) = write_header();
    let generated = generate_imports(
        &header,
        Language::Nim,
        &ImportOptions::default().allowlist_functions(["multiply"]),
    )
    .unwrap();

    assert!(generated.code.contains("multiply"));
    assert!(!generated.code.contains("add"));
}

#[test]
fn load_with_options_populates_requested_consumer_imports() {
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
"#,
    )
    .unwrap();

    let module = load_with_options(
        &source,
        LoadOptions::default()
            .output_dir(&output)
            .generate_bindings(false)
            .consumer_languages([Language::Zig, Language::Nim]),
    )
    .unwrap();

    assert_eq!(
        module
            .imports
            .iter()
            .map(|generated| generated.language)
            .collect::<Vec<_>>(),
        vec![Language::Zig, Language::Nim]
    );
}

#[test]
fn pointer_returns_keep_a_c_identifier_name() {
    let dir = tempdir().unwrap();
    let header = dir.path().join("strings.h");
    std::fs::write(
        &header,
        "const char *label(void);\nconst char **labels(void);\nint *count(void);\n",
    )
    .unwrap();

    let rust = generate_imports(&header, Language::Rust, &ImportOptions::default()).unwrap();
    assert_contains(&rust, "pub fn label() -> *const c_char;");
    assert_contains(&rust, "pub fn count() -> *mut c_int;");
    assert!(!rust.code.contains("pub fn *"), "{}", rust.code);

    let scriptc = generate_imports(&header, Language::ScriptC, &ImportOptions::default()).unwrap();
    assert!(
        scriptc
            .warnings
            .iter()
            .any(|warning| warning.contains("Skipped function label for scriptc")),
        "a `const char *` return is skipped for scriptc, not for its name: {:?}",
        scriptc.warnings
    );
}

fn assert_contains(generated: &GeneratedImport, needle: &str) {
    assert!(
        generated.code.contains(needle),
        "generated {:?} wrapper did not contain {needle}:\n{}",
        generated.language,
        generated.code
    );
}

#[test]
fn comments_in_declarations_do_not_leak_into_types() {
    let dir = tempdir().unwrap();
    let header = dir.path().join("commented.h");
    std::fs::write(
        &header,
        r#"/* Generated facade: markers + hash helper. */
/* enrichment */ uint32_t crc32fast_hash(const uint8_t *data, size_t len);
/** Adds two numbers. */ int32_t add(int32_t a, int32_t b); // trailing note
typedef struct /* inline */ Point { int x; int y; } Point;
/*
 * A multi-line note mentioning add(int a, int b); and `label` must not turn
 * into a declaration.
 */
const char *label(void);
"#,
    )
    .unwrap();

    let generated =
        generate_imports(&header, Language::ScriptC, &ImportOptions::default()).unwrap();
    assert_contains(&generated, "crc32fast_hash(data: Uint8Array)");
    assert_contains(&generated, "add(a: number, b: number)");
    assert!(
        !generated
            .warnings
            .iter()
            .any(|warning| warning.contains("`/*")),
        "comments must not become part of the type: {:?}",
        generated.warnings
    );
    assert!(
        generated
            .warnings
            .iter()
            .any(|warning| warning.contains("Skipped function label for scriptc: `const char *`")),
        "a pointer return is skipped for its class, not for a mangled name: {:?}",
        generated.warnings
    );

    let rust = generate_imports(&header, Language::Rust, &ImportOptions::default()).unwrap();
    assert_contains(&rust, "pub fn crc32fast_hash");
    assert!(rust.code.contains("u32"), "{}", rust.code);
}

#[test]
fn rust_consumer_emits_declared_types_and_keeps_all_functions() {
    // Previously the Rust consumer path dropped every function that referenced a header-declared
    // type (enum / opaque handle), emitting only scalar-signature functions. The Rust consumer now
    // routes through the full bindings generator, so the types are defined and no function is lost.
    let dir = tempdir().unwrap();
    let header = dir.path().join("handle.h");
    std::fs::write(
        &header,
        "typedef enum { S_OK = 0, S_ERR = 1 } status;\n\
         typedef struct Obj obj;\n\
         obj *obj_new(void);\n\
         void obj_free(obj *o);\n\
         status obj_do(obj *o, int n);\n",
    )
    .unwrap();

    let rust = generate_imports(&header, Language::Rust, &ImportOptions::default()).unwrap();
    assert_contains(&rust, "pub enum status");
    assert_contains(&rust, "pub struct obj");
    assert_contains(&rust, "pub fn obj_new() -> *mut obj;");
    assert_contains(&rust, "pub fn obj_free(o: *mut obj)");
    assert_contains(&rust, "pub fn obj_do(o: *mut obj, n: c_int) -> status;");
    assert!(
        rust.warnings
            .iter()
            .all(|w| !w.contains("not supported for generated imports")),
        "no function should be dropped for the Rust consumer: {:?}",
        rust.warnings
    );
}

#[test]
fn header_including_consumers_keep_functions_using_declared_types() {
    // Zig/C/C++ bind by including the header, so functions using a declared enum/opaque type must
    // not be dropped by the scalar-only gate.
    let dir = tempdir().unwrap();
    let header = dir.path().join("handle.h");
    std::fs::write(
        &header,
        "typedef enum { S_OK = 0 } status;\n\
         typedef struct Obj obj;\n\
         obj *obj_new(void);\n\
         status obj_do(obj *o, int n);\n",
    )
    .unwrap();

    let zig = generate_imports(&header, Language::Zig, &ImportOptions::default()).unwrap();
    assert_contains(&zig, "pub const obj_new = c.obj_new;");
    assert_contains(&zig, "pub const obj_do = c.obj_do;");

    let c = generate_imports(&header, Language::C, &ImportOptions::default()).unwrap();
    assert_contains(&c, "obj_new(");
    assert_contains(&c, "obj_do(");

    for generated in [&zig, &c] {
        assert!(
            generated
                .warnings
                .iter()
                .all(|w| !w.contains("not supported for generated imports")),
            "header-including consumer dropped a function: {:?}",
            generated.warnings
        );
    }
}
