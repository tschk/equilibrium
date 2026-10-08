use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::c_header::{
    header_stem, is_c_abi_safe_type, is_c_identifier, parse_c_header, parse_enum_discriminant,
    split_array_dims, FunctionDef, ParsedHeader,
};
use crate::detector::Language;
use crate::limits::read_header_content;

#[derive(Clone, Debug, Default)]
pub struct ImportOptions {
    pub allowlist_functions: Vec<String>,
    /// Native archives or objects the generated bindings link at build time.
    ///
    /// scriptc's `--ffi` manifest names them; `load()` fills this with the
    /// artifact it just compiled.
    pub native_libraries: Vec<PathBuf>,
    /// Length-delimited spans the scriptc bindings should type as UTF-8
    /// `string` instead of raw `bytes`, written as `"<function>:<parameter>"`.
    ///
    /// A `const uint8_t *` + `size_t` pair carries no text/bytes distinction,
    /// so `bytes` is the default: a mistaken JavaScript string then fails
    /// type-checking instead of being mangled by UTF-8 decoding.
    pub scriptc_string_spans: Vec<String>,
    /// File name for the scriptc `--ffi` manifest, written beside the bindings.
    ///
    /// Defaults to `<header stem>.ffi.json`. Callers that own the file layout
    /// (rig names one manifest per dependency) set it so the generated module's
    /// build hint names the file that actually exists.
    pub scriptc_manifest_name: Option<String>,
}

impl ImportOptions {
    pub fn allowlist_functions<I, S>(mut self, functions: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.allowlist_functions = functions.into_iter().map(Into::into).collect();
        self
    }

    pub fn native_libraries<I, P>(mut self, libraries: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        self.native_libraries = libraries
            .into_iter()
            .map(|path| path.as_ref().to_path_buf())
            .collect();
        self
    }

    pub fn scriptc_string_spans<I, S>(mut self, spans: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.scriptc_string_spans = spans.into_iter().map(Into::into).collect();
        self
    }

    pub fn scriptc_manifest_name<S: Into<String>>(mut self, name: S) -> Self {
        self.scriptc_manifest_name = Some(name.into());
        self
    }

    fn scriptc_span_is_text(&self, function: &str, parameter: &str) -> bool {
        let wanted = format!("{function}:{parameter}");
        self.scriptc_string_spans.iter().any(|span| span == &wanted)
    }
}

/// An extra file a generated import needs beside its main source file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeneratedCompanion {
    /// File name to write beside the bindings.
    pub name: String,
    /// File contents.
    pub contents: String,
}

#[derive(Clone, Debug)]
pub struct GeneratedImport {
    pub code: String,
    pub language: Language,
    pub source_header: PathBuf,
    pub warnings: Vec<String>,
    /// Extra files the bindings need beside their main source file — scriptc's
    /// `--ffi` manifest is one.
    pub companions: Vec<GeneratedCompanion>,
}

pub fn generate_imports(
    header: &Path,
    language: Language,
    options: &ImportOptions,
) -> Result<GeneratedImport, String> {
    let content = read_header_content(header)?;
    generate_imports_from_parsed(header, language, &parse_c_header(&content), options)
}

pub fn generate_imports_from_parsed(
    header: &Path,
    language: Language,
    parsed: &ParsedHeader,
    options: &ImportOptions,
) -> Result<GeneratedImport, String> {
    if language == Language::ScriptC {
        return generate_scriptc_imports(header, parsed, options);
    }

    if language == Language::Rust {
        // Rust consumer bindings ARE the full FFI bindings: types (enums, opaque handles, structs)
        // plus the extern block. Reuse the one binding generator so no function is dropped for
        // referencing a header-declared type and the output is self-contained and compiles.
        let binding = crate::bindings::generate_bindings_from_parsed(
            header,
            parsed,
            &crate::bindings::BindingOptions {
                allowlist_functions: options.allowlist_functions.clone(),
                ..Default::default()
            },
        );
        return Ok(GeneratedImport {
            code: binding.code,
            language,
            source_header: header.to_path_buf(),
            warnings: binding.warnings,
            companions: Vec::new(),
        });
    }

    let declared_types = declared_import_types(parsed, language);
    let mut functions = Vec::new();
    let mut warnings = Vec::new();

    if maps_declared_types(language) {
        let names = parsed
            .structs
            .iter()
            .chain(parsed.unions.iter())
            .map(|definition| &definition.name)
            .chain(parsed.enums.iter().map(|enumeration| &enumeration.name))
            .chain(parsed.typedefs.iter().map(|alias| &alias.name))
            .collect::<HashSet<_>>();
        for name in names {
            if !declared_types.contains(name) {
                warnings.push(format!("Skipped type {name} because its declaration is not supported for generated imports"));
            }
        }
    }

    for function in &parsed.functions {
        let function = function.clone();
        if !options.allowlist_functions.is_empty()
            && !options
                .allowlist_functions
                .iter()
                .any(|name| name == &function.name)
        {
            continue;
        }
        // Languages that bind by including the C header (or aliasing the @cImport symbol) let the
        // C toolchain resolve every type, so the scalar-only `supports_import` gate would only
        // drop functions it has no reason to. Gate only the languages that re-map signatures
        // with the declarations emitted below.
        if binds_against_c_header(language) || supports_import(&function, language, &declared_types)
        {
            functions.push(function);
        } else {
            warnings.push(format!(
                "Skipped function {} because its signature is not supported for generated imports",
                function.name
            ));
        }
    }

    let code = render_imports(language, header, &functions, parsed)?;
    Ok(GeneratedImport {
        code,
        language,
        source_header: header.to_path_buf(),
        warnings,
        companions: Vec::new(),
    })
}

/// True for consumer languages whose generated bindings include the original C header (or alias the
/// `@cImport` symbols), so the C compiler resolves every declared type and no function need be
/// dropped for using one. Languages that instead re-map signatures into their own type system are
/// not listed here, because an un-emitted declared type would dangle.
fn binds_against_c_header(language: Language) -> bool {
    matches!(language, Language::Zig | Language::C | Language::Cpp)
}

