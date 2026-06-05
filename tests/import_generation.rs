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
    let (_dir, header) = write_header();
    let generated = generate_imports(&header, Language::Zig, &ImportOptions::default()).unwrap();

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

fn assert_contains(generated: &GeneratedImport, needle: &str) {
    assert!(
        generated.code.contains(needle),
        "generated {:?} wrapper did not contain {needle}:\n{}",
        generated.language,
        generated.code
    );
}
