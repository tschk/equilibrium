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
    // Header-declared aggregates now have emitted definitions. A genuinely unknown return
    // type still cannot be remapped and must be skipped with a warning.
    let (_dir, header) = write_header();
    let mut content = std::fs::read_to_string(&header).unwrap();
    content.push_str("\nUnknownType unsupported_unknown(void);\n");
    std::fs::write(&header, content).unwrap();
    let generated = generate_imports(&header, Language::Nim, &ImportOptions::default()).unwrap();

    assert!(!generated.code.contains("unsupported_unknown"));
    assert!(generated
        .warnings
        .iter()
        .any(|warning| warning.contains("unsupported_unknown")));
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

fn declared_type_imports(language: Language) -> GeneratedImport {
    let dir = tempdir().unwrap();
    let header = dir.path().join("declared.h");
    std::fs::write(
        &header,
        r#"
typedef enum Mode { MODE_FIRST = 2, MODE_NEXT, MODE_ALIAS = 5 } Mode;
typedef struct Handle Handle;
typedef struct Point { int x; unsigned int y; int samples[3]; } Point;
typedef union Value { int integer; float real; } Value;
typedef Point Position;
typedef int Count;
Mode get_mode(Handle *handle);
Handle *open_handle(Mode mode);
Point move_point(Point point);
Value read_value(Handle *handle);
Position locate(Count count);
NotDeclared unknown_type(void);
typedef NotDeclared MissingAlias;
MissingAlias unknown_alias(void);
"#,
    )
    .unwrap();
    let generated = generate_imports(&header, language, &ImportOptions::default()).unwrap();
    for name in [
        "get_mode",
        "open_handle",
        "move_point",
        "read_value",
        "locate",
    ] {
        assert!(
            generated.code.contains(name),
            "{language:?} dropped {name}: {}",
            generated.code
        );
        assert!(!generated
            .warnings
            .iter()
            .any(|warning| warning.contains(name)));
    }
    for name in ["unknown_type", "unknown_alias"] {
        assert!(!generated.code.contains(name));
        assert!(generated
            .warnings
            .iter()
            .any(|warning| warning.contains(name)));
    }
    generated
}

#[test]
fn nim_imports_emit_declared_types_and_keep_their_functions() {
    let generated = declared_type_imports(Language::Nim);
    for expected in [
        "  Mode* = cint",
        "const MODE_NEXT* = Mode(MODE_FIRST + 1)",
        "  Handle* {.bycopy.} = object",
        "  Point* {.bycopy.} = object",
        "  Value* {.bycopy, union.} = object",
        "samples*: array[3, cint]",
        "proc get_mode(handle: ptr Handle): Mode",
        "  Position* = Point",
        "  Count* = cint",
    ] {
        assert_contains(&generated, expected);
    }
}

#[test]
fn csharp_imports_emit_declared_types_and_keep_their_functions() {
    let generated = declared_type_imports(Language::CSharp);
    for expected in [
        "public enum Mode : int",
        "MODE_NEXT = MODE_FIRST + 1",
        "public struct Handle",
        "public struct Point",
        "public struct Value",
        "[StructLayout(LayoutKind.Explicit)]",
        "[FieldOffset(0)]",
        "public fixed int samples[3]",
        "public static extern Mode get_mode(IntPtr handle)",
        "public static extern Point locate(int count)",
    ] {
        assert_contains(&generated, expected);
    }
}

#[test]
fn d_imports_emit_declared_types_and_keep_their_functions() {
    let generated = declared_type_imports(Language::D);
    for expected in [
        "alias Mode = int",
        "enum Mode MODE_NEXT = MODE_FIRST + 1",
        "struct Handle",
        "struct Point",
        "union Value",
        "int[3] samples",
        "Mode get_mode(Handle* handle)",
        "alias Position = Point",
        "alias Count = int",
    ] {
        assert_contains(&generated, expected);
    }
}

#[test]
fn odin_imports_emit_declared_types_and_keep_their_functions() {
    let generated = declared_type_imports(Language::Odin);
    for expected in [
        "package bindings",
        "import c \"core:c\"",
        "foreign eq {",
        "Mode :: c.int",
        "MODE_NEXT :: Mode(MODE_FIRST + 1)",
        "Handle :: struct",
        "Point :: struct",
        "Value :: struct #raw_union",
        "samples: [3]c.int",
        "get_mode :: proc(handle: ^Handle) -> Mode",
        "Position :: Point",
        "Count :: c.int",
    ] {
        assert_contains(&generated, expected);
    }
}

