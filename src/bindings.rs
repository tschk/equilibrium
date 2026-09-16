//! Rust binding generation from C headers.

use std::path::{Path, PathBuf};

use crate::c_header::{
    c_type_to_rust_checked, parse_c_header, parse_enum_discriminant, rust_ident, EnumDef,
    FunctionDef, ParsedHeader, StructDef, TypedefDef,
};
use crate::limits::read_header_content;

/// Options for binding generation.
#[derive(Clone, Debug, Default)]
pub struct BindingOptions {
    /// Module name for the generated bindings.
    pub module_name: Option<String>,
    /// Additional include paths.
    pub include_paths: Vec<PathBuf>,
    /// Functions to allowlist (if empty, include all).
    pub allowlist_functions: Vec<String>,
    /// Types to allowlist (if empty, include all).
    pub allowlist_types: Vec<String>,
    /// Generate impl blocks for types.
    pub derive_debug: bool,
    /// Generate Default impl.
    pub derive_default: bool,
}

/// A generated Rust binding.
#[derive(Clone, Debug)]
pub struct GeneratedBinding {
    /// The generated Rust code.
    pub code: String,
    /// The source header file.
    pub source_header: PathBuf,
    /// Any warnings during generation.
    pub warnings: Vec<String>,
}

/// Generate Rust bindings from a C header file.
///
/// This creates a Rust module with extern "C" declarations
/// that can be used to call the compiled C code.
pub fn generate_bindings(
    header: &Path,
    options: &BindingOptions,
) -> Result<GeneratedBinding, String> {
    if !header.exists() {
        return Err(format!("Header file not found: {}", header.display()));
    }
    let content = read_header_content(header)?;
    generate_bindings_from_content(header, &content, options)
}

pub fn generate_bindings_from_content(
    header: &Path,
    content: &str,
    options: &BindingOptions,
) -> Result<GeneratedBinding, String> {
    let parsed = parse_c_header(content);
    let mut warnings = Vec::new();
    let mut code = String::new();

    code.push_str("// Auto-generated bindings by equilibrium-ffi\n");
    code.push_str("//\n");
    code.push_str(&format!(
        "// Source: {}\n",
        sanitize_path_for_comment(header)
    ));
    code.push('\n');
    code.push_str("use std::os::raw::*;\n");
    code.push('\n');

    emit_bindings_from_parsed(&parsed, options, &mut code, &mut warnings);

    Ok(GeneratedBinding {
        code,
        source_header: header.to_path_buf(),
        warnings,
    })
}

fn should_include(name: &str, allowlist: &[String]) -> bool {
    allowlist.is_empty() || allowlist.iter().any(|a| a == name)
}

/// Strip control characters from a path so a generated `//` comment stays single-line.
fn sanitize_path_for_comment(path: &Path) -> String {
    path.display()
        .to_string()
        .chars()
        .filter(|c| !matches!(c, '\n' | '\r' | '\0'))
        .collect()
}

fn emit_bindings_from_parsed(
    parsed: &ParsedHeader,
    options: &BindingOptions,
    code: &mut String,
    warnings: &mut Vec<String>,
) {
    for enum_def in &parsed.enums {
        if should_include(&enum_def.name, &options.allowlist_types) {
            if let Some(generated) = generate_enum(enum_def, warnings) {
                code.push_str(&generated);
                code.push('\n');
            }
        }
    }

    for struct_def in &parsed.structs {
        if should_include(&struct_def.name, &options.allowlist_types) {
            if let Some(generated) = generate_struct(struct_def, options, warnings) {
                code.push_str(&generated);
                code.push('\n');
            }
        }
    }

    for typedef in &parsed.typedefs {
        if should_include(&typedef.name, &options.allowlist_types) {
            let is_struct_alias = typedef.target.starts_with("struct ")
                && parsed
                    .structs
                    .iter()
                    .any(|s| format!("struct {}", s.name) == typedef.target);
            let is_enum_alias = typedef.target.starts_with("enum ")
                && parsed
                    .enums
                    .iter()
                    .any(|e| format!("enum {}", e.name) == typedef.target);
            if !is_struct_alias && !is_enum_alias {
                if let Some(generated) = generate_typedef(typedef, warnings) {
                    code.push_str(&generated);
                    code.push('\n');
                }
            }
        }
    }

    code.push_str("#[allow(non_camel_case_types, non_snake_case, dead_code)]\n");
    code.push_str("extern \"C\" {\n");
    for func in &parsed.functions {
        if should_include(&func.name, &options.allowlist_functions) {
            match generate_function(func) {
                Ok(generated) => code.push_str(&generated),
                Err(reason) => warnings.push(reason),
            }
        } else {
            warnings.push(format!("Skipped function: {}", func.name));
        }
    }
    code.push_str("}\n");
}

