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
fn unverified_pointer_integer_abi_is_refused() {
    let dir = tempdir().unwrap();
    let header = dir.path().join("pointer_integers.h");
    for source in [
        "intptr_t signed_value(void);",
        "void consume(uintptr_t value);",
        "void consume(const volatile intptr_t *input);",
        "void consume(uintptr_t *output);",
        "typedef intptr_t Alias;",
        "typedef uintptr_t (*Callback)(void);",
        "typedef struct Value { intptr_t value; } Value;",
    ] {
        std::fs::write(&header, source).unwrap();
        let result = equilibrium_ffi::generate_bindings(&header, &Default::default());
        let error = result.unwrap_err();
        assert!(error.contains("ABI"), "{source}: {error}");
        let error =
            equilibrium_ffi::generate_bindings_from_content(&header, source, &Default::default())
                .unwrap_err();
        assert!(error.contains("ABI"), "{source}: {error}");
        let error = generate_imports(&header, Language::Rust, &Default::default()).unwrap_err();
        assert!(error.contains("ABI"), "{source}: {error}");
    }
    std::fs::write(&header, "void consume(intptr_t_like value);").unwrap();
    assert!(equilibrium_ffi::generate_bindings(&header, &Default::default()).is_ok());
}

#[test]
fn tagged_pointer_typedefs_compile_with_pointer_signatures() {
    let dir = tempdir().unwrap();
    let header = dir.path().join("tag_pointers.h");
    std::fs::write(
        &header,
        r#"
struct S { int x; };
union U { int x; double y; };
typedef struct S Value;
typedef struct S *StructPtr;
typedef const struct S *ConstStructPtr;
typedef union U *UnionPtr;
typedef struct Missing *OpaqueStructPtr;
typedef union MissingUnion *OpaqueUnionPtr;
void consume_struct(StructPtr p);
void consume_const(ConstStructPtr p);
void consume_union(UnionPtr p);
void consume_opaque_struct(OpaqueStructPtr p);
void consume_opaque_union(OpaqueUnionPtr p);
void consume_value(Value v);
"#,
    )
    .unwrap();
    for binding in [
        equilibrium_ffi::generate_bindings(&header, &Default::default()).unwrap(),
        {
            let import = generate_imports(&header, Language::Rust, &Default::default()).unwrap();
            equilibrium_ffi::GeneratedBinding {
                code: import.code,
                source_header: import.source_header,
                warnings: import.warnings,
            }
        },
    ] {
        let source = format!(
            r#"{}
const _: () = assert!(std::mem::size_of::<StructPtr>() == std::mem::size_of::<*mut c_void>());
const _: () = assert!(std::mem::size_of::<UnionPtr>() == std::mem::size_of::<*mut c_void>());
const _: () = assert!(std::mem::size_of::<OpaqueStructPtr>() == std::mem::size_of::<*mut c_void>());
const _: () = assert!(std::mem::size_of::<OpaqueUnionPtr>() == std::mem::size_of::<*mut c_void>());
fn check() {{
    let _: unsafe extern "C" fn(*mut c_void) = consume_struct;
    let _: unsafe extern "C" fn(*const c_void) = consume_const;
    let _: unsafe extern "C" fn(*mut c_void) = consume_union;
    let _: unsafe extern "C" fn(*mut c_void) = consume_opaque_struct;
    let _: unsafe extern "C" fn(*mut c_void) = consume_opaque_union;
    let _: unsafe extern "C" fn(S) = consume_value;
}}
"#,
            binding.code
        );
        let rust_file = dir.path().join("generated.rs");
        std::fs::write(&rust_file, source).unwrap();
        let output = std::process::Command::new("rustc")
            .args([
                "--edition=2021",
                "--crate-type=lib",
                "--emit=metadata",
                "-o",
            ])
            .arg(dir.path().join("generated.rmeta"))
            .arg(&rust_file)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stderr),
            binding.code
        );
    }
}

