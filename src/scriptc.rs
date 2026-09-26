//! scriptc (TypeScript → native) library-mode support.
//!
//! scriptc's only host-callable C ABI surface is *library mode*: it compiles a
//! single entry module against a JSON *library profile* that maps every
//! TypeScript export to a C symbol plus marshalling classes, and produces a
//! self-contained static archive. scriptc emits no header, so equilibrium
//! derives both the profile and a matching C header from the entry module's
//! `export function` declarations before invoking `scriptc build --lib`.
//!
//! Type annotations alone give the common classes (`number` → `f64`,
//! `boolean` → `bool`, `string` → `string`, `Uint8Array` → `bytes`, `void`),
//! so the `[target.<name>]` table in `equilibrium.toml` can refine them:
//!
//! ```toml
//! [target.math]
//! language = "scriptc"
//! sources = ["native/math.ts"]
//! emission = "c"                       # "llvm" (default) or "c"
//!
//! [target.math.signatures]
//! mix = { params = ["u32", "u32"], returns = "f64" }
//! ```
//!
//! The overrides are validated against the TypeScript annotations, so a class
//! that cannot describe the annotated value is refused instead of silently
//! changing the ABI.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config;
use crate::detector::{find_tool, Language};

/// The declaration keyword that marks a TypeScript export as part of the ABI.
const EXPORT_KEYWORD: &str = "export function";

/// Guard against a stray keyword in a comment swallowing the rest of a file.
const MAX_DECLARATION_BYTES: usize = 8 * 1024;

/// Default profile emission: scriptc's production LLVM lane.
const DEFAULT_EMISSION: &str = "llvm";

/// Marshalling classes scriptc library profiles accept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScriptcClass {
    /// TypeScript `number`.
    F64,
    /// TypeScript `boolean`.
    Bool,
    /// TypeScript `number`, inbound plumbing only.
    U8,
    /// TypeScript `number`, inbound plumbing only.
    U32,
    /// TypeScript `number`, inbound plumbing only.
    I32,
    /// TypeScript `number`, whole-value inbound and outbound integers.
    I64,
    /// TypeScript `number`, whole-value inbound and outbound integers.
    U64,
    /// TypeScript `string`.
    Str,
    /// TypeScript `Uint8Array`.
    Bytes,
    /// No value: TypeScript `void`, or an omitted return annotation.
    Void,
}

impl ScriptcClass {
    /// Parse the class name scriptc profiles use.
    fn from_name(name: &str) -> Option<Self> {
        match name.trim() {
            "f64" => Some(ScriptcClass::F64),
            "bool" => Some(ScriptcClass::Bool),
            "u8" => Some(ScriptcClass::U8),
            "u32" => Some(ScriptcClass::U32),
            "i32" => Some(ScriptcClass::I32),
            "i64" => Some(ScriptcClass::I64),
            "u64" => Some(ScriptcClass::U64),
            "string" => Some(ScriptcClass::Str),
            "bytes" => Some(ScriptcClass::Bytes),
            "void" => Some(ScriptcClass::Void),
            _ => None,
        }
    }