fn generate_typedef(typedef: &TypedefDef, warnings: &mut Vec<String>) -> Option<String> {
    let Some(name) = rust_ident(&typedef.name) else {
        warnings.push(format!(
            "Skipped typedef with invalid name: {}",
            typedef.name
        ));
        return None;
    };
    match c_type_to_rust_checked(&typedef.target) {
        Ok(rust_type) => Some(format!("pub type {name} = {rust_type};\n")),
        Err(reason) => {
            warnings.push(format!("Skipped typedef {name}: {reason}"));
            None
        }
    }
}

fn generate_enum(enum_def: &EnumDef, warnings: &mut Vec<String>) -> Option<String> {
    let Some(name) = rust_ident(&enum_def.name) else {
        warnings.push(format!("Skipped enum with invalid name: {}", enum_def.name));
        return None;
    };
    let mut code = String::new();
    code.push_str("#[repr(C)]\n");
    code.push_str("#[derive(Debug, Copy, Clone, PartialEq, Eq)]\n");
    code.push_str(&format!("pub enum {name} {{\n"));
    let mut any = false;
    for (variant_name, variant_value) in &enum_def.variants {
        let Some(variant) = rust_ident(variant_name) else {
            warnings.push(format!(
                "Skipped enum variant with invalid name: {variant_name}"
            ));
            continue;
        };
        if let Some(value) = variant_value {
            let Some(n) = parse_enum_discriminant(value) else {
                warnings.push(format!(
                    "Skipped enum discriminant for {name}::{variant}: `{value}`"
                ));
                continue;
            };
            code.push_str(&format!("    {variant} = {n},\n"));
        } else {
            code.push_str(&format!("    {variant},\n"));
        }
        any = true;
    }
    if !any {
        warnings.push(format!("Skipped empty enum: {name}"));
        return None;
    }
    code.push_str("}\n");
    Some(code)
}

fn generate_struct(
    struct_def: &StructDef,
    options: &BindingOptions,
    warnings: &mut Vec<String>,
) -> Option<String> {
    let Some(name) = rust_ident(&struct_def.name) else {
        warnings.push(format!(
            "Skipped struct with invalid name: {}",
            struct_def.name
        ));
        return None;
    };
    let mut code = String::new();
    let mut derives = vec!["Copy", "Clone"];
    if options.derive_debug {
        derives.push("Debug");
    }
    if options.derive_default {
        derives.push("Default");
    }
    code.push_str(&format!("#[derive({})]\n", derives.join(", ")));
    code.push_str("#[repr(C)]\n");
    code.push_str(&format!("pub struct {name} {{\n"));
    for (field_type, field_name) in &struct_def.fields {
        let Some(field) = rust_ident(field_name) else {
            warnings.push(format!(
                "Skipped field with invalid name on {name}: {field_name}"
            ));
            continue;
        };
        match c_type_to_rust_checked(field_type) {
            Ok(rust_type) => {
                code.push_str(&format!("    pub {field}: {rust_type},\n"));
            }
            Err(reason) => {
                warnings.push(format!("Skipped field {name}.{field}: {reason}"));
            }
        }
    }
    code.push_str("}\n");
    Some(code)
}