#[test]
fn reconstructed_consumers_do_not_treat_tagged_pointers_as_tag_definitions() {
    let dir = tempdir().unwrap();
    let header = dir.path().join("tag_pointer.h");
    std::fs::write(
        &header,
        "struct Tag { int x; };\ntypedef struct Tag *Ptr;\nvoid consume(Ptr value);\n",
    )
    .unwrap();
    for (language, expected) in [
        (Language::Nim, "Ptr* = ptr Tag"),
        (Language::D, "alias Ptr = Tag*;"),
        (Language::Odin, "Ptr :: ^Tag"),
        (Language::CSharp, "extern void consume(IntPtr value)"),
    ] {
        let import = generate_imports(&header, language, &Default::default()).unwrap();
        assert!(
            import.code.contains(expected),
            "{language:?}: {}",
            import.code
        );
        assert!(
            !import.code.contains("Tag * {"),
            "{language:?}: {}",
            import.code
        );
        assert!(
            !import.code.contains("struct Ptr"),
            "{language:?}: {}",
            import.code
        );
    }
}

#[test]
fn incomplete_aggregate_layouts_do_not_escape_into_reconstructed_bindings() {
    let dir = tempdir().unwrap();
    let header = dir.path().join("records.h");
    std::fs::write(
        &header,
        r#"
typedef struct { int prefix; void (*callback)(void); int suffix; } Record;
typedef Record Alias;
typedef union { int number; void (*callback)(void); } Choice;
typedef struct { Alias items[2]; } Outer;
void consume(Record value);
void consume_alias(Alias value);
void consume_outer(Outer value);
void consume_choice(Choice value);
void inspect(Record *value);
"#,
    )
    .unwrap();
    let rust = equilibrium_ffi::generate_bindings(&header, &Default::default()).unwrap();
    assert!(rust.code.contains("_private: [u8; 0]"));
    assert!(!rust.code.contains("pub prefix"));
    assert!(rust.code.contains("pub fn inspect"));
    for name in [
        "consume",
        "consume_alias",
        "consume_outer",
        "consume_choice",
    ] {
        assert!(
            !rust.code.contains(&format!("pub fn {name}(")),
            "{}",
            rust.code
        );
        assert!(rust.warnings.iter().any(|warning| warning.contains(name)));
    }
    for language in [
        Language::Nim,
        Language::CSharp,
        Language::D,
        Language::Odin,
        Language::V,
    ] {
        let error = generate_imports(&header, language, &ImportOptions::default()).unwrap_err();
        assert!(error.contains("inspect"), "{language:?}: {error}");
    }
    for language in [Language::C, Language::Cpp, Language::Zig] {
        let result = generate_imports(&header, language, &ImportOptions::default()).unwrap();
        assert!(
            result.code.contains("consume"),
            "{language:?}: {}",
            result.code
        );
    }
}

#[test]
fn rejects_ambiguous_and_unmappable_fields_transitively() {
    let dir = tempdir().unwrap();
    let header = dir.path().join("ambiguous.h");
    std::fs::write(
        &header,
        r#"
typedef struct BadTag { int a, b; } Bad;
typedef struct BadTag Other;
typedef struct { struct BadTag value; } Outer;
typedef Outer Alias;
void consume(Alias value);
void consume_other(Other value);
void inspect(Bad *value);
"#,
    )
    .unwrap();
    let result = equilibrium_ffi::generate_bindings(&header, &Default::default()).unwrap();
    assert!(!result.code.contains("pub fn consume("), "{}", result.code);
    assert!(
        !result.code.contains("pub fn consume_other("),
        "{}",
        result.code
    );
    assert!(result.code.contains("pub type Other = Bad;"));
    assert!(result.code.contains("pub fn inspect("));
    assert!(!result.code.contains("pub b:"));
}

#[test]
fn unmappable_field_and_its_union_dependency_are_not_emitted_by_value() {
    let dir = tempdir().unwrap();
    let header = dir.path().join("unmapped.h");
    std::fs::write(
        &header,
        r#"
typedef struct { long double value; } Bad;
typedef union { Bad bad; int other; } Outer;
void consume(Outer value);
"#,
    )
    .unwrap();
    let binding = equilibrium_ffi::generate_bindings(&header, &Default::default()).unwrap();
    assert!(!binding.code.contains("pub fn consume("));
    assert!(!binding.code.contains("pub value:"));
    assert!(!binding.code.contains("pub union Outer"));
    for language in [
        Language::Nim,
        Language::CSharp,
        Language::D,
        Language::Odin,
        Language::V,
    ] {
        let result = generate_imports(&header, language, &ImportOptions::default()).unwrap();
        assert!(
            !result.code.contains("consume"),
            "{language:?}: {}",
            result.code
        );
    }
}

