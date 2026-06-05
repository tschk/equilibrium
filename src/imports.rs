use std::path::{Path, PathBuf};

use crate::c_header::{header_stem, is_c_abi_safe_type, parse_c_header, FunctionDef};
use crate::detector::Language;

#[derive(Clone, Debug, Default)]
pub struct ImportOptions {
    pub allowlist_functions: Vec<String>,
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
}

#[derive(Clone, Debug)]
pub struct GeneratedImport {
    pub code: String,
    pub language: Language,
    pub source_header: PathBuf,
    pub warnings: Vec<String>,
}

pub fn generate_imports(
    header: &Path,
    language: Language,
    options: &ImportOptions,
) -> Result<GeneratedImport, String> {
    let content =
        std::fs::read_to_string(header).map_err(|e| format!("Failed to read header: {e}"))?;
    let parsed = parse_c_header(&content);
    let mut functions = Vec::new();
    let mut warnings = Vec::new();

    for function in parsed.functions {
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
    })
}

fn supports_import(function: &FunctionDef) -> bool {
    is_c_abi_safe_type(&function.return_type)
        && function
            .params
            .iter()
            .all(|(param_type, _)| is_c_abi_safe_type(param_type))
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
    }
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