fn generate_function(func: &FunctionDef) -> Result<String, String> {
    let name = rust_ident(&func.name)
        .ok_or_else(|| format!("Skipped function with invalid name: {}", func.name))?;
    let rust_return = c_type_to_rust_checked(&func.return_type)
        .map_err(|reason| format!("Skipped function {name}: {reason}"))?;
    let mut params = Vec::new();
    for (typ, param_name) in &func.params {
        let pname = rust_ident(param_name)
            .ok_or_else(|| format!("Skipped function {name}: invalid parameter `{param_name}`"))?;
        let rust_type = c_type_to_rust_checked(typ)
            .map_err(|reason| format!("Skipped function {name}: {reason}"))?;
        params.push(format!("{pname}: {rust_type}"));
    }
    let return_clause = if rust_return == "()" {
        String::new()
    } else {
        format!(" -> {rust_return}")
    };
    Ok(format!(
        "    pub fn {name}({}){return_clause};\n",
        params.join(", "),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::c_header::{c_type_to_rust, parse_function_line, parse_typedef_line};
    use crate::limits::{MAX_HEADER_BYTES, MAX_HEADER_LINES};
    use tempfile::tempdir;

    #[test]
    fn test_c_type_to_rust() {
        assert_eq!(c_type_to_rust("int"), "c_int");
        assert_eq!(c_type_to_rust("void"), "()");
        assert_eq!(c_type_to_rust("char *"), "*mut c_char");
        assert_eq!(c_type_to_rust("const char *"), "*const c_char");
    }

    #[test]
    fn test_c_type_to_rust_extended() {
        assert_eq!(c_type_to_rust("unsigned int"), "c_uint");
        assert_eq!(c_type_to_rust("unsigned long"), "c_ulong");
        assert_eq!(c_type_to_rust("long long"), "c_longlong");
        assert_eq!(c_type_to_rust("size_t"), "usize");
        assert_eq!(c_type_to_rust("ssize_t"), "isize");
        assert_eq!(c_type_to_rust("bool"), "bool");
        assert_eq!(c_type_to_rust("float"), "c_float");
        assert_eq!(c_type_to_rust("double"), "c_double");
        assert_eq!(c_type_to_rust("void*"), "*mut c_void");
        // const stripping
        assert_eq!(c_type_to_rust("const int"), "c_int");
    }

    #[test]
    fn test_parse_function() {
        let func = parse_function_line("int add(int a, int b);").unwrap();
        assert_eq!(func.name, "add");
        assert_eq!(func.return_type, "int");
        assert_eq!(func.params.len(), 2);
    }

    #[test]
    fn test_parse_function_void_params() {
        let func = parse_function_line("void cleanup(void);").unwrap();
        assert_eq!(func.name, "cleanup");
        assert_eq!(func.return_type, "void");
        assert_eq!(func.params.len(), 0);
    }

    #[test]
    fn test_parse_function_no_params() {
        let func = parse_function_line("int get_count();").unwrap();
        assert_eq!(func.name, "get_count");
        assert_eq!(func.return_type, "int");
        assert_eq!(func.params.len(), 0);
    }

    #[test]
    fn test_parse_function_pointer_param() {
        let func = parse_function_line("int string_length(const char* str);").unwrap();
        assert_eq!(func.name, "string_length");
        assert_eq!(func.return_type, "int");
        assert_eq!(func.params.len(), 1);
    }

    #[test]
    fn test_parse_typedef() {
        let (target, name) = parse_typedef_line("typedef int myint;").unwrap();
        assert_eq!(name, "myint");
        assert_eq!(target, "int");
    }

    #[test]
    fn test_parse_header_with_guards() {
        // Preprocessor directives should be ignored; typedefs inside guards should parse
        let content = "#ifndef MYLIB_H\n#define MYLIB_H\ntypedef int myint;\nint add(int a, int b);\n#endif\n";
        let parsed = parse_c_header(content);
        assert_eq!(parsed.typedefs.len(), 1);
        assert_eq!(parsed.typedefs[0].name, "myint");
        assert_eq!(parsed.functions.len(), 1);
        assert_eq!(parsed.functions[0].name, "add");
    }

    #[test]
    fn test_generate_bindings_basic() {
        let dir = tempdir().unwrap();
        let header = dir.path().join("mylib.h");
        std::fs::write(&header, "int add(int a, int b);\nvoid noop(void);\n").unwrap();

        let opts = BindingOptions::default();
        let binding = generate_bindings(&header, &opts).unwrap();

        assert!(binding.code.contains("extern \"C\""));
        assert!(binding.code.contains("pub fn add("));
        assert!(binding.code.contains("pub fn noop()"));
        assert!(binding.warnings.is_empty());
    }

    #[test]
    fn test_generate_bindings_missing_file() {
        let opts = BindingOptions::default();
        let result = generate_bindings(Path::new("/nonexistent/header.h"), &opts);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found"));
    }

    #[test]
    fn test_generate_bindings_rejects_too_many_lines() {
        let dir = tempdir().unwrap();
        let header = dir.path().join("long.h");
        let mut body = String::with_capacity(MAX_HEADER_LINES * 4 + 16);
        for _ in 0..=MAX_HEADER_LINES {
            body.push_str("//x\n");
        }
        std::fs::write(&header, body).unwrap();
        let opts = BindingOptions::default();
        let err = generate_bindings(&header, &opts).unwrap_err();
        assert!(err.contains("too many lines"), "got: {err}");
    }

    #[test]
    fn test_generate_bindings_rejects_oversized_file() {
        let dir = tempdir().unwrap();
        let header = dir.path().join("huge.h");
        let f = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&header)
            .unwrap();
        f.set_len(MAX_HEADER_BYTES + 1).unwrap();
        drop(f);
        let opts = BindingOptions::default();
        let err = generate_bindings(&header, &opts).unwrap_err();
        assert!(err.contains("too large"), "got: {err}");
    }

    #[test]
    fn test_generate_bindings_allowlist_functions() {
        let dir = tempdir().unwrap();
        let header = dir.path().join("mylib.h");
        std::fs::write(&header, "int add(int a, int b);\nint sub(int a, int b);\n").unwrap();

        let opts = BindingOptions {
            allowlist_functions: vec!["add".to_string()],
            ..Default::default()
        };
        let binding = generate_bindings(&header, &opts).unwrap();
        assert!(binding.code.contains("pub fn add("));
        assert!(!binding.code.contains("pub fn sub("));
        assert_eq!(binding.warnings.len(), 1);
        assert!(binding.warnings[0].contains("sub"));
    }

    #[test]
    fn test_generate_bindings_mathlib_header() {
        // Verify against the real mathlib.h in the repo
        let header = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/c-ffi/mathlib.h");
        if !header.exists() {
            return;
        }

        let opts = BindingOptions::default();
        let binding = generate_bindings(&header, &opts).unwrap();

        assert!(binding.code.contains("pub fn add("));
        assert!(binding.code.contains("pub fn subtract("));
        assert!(binding.code.contains("pub fn multiply("));
        assert!(binding.code.contains("pub fn fibonacci("));
        assert!(binding.code.contains("pub fn string_length("));
    }

    #[test]
    fn test_generate_bindings_with_typedef() {
        let dir = tempdir().unwrap();
        let header = dir.path().join("types.h");
        std::fs::write(&header, "typedef int handle_t;\nhandle_t open(void);\n").unwrap();

        let opts = BindingOptions::default();
        let binding = generate_bindings(&header, &opts).unwrap();
        assert!(binding.code.contains("pub type handle_t = c_int;"));
        assert!(binding.code.contains("pub fn open()"));
    }

    #[test]
    fn test_generate_bindings_rejects_malicious_enum_discriminant() {
        let dir = tempdir().unwrap();
        let header = dir.path().join("evil.h");
        std::fs::write(
            &header,
            "typedef enum { OK = 1, BAD = 1; include!(\"/tmp/pwn.rs\"); 0 } Evil;\nint foo(void);\n",
        )
        .unwrap();

        let opts = BindingOptions::default();
        let binding = generate_bindings(&header, &opts).unwrap();
        assert!(!binding.code.contains("include!"));
        assert!(!binding.code.contains("/tmp/pwn"));
        assert!(binding.code.contains("pub fn foo()"));
        assert!(binding
            .warnings
            .iter()
            .any(|w| w.contains("discriminant") || w.contains("Skipped")));
    }

    #[test]
    fn test_generate_bindings_char_double_pointer() {
        let dir = tempdir().unwrap();
        let header = dir.path().join("argv.h");
        std::fs::write(&header, "int count_args(char **argv);\n").unwrap();

        let opts = BindingOptions::default();
        let binding = generate_bindings(&header, &opts).unwrap();
        assert!(
            binding.code.contains("*mut *mut c_char"),
            "got: {}",
            binding.code
        );
        assert!(binding.warnings.is_empty(), "{:?}", binding.warnings);
    }

    #[test]
    fn test_generate_bindings_skips_multiline_prototype() {
        let dir = tempdir().unwrap();
        let header = dir.path().join("multi.h");
        std::fs::write(
            &header,
            "int sneaky(\n    int a,\n    int b);\nint ok(void);\n",
        )
        .unwrap();

        let opts = BindingOptions::default();
        let binding = generate_bindings(&header, &opts).unwrap();
        assert!(!binding.code.contains("sneaky"));
        assert!(binding.code.contains("pub fn ok()"));
    }

    #[test]
    fn test_generate_bindings_skips_unknown_type_passthrough() {
        let dir = tempdir().unwrap();
        let header = dir.path().join("weird.h");
        std::fs::write(&header, "void evil(not a type x);\nint ok(void);\n").unwrap();

        let opts = BindingOptions::default();
        let binding = generate_bindings(&header, &opts).unwrap();
        assert!(!binding.code.contains("not a type"));
        assert!(!binding.code.contains("pub fn evil("));
        assert!(binding.code.contains("pub fn ok()"));
    }
}