    /// The name scriptc library profiles use for this class.
    fn profile_name(self) -> &'static str {
        match self {
            ScriptcClass::F64 => "f64",
            ScriptcClass::Bool => "bool",
            ScriptcClass::U8 => "u8",
            ScriptcClass::U32 => "u32",
            ScriptcClass::I32 => "i32",
            ScriptcClass::I64 => "i64",
            ScriptcClass::U64 => "u64",
            ScriptcClass::Str => "string",
            ScriptcClass::Bytes => "bytes",
            ScriptcClass::Void => "void",
        }
    }

    /// Whether the class can appear in parameter position.
    fn is_param(self) -> bool {
        self != ScriptcClass::Void
    }

    /// Whether scriptc accepts the class as a return value. The `u8`/`u32`/
    /// `i32` plumbing classes are inbound only.
    fn is_return(self) -> bool {
        !matches!(
            self,
            ScriptcClass::U8 | ScriptcClass::U32 | ScriptcClass::I32
        )
    }

    /// Whether scriptc returns the value through `(out, out_len)` parameters
    /// instead of as the function result.
    fn is_out_param(self) -> bool {
        matches!(self, ScriptcClass::Str | ScriptcClass::Bytes)
    }

    /// The TypeScript annotations this class can describe.
    fn annotations(self) -> &'static [&'static str] {
        match self {
            ScriptcClass::F64
            | ScriptcClass::U8
            | ScriptcClass::U32
            | ScriptcClass::I32
            | ScriptcClass::I64
            | ScriptcClass::U64 => &["number"],
            ScriptcClass::Bool => &["boolean"],
            ScriptcClass::Str => &["string"],
            ScriptcClass::Bytes => &["Uint8Array"],
            ScriptcClass::Void => &["void"],
        }
    }

    /// The C declaration of one parameter of this class.
    fn c_params(self, name: &str) -> Vec<String> {
        match self {
            ScriptcClass::F64 => vec![format!("double {name}")],
            ScriptcClass::Bool | ScriptcClass::U8 => vec![format!("uint8_t {name}")],
            ScriptcClass::U32 => vec![format!("uint32_t {name}")],
            ScriptcClass::I32 => vec![format!("int32_t {name}")],
            ScriptcClass::I64 => vec![format!("int64_t {name}")],
            ScriptcClass::U64 => vec![format!("uint64_t {name}")],
            ScriptcClass::Str | ScriptcClass::Bytes => vec![
                format!("const uint8_t *{name}_ptr"),
                format!("size_t {name}_len"),
            ],
            ScriptcClass::Void => Vec::new(),
        }
    }

    /// The C return type of a function returning this class.
    fn c_return(self) -> &'static str {
        match self {
            ScriptcClass::F64 => "double",
            ScriptcClass::Bool | ScriptcClass::U8 => "uint8_t",
            ScriptcClass::U32 => "uint32_t",
            ScriptcClass::I32 => "int32_t",
            ScriptcClass::I64 => "int64_t",
            ScriptcClass::U64 => "uint64_t",
            ScriptcClass::Str | ScriptcClass::Bytes => "void",
            ScriptcClass::Void => "void",
        }
    }
}

/// An `export function` declaration found in a module.
#[derive(Clone, Debug)]
pub(crate) struct ScriptcDeclaration {
    /// The TypeScript export name.
    pub(crate) name: String,
    /// The declaration text as written in the source.
    pub(crate) signature: String,
}

/// An exported function with the C ABI shape derived from its annotations.
#[derive(Clone, Debug)]
pub(crate) struct ScriptcExport {
    /// The TypeScript export name.
    pub(crate) name: String,
    /// Parameter names as written, paired with their marshalling class.
    pub(crate) params: Vec<(String, ScriptcClass)>,
    /// The marshalling class of the returned value.
    pub(crate) returns: ScriptcClass,
    /// The annotation each parameter carried, for validating overrides.
    param_annotations: Vec<String>,
    /// The return annotation, for validating overrides.
    return_annotation: String,
}

/// The scriptc settings `equilibrium.toml` supplies for one source.
#[derive(Clone, Debug)]
pub(crate) struct ScriptcSettings {
    /// The profile's `emission` value.
    pub(crate) emission: &'static str,
    signatures: BTreeMap<String, SignatureOverride>,
}