fn maps_declared_types(language: Language) -> bool {
    matches!(
        language,
        Language::Nim | Language::CSharp | Language::D | Language::Odin | Language::V
    )
}

fn unqualified_type(c_type: &str) -> String {
    c_type
        .replace('*', " * ")
        .split_whitespace()
        .filter(|word| {
            !matches!(
                *word,
                "const" | "volatile" | "restrict" | "__restrict" | "__restrict__"
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn supports_declared_type(c_type: &str, declared: &HashSet<String>) -> bool {
    let normalized = unqualified_type(c_type);
    if is_c_abi_safe_type(&normalized) {
        return true;
    }
    if let Some(inner) = normalized.strip_suffix('*') {
        return supports_declared_type(inner.trim(), declared);
    }
    if let Some((base, dims)) = split_array_dims(&normalized) {
        return dims.iter().all(|dim| dim.parse::<usize>().is_ok())
            && supports_declared_type(&base, declared);
    }
    declared.contains(&normalized)
}

/// C# fixed buffers accept primitive elements only. Reject other arrays instead of
/// emitting uncompilable fixed aggregate/pointer buffers or changing an inline array's ABI.
fn supports_mapping_field(language: Language, ty: &str, parsed: &ParsedHeader) -> bool {
    if language != Language::CSharp {
        return true;
    }
    let resolved = resolve_consumer_alias(ty, parsed, 0);
    let Some((base, _)) = split_array_dims(&resolved) else {
        return true;
    };
    matches!(
        mapped_type(language, &base).as_str(),
        "bool"
            | "byte"
            | "sbyte"
            | "short"
            | "ushort"
            | "int"
            | "uint"
            | "long"
            | "ulong"
            | "char"
            | "float"
            | "double"
    )
}

/// Only actual definitions and resolvable aliases are accepted. An unknown typedef or a
/// function-pointer alias cannot acquire an invented pointer mapping and slip through the gate.
fn declared_import_types(parsed: &ParsedHeader, language: Language) -> HashSet<String> {
    let mut declared = HashSet::new();
    for (kind, names) in [
        (
            "struct",
            parsed.structs.iter().map(|s| &s.name).collect::<Vec<_>>(),
        ),
        ("union", parsed.unions.iter().map(|s| &s.name).collect()),
        ("enum", parsed.enums.iter().map(|s| &s.name).collect()),
    ] {
        for name in names {
            if is_c_identifier(name) {
                declared.insert(name.clone());
                declared.insert(format!("{kind} {name}"));
            }
        }
    }
    for alias in &parsed.typedefs {
        if alias.rust_override.is_none() && is_c_identifier(&alias.name) {
            for kind in ["struct ", "union "] {
                if let Some(tag) = alias.target.strip_prefix(kind) {
                    if is_c_identifier(tag) {
                        declared.insert(alias.target.clone());
                        declared.insert(tag.to_string());
                    }
                }
            }
        }
    }
    loop {
        let before = declared.len();
        for alias in &parsed.typedefs {
            if alias.rust_override.is_none()
                && is_c_identifier(&alias.name)
                && supports_declared_type(&alias.target, &declared)
            {
                declared.insert(alias.name.clone());
            }
        }
        if before == declared.len() {
            break;
        }
    }
    // Prune unsupported definitions and every alias depending on them. Definitions must be
    // renderable too: merely recognizing a name cannot justify a dangling field type.
    loop {
        let before = declared.len();
        for (kind, definitions) in [("struct", &parsed.structs), ("union", &parsed.unions)] {
            for definition in definitions {
                if !definition.bitfields.is_empty()
                    || (language == Language::V && definition.name.contains("__anon_"))
                    || !definition.fields.iter().all(|(ty, name)| {
                        is_c_identifier(name)
                            && supports_declared_type(ty, &declared)
                            && supports_mapping_field(language, ty, parsed)
                    })
                {
                    declared.remove(&definition.name);
                    declared.remove(&format!("{kind} {}", definition.name));
                }
            }
        }
        for enumeration in &parsed.enums {
            if (language == Language::V && enumeration.name.contains("__anon_"))
                || !enumeration.variants.iter().all(|(name, value)| {
                    is_c_identifier(name)
                        && value
                            .as_ref()
                            .is_none_or(|value| parse_enum_discriminant(value).is_some())
                })
            {
                declared.remove(&enumeration.name);
                declared.remove(&format!("enum {}", enumeration.name));
            }
        }
        for alias in &parsed.typedefs {
            if !supports_declared_type(&alias.target, &declared) {
                declared.remove(&alias.name);
            }
        }
        if before == declared.len() {
            break;
        }
    }
    declared
}

fn supports_import(function: &FunctionDef, language: Language, declared: &HashSet<String>) -> bool {
    let supports = |ty: &str| {
        if maps_declared_types(language) {
            supports_declared_type(ty, declared)
        } else {
            is_c_abi_safe_type(ty)
        }
    };
    is_c_identifier(&function.name)
        && supports(&function.return_type)
        && function
            .params
            .iter()
            .all(|(ty, name)| is_c_identifier(name) && supports(ty))
}

/// C# cannot export aliases, and V's C declarations use the header's tag names.
/// Expand aliases in those signatures/fields instead of leaving unresolved target types.
fn resolve_consumer_alias(c_type: &str, parsed: &ParsedHeader, depth: usize) -> String {
    if depth > parsed.typedefs.len() {
        return c_type.to_string();
    }
    let normalized = unqualified_type(c_type);
    if let Some((base, dims)) = split_array_dims(&normalized) {
        let mut resolved = resolve_consumer_alias(&base, parsed, depth + 1);
        for dim in dims {
            resolved.push_str(&format!("[{dim}]"));
        }
        return resolved;
    }
    if let Some(inner) = normalized.strip_suffix('*') {
        return format!(
            "{} *",
            resolve_consumer_alias(inner.trim(), parsed, depth + 1)
        );
    }
    if let Some(alias) = parsed
        .typedefs
        .iter()
        .find(|alias| alias.name == normalized)
    {
        return resolve_consumer_alias(&alias.target, parsed, depth + 1);
    }
    normalized
}

fn expand_consumer_aliases(parsed: &ParsedHeader) -> ParsedHeader {
    let mut expanded = parsed.clone();
    for function in &mut expanded.functions {
        function.return_type = resolve_consumer_alias(&function.return_type, parsed, 0);
        for (ty, _) in &mut function.params {
            *ty = resolve_consumer_alias(ty, parsed, 0);
        }
    }
    for definition in expanded
        .structs
        .iter_mut()
        .chain(expanded.unions.iter_mut())
    {
        for (ty, _) in &mut definition.fields {
            *ty = resolve_consumer_alias(ty, parsed, 0);
        }
    }
    expanded
}

fn render_imports(
    language: Language,
    header: &Path,
    functions: &[FunctionDef],
    parsed: &ParsedHeader,
) -> Result<String, String> {
    let expanded;
    let expanded_functions;
    let (parsed, functions) = if matches!(language, Language::CSharp | Language::V) {
        expanded = expand_consumer_aliases(parsed);
        expanded_functions = functions
            .iter()
            .map(|function| {
                let mut function = function.clone();
                function.return_type = resolve_consumer_alias(&function.return_type, parsed, 0);
                for (ty, _) in &mut function.params {
                    *ty = resolve_consumer_alias(ty, parsed, 0);
                }
                function
            })
            .collect::<Vec<_>>();
        (&expanded, expanded_functions.as_slice())
    } else {
        (parsed, functions)
    };
    match language {
        Language::Rust => unreachable!("Rust imports are generated via the bindings path"),
        Language::Zig => Ok(render_zig(header, functions)),
        Language::C => Ok(render_c(header, functions)),
        Language::Cpp => Ok(render_cpp(header, functions)),
        Language::CSharp => Ok(render_csharp(header, functions, parsed)),
        Language::D => Ok(render_d(functions, parsed)),
        Language::Nim => Ok(render_nim(functions, parsed)),
        Language::Odin => Ok(render_odin(header, functions, parsed)),
        Language::Hare => Ok(render_hare(functions)),
        Language::V => Ok(render_v(header, functions, parsed)),
        Language::ScriptC => {
            Err("scriptc bindings are generated with their --ffi manifest".to_string())
        }
    }
}

/// One function bound through scriptc's outbound FFI.
struct ScriptcFunction {
    name: String,
    declaration: String,
    params: Vec<&'static str>,
    returns: &'static str,
}

/// scriptc consumers: TypeScript declarations plus the `--ffi` manifest that
/// binds them to native symbols.
///
/// scriptc reaches C through a JSON manifest rather than a generated wrapper,
/// so these bindings are two files: a declaration module the host imports
///
/// ```ts
/// import { c_add } from "./bindings";
/// ```
///
/// and a manifest naming the archive to link, which `load()` fills in from the
/// artifact it just compiled.
fn generate_scriptc_imports(
    header: &Path,
    parsed: &ParsedHeader,
    options: &ImportOptions,
) -> Result<GeneratedImport, String> {
    let mut bindings = Vec::new();
    let mut warnings = Vec::new();

    for function in &parsed.functions {
        if !options.allowlist_functions.is_empty()
            && !options
                .allowlist_functions
                .iter()
                .any(|name| name == &function.name)
        {
            continue;
        }
        match scriptc_binding(function, options) {
            Ok(binding) => bindings.push(binding),
            Err(reason) => warnings.push(format!(
                "Skipped function {} for scriptc: {reason}",
                function.name
            )),
        }
    }

    if options.native_libraries.is_empty() {
        warnings.push(
            "no native library configured: add the archive to the manifest's `libraries` array"
                .to_string(),
        );
    }

    let manifest_name = options
        .scriptc_manifest_name
        .clone()
        .unwrap_or_else(|| format!("{}.ffi.json", header_stem(header)));
    let mut code = String::new();
    code.push_str("// Generated by equilibrium-ffi for a scriptc host.\n");
    code.push_str(&format!(
        "// Bound to native symbols by {manifest_name}; build with\n"
    ));
    code.push_str(&format!(
        "// `scriptc build <entry>.ts --ffi {manifest_name} -o <app>`.\n"
    ));
    code.push_str("//\n");
    code.push_str("// Length-delimited spans arrive as Uint8Array; encode text with\n");
    code.push_str("// TextEncoder, or declare the span as `string` through\n");
    code.push_str("// ImportOptions::scriptc_string_spans.\n\n");
    if bindings.is_empty() {
        code.push_str("// No C ABI safe functions were found in the header.\n");
    }
    for binding in &bindings {
        code.push_str(&binding.declaration);
        code.push('\n');
    }

    Ok(GeneratedImport {
        code,
        language: Language::ScriptC,
        source_header: header.to_path_buf(),
        warnings,
        companions: vec![GeneratedCompanion {
            name: manifest_name,
            contents: scriptc_manifest(&bindings, options),
        }],
    })
}

/// Derive one function's TypeScript declaration and manifest classes.
fn scriptc_binding(
    function: &FunctionDef,
    options: &ImportOptions,
) -> Result<ScriptcFunction, String> {
    if !is_c_identifier(&function.name) {
        return Err("its name is not a C identifier".to_string());
    }
    let returns = scriptc_class(&function.return_type, true)?;

    let mut params = Vec::new();
    let mut parameters = Vec::new();
    let mut index = 0;
    while index < function.params.len() {
        let (c_type, name) = &function.params[index];
        // `const uint8_t *` + `size_t` is one length-delimited span, which is a
        // single scriptc parameter.
        if let Some((next_type, _)) = function.params.get(index + 1) {
            if is_byte_pointer(c_type) && is_size_type(next_type) {
                let class = if options.scriptc_span_is_text(&function.name, name) {
                    "string"
                } else {
                    "bytes"
                };
                params.push(class);
                parameters.push(format!(
                    "{}: {}",
                    scriptc_parameter(name, index),
                    scriptc_ts_type(class)
                ));
                index += 2;
                continue;
            }
        }
        let class = scriptc_class(c_type, false)?;
        params.push(class);
        parameters.push(format!(
            "{}: {}",
            scriptc_parameter(name, index),
            scriptc_ts_type(class)
        ));
        index += 1;
    }

    Ok(ScriptcFunction {
        declaration: format!(
            "export declare function {}({}): {};",
            function.name,
            parameters.join(", "),
            scriptc_ts_type(returns)
        ),
        name: function.name.clone(),
        params,
        returns,
    })
}

/// The TypeScript type a scriptc class is declared with.
fn scriptc_ts_type(class: &str) -> &'static str {
    match class {
        "f64" | "u8" | "u32" | "i32" => "number",
        "bool" => "boolean",
        "string" => "string",
        "bytes" => "Uint8Array",
        _ => "void",
    }
}

/// The scriptc class for a C type, or why it has none.
fn scriptc_class(c_type: &str, is_return: bool) -> Result<&'static str, String> {
    let normalized = normalize_c_type(c_type);
    let class = match normalized.as_str() {
        "void" if is_return => "void",
        "double" => "f64",
        "bool" | "_Bool" => "bool",
        "uint8_t" | "unsigned char" => "u8",
        "uint32_t" | "unsigned int" | "unsigned" => "u32",
        "int" | "int32_t" => "i32",
        other => {
            return Err(match other {
                "void" => "a parameter cannot be void".to_string(),
                "char *" | "const char *" => format!(
                    "`{other}` has no scriptc outbound class (its `cstring` is callback-only); \
                     declare a `const uint8_t *` + `size_t` span instead"
                ),
                "float" => "`float` has no scriptc outbound class (use `double`)".to_string(),
                "int64_t" | "uint64_t" | "long long" | "unsigned long long" => format!(
                    "`{other}` has no scriptc outbound class (outbound integers stop at `i32`)"
                ),
                _ => format!("`{other}` has no scriptc outbound class"),
            })
        }
    };
    Ok(class)
}

/// The JSON manifest that binds the declarations to native symbols.
fn scriptc_manifest(bindings: &[ScriptcFunction], options: &ImportOptions) -> String {
    let mut json = String::new();
    json.push_str("{\n");
    json.push_str("  \"ffi_format\": 1,\n");
    json.push_str("  \"functions\": [\n");
    for (index, binding) in bindings.iter().enumerate() {
        let params: Vec<String> = binding
            .params
            .iter()
            .map(|class| format!("\"{class}\""))
            .collect();
        json.push_str(&format!(
            "    {{ \"name\": {}, \"symbol\": {}, \"params\": [{}], \"returns\": {} }}{}\n",
            crate::scriptc::json_string(&binding.name),
            crate::scriptc::json_string(&binding.name),
            params.join(", "),
            crate::scriptc::json_string(binding.returns),
            if index + 1 == bindings.len() { "" } else { "," },
        ));
    }
    json.push_str("  ],\n");
    let libraries: Vec<String> = options
        .native_libraries
        .iter()
        .map(|path| crate::scriptc::json_string(&manifest_library_path(path)))
        .collect();
    json.push_str(&format!("  \"libraries\": [{}],\n", libraries.join(", ")));
    json.push_str("  \"system_libraries\": []\n}\n");
    json
}

/// Manifest paths resolve from the manifest's directory, so a relative library
/// path is anchored to the directory that generated it.
fn manifest_library_path(path: &Path) -> String {
    // A rooted path (`/build/libc.a`) already says where it lives; joining it
    // onto the current directory would splice a Windows drive letter in front.
    if path.is_absolute() || path.has_root() {
        return path.display().to_string();
    }
    match std::env::current_dir() {
        Ok(dir) => dir.join(path).display().to_string(),
        Err(_) => path.display().to_string(),
    }
}

/// A TypeScript parameter name for a C parameter.
fn scriptc_parameter(name: &str, index: usize) -> String {
    if is_c_identifier(name) && !is_typescript_reserved(name) {
        name.to_string()
    } else {
        format!("arg{index}")
    }
}

fn is_typescript_reserved(name: &str) -> bool {
    matches!(
        name,
        "await"
            | "break"
            | "case"
            | "catch"
            | "class"
            | "const"
            | "continue"
            | "debugger"
            | "default"
            | "delete"
            | "do"
            | "else"
            | "enum"
            | "export"
            | "extends"
            | "false"
            | "finally"
            | "for"
            | "function"
            | "if"
            | "import"
            | "in"
            | "instanceof"
            | "new"
            | "null"
            | "return"
            | "super"
            | "switch"
            | "this"
            | "throw"
            | "true"
            | "try"
            | "typeof"
            | "var"
            | "void"
            | "while"
            | "with"
            | "yield"
    )
}

fn normalize_c_type(c_type: &str) -> String {
    c_type.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Whether the C type is a byte pointer (`const uint8_t *`, `unsigned char *`).
fn is_byte_pointer(c_type: &str) -> bool {
    matches!(
        normalize_c_type(c_type).as_str(),
        "const uint8_t *" | "uint8_t *" | "const unsigned char *" | "unsigned char *"
    )
}

fn is_size_type(c_type: &str) -> bool {
    normalize_c_type(c_type) == "size_t"
}

fn render_zig(header: &Path, functions: &[FunctionDef]) -> String {
    let mut code = format!(
        "const c = @cImport({{\n    @cInclude(\"{}\");\n}});\n\n",
        header
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("equilibrium.h")
    );
    for function in functions {
        code.push_str("pub const ");
        code.push_str(&function.name);
        code.push_str(" = c.");
        code.push_str(&function.name);
        code.push_str(";\n");
    }
    code
}

fn render_c(header: &Path, functions: &[FunctionDef]) -> String {
    let stem = header_stem(header);
    let mut code = format!(
        "#include \"{}\"\n\n",
        header
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("equilibrium.h")
    );
    for function in functions {
        code.push_str(&function.return_type);
        code.push(' ');
        code.push_str("eq_");
        code.push_str(&stem);
        code.push('_');
        code.push_str(&function.name);
        code.push('(');
        code.push_str(&render_c_params(function));
        code.push_str(") {\n    ");
        if function.return_type.trim() != "void" {
            code.push_str("return ");
        }
        code.push_str(&function.name);
        code.push('(');
        code.push_str(&render_arg_names(function));
        code.push_str(");\n}\n\n");
    }
    code
}

fn render_cpp(header: &Path, functions: &[FunctionDef]) -> String {
    let stem = header_stem(header);
    let mut code = format!(
        "#include \"{}\"\n\nextern \"C\" {{\n",
        header
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("equilibrium.h")
    );
    for function in functions {
        code.push_str(&function.return_type);
        code.push(' ');
        code.push_str("eq_");
        code.push_str(&stem);
        code.push('_');
        code.push_str(&function.name);
        code.push('(');
        code.push_str(&render_c_params(function));
        code.push_str(") {\n    ");
        if function.return_type.trim() != "void" {
            code.push_str("return ");
        }
        code.push_str(&function.name);
        code.push('(');
        code.push_str(&render_arg_names(function));
        code.push_str(");\n}\n");
    }
    code.push_str("}\n");
    code
}

fn render_csharp(header: &Path, functions: &[FunctionDef], parsed: &ParsedHeader) -> String {
    let library_name = header_stem(header);
    let mut code = String::from(
        "using System;\nusing System.Runtime.InteropServices;\n\npublic static unsafe class EquilibriumImports\n{\n",
    );
    code.push_str(&render_declared_types(Language::CSharp, parsed));
    for function in functions {
        code.push_str("    [DllImport(\"");
        code.push_str(&library_name);
        code.push_str("\")]\n    public static extern ");
        code.push_str(&csharp_type(&function.return_type));
        code.push(' ');
        code.push_str(&function.name);
        code.push('(');
        code.push_str(&render_csharp_params(function));
        code.push_str(");\n");
    }
    code.push_str("}\n");
    code
}

fn render_d(functions: &[FunctionDef], parsed: &ParsedHeader) -> String {
    let mut code = String::from("import core.stdc.stddef : size_t, ptrdiff_t;\nextern(C) {\n");
    code.push_str(&render_declared_types(Language::D, parsed));
    for function in functions {
        code.push_str("    ");
        code.push_str(&d_type(&function.return_type));
        code.push(' ');
        code.push_str(&function.name);
        code.push('(');
        code.push_str(&render_d_params(function));
        code.push_str(");\n");
    }
    code.push_str("}\n");
    code
}

fn render_nim(functions: &[FunctionDef], parsed: &ParsedHeader) -> String {
    let mut code = String::new();
    code.push_str(&render_declared_types(Language::Nim, parsed));
    for function in functions {
        code.push_str("proc ");
        code.push_str(&function.name);
        code.push('(');
        code.push_str(&render_nim_params(function));
        code.push_str("): ");
        code.push_str(&nim_type(&function.return_type));
        code.push_str(" {.importc: \"");
        code.push_str(&function.name);
        code.push_str("\", cdecl.}\n");
    }
    code
}

fn render_odin(header: &Path, functions: &[FunctionDef], parsed: &ParsedHeader) -> String {
    let mut code = format!(
        "package bindings\nimport c \"core:c\"\nforeign import eq \"{}\"\n\n",
        header_stem(header)
    );
    code.push_str(&render_declared_types(Language::Odin, parsed));
    code.push_str("foreign eq {\n");
    for function in functions {
        code.push_str(&function.name);
        code.push_str(" :: proc(");
        code.push_str(&render_odin_params(function));
        code.push(')');
        let return_type = odin_type(&function.return_type);
        if return_type != "void" {
            code.push_str(" -> ");
            code.push_str(&return_type);
        }
        code.push_str(" ---\n");
    }
    code.push_str("}\n");
    code
}

fn render_hare(functions: &[FunctionDef]) -> String {
    let mut code = String::new();
    for function in functions {
        code.push_str("@symbol(\"");
        code.push_str(&function.name);
        code.push_str("\")\nfn ");
        code.push_str(&function.name);
        code.push('(');
        code.push_str(&render_hare_params(function));
        code.push(')');
        let return_type = hare_type(&function.return_type);
        if return_type != "void" {
            code.push(' ');
            code.push_str(return_type);
        }
        code.push_str(";\n");
    }
    code
}

fn render_v(header: &Path, functions: &[FunctionDef], parsed: &ParsedHeader) -> String {
    let include_dir = header
        .parent()
        .and_then(|parent| parent.to_str())
        .unwrap_or(".");
    let mut code = format!(
        "#flag -I {}\n#include \"{}\"\n\n",
        include_dir,
        header
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("equilibrium.h")
    );
    code.push_str(&render_declared_types(Language::V, parsed));
    for function in functions {
        code.push_str("fn C.");
        code.push_str(&function.name);
        code.push('(');
        code.push_str(&render_v_params(function));
        code.push(')');
        let return_type = v_type(&function.return_type);
        if return_type != "void" {
            code.push(' ');
            code.push_str(&return_type);
        }
        code.push('\n');
    }
    code
}

fn render_c_params(function: &FunctionDef) -> String {
    function
        .params
        .iter()
        .map(|(param_type, name)| format!("{param_type} {name}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn render_arg_names(function: &FunctionDef) -> String {
    function
        .params
        .iter()
        .map(|(_, name)| name.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

fn render_csharp_params(function: &FunctionDef) -> String {
    function
        .params
        .iter()
        .map(|(param_type, name)| format!("{} {}", csharp_type(param_type), name))
        .collect::<Vec<_>>()
        .join(", ")
}

fn render_d_params(function: &FunctionDef) -> String {
    function
        .params
        .iter()
        .map(|(param_type, name)| format!("{} {}", d_type(param_type), name))
        .collect::<Vec<_>>()
        .join(", ")
}

fn render_nim_params(function: &FunctionDef) -> String {
    function
        .params
        .iter()
        .map(|(param_type, name)| format!("{name}: {}", nim_type(param_type)))
        .collect::<Vec<_>>()
        .join(", ")
}

fn render_odin_params(function: &FunctionDef) -> String {
    function
        .params
        .iter()
        .map(|(param_type, name)| format!("{name}: {}", odin_type(param_type)))
        .collect::<Vec<_>>()
        .join(", ")
}

fn render_hare_params(function: &FunctionDef) -> String {
    function
        .params
        .iter()
        .map(|(param_type, name)| format!("{name}: {}", hare_type(param_type)))
        .collect::<Vec<_>>()
        .join(", ")
}

fn render_v_params(function: &FunctionDef) -> String {
    function
        .params
        .iter()
        .map(|(param_type, name)| format!("{name} {}", v_type(param_type)))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Map the same type syntax for fields, aliases and signatures; declared names must never
/// silently fall back to a generic pointer (which changes by-value enum/aggregate ABIs).
fn mapped_type(language: Language, c_type: &str) -> String {
    let normalized = unqualified_type(c_type);
    if let Some((base, dims)) = split_array_dims(&normalized) {
        let mut ty = mapped_type(language, &base);
        for dim in dims.iter().rev() {
            ty = match language {
                Language::Nim => format!("array[{dim}, {ty}]"),
                Language::D => format!("{ty}[{dim}]"),
                Language::Odin | Language::V => format!("[{dim}]{ty}"),
                Language::CSharp => format!("{ty}[]"),
                _ => unreachable!(),
            };
        }
        return ty;
    }
    if let Some(inner) = normalized.strip_suffix('*') {
        let inner = inner.trim();
        let ty = mapped_type(language, inner);
        return match language {
            Language::CSharp => "IntPtr".into(),
            Language::D if c_type.trim().starts_with("const ") => format!("const({ty})*"),
            Language::D => format!("{ty}*"),
            Language::Nim if inner == "void" => "pointer".into(),
            Language::Nim if inner == "char" => "cstring".into(),
            Language::Nim => format!("ptr {ty}"),
            Language::Odin if inner == "void" => "rawptr".into(),
            Language::Odin if inner == "char" => "cstring".into(),
            Language::Odin => format!("^{ty}"),
            Language::V if inner == "void" => "voidptr".into(),
            Language::V => format!("&{ty}"),
            _ => unreachable!(),
        };
    }
    let name = normalized
        .strip_prefix("struct ")
        .or_else(|| normalized.strip_prefix("union "))
        .or_else(|| normalized.strip_prefix("enum "))
        .unwrap_or(&normalized);
    // Each row has C#, D, Nim, Odin and V spellings, respectively. C long follows the
    // target C ABI (64 bits on Unix, 32 bits on Windows), rather than the host language's long.
    let long = if cfg!(windows) { "int" } else { "int64_t" };
    if name == "long" || name == "long int" {
        return mapped_type(language, long);
    }
    if name == "unsigned long" || name == "unsigned long int" {
        return mapped_type(
            language,
            if cfg!(windows) {
                "unsigned int"
            } else {
                "uint64_t"
            },
        );
    }
    let row = match name {
        "void" => ["void", "void", "void", "void", "void"],
        "char" => ["byte", "char", "cchar", "c.char", "char"],
        "signed char" | "int8_t" => ["sbyte", "byte", "int8", "i8", "i8"],
        "unsigned char" | "uchar" | "uint8_t" => ["byte", "ubyte", "uint8", "u8", "u8"],
        "short" | "short int" | "int16_t" => ["short", "short", "cshort", "c.short", "i16"],
        "unsigned short" | "ushort" | "uint16_t" => {
            ["ushort", "ushort", "cushort", "c.ushort", "u16"]
        }
        "int" | "int32_t" => ["int", "int", "cint", "c.int", "int"],
        "unsigned" | "unsigned int" | "uint" | "uint32_t" => {
            ["uint", "uint", "cuint", "c.uint", "u32"]
        }
        "long long" | "int64_t" => ["long", "long", "clonglong", "i64", "i64"],
        "unsigned long long" | "uint64_t" => ["ulong", "ulong", "culonglong", "u64", "u64"],
        "size_t" => ["UIntPtr", "size_t", "csize_t", "uintptr", "usize"],
        "ssize_t" => ["IntPtr", "ptrdiff_t", "int", "int", "isize"],
        "float" => ["float", "float", "cfloat", "f32", "f32"],
        "double" => ["double", "double", "cdouble", "f64", "f64"],
        "bool" | "_Bool" => ["byte", "bool", "bool", "bool", "bool"],
        _ => {
            return if language == Language::V {
                format!("C.{name}")
            } else {
                name.to_string()
            }
        }
    };
    row[match language {
        Language::CSharp => 0,
        Language::D => 1,
        Language::Nim => 2,
        Language::Odin => 3,
        Language::V => 4,
        _ => unreachable!(),
    }]
    .to_string()
}

fn render_declared_types(language: Language, parsed: &ParsedHeader) -> String {
    let mut code = String::new();
    let declared = declared_import_types(parsed, language);
    let mut emitted = HashSet::new();
    for enumeration in &parsed.enums {
        if !declared.contains(&enumeration.name) {
            continue;
        }
        let name = &enumeration.name;
        emitted.insert(name.clone());
        match language {
            Language::CSharp => code.push_str(&format!("    public enum {name} : int {{\n")),
            // Integer aliases allow C enums with negative, duplicate and out-of-order values.
            Language::Nim => code.push_str(&format!("type {name}* = cint\n")),
            Language::D => code.push_str(&format!("alias {name} = int;\n")),
            Language::Odin => code.push_str(&format!("{name} :: c.int\n")),
            Language::V => code.push_str(&format!("enum C.{name} {{\n")),
            _ => unreachable!(),
        }
        let mut previous: Option<String> = None;
        for (variant, explicit) in &enumeration.variants {
            let value = explicit
                .as_ref()
                .and_then(|value| parse_enum_discriminant(value))
                .map(|value| value.to_string())
                .unwrap_or_else(|| {
                    previous
                        .as_ref()
                        .map(|prev| format!("{prev} + 1"))
                        .unwrap_or_else(|| "0".into())
                });
            match language {
                Language::CSharp => code.push_str(&format!("        {variant} = {value},\n")),
                Language::Nim => code.push_str(&format!("const {variant}* = {name}({value})\n")),
                Language::D => code.push_str(&format!("enum {name} {variant} = {value};\n")),
                Language::Odin => code.push_str(&format!("{variant} :: {name}({value})\n")),
                Language::V => code.push_str(&format!(
                    "    {variant_lower} = {value}\n",
                    variant_lower = variant.to_lowercase()
                )),
                _ => unreachable!(),
            }
            previous = Some(if language == Language::V {
                variant.to_lowercase()
            } else {
                variant.clone()
            });
        }
        if language == Language::CSharp {
            code.push_str("    }\n");
        }
        if language == Language::V {
            code.push_str("}\n");
        }
    }
    for (definitions, union) in [(&parsed.structs, false), (&parsed.unions, true)] {
        for definition in definitions {
            if !declared.contains(&definition.name) {
                continue;
            }
            emitted.insert(definition.name.clone());
            render_aggregate(
                &mut code,
                language,
                &definition.name,
                &definition.fields,
                union,
            );
        }
    }
    // Forward tags are opaque objects; only their pointers are meaningful. Share the tag
    // definition among aliases, rather than emitting incompatible placeholder objects.
    for alias in &parsed.typedefs {
        if !declared.contains(&alias.name) {
            continue;
        }
        if let Some(tag) = alias
            .target
            .strip_prefix("struct ")
            .or_else(|| alias.target.strip_prefix("union "))
        {
            if emitted.insert(tag.to_string()) {
                render_aggregate(&mut code, language, tag, &[], false);
            }
        }
    }
    for alias in &parsed.typedefs {
        if !declared.contains(&alias.name) || emitted.contains(&alias.name) {
            continue;
        }
        let target = mapped_type(language, &alias.target);
        match language {
            // C# has no exported typedefs. Expand aliases when mapping signatures and fields
            // (below), while preserving an opaque alias as its own named declaration.
            Language::CSharp => {
                if let Some(tag) = alias
                    .target
                    .strip_prefix("struct ")
                    .or_else(|| alias.target.strip_prefix("union "))
                {
                    let definition = parsed
                        .structs
                        .iter()
                        .chain(parsed.unions.iter())
                        .find(|definition| definition.name == tag);
                    let fields = definition
                        .map(|definition| definition.fields.as_slice())
                        .unwrap_or(&[]);
                    render_aggregate(
                        &mut code,
                        language,
                        &alias.name,
                        fields,
                        alias.target.starts_with("union "),
                    );
                }
            }
            Language::Nim => {
                code.push_str(&format!("type {name}* = {target}\n", name = alias.name))
            }
            Language::D => code.push_str(&format!("alias {name} = {target};\n", name = alias.name)),
            Language::Odin => code.push_str(&format!("{name} :: {target}\n", name = alias.name)),
            Language::V => {}
            _ => unreachable!(),
        }
    }
    code.push('\n');
    if language == Language::Nim {
        return nim_type_block(&code);
    }
    code
}

/// Nim requires mutually-referencing objects and aliases in one type section.
fn nim_type_block(declarations: &str) -> String {
    let mut types = String::from("type\n");
    let mut constants = String::new();
    for line in declarations.lines() {
        if let Some(declaration) = line.strip_prefix("type ") {
            types.push_str(&format!("  {declaration}\n"));
        } else if line.starts_with("const ") {
            constants.push_str(line);
            constants.push('\n');
        } else if !line.is_empty() {
            types.push_str(&format!("  {line}\n"));
        }
    }
    if types == "type\n" {
        types.clear();
    }
    types.push_str(&constants);
    types.push('\n');
    types
}

fn render_aggregate(
    code: &mut String,
    language: Language,
    name: &str,
    fields: &[(String, String)],
    union: bool,
) {
    match language {
        Language::CSharp => code.push_str(&format!(
            "    [StructLayout(LayoutKind.{layout})]\n    public struct {name}\n    {{\n",
            layout = if union { "Explicit" } else { "Sequential" }
        )),
        Language::D => code.push_str(&format!(
            "{kind} {name} {{\n",
            kind = if union { "union" } else { "struct" }
        )),
        Language::Nim => code.push_str(&format!(
            "type {name}* {{.bycopy{union}.}} = object\n",
            union = if union { ", union" } else { "" }
        )),
        Language::Odin => code.push_str(&format!(
            "{name} :: struct {union}{{\n",
            union = if union { "#raw_union " } else { "" }
        )),
        Language::V => code.push_str(&format!(
            "{kind} C.{name} {{\n",
            kind = if union { "union" } else { "struct" }
        )),
        _ => unreachable!(),
    }
    for (ty, field) in fields {
        let mapped = mapped_type(language, ty);
        match language {
            Language::CSharp => {
                if union {
                    code.push_str("        [FieldOffset(0)]\n");
                }
                if let Some((base, dims)) = split_array_dims(ty) {
                    let length = dims
                        .iter()
                        .filter_map(|d| d.parse::<usize>().ok())
                        .product::<usize>();
                    code.push_str(&format!(
                        "        public fixed {} {field}[{length}];\n",
                        mapped_type(language, &base)
                    ));
                } else {
                    code.push_str(&format!("        public {mapped} {field};\n"));
                }
            }
            Language::D => code.push_str(&format!("    {mapped} {field};\n")),
            Language::Nim => code.push_str(&format!("  {field}*: {mapped}\n")),
            Language::Odin => code.push_str(&format!("    {field}: {mapped},\n")),
            Language::V => code.push_str(&format!("    {field} {mapped}\n")),
            _ => unreachable!(),
        }
    }
    if language != Language::Nim {
        code.push_str("}\n");
    }
}

fn csharp_type(c_type: &str) -> String {
    mapped_type(Language::CSharp, c_type)
}

fn d_type(c_type: &str) -> String {
    mapped_type(Language::D, c_type)
}

fn nim_type(c_type: &str) -> String {
    mapped_type(Language::Nim, c_type)
}

fn odin_type(c_type: &str) -> String {
    mapped_type(Language::Odin, c_type)
}

fn hare_type(c_type: &str) -> &'static str {
    match c_type.trim() {
        "void" => "void",
        "int" => "int",
        "const char *" | "char *" | "char*" | "const char*" => "*u8",
        "int *" | "int*" => "*int",
        _ => "*opaque",
    }
}

fn v_type(c_type: &str) -> String {
    mapped_type(Language::V, c_type)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    const HEADER: &str = r#"
double c_scale(double value);
int c_add(int a, int b);
unsigned c_or(unsigned a, unsigned b);
unsigned char c_is_empty(const uint8_t *p, size_t p_len);
bool c_flag(int a);
int c_len(const uint8_t *p, size_t p_len);
void c_reset(void);
int c_label(const char *name);
float c_half(double value);
long long c_big(int a);
int *c_pointer(void);
"#;

    fn generate(options: &ImportOptions) -> GeneratedImport {
        let dir = tempdir().unwrap();
        let header = dir.path().join("c_module.h");
        std::fs::write(&header, HEADER).unwrap();
        generate_imports(&header, Language::ScriptC, options).expect("scriptc imports")
    }

    #[test]
    fn scriptc_manifest_name_is_configurable() {
        let generated =
            generate(&ImportOptions::default().scriptc_manifest_name("crc32fast.ffi.json"));
        assert_eq!(generated.companions[0].name, "crc32fast.ffi.json");
        assert!(generated.code.contains("--ffi crc32fast.ffi.json -o <app>"));
    }

    #[test]
    fn scriptc_bindings_use_the_outbound_classes() {
        let generated = generate(&ImportOptions::default());
        let code = &generated.code;

        assert!(code.contains("export declare function c_scale(value: number): number;"));
        assert!(code.contains("export declare function c_add(a: number, b: number): number;"));
        assert!(code.contains("export declare function c_or(a: number, b: number): number;"));
        assert!(code.contains("export declare function c_reset(): void;"));
        assert!(code.contains("export declare function c_is_empty(p: Uint8Array): number;"));
        assert!(code.contains("export declare function c_flag(a: number): boolean;"));
        assert_eq!(generated.language, Language::ScriptC);

        let manifest = &generated.companions[0];
        assert_eq!(manifest.name, "c_module.ffi.json");
        assert!(manifest.contents.contains(
            "{ \"name\": \"c_add\", \"symbol\": \"c_add\", \"params\": [\"i32\", \"i32\"], \"returns\": \"i32\" }"
        ));
        assert!(manifest.contents.contains(
            "{ \"name\": \"c_scale\", \"symbol\": \"c_scale\", \"params\": [\"f64\"], \"returns\": \"f64\" }"
        ));
        assert!(manifest.contents.contains(
            "{ \"name\": \"c_or\", \"symbol\": \"c_or\", \"params\": [\"u32\", \"u32\"], \"returns\": \"u32\" }"
        ));
        assert!(manifest.contents.contains(
            "{ \"name\": \"c_is_empty\", \"symbol\": \"c_is_empty\", \"params\": [\"bytes\"], \"returns\": \"u8\" }"
        ));
        assert!(manifest.contents.contains(
            "{ \"name\": \"c_flag\", \"symbol\": \"c_flag\", \"params\": [\"i32\"], \"returns\": \"bool\" }"
        ));
        assert!(manifest.contents.contains(
            "{ \"name\": \"c_reset\", \"symbol\": \"c_reset\", \"params\": [], \"returns\": \"void\" }"
        ));
        assert!(manifest.contents.contains("\"ffi_format\": 1"));
    }

    #[test]
    fn scriptc_bindings_fuse_length_delimited_spans() {
        let generated = generate(&ImportOptions::default().scriptc_string_spans(["c_len:p"]));
        assert!(generated
            .code
            .contains("export declare function c_len(p: string): number;"));
        let manifest = &generated.companions[0].contents;
        assert!(manifest.contains(
            "{ \"name\": \"c_len\", \"symbol\": \"c_len\", \"params\": [\"string\"], \"returns\": \"i32\" }"
        ));
        // The other span stays raw bytes, and neither keeps a `size_t`.
        assert!(manifest.contains(
            "{ \"name\": \"c_is_empty\", \"symbol\": \"c_is_empty\", \"params\": [\"bytes\"], \"returns\": \"u8\" }"
        ));
        assert!(!manifest.contains("size_t"));
    }

    #[test]
    fn scriptc_bindings_skip_shapes_without_an_outbound_class() {
        let generated = generate(&ImportOptions::default());
        for skipped in ["c_label", "c_half", "c_big", "c_pointer"] {
            assert!(
                !generated.code.contains(skipped),
                "{skipped} should not be bound"
            );
            assert!(
                generated
                    .warnings
                    .iter()
                    .any(|warning| warning.contains(skipped)),
                "{skipped} should be reported"
            );
        }
        assert!(generated
            .warnings
            .iter()
            .any(|warning| warning.contains("outbound integers stop at `i32`")));
        assert!(generated
            .warnings
            .iter()
            .any(|warning| warning.contains("`cstring` is callback-only")));
    }

    #[test]
    fn scriptc_manifest_names_the_configured_libraries() {
        let generated = generate(&ImportOptions::default());
        assert!(
            generated
                .warnings
                .iter()
                .any(|warning| warning.contains("no native library configured")),
            "a manifest without libraries should be called out"
        );

        let generated =
            generate(&ImportOptions::default().native_libraries(["/build/libc_module.a"]));
        let manifest = &generated.companions[0].contents;
        assert!(manifest.contains("\"libraries\": [\"/build/libc_module.a\"]"));
        assert!(manifest.contains("\"system_libraries\": []"));
        assert!(!generated
            .warnings
            .iter()
            .any(|warning| warning.contains("no native library")));
    }

    #[test]
    fn scriptc_manifest_anchors_relative_libraries() {
        let generated = generate(&ImportOptions::default().native_libraries(["build/libc.a"]));
        let manifest = &generated.companions[0].contents;
        let anchored = std::env::current_dir()
            .unwrap()
            .join("build/libc.a")
            .display()
            .to_string();
        // The manifest is JSON, so compare against its JSON encoding: Windows
        // backslashes arrive escaped.
        let expected = crate::scriptc::json_string(&anchored);
        assert!(
            manifest.contains(&format!("\"libraries\": [{expected}]")),
            "manifest:\n{manifest}"
        );
    }

    #[test]
    fn scriptc_bindings_respect_the_function_allowlist() {
        let generated = generate(&ImportOptions::default().allowlist_functions(["c_add"]));
        assert!(generated.code.contains("c_add"));
        assert!(!generated.code.contains("c_scale"));
        assert!(!generated.companions[0].contents.contains("c_scale"));
    }
}
