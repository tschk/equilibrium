use equilibrium_ffi::{
    generate_bindings, generate_bindings_from_content, generate_imports, BindingOptions,
    ImportOptions, Language,
};

#[test]
fn unresolved_include_error_identifies_the_abi_risk() {
    let dir = tempfile::tempdir().unwrap();
    let header = dir.path().join("uses_include.h");
    let source = "#include \"foreign_types.h\"\nint value(void);\n";
    std::fs::write(&header, source).unwrap();
    let binding_error =
        generate_bindings_from_content(&header, source, &BindingOptions::default()).unwrap_err();
    let import_error =
        generate_imports(&header, Language::Nim, &ImportOptions::default()).unwrap_err();
    for error in [binding_error, import_error] {
        assert!(error.contains("#include \"foreign_types.h\""), "{error}");
        assert!(error.contains("ABI mismatch"), "{error}");
    }
}

#[test]
fn reconstructed_consumers_reject_unresolved_preprocessing() {
    let dir = tempfile::tempdir().unwrap();
    let header = dir.path().join("macros.h");
    for source in [
        "#include \"types.h\"\nint read_value(void);\n",
        "#define JOIN(a,b) a##b\nJOIN(en,um) State { OK };\n",
        "#define KIND en\\\num\nKIND State { OK };\n",
        "%:define TYPE int\nTYPE read_value(void);\n",
        "??=include \"types.h\"\nint read_value(void);\n",
        "#if FLAG\ntypedef int Value;\n#else\ntypedef float Value;\n#endif\n",
        "#ifndef GUARDED_H\n#define GUARDED_H\nint value(void);\n#else\nint other(void);\n#endif\n",
        "#define TYPE int\nTYPE read_value(void);\n",
    ] {
        std::fs::write(&header, source).unwrap();
        assert!(
            generate_bindings_from_content(&header, source, &BindingOptions::default()).is_err(),
            "{source}"
        );
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
            assert!(
                generate_imports(&header, language, &ImportOptions::default()).is_err(),
                "{language:?}: {source}"
            );
        }
    }
}

#[test]
fn conventional_include_guards_remain_supported() {
    let dir = tempfile::tempdir().unwrap();
    let header = dir.path().join("guarded.h");
    let source =
        "#ifndef GUARDED_H\n#define GUARDED_H\n#define VALUE 42\nint value(void);\n#endif\n";
    std::fs::write(&header, source).unwrap();
    assert!(generate_bindings_from_content(&header, source, &BindingOptions::default()).is_ok());
    assert!(generate_imports(&header, Language::Nim, &ImportOptions::default()).is_ok());
}

#[test]
fn header_based_consumers_can_use_includes() {
    let dir = tempfile::tempdir().unwrap();
    let header = dir.path().join("included.h");
    std::fs::write(&header, "#include \"types.h\"\nint value(void);\n").unwrap();
    for language in [Language::C, Language::Cpp, Language::Zig] {
        assert!(
            generate_imports(&header, language, &ImportOptions::default()).is_ok(),
            "{language:?}"
        );
    }
}

#[test]
fn synthesized_headers_remain_unverified_and_obey_the_same_checks() {
    if equilibrium_ffi::find_compiler(Language::C).is_none() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("sample.c");
    std::fs::write(&source, "int value(int n) { return n; }\n").unwrap();
    let result = equilibrium_ffi::compile_to_c(&source, &dir.path().join("out")).unwrap();
    assert!(result
        .warnings
        .iter()
        .any(|warning| warning.contains("ABI is unverified")));
    let header = result.header_path.unwrap();
    let text = std::fs::read_to_string(&header).unwrap();
    assert!(text.contains("ABI unverified"));
    let binding = generate_bindings(&header, &BindingOptions::default()).unwrap();
    assert!(binding
        .warnings
        .iter()
        .any(|warning| warning.contains("ABI is unverified")));
    for language in [
        Language::Rust,
        Language::Nim,
        Language::ScriptC,
        Language::C,
    ] {
        let generated = generate_imports(&header, language, &ImportOptions::default()).unwrap();
        assert!(
            generated
                .warnings
                .iter()
                .any(|warning| warning.contains("ABI is unverified")),
            "{language:?}"
        );
    }

    std::fs::write(
        &source,
        "#include <stdint.h>\nuint32_t value(uint32_t n) { return n; }\n",
    )
    .unwrap();
    let result = equilibrium_ffi::compile_to_c(&source, &dir.path().join("out")).unwrap();
    let header = result.header_path.unwrap();
    assert!(generate_bindings(&header, &BindingOptions::default()).is_err());
    assert!(generate_imports(&header, Language::Nim, &ImportOptions::default()).is_err());
}

#[test]
fn public_generators_refuse_unparsed_declarations() {
    let dir = tempfile::tempdir().unwrap();
    let header = dir.path().join("declarations.h");
    for (declaration, name) in [
        ("int multiline(\n int x);", "multiline"),
        ("int\nsplit_return(int x);", "split_return"),
        ("int\n\nsplit_blank(void);", "split_blank"),
        ("int first(void); int second(void);", "second"),
        ("typedef int Alias; int after_alias(void);", "after_alias"),
        ("typedef struct { int x; } A, *AP;", "A, *AP"),
        ("#define KIND en\\\num\nKIND x;", "line splicing"),
        (
            "typedef struct {\n int x;\n} Pair; int after_pair(void);",
            "after_pair",
        ),
        ("typedef void (*callback)(int, ...);", "callback"),
        ("typedef int First, Second;", "First"),
        ("int variadic(int x, ...);", "variadic"),
    ] {
        let content = format!("{declaration}\nint valid(void);\n");
        std::fs::write(&header, &content).unwrap();
        let binding_options = BindingOptions::default();
        for result in [
            generate_bindings(&header, &binding_options),
            generate_bindings_from_content(&header, &content, &binding_options),
        ] {
            let error = result.expect_err("incomplete bindings must be refused");
            assert!(error.contains(name), "{error}");
        }
        for language in [
            Language::Rust,
            Language::Nim,
            Language::C,
            Language::ScriptC,
        ] {
            let error = generate_imports(&header, language, &ImportOptions::default())
                .expect_err("incomplete imports must be refused");
            assert!(error.contains(name), "{language:?}: {error}");
        }
    }
}

#[test]
fn tagged_pointer_return_is_not_silently_dropped() {
    let source = "struct S;\nstruct S *foo(void);\n";
    let binding = generate_bindings_from_content(
        std::path::Path::new("tag.h"),
        source,
        &BindingOptions::default(),
    )
    .unwrap();
    assert!(
        binding.code.contains("pub fn foo() -> *mut c_void;"),
        "{}",
        binding.code
    );
}

#[test]
fn public_generators_accept_supported_declarations() {
    let dir = tempfile::tempdir().unwrap();
    let header = dir.path().join("supported.h");
    std::fs::write(&header, "int valid(int value);\n").unwrap();
    assert!(generate_bindings(&header, &BindingOptions::default()).is_ok());
    for language in [
        Language::Rust,
        Language::Nim,
        Language::C,
        Language::ScriptC,
    ] {
        assert!(generate_imports(&header, language, &ImportOptions::default()).is_ok());
    }
}
