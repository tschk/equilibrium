use std::path::{Path, PathBuf};

use crate::c_header::{
    header_stem, is_c_abi_safe_type, is_c_identifier, parse_c_header, FunctionDef, ParsedHeader,
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

    let mut functions = Vec::new();
    let mut warnings = Vec::new();

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
        if supports_import(&function) {
            functions.push(function);
        } else {
            warnings.push(format!(
                "Skipped function {} because its signature is not supported for generated imports",
                function.name
            ));
        }
    }

    let code = render_imports(language, header, &functions)?;
    Ok(GeneratedImport {
        code,
        language,
        source_header: header.to_path_buf(),
        warnings,
        companions: Vec::new(),
    })
}

fn supports_import(function: &FunctionDef) -> bool {
    is_c_identifier(&function.name)
        && is_c_abi_safe_type(&function.return_type)
        && function
            .params
            .iter()
            .all(|(param_type, name)| is_c_identifier(name) && is_c_abi_safe_type(param_type))
}

fn render_imports(
    language: Language,
    header: &Path,
    functions: &[FunctionDef],
) -> Result<String, String> {
    match language {
        Language::Rust => Ok(render_rust(functions)),
        Language::Zig => Ok(render_zig(header, functions)),
        Language::C => Ok(render_c(header, functions)),
        Language::Cpp => Ok(render_cpp(header, functions)),
        Language::CSharp => Ok(render_csharp(header, functions)),
        Language::D => Ok(render_d(functions)),
        Language::Nim => Ok(render_nim(functions)),
        Language::Odin => Ok(render_odin(header, functions)),
        Language::Hare => Ok(render_hare(functions)),
        Language::V => Ok(render_v(header, functions)),
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
    if path.is_absolute() {
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

fn render_rust(functions: &[FunctionDef]) -> String {
    let mut code = String::from("use std::os::raw::*;\n\nextern \"C\" {\n");
    for function in functions {
        code.push_str("    pub fn ");
        code.push_str(&function.name);
        code.push('(');
        code.push_str(&render_rust_params(function));
        code.push(')');
        let return_type = rust_return_type(&function.return_type);
        if return_type != "()" {
            code.push_str(" -> ");
            code.push_str(&return_type);
        }
        code.push_str(";\n");
    }
    code.push_str("}\n");
    code
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

fn render_csharp(header: &Path, functions: &[FunctionDef]) -> String {
    let library_name = header_stem(header);
    let mut code = String::from(
        "using System;\nusing System.Runtime.InteropServices;\n\npublic static class EquilibriumImports\n{\n",
    );
    for function in functions {
        code.push_str("    [DllImport(\"");
        code.push_str(&library_name);
        code.push_str("\")]\n    public static extern ");
        code.push_str(csharp_type(&function.return_type));
        code.push(' ');
        code.push_str(&function.name);
        code.push('(');
        code.push_str(&render_csharp_params(function));
        code.push_str(");\n");
    }
    code.push_str("}\n");
    code
}

fn render_d(functions: &[FunctionDef]) -> String {
    let mut code = String::from("extern(C) {\n");
    for function in functions {
        code.push_str("    ");
        code.push_str(d_type(&function.return_type));
        code.push(' ');
        code.push_str(&function.name);
        code.push('(');
        code.push_str(&render_d_params(function));
        code.push_str(");\n");
    }
    code.push_str("}\n");
    code
}

fn render_nim(functions: &[FunctionDef]) -> String {
    let mut code = String::new();
    for function in functions {
        code.push_str("proc ");
        code.push_str(&function.name);
        code.push('(');
        code.push_str(&render_nim_params(function));
        code.push_str("): ");
        code.push_str(nim_type(&function.return_type));
        code.push_str(" {.importc: \"");
        code.push_str(&function.name);
        code.push_str("\", cdecl.}\n");
    }
    code
}

fn render_odin(header: &Path, functions: &[FunctionDef]) -> String {
    let mut code = format!("foreign import eq \"{}\"\n\n", header_stem(header));
    for function in functions {
        code.push_str(&function.name);
        code.push_str(" :: proc(");
        code.push_str(&render_odin_params(function));
        code.push(')');
        let return_type = odin_type(&function.return_type);
        if return_type != "void" {
            code.push_str(" -> ");
            code.push_str(return_type);
        }
        code.push_str(" ---\n");
    }
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

fn render_v(header: &Path, functions: &[FunctionDef]) -> String {
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
    for function in functions {
        code.push_str("fn C.");
        code.push_str(&function.name);
        code.push('(');
        code.push_str(&render_v_params(function));
        code.push(')');
        let return_type = v_type(&function.return_type);
        if return_type != "void" {
            code.push(' ');
            code.push_str(return_type);
        }
        code.push('\n');
    }
    code
}

fn render_rust_params(function: &FunctionDef) -> String {
    function
        .params
        .iter()
        .map(|(param_type, name)| format!("{name}: {}", rust_return_type(param_type)))
        .collect::<Vec<_>>()
        .join(", ")
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

fn rust_return_type(c_type: &str) -> String {
    crate::c_header::c_type_to_rust(c_type)
}

fn csharp_type(c_type: &str) -> &'static str {
    match c_type.trim() {
        "void" => "void",
        "int" => "int",
        "const char *" | "char *" | "char*" | "const char*" => "IntPtr",
        "int *" | "int*" => "IntPtr",
        _ => "IntPtr",
    }
}

fn d_type(c_type: &str) -> &'static str {
    match c_type.trim() {
        "void" => "void",
        "int" => "int",
        "const char *" | "char *" | "char*" | "const char*" => "const(char)*",
        "int *" | "int*" => "int*",
        _ => "void*",
    }
}

fn nim_type(c_type: &str) -> &'static str {
    match c_type.trim() {
        "void" => "void",
        "int" => "cint",
        "const char *" | "char *" | "char*" | "const char*" => "cstring",
        "int *" | "int*" => "ptr cint",
        _ => "pointer",
    }
}

fn odin_type(c_type: &str) -> &'static str {
    match c_type.trim() {
        "void" => "void",
        "int" => "c.int",
        "const char *" | "char *" | "char*" | "const char*" => "cstring",
        "int *" | "int*" => "^c.int",
        _ => "rawptr",
    }
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

fn v_type(c_type: &str) -> &'static str {
    match c_type.trim() {
        "void" => "void",
        "int" => "int",
        "const char *" | "char *" | "char*" | "const char*" => "&char",
        "int *" | "int*" => "&int",
        _ => "voidptr",
    }
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
        let expected = std::env::current_dir()
            .unwrap()
            .join("build/libc.a")
            .display()
            .to_string();
        assert!(
            manifest.contains(&format!("\"libraries\": [\"{expected}\"]")),
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