impl Default for ScriptcSettings {
    fn default() -> Self {
        Self {
            emission: DEFAULT_EMISSION,
            signatures: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug)]
struct SignatureOverride {
    params: Option<Vec<ScriptcClass>>,
    returns: Option<ScriptcClass>,
}

/// Read the scriptc settings the target config declares for `source`.
pub(crate) fn target_settings(
    source: &Path,
    config_path: Option<&Path>,
) -> Result<ScriptcSettings, String> {
    let Some(target) = config::target_for(source, config_path, Language::ScriptC)
        .map_err(|error| format!("{}: {}", error.path.display(), error.message))?
    else {
        return Ok(ScriptcSettings::default());
    };

    let emission = match target.emission.as_deref() {
        None => DEFAULT_EMISSION,
        Some("llvm") => "llvm",
        Some("c") => "c",
        Some(other) => {
            return Err(format!(
                "unsupported scriptc emission `{other}` (expected \"llvm\" or \"c\")"
            ))
        }
    };

    let mut signatures = BTreeMap::new();
    for (export, signature) in target.signatures.unwrap_or_default() {
        let params = signature
            .params
            .map(|params| {
                params
                    .iter()
                    .map(|name| {
                        ScriptcClass::from_name(name).ok_or_else(|| {
                            format!(
                                "export {export}: unknown scriptc marshalling class `{name}` \
                                 (expected one of f64, bool, u8, u32, i32, i64, u64, string, bytes)"
                            )
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()?;
        let returns = signature
            .returns
            .map(|name| {
                ScriptcClass::from_name(&name).ok_or_else(|| {
                    format!(
                        "export {export}: unknown scriptc marshalling class `{name}` \
                         (expected one of f64, bool, i64, u64, string, bytes, void)"
                    )
                })
            })
            .transpose()?;
        if let Some(returns) = returns {
            if !returns.is_return() {
                return Err(format!(
                    "export {export}: `{}` is an inbound-only scriptc class and cannot be a return type",
                    returns.profile_name()
                ));
            }
        }
        signatures.insert(export, SignatureOverride { params, returns });
    }

    Ok(ScriptcSettings {
        emission,
        signatures,
    })
}

/// Apply the configured overrides to the exports derived from the source.
pub(crate) fn apply_overrides(
    exports: Vec<ScriptcExport>,
    settings: &ScriptcSettings,
) -> Result<Vec<ScriptcExport>, String> {
    let mut overridden: Vec<ScriptcExport> = Vec::with_capacity(exports.len());
    for mut export in exports {
        if let Some(signature) = settings.signatures.get(&export.name) {
            if let Some(params) = &signature.params {
                if params.len() != export.params.len() {
                    return Err(format!(
                        "export {}: {} scriptc parameters configured for {} TypeScript parameters",
                        export.name,
                        params.len(),
                        export.params.len()
                    ));
                }
                for (index, class) in params.iter().enumerate() {
                    let annotation = export.param_annotations[index].as_str();
                    if !class.annotations().contains(&annotation) {
                        return Err(format!(
                            "export {}: scriptc class `{}` cannot describe the TypeScript parameter `{}: {annotation}`",
                            export.name,
                            class.profile_name(),
                            export.params[index].0
                        ));
                    }
                    export.params[index].1 = *class;
                }
            }
            if let Some(returns) = signature.returns {
                let annotation = export.return_annotation.as_str();
                if !returns.annotations().contains(&annotation) {
                    return Err(format!(
                        "export {}: scriptc class `{}` cannot describe the TypeScript return type `{annotation}`",
                        export.name,
                        returns.profile_name()
                    ));
                }
                export.returns = returns;
            }
        }
        overridden.push(export);
    }

    for configured in settings.signatures.keys() {
        if !overridden.iter().any(|export| &export.name == configured) {
            return Err(format!(
                "signature override for `{configured}` matches no C ABI safe export in the module"
            ));
        }
    }

    Ok(overridden)
}

/// Find every `export function` declaration in a module.
///
/// Declarations whose signature cannot cross the C ABI are still returned, so
/// callers can report them; use [`parse_signature`] to classify each one.
pub(crate) fn scan_declarations(content: &str) -> Vec<ScriptcDeclaration> {
    let mut declarations = Vec::new();
    let mut cursor = 0usize;
    while let Some(offset) = content[cursor..].find(EXPORT_KEYWORD) {
        let start = cursor + offset;
        cursor = start + EXPORT_KEYWORD.len();
        let Some(end) = declaration_end(content, cursor) else {
            continue;
        };
        let signature = content[start..end].trim();
        if let Some(name) = declared_name(signature) {
            declarations.push(ScriptcDeclaration {
                name,
                signature: signature.to_string(),
            });
        }
    }
    declarations
}

/// Index of the `{` or `;` that ends the declaration starting at `from`.
fn declaration_end(content: &str, from: usize) -> Option<usize> {
    let mut depth = 0i32;
    for (index, character) in content[from..].char_indices() {
        if index > MAX_DECLARATION_BYTES {
            return None;
        }
        match character {
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            '{' | ';' if depth <= 0 => return Some(from + index),
            _ => {}
        }
    }
    None
}

/// The exported name at the start of a declaration, if there is one.
fn declared_name(signature: &str) -> Option<String> {
    let rest = signature.strip_prefix(EXPORT_KEYWORD)?.trim_start();
    let end = rest.find(|c: char| !is_ident_char(c)).unwrap_or(rest.len());
    (!rest[..end].is_empty()).then(|| rest[..end].to_string())
}

/// Derive the C ABI shape of one `export function` declaration.
pub(crate) fn parse_signature(signature: &str) -> Result<ScriptcExport, String> {
    let text = signature.trim();
    let rest = text
        .strip_prefix(EXPORT_KEYWORD)
        .ok_or_else(|| "not an exported function declaration".to_string())?
        .trim_start();
    let name_end = rest.find(|c: char| !is_ident_char(c)).unwrap_or(rest.len());
    let name = &rest[..name_end];
    if name.is_empty() {
        return Err("missing function name".to_string());
    }
    let after_name = rest[name_end..].trim_start();
    if after_name.starts_with('<') {
        return Err("generic functions are not C ABI safe".to_string());
    }

    let (params_text, after_params) = parenthesized(after_name)?;
    let tail = after_name[after_params..].trim_start();
    let return_type = tail
        .strip_prefix(':')
        .ok_or_else(|| "missing return type annotation".to_string())?
        .trim();
    let return_type = return_type
        .split(['{', ';'])
        .next()
        .unwrap_or(return_type)
        .trim();

    let mut params = Vec::new();
    let mut param_annotations = Vec::new();
    for param in split_top_level(params_text) {
        let param = param.trim();
        if param.is_empty() {
            continue;
        }
        let (name, class, annotation) = parse_param(param)?;
        params.push((name, class));
        param_annotations.push(annotation);
    }

    Ok(ScriptcExport {
        name: name.to_string(),
        params,
        returns: class_from_ts(return_type)?,
        param_annotations,
        return_annotation: return_type.to_string(),
    })
}

/// The text between the first `(` and its matching `)`, plus the index just
/// past that `)`.
fn parenthesized(text: &str) -> Result<(&str, usize), String> {
    let open = text
        .find('(')
        .ok_or_else(|| "missing parameter list".to_string())?;
    let mut depth = 0i32;
    for (index, character) in text[open..].char_indices() {
        match character {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Ok((&text[open + 1..open + index], open + index + 1));
                }
            }
            _ => {}
        }
    }
    Err("unterminated parameter list".to_string())
}

/// Split a parameter list on the commas that are not nested in brackets.
fn split_top_level(text: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0usize;
    for (index, character) in text.char_indices() {
        match character {
            '(' | '[' | '{' | '<' => depth += 1,
            ')' | ']' | '}' | '>' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(&text[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    parts.push(&text[start..]);
    parts
}

/// Derive one parameter's name, marshalling class, and annotation.
fn parse_param(param: &str) -> Result<(String, ScriptcClass, String), String> {
    let (name, annotation) = param
        .split_once(':')
        .ok_or_else(|| format!("parameter `{param}` has no type annotation"))?;
    let name = name.trim();
    if let Some(rest) = name.strip_prefix("...") {
        return Err(format!("rest parameter `{rest}` is not C ABI safe"));
    }
    if let Some(rest) = name.strip_suffix('?') {
        return Err(format!("optional parameter `{rest}` is not C ABI safe"));
    }
    if name.is_empty() || !name.chars().next().is_some_and(is_ident_start) {
        return Err(format!("parameter `{name}` is not a plain identifier"));
    }
    // A default value (`a: number = 1`) never crosses the ABI.
    let annotation = annotation.split('=').next().unwrap_or(annotation).trim();
    let class = class_from_ts(annotation)?;
    if !class.is_param() {
        return Err(format!("parameter `{name}` cannot be void"));
    }
    Ok((name.to_string(), class, annotation.to_string()))
}

/// Map a TypeScript type annotation to a scriptc marshalling class.
fn class_from_ts(annotation: &str) -> Result<ScriptcClass, String> {
    match annotation.trim() {
        "number" => Ok(ScriptcClass::F64),
        "boolean" => Ok(ScriptcClass::Bool),
        "string" => Ok(ScriptcClass::Str),
        "Uint8Array" => Ok(ScriptcClass::Bytes),
        "void" => Ok(ScriptcClass::Void),
        other => Err(format!("unsupported TypeScript type `{other}`")),
    }
}

fn is_ident_start(character: char) -> bool {
    character.is_ascii_alphabetic() || character == '_' || character == '$'
}

fn is_ident_char(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_' || character == '$'
}

/// The C symbol prefix scriptc requires for every export of a module.
///
/// Every declared symbol must start with the prefix, and the prefix must be a
/// C identifier fragment, so the module stem is sanitised into one.
pub(crate) fn symbol_prefix(stem: &str) -> String {
    let mut prefix: String = stem
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '_' {
                character
            } else {
                '_'
            }
        })
        .collect();
    if prefix.is_empty() {
        prefix.push_str("sc");
    }
    if prefix.starts_with(|c: char| c.is_ascii_digit()) {
        prefix.insert(0, '_');
    }
    prefix.push('_');
    prefix
}

/// The symbol that initialises the archive's runtime state.
pub(crate) fn init_symbol(prefix: &str) -> String {
    format!("{prefix}init")
}

/// The symbol that registers the host's panic sink.
pub(crate) fn sink_symbol(prefix: &str) -> String {
    format!("{prefix}set_panic_sink")
}

/// The symbol that releases the archive's buffered results.
pub(crate) fn collect_symbol(prefix: &str) -> String {
    format!("{prefix}collect")
}

/// Where the generated library profile is written for a given archive path.
pub(crate) fn profile_path(input: &Path, output: &Path) -> PathBuf {
    let stem = input
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("output");
    output.with_file_name(format!("{stem}.profile.json"))
}

/// The library profile that declares `exports` as the module's C ABI.
pub(crate) fn profile_json(
    name: &str,
    prefix: &str,
    entry: &Path,
    exports: &[ScriptcExport],
    emission: &str,
) -> String {
    let mut json = String::new();
    json.push_str("{\n");
    json.push_str("  \"profile_format\": 1,\n");
    json.push_str(&format!("  \"name\": {},\n", json_string(name)));
    json.push_str(&format!(
        "  \"entry\": {},\n",
        json_string(&entry.to_string_lossy())
    ));
    json.push_str(&format!("  \"emission\": {},\n", json_string(emission)));
    json.push_str("  \"abi\": {\n");
    json.push_str(&format!("    \"prefix\": {},\n", json_string(prefix)));
    json.push_str(&format!(
        "    \"init_symbol\": {},\n",
        json_string(&init_symbol(prefix))
    ));
    json.push_str(&format!(
        "    \"sink_register_symbol\": {},\n",
        json_string(&sink_symbol(prefix))
    ));
    json.push_str(&format!(
        "    \"collect_symbol\": {},\n",
        json_string(&collect_symbol(prefix))
    ));
    json.push_str("    \"result_reset_symbol\": null,\n");
    // Give every module a private copy of the scriptc runtime, so several
    // compiled modules can be linked into one process.
    json.push_str("    \"localize_runtime\": true\n");
    json.push_str("  },\n");
    json.push_str("  \"exports\": [\n");
    for (index, export) in exports.iter().enumerate() {
        let params: Vec<String> = export
            .params
            .iter()
            .map(|(_, class)| json_string(class.profile_name()))
            .collect();
        json.push_str(&format!(
            "    {{ \"export\": {}, \"symbol\": {}, \"params\": [{}], \"returns\": {} }}{}\n",
            json_string(&export.name),
            json_string(&format!("{prefix}{}", export.name)),
            params.join(", "),
            json_string(export.returns.profile_name()),
            if index + 1 == exports.len() { "" } else { "," },
        ));
    }
    json.push_str("  ]\n}\n");
    json
}

/// The C header mirroring the generated library profile.
///
/// scriptc emits no header, so binding generation would have nothing to parse
/// without this file. Parameter names follow the TypeScript source, and the
/// `string`/`bytes` results arrive through the `out`/`out_len` parameters that
/// scriptc's wrappers write.
pub(crate) fn header(prefix: &str, module: &str, exports: &[ScriptcExport]) -> String {
    let mut header = String::new();
    header.push_str(&format!(
        "// Generated by equilibrium-ffi for the scriptc library built from {module}.\n"
    ));
    header.push_str(
        "// Do not edit: it mirrors the library profile passed to `scriptc build --lib`.\n",
    );
    header.push_str("//\n");
    header.push_str(&format!(
        "// The archive is self-contained. Call {}() once before the exports below;\n",
        init_symbol(prefix)
    ));
    header.push_str(&format!(
        "// register {}() first to receive trap messages, since an\n",
        sink_symbol(prefix)
    ));
    header
        .push_str("// unregistered trap aborts the process. Buffered results stay owned by the\n");
    header.push_str(&format!(
        "// archive until {}() is called.\n",
        collect_symbol(prefix)
    ));
    header.push_str("//\n");
    header
        .push_str("// void (*)(void *ctx, const uint8_t *msg, size_t msg_len, uint64_t address)\n");
    header.push_str("#include <stddef.h>\n");
    header.push_str("#include <stdint.h>\n");
    header.push('\n');
    header.push_str(&format!("void {}(void);\n", init_symbol(prefix)));
    header.push_str(&format!(
        "void {}(void *fn_ptr, void *ctx);\n",
        sink_symbol(prefix)
    ));
    header.push_str(&format!("void {}(void);\n", collect_symbol(prefix)));
    header.push('\n');

    for export in exports {
        let mut params = Vec::new();
        let mut used = Vec::new();
        for (name, class) in &export.params {
            let name = c_param_name(name, &mut used);
            params.extend(class.c_params(&name));
        }
        if export.returns.is_out_param() {
            params.push("const uint8_t **out".to_string());
            params.push("size_t *out_len".to_string());
        }
        let params = if params.is_empty() {
            "void".to_string()
        } else {
            params.join(", ")
        };
        header.push_str(&format!(
            "{} {}{}({});\n",
            export.returns.c_return(),
            prefix,
            export.name,
            params
        ));
    }
    header
}

/// A C-safe, unique parameter name derived from the TypeScript one.
fn c_param_name(name: &str, used: &mut Vec<String>) -> String {
    let mut sanitised: String = name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '_' {
                character
            } else {
                '_'
            }
        })
        .collect();
    if sanitised.is_empty() {
        sanitised.push('a');
    }
    if sanitised.starts_with(|c: char| c.is_ascii_digit()) {
        sanitised.insert(0, '_');
    }
    if is_c_keyword(&sanitised) {
        sanitised.push('_');
    }
    let mut candidate = sanitised;
    while used.contains(&candidate) {
        candidate.push('_');
    }
    used.push(candidate.clone());
    candidate
}

fn is_c_keyword(name: &str) -> bool {
    matches!(
        name,
        "auto"
            | "break"
            | "case"
            | "char"
            | "const"
            | "continue"
            | "default"
            | "do"
            | "double"
            | "else"
            | "enum"
            | "extern"
            | "float"
            | "for"
            | "goto"
            | "if"
            | "inline"
            | "int"
            | "long"
            | "register"
            | "restrict"
            | "return"
            | "short"
            | "signed"
            | "sizeof"
            | "static"
            | "struct"
            | "switch"
            | "typedef"
            | "union"
            | "unsigned"
            | "void"
            | "volatile"
            | "while"
    )
}

pub(crate) fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Pick a C compiler for scriptc's library lane when the caller has not.
///
/// Library mode always compiles runtime and program C, and scriptc's default
/// `clang` driver is missing or too old on many Linux hosts. zig ships the
/// driver scriptc documents for this lane (`zigcc`), and equilibrium already
/// expects zig for its Zig support, so fall back to it instead of failing.
pub(crate) fn configure_command(command: &mut Command) {
    if std::env::var_os("SCRIPTC_CC").is_some() {
        return;
    }
    if find_tool("zig").is_some() {
        command.env("SCRIPTC_CC", "zigcc");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODULE: &str = r#"
import { something } from "./elsewhere";

// not an export
function helper(a: number): number {
  return a;
}

export function add(a: number, b: number): number {
  return a + b;
}

export function greet(
  who: string,
  loud: boolean,
): string {
  return loud ? who.toUpperCase() : who;
}

export function firstByte(data: Uint8Array): number {
  return data.length;
}

export function ping(): void {}
"#;

    #[test]
    fn scan_finds_only_exported_functions() {
        let declarations = scan_declarations(MODULE);
        let names: Vec<&str> = declarations.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, vec!["add", "greet", "firstByte", "ping"]);
    }

    #[test]
    fn parse_scalar_signature() {
        let export = parse_signature("export function add(a: number, b: number): number").unwrap();
        assert_eq!(export.name, "add");
        assert_eq!(
            export.params,
            vec![
                ("a".to_string(), ScriptcClass::F64),
                ("b".to_string(), ScriptcClass::F64)
            ]
        );
        assert_eq!(export.returns, ScriptcClass::F64);
    }

    #[test]
    fn parse_multi_line_signature_with_buffer_classes() {
        let declaration = scan_declarations(MODULE)
            .into_iter()
            .find(|d| d.name == "greet")
            .unwrap();
        let export = parse_signature(&declaration.signature).unwrap();
        assert_eq!(
            export.params,
            vec![
                ("who".to_string(), ScriptcClass::Str),
                ("loud".to_string(), ScriptcClass::Bool)
            ]
        );
        assert_eq!(export.returns, ScriptcClass::Str);
    }

    #[test]
    fn parse_bytes_and_void() {
        let bytes = parse_signature("export function firstByte(data: Uint8Array): number").unwrap();
        assert_eq!(
            bytes.params,
            vec![("data".to_string(), ScriptcClass::Bytes)]
        );
        let void = parse_signature("export function ping(): void").unwrap();
        assert!(void.params.is_empty());
        assert_eq!(void.returns, ScriptcClass::Void);
    }

    #[test]
    fn parse_rejects_shapes_outside_the_c_abi() {
        assert!(parse_signature("export function f(a: number)")
            .unwrap_err()
            .contains("missing return type"));
        assert!(
            parse_signature("export function f(a: { x: number }): number")
                .unwrap_err()
                .contains("unsupported TypeScript type")
        );
        assert!(
            parse_signature("export function f(a: string | null): number")
                .unwrap_err()
                .contains("unsupported TypeScript type")
        );
        assert!(parse_signature("export function f(a?: number): number")
            .unwrap_err()
            .contains("optional parameter"));
        assert!(parse_signature("export function f(...a: number[]): number")
            .unwrap_err()
            .contains("rest parameter"));
        assert!(parse_signature("export function f(a): number")
            .unwrap_err()
            .contains("no type annotation"));
        assert!(parse_signature("export function f<T>(a: T): number")
            .unwrap_err()
            .contains("generic"));
        assert!(parse_signature("export function f(a: void): number")
            .unwrap_err()
            .contains("cannot be void"));
        assert!(parse_signature("export function f(): bigint")
            .unwrap_err()
            .contains("unsupported TypeScript type"));
    }

    #[test]
    fn symbol_prefix_is_a_c_identifier_fragment() {
        assert_eq!(symbol_prefix("math"), "math_");
        assert_eq!(symbol_prefix("my-lib"), "my_lib_");
        assert_eq!(symbol_prefix("2fast"), "_2fast_");
        assert_eq!(symbol_prefix(""), "sc_");
    }

    #[test]
    fn profile_declares_the_abi_and_localizes_the_runtime() {
        let exports = vec![
            parse_signature("export function add(a: number, b: number): number").unwrap(),
            parse_signature("export function greet(who: string): string").unwrap(),
        ];
        let profile = profile_json(
            "math",
            &symbol_prefix("math"),
            Path::new("/src/math.ts"),
            &exports,
            "llvm",
        );
        assert!(profile.contains("\"profile_format\": 1"));
        assert!(profile.contains("\"entry\": \"/src/math.ts\""));
        assert!(profile.contains("\"emission\": \"llvm\""));
        assert!(profile.contains("\"prefix\": \"math_\""));
        assert!(profile.contains("\"init_symbol\": \"math_init\""));
        assert!(profile.contains("\"sink_register_symbol\": \"math_set_panic_sink\""));
        assert!(profile.contains("\"collect_symbol\": \"math_collect\""));
        assert!(profile.contains("\"localize_runtime\": true"));
        assert!(profile.contains(
            "{ \"export\": \"add\", \"symbol\": \"math_add\", \"params\": [\"f64\", \"f64\"], \"returns\": \"f64\" },"
        ));
        assert!(profile.contains(
            "{ \"export\": \"greet\", \"symbol\": \"math_greet\", \"params\": [\"string\"], \"returns\": \"string\" }"
        ));
    }

    #[test]
    fn profile_escapes_windows_entry_paths() {
        let profile = profile_json("math", "math_", Path::new(r"C:\src\math.ts"), &[], "c");
        assert!(profile.contains(r#""entry": "C:\\src\\math.ts""#));
        assert!(profile.contains("\"emission\": \"c\""));
        assert!(profile.contains("\"exports\": [\n  ]"));
    }

    #[test]
    fn header_matches_the_profile_surface() {
        let exports = vec![
            parse_signature("export function add(a: number, b: number): number").unwrap(),
            parse_signature("export function greet(who: string): string").unwrap(),
        ];
        let header = header("math_", "math", &exports);
        assert!(header.contains("void math_init(void);"));
        assert!(header.contains("void math_set_panic_sink(void *fn_ptr, void *ctx);"));
        assert!(header.contains("void math_collect(void);"));
        assert!(header.contains("double math_add(double a, double b);"));
        assert!(header.contains(
            "void math_greet(const uint8_t *who_ptr, size_t who_len, const uint8_t **out, size_t *out_len);"
        ));
        // The header is what binding generation parses, so the real parser has
        // to see exactly the mode symbols and exports — never the comments.
        let parsed = crate::c_header::parse_c_header(&header);
        let names: Vec<&str> = parsed
            .functions
            .iter()
            .map(|function| function.name.as_str())
            .collect();
        assert_eq!(
            names,
            vec![
                "math_init",
                "math_set_panic_sink",
                "math_collect",
                "math_add",
                "math_greet"
            ]
        );
        let greet = &parsed.functions[4];
        assert_eq!(greet.return_type, "void");
        assert_eq!(
            greet.params,
            vec![
                ("const uint8_t *".to_string(), "who_ptr".to_string()),
                ("size_t".to_string(), "who_len".to_string()),
                ("const uint8_t **".to_string(), "out".to_string()),
                ("size_t *".to_string(), "out_len".to_string()),
            ]
        );
    }

    #[test]
    fn header_sanitises_keyword_parameter_names() {
        let exports =
            vec![parse_signature("export function f(int: number, int_2: number): number").unwrap()];
        let header = header("sc_", "sc", &exports);
        assert!(header.contains("double sc_f(double int_, double int_2);"));
    }

    #[test]
    fn profile_path_sits_beside_the_archive() {
        let path = profile_path(Path::new("/src/math.ts"), Path::new("/out/libmath.a"));
        // Compare the parts, not the separator style.
        assert_eq!(
            path.file_name().and_then(|name| name.to_str()),
            Some("math.profile.json")
        );
        assert_eq!(path.parent().and_then(|dir| dir.to_str()), Some("/out"));
    }

    /// A module plus a target config that refines its ABI.
    fn module_with_config(config: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("math.ts");
        std::fs::write(
            &source,
            r#"
export function mix(tag: number, idx: number): number {
  return tag * 1000 + idx;
}

export function count(data: Uint8Array): number {
  return data.length;
}
"#,
        )
        .unwrap();
        std::fs::write(dir.path().join("equilibrium.toml"), config).unwrap();
        (dir, source)
    }

    fn exports_of(source: &Path) -> Vec<ScriptcExport> {
        let content = std::fs::read_to_string(source).unwrap();
        scan_declarations(&content)
            .into_iter()
            .filter_map(|declaration| parse_signature(&declaration.signature).ok())
            .collect()
    }

    #[test]
    fn target_config_refines_classes_and_emission() {
        let (_dir, source) = module_with_config(
            r#"
[target.math]
language = "scriptc"
sources = ["math.ts"]
emission = "c"

[target.math.signatures]
mix = { params = ["u32", "u32"], returns = "f64" }
count = { returns = "u64" }
"#,
        );

        let settings = target_settings(&source, None).unwrap();
        assert_eq!(settings.emission, "c");
        let exports = apply_overrides(exports_of(&source), &settings).unwrap();

        let mix = exports.iter().find(|export| export.name == "mix").unwrap();
        assert_eq!(
            mix.params,
            vec![
                ("tag".to_string(), ScriptcClass::U32),
                ("idx".to_string(), ScriptcClass::U32)
            ]
        );
        let count = exports
            .iter()
            .find(|export| export.name == "count")
            .unwrap();
        assert_eq!(count.returns, ScriptcClass::U64);

        // The header follows the configured classes, so the host passes
        // integers in their C-native widths.
        let header = header("math_", "math", &exports);
        assert!(
            header.contains("double math_mix(uint32_t tag, uint32_t idx);"),
            "header:\n{header}"
        );
        assert!(
            header.contains("uint64_t math_count(const uint8_t *data_ptr, size_t data_len);"),
            "header:\n{header}"
        );
    }

    #[test]
    fn target_config_defaults_to_the_llvm_emission() {
        let (_dir, source) = module_with_config(
            r#"
[target.math]
language = "scriptc"
sources = ["math.ts"]
"#,
        );

        let settings = target_settings(&source, None).unwrap();
        assert_eq!(settings.emission, "llvm");
        assert!(
            apply_overrides(exports_of(&source), &settings)
                .unwrap()
                .len()
                == 2
        );
    }

    #[test]
    fn target_config_refuses_unusable_overrides() {
        let cases = [
            (
                r#"
[target.math]
language = "scriptc"
sources = ["math.ts"]
emission = "wasm"
"#,
                "unsupported scriptc emission `wasm`",
            ),
            (
                r#"
[target.math]
language = "scriptc"
sources = ["math.ts"]

[target.math.signatures]
mix = { params = ["u32"], returns = "f64" }
"#,
                "1 scriptc parameters configured for 2 TypeScript parameters",
            ),
            (
                r#"
[target.math]
language = "scriptc"
sources = ["math.ts"]

[target.math.signatures]
mix = { params = ["string", "u32"], returns = "f64" }
"#,
                "cannot describe the TypeScript parameter `tag: number`",
            ),
            (
                r#"
[target.math]
language = "scriptc"
sources = ["math.ts"]

[target.math.signatures]
mix = { params = ["u32", "u32"], returns = "u32" }
"#,
                "inbound-only scriptc class and cannot be a return type",
            ),
            (
                r#"
[target.math]
language = "scriptc"
sources = ["math.ts"]

[target.math.signatures]
mix = { params = ["u32", "u32"], returns = "boolean" }
"#,
                "unknown scriptc marshalling class `boolean`",
            ),
            (
                r#"
[target.math]
language = "scriptc"
sources = ["math.ts"]

[target.math.signatures]
missing = { params = ["u32"], returns = "f64" }
"#,
                "signature override for `missing` matches no C ABI safe export",
            ),
            (
                r#"
[target.math]
language = "scriptc"
sources = ["math.ts"]

[target.math.signatures]
count = { returns = "bytes" }
"#,
                "cannot describe the TypeScript return type `number`",
            ),
        ];

        for (config, expected) in cases {
            let (_dir, source) = module_with_config(config);
            let error = target_settings(&source, None)
                .and_then(|settings| apply_overrides(exports_of(&source), &settings))
                .expect_err("config should be refused");
            assert!(
                error.contains(expected),
                "expected {expected:?} in {error:?}"
            );
        }
    }
}
