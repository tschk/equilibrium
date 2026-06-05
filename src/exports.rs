use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::detector::Language;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExportSource {
    Requested,
    Config,
    ExplicitMarkers,
    AllFunctions,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportDiscovery {
    pub exports: Vec<String>,
    pub source: ExportSource,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct ExportOptions {
    pub exports: Vec<String>,
    pub config_path: Option<PathBuf>,
}

impl ExportOptions {
    pub fn exports<I, S>(mut self, exports: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.exports = exports.into_iter().map(Into::into).collect();
        self
    }

    pub fn config_path<P: AsRef<Path>>(mut self, path: P) -> Self {
        self.config_path = Some(path.as_ref().to_path_buf());
        self
    }
}

#[derive(Debug)]
pub enum ExportError {
    Io {
        path: PathBuf,
        error: std::io::Error,
    },
    Config {
        path: PathBuf,
        message: String,
    },
}

impl std::fmt::Display for ExportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExportError::Io { path, error } => {
                write!(
                    f,
                    "failed to read exports from {}: {}",
                    path.display(),
                    error
                )
            }
            ExportError::Config { path, message } => {
                write!(f, "failed to read config {}: {}", path.display(), message)
            }
        }
    }
}

impl std::error::Error for ExportError {}

#[derive(Deserialize)]
struct EquilibriumConfig {
    target: Option<BTreeMap<String, TargetConfig>>,
}

#[derive(Deserialize)]
struct TargetConfig {
    language: Option<String>,
    sources: Option<Vec<String>>,
    exports: Option<Vec<String>>,
}

#[derive(Clone)]
struct FunctionCandidate {
    name: String,
    signature: String,
    explicit: bool,
}

pub fn discover_exports(path: &Path, language: Language) -> Result<ExportDiscovery, ExportError> {
    discover_exports_with_options(path, language, &ExportOptions::default())
}

pub fn discover_exports_with_options(
    path: &Path,
    language: Language,
    options: &ExportOptions,
) -> Result<ExportDiscovery, ExportError> {
    if !options.exports.is_empty() {
        return Ok(ExportDiscovery {
            exports: dedupe(options.exports.clone()),
            source: ExportSource::Requested,
            warnings: Vec::new(),
        });
    }

    if let Some(exports) = config_exports(path, language, options)? {
        return Ok(ExportDiscovery {
            exports,
            source: ExportSource::Config,
            warnings: Vec::new(),
        });
    }

    let content = std::fs::read_to_string(path).map_err(|error| ExportError::Io {
        path: path.to_path_buf(),
        error,
    })?;
    let candidates = language_candidates(language, &content);
    let explicit: Vec<FunctionCandidate> =
        candidates.iter().filter(|c| c.explicit).cloned().collect();
    if !explicit.is_empty() {
        let (exports, warnings) = supported_exports(explicit, language);
        return Ok(ExportDiscovery {
            exports,
            source: ExportSource::ExplicitMarkers,
            warnings,
        });
    }

    let (exports, warnings) = supported_exports(candidates, language);
    Ok(ExportDiscovery {
        exports,
        source: ExportSource::AllFunctions,
        warnings,
    })
}

fn config_exports(
    source: &Path,
    language: Language,
    options: &ExportOptions,
) -> Result<Option<Vec<String>>, ExportError> {
    for config_path in config_candidates(source, options) {
        if !config_path.is_file() {
            continue;
        }
        let config_text =
            std::fs::read_to_string(&config_path).map_err(|error| ExportError::Io {
                path: config_path.clone(),
                error,
            })?;
        let config: EquilibriumConfig =
            toml::from_str(&config_text).map_err(|error| ExportError::Config {
                path: config_path.clone(),
                message: error.to_string(),
            })?;
        let Some(targets) = config.target else {
            continue;
        };
        let base = config_path.parent().unwrap_or(Path::new("."));
        for target in targets.values() {
            if target_matches(target, source, base, language) {
                if let Some(exports) = &target.exports {
                    return Ok(Some(dedupe(exports.clone())));
                }
            }
        }
    }
    Ok(None)
}