#[test]
fn v_imports_emit_declared_types_and_keep_their_functions() {
    let generated = declared_type_imports(Language::V);
    for expected in [
        "enum C.Mode",
        "mode_next = mode_first + 1",
        "struct C.Handle",
        "struct C.Point",
        "union C.Value",
        "samples [3]int",
        "fn C.get_mode(handle &C.Handle) C.Mode",
        "fn C.locate(count int) C.Point",
    ] {
        assert_contains(&generated, expected);
    }
}

#[test]
fn mapping_imports_reject_aggregates_without_supported_layouts() {
    let dir = tempdir().unwrap();
    let header = dir.path().join("unsupported.h");
    std::fs::write(
        &header,
        r#"
typedef struct Flags { unsigned flags : 3; } Flags;
typedef struct Bad { NotDeclared value; } Bad;
typedef Bad BadAlias;
Flags get_flags(void);
BadAlias get_bad(void);
"#,
    )
    .unwrap();
    for language in [
        Language::Nim,
        Language::CSharp,
        Language::D,
        Language::Odin,
        Language::V,
    ] {
        let generated = generate_imports(&header, language, &ImportOptions::default()).unwrap();
        for name in ["get_flags", "get_bad"] {
            assert!(
                !generated.code.contains(name),
                "{language:?}: {}",
                generated.code
            );
            assert!(generated
                .warnings
                .iter()
                .any(|warning| warning.contains(name)));
        }
        assert!(!generated.code.contains("NotDeclared"));
        assert!(!generated.code.contains("Flags"));
    }
}

#[test]
fn csharp_rejects_fixed_arrays_of_nonprimitive_elements() {
    let dir = tempdir().unwrap();
    let header = dir.path().join("array.h");
    std::fs::write(
        &header,
        r#"
typedef struct Point { int x; } Point;
typedef struct Points { Point values[3]; } Points;
Points get_points(void);
"#,
    )
    .unwrap();
    let generated = generate_imports(&header, Language::CSharp, &ImportOptions::default()).unwrap();
    assert!(!generated.code.contains("get_points"));
    assert!(!generated.code.contains("fixed Point"));
    assert!(generated
        .warnings
        .iter()
        .any(|warning| warning.contains("get_points")));
}

#[test]
fn nim_groups_forward_referencing_types_in_one_section() {
    let dir = tempdir().unwrap();
    let header = dir.path().join("forward.h");
    std::fs::write(
        &header,
        r#"
typedef struct First { struct Second *next; } First;
typedef struct Second { First *previous; } Second;
First *get_first(void);
"#,
    )
    .unwrap();
    let generated = generate_imports(&header, Language::Nim, &ImportOptions::default()).unwrap();
    assert_eq!(
        generated
            .code
            .lines()
            .filter(|line| *line == "type")
            .count(),
        1
    );
    assert_contains(
        &generated,
        "  First* {.bycopy.} = object\n    next*: ptr Second",
    );
    assert_contains(
        &generated,
        "  Second* {.bycopy.} = object\n    previous*: ptr First",
    );
    assert_contains(&generated, "proc get_first(): ptr First");
}

#[test]
fn v_rejects_synthetic_anonymous_types_absent_from_the_c_header() {
    let dir = tempdir().unwrap();
    let header = dir.path().join("anonymous.h");
    std::fs::write(
        &header,
        r#"
typedef struct Outer { struct { int x; } point; } Outer;
Outer get_outer(void);
"#,
    )
    .unwrap();
    let generated = generate_imports(&header, Language::V, &ImportOptions::default()).unwrap();
    assert!(!generated.code.contains("get_outer"));
    assert!(!generated.code.contains("C.Outer__anon"));
    assert!(generated
        .warnings
        .iter()
        .any(|warning| warning.contains("get_outer")));
}

#[test]
fn mapping_imports_keep_plain_tagged_aggregate_and_enum_signatures() {
    let dir = tempdir().unwrap();
    let header = dir.path().join("tagged.h");
    std::fs::write(
        &header,
        r#"
enum State { STATE_START = 0, STATE_STOP = 1 };
struct Record { int value; };
union Payload { int value; double real; };
int inspect_record(struct Record record, enum State state, union Payload payload);
"#,
    )
    .unwrap();
    for language in [
        Language::Nim,
        Language::CSharp,
        Language::D,
        Language::Odin,
        Language::V,
    ] {
        let generated = generate_imports(&header, language, &ImportOptions::default()).unwrap();
        assert!(
            generated.warnings.is_empty(),
            "{language:?}: {:?}",
            generated.warnings
        );
        assert_contains(&generated, "inspect_record");
        assert_contains(&generated, "Record");
        assert_contains(&generated, "State");
        assert_contains(&generated, "Payload");
    }
}