#[test]
fn callbacks_and_unsupported_bitfields_cannot_expose_incomplete_values() {
    let dir = tempdir().unwrap();
    let header = dir.path().join("callbacks.h");
    std::fs::write(
        &header,
        r#"
typedef struct { unsigned int bits : 100; } Bad;
typedef void (*Callback)(Bad value);
void consume(Bad value);
void register_callback(Callback cb);
"#,
    )
    .unwrap();
    let error = equilibrium_ffi::generate_bindings(&header, &Default::default()).unwrap_err();
    assert!(error.contains("Callback"));
    let error = generate_imports(&header, Language::Rust, &ImportOptions::default()).unwrap_err();
    assert!(error.contains("Callback"));
}

#[test]
fn qualified_aliases_do_not_hide_incomplete_layouts() {
    let header = std::path::Path::new("qualifiers.h");
    let result = equilibrium_ffi::generate_bindings_from_content(
        header,
        r#"
typedef struct { int a,b; } Bad;
typedef volatile const Bad Alias;
typedef struct { Alias value; int tail; } Outer;
void consume(Outer value);
"#,
        &Default::default(),
    )
    .unwrap();
    assert!(!result.code.contains("pub fn consume("), "{}", result.code);
    assert!(!result.code.contains("pub tail:"), "{}", result.code);
}

#[test]
fn callbacks_accept_incomplete_pointers_and_ignore_parameter_names() {
    let header = std::path::Path::new("callback_pointer.h");
    let result = equilibrium_ffi::generate_bindings_from_content(
        header,
        r#"
typedef struct { int a,b; } Bad;
typedef Bad *BadPtr;
typedef void (*Direct)(Bad *value);
typedef BadPtr (*Callback)(BadPtr value, int Bad);
void register_callback(Callback cb);
"#,
        &Default::default(),
    )
    .unwrap();
    assert!(
        result.code.contains("fn(BadPtr, c_int) -> BadPtr"),
        "{}",
        result.code
    );
    assert!(result.code.contains("fn(*mut Bad)"), "{}", result.code);
    assert!(result.code.contains("pub fn register_callback("));
}

#[test]
fn callbacks_reject_qualified_alias_passed_by_value() {
    let header = std::path::Path::new("callback_value.h");
    let error = equilibrium_ffi::generate_bindings_from_content(
        header,
        r#"
typedef struct { int a,b; } Bad;
typedef volatile const Bad Alias;
typedef Alias (*Callback)(int value);
"#,
        &Default::default(),
    )
    .unwrap_err();
    assert!(error.contains("Callback"));
}

#[test]
fn array_typedef_parameters_never_use_array_by_value() {
    let dir = tempdir().unwrap();
    let header = dir.path().join("array_alias.h");
    for declaration in [
        "void consume(Four value);",
        "typedef void (*Callback)(Four value);\nvoid register_callback(Callback cb);",
    ] {
        std::fs::write(
            &header,
            format!(
                "typedef int Four[4];\ntypedef Four Alias;\n{}\n",
                declaration.replace("Four value", "Alias value")
            ),
        )
        .unwrap();
        let error = equilibrium_ffi::generate_bindings(&header, &Default::default()).unwrap_err();
        assert!(error.contains("array typedef parameter"));
        for language in [
            Language::Nim,
            Language::CSharp,
            Language::D,
            Language::Odin,
            Language::V,
        ] {
            let error = generate_imports(&header, language, &Default::default()).unwrap_err();
            assert!(
                error.contains("array typedef parameter"),
                "{language:?}: {error}"
            );
        }
        for language in [Language::C, Language::Cpp, Language::Zig] {
            assert!(generate_imports(&header, language, &Default::default()).is_ok());
        }
    }
}