fn config_candidates(source: &Path, options: &ExportOptions) -> Vec<PathBuf> {
    if let Some(path) = &options.config_path {
        return vec![path.clone()];
    }
    let mut candidates = Vec::new();
    if let Some(parent) = source.parent() {
        candidates.push(parent.join("equilibrium.toml"));
    }
    if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
        candidates.push(PathBuf::from(manifest_dir).join("equilibrium.toml"));
    }
    candidates.dedup();
    candidates
}

fn target_matches(target: &TargetConfig, source: &Path, base: &Path, language: Language) -> bool {
    if let Some(target_language) = &target.language {
        if target_language.to_ascii_lowercase() != language_name(language) {
            return false;
        }
    }
    let Some(sources) = &target.sources else {
        return false;
    };
    let canonical_source = source
        .canonicalize()
        .unwrap_or_else(|_| source.to_path_buf());
    sources.iter().any(|candidate| {
        let candidate_path = base.join(candidate);
        let canonical_candidate = candidate_path
            .canonicalize()
            .unwrap_or_else(|_| candidate_path.clone());
        canonical_candidate == canonical_source || candidate_path == source
    })
}

fn language_name(language: Language) -> &'static str {
    match language {
        Language::V => "v",
        Language::Zig => "zig",
        Language::C => "c",
        Language::Cpp => "cpp",
        Language::CSharp => "csharp",
        Language::Rust => "rust",
        Language::D => "d",
        Language::Nim => "nim",
        Language::Odin => "odin",
        Language::Hare => "hare",
    }
}

fn language_candidates(language: Language, content: &str) -> Vec<FunctionCandidate> {
    match language {
        Language::Rust => rust_candidates(content),
        Language::Zig => zig_candidates(content),
        Language::Nim => nim_candidates(content),
        Language::D => d_candidates(content),
        Language::C | Language::Cpp => c_candidates(content),
        _ => Vec::new(),
    }
}

fn rust_candidates(content: &str) -> Vec<FunctionCandidate> {
    let mut out = Vec::new();
    let mut ffi_pending = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("#[") && trimmed.contains("ffi") {
            ffi_pending = true;
            continue;
        }
        if let Some(signature) = rust_signature(trimmed) {
            if let Some(name) = name_after_keyword(signature, "fn") {
                out.push(FunctionCandidate {
                    name,
                    signature: signature.to_string(),
                    explicit: ffi_pending,
                });
            }
            ffi_pending = false;
        } else if !trimmed.starts_with("#[") && !trimmed.is_empty() {
            ffi_pending = false;
        }
    }
    out
}

fn rust_signature(line: &str) -> Option<&str> {
    if line.starts_with("pub fn ") || line.starts_with("fn ") {
        return Some(line);
    }
    None
}

fn zig_candidates(content: &str) -> Vec<FunctionCandidate> {
    content
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            let explicit = trimmed.starts_with("pub export fn ");
            if explicit || trimmed.starts_with("pub fn ") || trimmed.starts_with("fn ") {
                name_after_keyword(trimmed, "fn").map(|name| FunctionCandidate {
                    name,
                    signature: trimmed.to_string(),
                    explicit,
                })
            } else {
                None
            }
        })
        .collect()
}

fn nim_candidates(content: &str) -> Vec<FunctionCandidate> {
    content
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if !trimmed.starts_with("proc ") {
                return None;
            }
            let rest = trimmed.trim_start_matches("proc ").trim_start();
            let name_end = rest
                .find(|c: char| c == '(' || c == '*' || c.is_whitespace())
                .unwrap_or(rest.len());
            let name = rest[..name_end].to_string();
            if name.is_empty() {
                return None;
            }
            Some(FunctionCandidate {
                name,
                signature: trimmed.to_string(),
                explicit: trimmed.contains("exportc") || rest.contains('*'),
            })
        })
        .collect()
}

