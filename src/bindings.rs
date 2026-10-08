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
    Ok(generate_bindings_from_parsed(header, &parsed, options))
}

/// Generate Rust bindings from an already-parsed header. This is the single source of complete
/// Rust output (types + extern block); the Rust consumer-imports path reuses it so the two do not
/// diverge.
pub fn generate_bindings_from_parsed(
    header: &Path,
    parsed: &ParsedHeader,
    options: &BindingOptions,
) -> GeneratedBinding {
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

    emit_bindings_from_parsed(parsed, options, &mut code, &mut warnings);

    GeneratedBinding {
        code,
        source_header: header.to_path_buf(),
        warnings,
    }
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
    for def in &parsed.defines {
        if should_include(&def.name, &options.allowlist_types) {
            if let Some(ident) = rust_ident(&def.name) {
                code.push_str(&format!(
                    "#[allow(non_upper_case_globals, dead_code)]\npub const {ident}: {} = {};\n\n",
                    def.rust_type, def.value
                ));
            } else {
                warnings.push(format!("Skipped #define with invalid name: {}", def.name));
            }
        }
    }

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

    for union_def in &parsed.unions {
        if should_include(&union_def.name, &options.allowlist_types) {
            if let Some(generated) = generate_union(union_def, warnings) {
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
            let is_union_alias = typedef.target.starts_with("union ")
                && parsed
                    .unions
                    .iter()
                    .any(|u| format!("union {}", u.name) == typedef.target);
            let is_enum_alias = typedef.target.starts_with("enum ")
                && parsed
                    .enums
                    .iter()
                    .any(|e| format!("enum {}", e.name) == typedef.target);
            if !is_struct_alias && !is_union_alias && !is_enum_alias {
                let target = typedef.target.trim();
                let opaque_tag = target
                    .strip_prefix("struct ")
                    .or_else(|| target.strip_prefix("union "));
                if opaque_tag.is_some() {
                    // A typedef to a struct/union tag with no definition in this header is an
                    // opaque handle (e.g. `typedef struct Foo foo;`). Emit a standard opaque FFI
                    // type so the pointers that reference it resolve, instead of dropping it.
                    if let Some(generated) = generate_opaque_typedef(&typedef.name, warnings) {
                        code.push_str(&generated);
                        code.push('\n');
                    }
                } else if let Some(generated) = generate_typedef(typedef, warnings) {
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

// C type/function names are rarely idiomatic Rust; generated items carry this so the bindings are
// clean under a consumer's default lints and `-D warnings`.
const TYPE_ALLOW: &str = "#[allow(non_camel_case_types, non_snake_case, dead_code)]\n";

fn generate_opaque_typedef(name: &str, warnings: &mut Vec<String>) -> Option<String> {
    let Some(ident) = rust_ident(name) else {
        warnings.push(format!("Skipped opaque typedef with invalid name: {name}"));
        return None;
    };
    // Zero-sized, private field: references through pointers resolve, and the type cannot be
    // constructed or dereferenced by consumers — the usual Rust representation of an opaque C type.
    Some(format!(
        "{TYPE_ALLOW}#[repr(C)]\npub struct {ident} {{\n    _private: [u8; 0],\n}}\n"
    ))
}

fn generate_typedef(typedef: &TypedefDef, warnings: &mut Vec<String>) -> Option<String> {
    let Some(name) = rust_ident(&typedef.name) else {
        warnings.push(format!(
            "Skipped typedef with invalid name: {}",
            typedef.name
        ));
        return None;
    };
    if let Some(rust_type) = &typedef.rust_override {
        return Some(format!("{TYPE_ALLOW}pub type {name} = {rust_type};\n"));
    }
    match c_type_to_rust_checked(&typedef.target) {
        Ok(rust_type) => Some(format!("{TYPE_ALLOW}pub type {name} = {rust_type};\n")),
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
    code.push_str(TYPE_ALLOW);
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

fn generate_union(union_def: &StructDef, warnings: &mut Vec<String>) -> Option<String> {
    let Some(name) = rust_ident(&union_def.name) else {
        warnings.push(format!(
            "Skipped union with invalid name: {}",
            union_def.name
        ));
        return None;
    };
    for field in &union_def.bitfields {
        let Some((size, _)) = bitfield_layout(&field.c_type) else {
            warnings.push(format!("Skipped union {name}: unsupported bitfield layout"));
            return None;
        };
        if field.width == 0 || field.width > size * 8 {
            warnings.push(format!("Skipped union {name}: unsupported bitfield width"));
            return None;
        }
    }
    let mut code = String::new();
    // No derives: C-ABI union fields (scalars, pointers, arrays of Copy) are themselves Copy so the
    // union is valid without ManuallyDrop, but Debug/Default cannot be derived for a union.
    code.push_str(TYPE_ALLOW);
    code.push_str("#[repr(C)]\n");
    code.push_str(&format!("pub union {name} {{\n"));
    for (index, field) in union_def.bitfields.iter().enumerate() {
        let typ = c_type_to_rust_checked(&field.c_type).ok()?;
        code.push_str(&format!("    pub __eq_bitfield_storage_{index}: {typ},\n"));
    }
    for (field_type, field_name) in &union_def.fields {
        let Some(field) = rust_ident(field_name) else {
            warnings.push(format!(
                "Skipped field with invalid name on {name}: {field_name}"
            ));
            continue;
        };
        match c_type_to_rust_checked(field_type) {
            Ok(rust_type) => code.push_str(&format!("    pub {field}: {rust_type},\n")),
            Err(reason) => warnings.push(format!("Skipped field {name}.{field}: {reason}")),
        }
    }
    code.push_str("}\n");
    Some(code)
}

/// Scalar/array layout used for the native GCC/Clang bitfield allocation convention.
/// Unknown aggregate layouts are rejected rather than silently emitting a smaller struct.
fn bitfield_layout(c_type: &str) -> Option<(usize, usize)> {
    let typ = c_type_to_rust_checked(c_type).ok()?;
    macro_rules! layout {
        ($typ:ty) => {
            (std::mem::size_of::<$typ>(), std::mem::align_of::<$typ>())
        };
    }
    let layout = match typ.as_str() {
        "u8" | "i8" | "c_char" | "c_schar" | "c_uchar" | "bool" => layout!(u8),
        "u16" | "i16" | "c_short" | "c_ushort" => layout!(std::os::raw::c_short),
        "u32" | "i32" | "c_int" | "c_uint" => layout!(std::os::raw::c_int),
        "c_float" => layout!(std::os::raw::c_float),
        "u64" | "i64" | "c_longlong" | "c_ulonglong" => layout!(std::os::raw::c_longlong),
        "c_double" => layout!(std::os::raw::c_double),
        "c_long" | "c_ulong" => layout!(std::os::raw::c_long),
        "usize" | "isize" => layout!(usize),
        _ if typ.starts_with("*mut ") || typ.starts_with("*const ") => layout!(*const ()),
        _ => {
            let (base, dims) = crate::c_header::split_array_dims(c_type)?;
            if dims.is_empty() {
                return None;
            }
            let (mut size, align) = bitfield_layout(&base)?;
            for dim in dims {
                size = size.checked_mul(dim.parse::<usize>().ok()?)?;
            }
            return Some((size, align));
        }
    };
    Some(layout)
}

fn bitfield_storage(def: &StructDef) -> Option<(Vec<String>, usize)> {
    if cfg!(target_env = "msvc") {
        return None;
    }
    let mut insertions = vec![String::new(); def.fields.len() + 1];
    let mut offset = 0usize;
    let mut alignment = 1;
    for (position, insertion) in insertions.iter_mut().enumerate() {
        let start = offset;
        let mut cursor = offset.checked_mul(8)?;
        for field in def
            .bitfields
            .iter()
            .filter(|field| field.position == position)
        {
            let (size, align) = bitfield_layout(&field.c_type)?;
            let rust_type = c_type_to_rust_checked(&field.c_type).ok()?;
            if !matches!(
                rust_type.as_str(),
                "u8" | "i8"
                    | "u16"
                    | "i16"
                    | "u32"
                    | "i32"
                    | "u64"
                    | "i64"
                    | "c_char"
                    | "c_schar"
                    | "c_uchar"
                    | "c_short"
                    | "c_ushort"
                    | "c_int"
                    | "c_uint"
                    | "c_long"
                    | "c_ulong"
                    | "c_longlong"
                    | "c_ulonglong"
                    | "bool"
            ) {
                return None;
            }
            let bits = size.checked_mul(8)?;
            if field.width > bits || (field.width == 0 && field.name.is_some()) {
                return None;
            }
            if field.width == 0 {
                cursor = cursor.div_ceil(bits).checked_mul(bits)?;
            } else {
                if cursor % bits + field.width > bits {
                    cursor = cursor.div_ceil(bits).checked_mul(bits)?;
                }
                cursor = cursor.checked_add(field.width)?;
                if field.name.is_some() {
                    alignment = alignment.max(align);
                }
            }
        }
        offset = cursor.div_ceil(8);
        if offset > start {
            *insertion = format!(
                "    pub __eq_bitfield_storage_{position}: [u8; {}],\n",
                offset - start
            );
        }
        if let Some((typ, _)) = def.fields.get(position) {
            let (size, align) = bitfield_layout(typ)?;
            offset = offset
                .div_ceil(align)
                .checked_mul(align)?
                .checked_add(size)?;
        }
    }
    Some((insertions, alignment))
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
    let storage = if struct_def.bitfields.is_empty() {
        None
    } else {
        match bitfield_storage(struct_def) {
            Some(storage) => Some(storage),
            None => {
                warnings.push(format!(
                    "Skipped struct {name}: unsupported bitfield layout"
                ));
                return None;
            }
        }
    };
    let mut code = String::new();
    let mut derives = vec!["Copy", "Clone"];
    if options.derive_debug {
        derives.push("Debug");
    }
    if options.derive_default {
        derives.push("Default");
    }
    code.push_str(TYPE_ALLOW);
    code.push_str(&format!("#[derive({})]\n", derives.join(", ")));
    code.push_str("#[repr(C)]\n");
    code.push_str(&format!("pub struct {name} {{\n"));
    if let Some((_, alignment)) = &storage {
        code.push_str(&format!(
            "    __eq_bitfield_alignment: [u{}; 0],\n",
            alignment * 8
        ));
    }
    for (position, (field_type, field_name)) in struct_def.fields.iter().enumerate() {
        if let Some((insertions, _)) = &storage {
            code.push_str(&insertions[position]);
        }
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
    if let Some((insertions, _)) = &storage {
        code.push_str(&insertions[struct_def.fields.len()]);
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
    fn test_c_type_to_rust_nested_pointers() {
        // `const` applies to the innermost type, so Rust spells it on the
        // pointer that encloses it — the outer pointer stays mutable.
        assert_eq!(c_type_to_rust("uint8_t **"), "*mut *mut u8");
        assert_eq!(c_type_to_rust("const uint8_t **"), "*mut *const u8");
        assert_eq!(c_type_to_rust("const char **"), "*mut *const c_char");
        assert_eq!(c_type_to_rust("const void **"), "*mut *const c_void");
        assert_eq!(c_type_to_rust("const char *"), "*const c_char");
        assert_eq!(c_type_to_rust("const void *"), "*const c_void");
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
    fn test_generate_bindings_opaque_struct_typedef() {
        let dir = tempdir().unwrap();
        let header = dir.path().join("opaque.h");
        std::fs::write(
            &header,
            "typedef struct Thing thing;\nthing *thing_new(void);\nvoid thing_free(thing *t);\n",
        )
        .unwrap();

        let opts = BindingOptions::default();
        let binding = generate_bindings(&header, &opts).unwrap();
        // The opaque handle must be defined, not referenced-but-undefined.
        assert!(binding.code.contains("pub struct thing {"));
        assert!(binding.code.contains("_private: [u8; 0]"));
        // And it must be warning-clean under a consumer's default lints / -D warnings.
        assert!(binding
            .code
            .contains("#[allow(non_camel_case_types, non_snake_case, dead_code)]\n#[repr(C)]\npub struct thing"));
        assert!(binding.code.contains("pub fn thing_new() -> *mut thing"));
        assert!(binding.code.contains("pub fn thing_free(t: *mut thing)"));
        // And it must not leak the C `struct Thing` spelling as a Rust type alias.
        assert!(!binding.code.contains("= struct"));
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
    #[test]
    fn bitfield_storage_preserves_native_c_layout() {
        let dir = tempdir().unwrap();
        let header = dir.path().join("bits.h");
        let declarations = "typedef struct { unsigned flags : 3; char tail; } Single;\n\
            typedef struct { unsigned a : 3; unsigned b : 5; char tail; } Adjacent;\n\
            typedef struct { unsigned a : 30; unsigned b : 3; char tail; } Overflow;\n\
            typedef struct { char head; unsigned a : 3; char tail; } Prefix;\n\
            typedef struct { unsigned a : 3; unsigned : 0; char tail; } Boundary;\n\
            typedef struct { unsigned a : 3; unsigned tail : 5; } Only;\n\
            typedef struct { unsigned : 3; unsigned a : 5; char tail; } Unnamed;\n\
            typedef struct { unsigned char a : 3; unsigned b : 5; char tail; } Mixed;\n";
        std::fs::write(&header, declarations).unwrap();
        let binding = generate_bindings(&header, &BindingOptions::default()).unwrap();
        assert!(binding.warnings.is_empty(), "{:?}", binding.warnings);
        assert!(binding.code.contains("__eq_bitfield_storage_0: [u8; 1]"));
        assert!(binding.code.contains("__eq_bitfield_storage_0: [u8; 5]"));
        let rust = dir.path().join("layout.rs");
        let mut checks = String::new();
        for name in [
            "Single", "Adjacent", "Overflow", "Prefix", "Boundary", "Only", "Unnamed", "Mixed",
        ] {
            let offset = if name == "Only" {
                "0".to_string()
            } else {
                format!("std::mem::offset_of!({name}, tail)")
            };
            checks.push_str(&format!("println!(\"{{}} {{}} {{}}\", std::mem::size_of::<{name}>(), std::mem::align_of::<{name}>(), {offset});\n"));
        }
        std::fs::write(&rust, format!("{}\nfn main() {{{checks}}}", binding.code)).unwrap();
        let rust_bin = dir.path().join("rust_layout");
        let status = std::process::Command::new("rustc")
            .args(["--edition", "2021", "-D", "warnings"])
            .arg(&rust)
            .arg("-o")
            .arg(&rust_bin)
            .status()
            .unwrap();
        assert!(status.success());
        let rust_output = std::process::Command::new(&rust_bin).output().unwrap();
        // Compare size, alignment and following-field offsets with the system C compiler.
        let c = dir.path().join("layout.c");
        let mut checks = String::new();
        for name in [
            "Single", "Adjacent", "Overflow", "Prefix", "Boundary", "Only", "Unnamed", "Mixed",
        ] {
            let offset = if name == "Only" {
                "(size_t)0".to_string()
            } else {
                format!("offsetof({name}, tail)")
            };
            checks.push_str(&format!(
                "printf(\"%zu %zu %zu\\n\", sizeof({name}), _Alignof({name}), {offset});\n"
            ));
        }
        std::fs::write(&c, format!("#include <stdio.h>\n#include <stddef.h>\n{declarations}\nint main(void) {{{checks}}}")).unwrap();
        let c_bin = dir.path().join("c_layout");
        let status = std::process::Command::new("cc")
            .arg(&c)
            .arg("-o")
            .arg(&c_bin)
            .status()
            .unwrap();
        assert!(status.success());
        let c_output = std::process::Command::new(c_bin).output().unwrap();
        assert_eq!(rust_output.stdout, c_output.stdout);
    }
    #[test]
    fn unsupported_bitfields_do_not_emit_corrupt_layouts() {
        let dir = tempdir().unwrap();
        let header = dir.path().join("unsupported.h");
        std::fs::write(
            &header,
            "typedef struct { unsigned flags : 99; char tail; } Invalid;\n",
        )
        .unwrap();
        let binding = generate_bindings(&header, &BindingOptions::default()).unwrap();
        assert!(!binding.code.contains("pub struct Invalid"));
        assert!(binding
            .warnings
            .iter()
            .any(|warning| warning.contains("unsupported bitfield layout")));
    }
}