#[test]
fn array_of_pointers_typedef_parameters_are_rejected() {
    let header = std::path::Path::new("array_of_pointers.h");
    for declaration in [
        "void consume(Four value);",
        "typedef void (*Callback)(Four value);",
    ] {
        let source = format!("typedef int *Four[4];\n{declaration}\n");
        let error =
            equilibrium_ffi::generate_bindings_from_content(header, &source, &Default::default())
                .unwrap_err();
        assert!(error.contains("array typedef parameter"));
    }
    let control =
        "typedef int *Ptr;\nvoid consume(Ptr value);\ntypedef void (*Callback)(Ptr value);\n";
    let binding =
        equilibrium_ffi::generate_bindings_from_content(header, control, &Default::default())
            .unwrap();
    assert!(binding.code.contains("pub fn consume(value: Ptr)"));
    assert!(binding.code.contains("fn(Ptr)"));
}

#[test]
fn unused_opaque_pointer_alias_does_not_block_default_imports() {
    let dir = tempdir().unwrap();
    let header = dir.path().join("unused.h");
    std::fs::write(
        &header,
        "typedef struct { int a,b; } Bad;\ntypedef Bad *BadPtr;\nint good(int x);\n",
    )
    .unwrap();
    for language in [
        Language::Nim,
        Language::CSharp,
        Language::D,
        Language::Odin,
        Language::V,
    ] {
        let result = generate_imports(&header, language, &Default::default()).unwrap();
        assert!(result.code.contains("good"));
        assert!(!result.code.contains("BadPtr"));
        assert!(result
            .warnings
            .iter()
            .any(|warning| warning.contains("BadPtr")));
    }
}

#[test]
fn unused_opaque_pointer_alias_does_not_block_allowlisted_function() {
    let dir = tempdir().unwrap();
    let header = dir.path().join("allowed.h");
    std::fs::write(&header, "typedef struct { int a,b; } Bad;\ntypedef Bad *BadPtr;\nvoid inspect(BadPtr p);\nint safe(int x);\n").unwrap();
    for language in [
        Language::Nim,
        Language::CSharp,
        Language::D,
        Language::Odin,
        Language::V,
    ] {
        let result = generate_imports(
            &header,
            language,
            &ImportOptions::default().allowlist_functions(["safe"]),
        )
        .unwrap();
        assert!(
            result.code.contains("safe"),
            "{language:?}: {}",
            result.code
        );
        assert!(!result.code.contains("inspect"));
        assert!(result
            .warnings
            .iter()
            .any(|warning| warning.contains("BadPtr")));
    }
}