fn d_candidates(content: &str) -> Vec<FunctionCandidate> {
    content
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if !trimmed.contains('(') || trimmed.starts_with('@') {
                return None;
            }
            let explicit = trimmed.contains("extern(C)") && trimmed.contains("export");
            d_name(trimmed).map(|name| FunctionCandidate {
                name,
                signature: trimmed.to_string(),
                explicit,
            })
        })
        .collect()
}

fn c_candidates(content: &str) -> Vec<FunctionCandidate> {
    content
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with("static ")
                || trimmed.starts_with('#')
                || !trimmed.contains('(')
                || !trimmed.contains(')')
            {
                return None;
            }
            let explicit = trimmed.ends_with(';');
            if !explicit && !trimmed.ends_with('{') {
                return None;
            }
            c_name(trimmed).map(|name| FunctionCandidate {
                name,
                signature: trimmed.to_string(),
                explicit,
            })
        })
        .collect()
}

fn name_after_keyword(line: &str, keyword: &str) -> Option<String> {
    let start = line.find(keyword)?;
    let rest = line[start + keyword.len()..].trim_start();
    let end = rest.find(|c: char| c == '(' || c.is_whitespace())?;
    let name = &rest[..end];
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

fn d_name(line: &str) -> Option<String> {
    let before_paren = line.rsplit_once('(')?.0.trim();
    let name = before_paren.split_whitespace().last()?;
    if matches!(name, "if" | "for" | "while" | "switch") {
        None
    } else {
        Some(name.to_string())
    }
}

fn c_name(line: &str) -> Option<String> {
    let before_paren = line.rsplit_once('(')?.0.trim();
    let name = before_paren.split_whitespace().last()?;
    if matches!(name, "if" | "for" | "while" | "switch") {
        None
    } else {
        Some(name.trim_start_matches('*').to_string())
    }
}

fn supported_exports(
    candidates: Vec<FunctionCandidate>,
    language: Language,
) -> (Vec<String>, Vec<String>) {
    let mut exports = Vec::new();
    let mut warnings = Vec::new();
    for candidate in candidates {
        if signature_supported(&candidate.signature, language) {
            exports.push(candidate.name);
        } else {
            warnings.push(format!(
                "skipped export {} because its signature is not C ABI safe",
                candidate.name
            ));
        }
    }
    (dedupe(exports), warnings)
}

fn signature_supported(signature: &str, language: Language) -> bool {
    match language {
        Language::Rust => rust_signature_supported(signature),
        _ => true,
    }
}

fn rust_signature_supported(signature: &str) -> bool {
    let Some(params_start) = signature.find('(') else {
        return false;
    };
    let Some(params_end) = signature[params_start..].find(')') else {
        return false;
    };
    let params = &signature[params_start + 1..params_start + params_end];
    for param in params.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let Some((_, ty)) = param.rsplit_once(':') else {
            return false;
        };
        if !rust_type_supported(ty.trim()) {
            return false;
        }
    }
    if let Some((_, return_type)) = signature.split_once("->") {
        let ty = return_type
            .split('{')
            .next()
            .unwrap_or(return_type)
            .trim()
            .trim_end_matches(';');
        rust_type_supported(ty)
    } else {
        true
    }
}

fn rust_type_supported(ty: &str) -> bool {
    if ty.starts_with("*const ") || ty.starts_with("*mut ") {
        return true;
    }
    matches!(
        ty,
        "()" | "bool"
            | "i8"
            | "u8"
            | "i16"
            | "u16"
            | "i32"
            | "u32"
            | "i64"
            | "u64"
            | "isize"
            | "usize"
            | "f32"
            | "f64"
            | "c_int"
            | "c_uint"
            | "c_char"
            | "c_void"
    )
}

fn dedupe(values: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    for value in values {
        if !out.contains(&value) {
            out.push(value);
        }
    }
    out
}