#[test]
fn reconstructed_consumers_explicitly_reject_opaque_pointer_aliases() {
    let dir = tempdir().unwrap();
    let header = dir.path().join("pointer_alias.h");
    std::fs::write(
        &header,
        r#"
typedef struct { int a,b; } Bad;
typedef Bad *BadPtr;
typedef BadPtr OtherPtr;
void inspect(OtherPtr value);
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
        let error = generate_imports(&header, language, &ImportOptions::default()).unwrap_err();
        assert!(error.contains("inspect"), "{language:?}: {error}");
    }
    std::fs::write(
        &header,
        "typedef struct { int a,b; } Bad;\ntypedef Bad *BadPtr;\n",
    )
    .unwrap();
    for language in [
        Language::Nim,
        Language::CSharp,
        Language::D,
        Language::Odin,
        Language::V,
    ] {
        let result = generate_imports(&header, language, &ImportOptions::default()).unwrap();
        assert!(!result.code.contains("BadPtr"));
        assert!(result
            .warnings
            .iter()
            .any(|warning| warning.contains("BadPtr")));
    }
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

#[test]
fn unnamed_parameters_keep_their_arity_and_unparseable_ones_are_reported() {
    let dir = tempdir().unwrap();
    let header = dir.path().join("proto.h");
    std::fs::write(
        &header,
        "int add(int, int);\n\
         unsigned int mask(unsigned int, const char *);\n\
         void tag(struct node *, size_t);\n\
         void tag_restrict(struct node *restrict n);\n",
    )
    .unwrap();

    let rust = generate_imports(&header, Language::Rust, &ImportOptions::default()).unwrap();
    assert_contains(&rust, "pub fn add(arg0: c_int, arg1: c_int) -> c_int;");
    assert_contains(
        &rust,
        "pub fn mask(arg0: c_uint, arg1: *const c_char) -> c_uint;",
    );
    assert_contains(&rust, "pub fn tag(arg0: *mut c_void, arg1: usize);");
    assert_contains(&rust, "pub fn tag_restrict(n: *mut c_void);");
    std::fs::write(&header, "void on(void (*cb)(int, int), void *ctx);\n").unwrap();
    let error = generate_imports(&header, Language::Rust, &ImportOptions::default()).unwrap_err();
    assert!(
        error.contains("void on(") && error.contains("parameter"),
        "{error}"
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
fn reconstructed_consumers_reject_unverified_enum_representation() {
    let dir = tempdir().unwrap();
    let header = dir.path().join("handle.h");
    for source in [
        "typedef enum { S_OK = 0, S_ERR = 1 } status;\nstatus obj_do(void);\n",
        "struct Container { enum { SMALL = 0 } value; };\n",
        "typedef enum Forward Forward;\nForward *obj_do(void);\n",
        "typedef enum { WIDE = 4294967295U } Wide;\n",
    ] {
        std::fs::write(&header, source).unwrap();
        for language in [
            Language::Rust,
            Language::Nim,
            Language::CSharp,
            Language::D,
            Language::Odin,
            Language::V,
            Language::Hare,
            Language::ScriptC,
        ] {
            for options in [
                ImportOptions::default(),
                ImportOptions::default().allowlist_functions(["unrelated"]),
            ] {
                let error = generate_imports(&header, language, &options).unwrap_err();
                assert!(
                    error.contains("enum representation"),
                    "{language:?}: {error}"
                );
            }
        }
    }
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

    let cpp = generate_imports(&header, Language::Cpp, &ImportOptions::default()).unwrap();
    assert_contains(&cpp, "obj_do(");

    for generated in [&zig, &c, &cpp] {
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
typedef struct Handle Handle;
typedef struct Point { int x; unsigned int y; int samples[3]; } Point;
typedef union Value { int integer; float real; } Value;
typedef Point Position;
typedef int Count;
int get_mode(Handle *handle);
Handle *open_handle(int mode);
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
        "  Handle* {.bycopy.} = object",
        "  Point* {.bycopy.} = object",
        "  Value* {.bycopy, union.} = object",
        "samples*: array[3, cint]",
        "proc get_mode(handle: ptr Handle): cint",
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
        "public struct Handle",
        "public struct Point",
        "public struct Value",
        "[StructLayout(LayoutKind.Explicit)]",
        "[FieldOffset(0)]",
        "public fixed int samples[3]",
        "public static extern int get_mode(IntPtr handle)",
        "public static extern Point locate(int count)",
    ] {
        assert_contains(&generated, expected);
    }
}

#[test]
fn d_imports_emit_declared_types_and_keep_their_functions() {
    let generated = declared_type_imports(Language::D);
    for expected in [
        "struct Handle",
        "struct Point",
        "union Value",
        "int[3] samples",
        "int get_mode(Handle* handle)",
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
        "Handle :: struct",
        "Point :: struct",
        "Value :: struct #raw_union",
        "samples: [3]c.int",
        "get_mode :: proc(handle: ^Handle) -> c.int",
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
        "struct C.Handle",
        "struct C.Point",
        "union C.Value",
        "samples [3]int",
        "fn C.get_mode(handle &C.Handle) int",
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
fn mapping_imports_keep_plain_tagged_aggregate_signatures() {
    let dir = tempdir().unwrap();
    let header = dir.path().join("tagged.h");
    std::fs::write(
        &header,
        r#"
struct Record { int value; };
union Payload { int value; double real; };
int inspect_record(struct Record record, union Payload payload);
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
        assert_contains(&generated, "Payload");
    }
}
