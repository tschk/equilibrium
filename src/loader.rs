//! One-liner FFI loading — compile, generate bindings, link.

use std::path::{Path, PathBuf};

use crate::bindings::{generate_bindings_from_content, BindingOptions, GeneratedBinding};
use crate::c_header::parse_c_header;
use crate::compiler::{
    compile_to_c_with_extra, extra_link_arg_allowed, validate_extra_args, CompileResult,
};
use crate::detector::{detect_language, find_compiler, Language};
use crate::exports::{discover_exports_with_options, ExportOptions, ExportSource};
use crate::imports::{generate_imports, GeneratedImport, ImportOptions};
use crate::limits::read_header_content;

/// Options for loading a foreign module.
#[derive(Clone, Debug)]
pub struct LoadOptions {
    /// Generate Rust bindings from the header (default: true)
    pub generate_bindings: bool,
    /// Compile the source (default: true)
    pub compile: bool,
    /// Output directory (default: target/native)
    pub output_dir: Option<PathBuf>,
    /// Custom binding options
    pub binding_options: Option<BindingOptions>,
    /// Link the compiled library (default: true for object files)
    pub link: bool,
    /// Extra compile args appended to the compiler invocation
    pub compile_args: Vec<String>,
    /// Reserved for a future link step (stored on options, not applied yet)
    pub link_args: Vec<String>,
    pub exports: Vec<String>,
    pub config_path: Option<PathBuf>,
    pub consumer_languages: Vec<Language>,
}

impl Default for LoadOptions {
    fn default() -> Self {
        Self {
            generate_bindings: true,
            compile: true,
            output_dir: None,
            binding_options: None,
            link: true,
            link_args: Vec::new(),
            compile_args: Vec::new(),
            exports: Vec::new(),
            config_path: None,
            consumer_languages: Vec::new(),
        }
    }
}

impl LoadOptions {
    pub fn exports<I, S>(mut self, exports: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.exports = exports.into_iter().map(Into::into).collect();
        self
    }

    pub fn output_dir<P: AsRef<Path>>(mut self, output_dir: P) -> Self {
        self.output_dir = Some(output_dir.as_ref().to_path_buf());
        self
    }

    pub fn generate_bindings(mut self, generate_bindings: bool) -> Self {
        self.generate_bindings = generate_bindings;
        self
    }

    pub fn compile(mut self, compile: bool) -> Self {
        self.compile = compile;
        self
    }

    pub fn link(mut self, link: bool) -> Self {
        self.link = link;
        self
    }

    pub fn compile_args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.compile_args = args.into_iter().map(Into::into).collect();
        self
    }

    pub fn config_path<P: AsRef<Path>>(mut self, path: P) -> Self {
        self.config_path = Some(path.as_ref().to_path_buf());
        self
    }

    pub fn consumer_languages<I>(mut self, languages: I) -> Self
    where
        I: IntoIterator<Item = Language>,
    {
        self.consumer_languages = languages.into_iter().collect();
        self
    }
}

/// Result of loading a foreign module.
#[derive(Clone, Debug)]
pub struct LoadedModule {
    /// Path to the compiled output (C file or object)
    pub output_path: PathBuf,
    /// Path to the generated header (if any)
    pub header_path: Option<PathBuf>,
    /// The generated Rust bindings
    pub bindings: Option<GeneratedBinding>,
    /// The language that was loaded
    pub language: Language,
    /// Path to the original source
    pub source_path: PathBuf,
    pub exports: Vec<String>,
    pub export_source: ExportSource,
    pub warnings: Vec<String>,
    pub imports: Vec<GeneratedImport>,
}

impl LoadedModule {
    /// Get the binding code if available.
    pub fn bindings_code(&self) -> Option<&str> {
        self.bindings.as_ref().map(|b| b.code.as_str())
    }
}

/// Load a foreign source file — compiles, generates bindings, returns ready-to-use module.
///
/// # Example
///
/// ```ignore
/// use equilibrium::load;
///
/// // Simple one-liner
/// let lib = load("native/math.c")?;
///
/// // Access compiled output and bindings
/// println!("Compiled: {:?}", lib.output_path);
/// if let Some(code) = lib.bindings_code() {
///     println!("Bindings: {}", code);
/// }
/// ```
///
/// # Arguments
/// * `source` - Path to the source file (e.g., "native/v/math.v")
pub fn load<S: AsRef<Path>>(source: S) -> Result<LoadedModule, LoadError> {
    load_with_options(source, LoadOptions::default())
}

/// Load with custom options.
pub fn load_with_options<S: AsRef<Path>>(
    source: S,
    options: LoadOptions,
) -> Result<LoadedModule, LoadError> {
    let source = source.as_ref();
    let source = source
        .canonicalize()
        .unwrap_or_else(|_| source.to_path_buf());

    let Some(lang) = detect_language(&source) else {
        return Err(LoadError::UnknownLanguage(source.clone()));
    };

    let export_options = ExportOptions {
        exports: options.exports.clone(),
        config_path: options.config_path.clone(),
    };
    let export_discovery = discover_exports_with_options(&source, lang, &export_options)
        .map_err(|e| LoadError::ExportFailed(e.to_string()))?;

    let output_dir = options.output_dir.clone().unwrap_or_else(|| {
        // Use CARGO_MANIFEST_DIR if available, otherwise current dir
        std::env::var("CARGO_MANIFEST_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("target"))
            .join("native")
            .join(format!("{:?}", lang).to_lowercase())
    });

    std::fs::create_dir_all(&output_dir).map_err(|e| LoadError::Io {
        path: output_dir.clone(),
        error: e,
    })?;

    if options.compile {
        find_compiler(lang).ok_or(LoadError::CompilerNotFound(lang))?;
        validate_extra_args(&options.compile_args)
            .map_err(|e| LoadError::CompilationFailed(lang, e.to_string()))?;
    }

    let result = if options.compile {
        compile_to_c_with_extra(
            &source,
            &output_dir,
            &options.compile_args,
            &options.link_args,
        )
        .map_err(|e| LoadError::CompilationFailed(lang, e.to_string()))?
    } else {
        CompileResult {
            output_path: source.clone(),
            header_path: sibling_header(&source),
            language: lang,
            stdout: String::new(),
            stderr: String::new(),
        }
    };

    let import_source = result.header_path.clone().unwrap_or_else(|| source.clone());
    let header_content = if import_source
        .extension()
        .is_some_and(|ext| matches!(ext.to_str(), Some("h" | "hh" | "hpp" | "hxx")))
        || options.generate_bindings
        || !options.consumer_languages.is_empty()
    {
        read_header_content(&import_source).ok()
    } else {
        None
    };

    let parsed_header = header_content.as_deref().map(parse_c_header);

    let default_binding_opts = BindingOptions::default();
    let binding_opts = options
        .binding_options
        .as_ref()
        .unwrap_or(&default_binding_opts);

    let bindings = if options.generate_bindings {
        if let (Some(content), Some(header_path)) = (&header_content, result.header_path.as_ref()) {
            Some(
                generate_bindings_from_content(header_path, content, binding_opts)
                    .map_err(LoadError::BindingFailed)?,
            )
        } else {
            None
        }
    } else {
        None
    };

    let import_options =
        ImportOptions::default().allowlist_functions(export_discovery.exports.clone());
    let mut imports = Vec::new();
    let mut warnings = export_discovery.warnings;
    if options.generate_bindings && bindings.is_none() {
        warnings.push("no header found; bindings not generated".to_string());
    }
    if options.link {
        emit_cargo_link(lang, &result.output_path, &options.link_args, &mut warnings)?;
    }
    for language in &options.consumer_languages {
        let generated = if let Some(parsed) = &parsed_header {
            crate::imports::generate_imports_from_parsed(
                &import_source,
                *language,
                parsed,
                &import_options,
            )
            .map_err(LoadError::ImportFailed)?
        } else {
            generate_imports(&import_source, *language, &import_options)
                .map_err(LoadError::ImportFailed)?
        };
        warnings.extend(generated.warnings.clone());
        imports.push(generated);
    }

    Ok(LoadedModule {
        output_path: result.output_path,
        header_path: result.header_path,
        bindings,
        language: lang,
        source_path: source,
        exports: export_discovery.exports,
        export_source: export_discovery.source,
        warnings,
        imports,
    })
}

/// Errors that can occur when loading a module.
#[derive(Debug)]
pub enum LoadError {
    UnknownLanguage(PathBuf),
    CompilerNotFound(Language),
    CompilationFailed(Language, String),
    Io {
        path: PathBuf,
        error: std::io::Error,
    },
    BindingFailed(String),
    ExportFailed(String),
    ImportFailed(String),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::UnknownLanguage(path) => {
                write!(f, "Unknown language for file: {}", path.display())
            }
            LoadError::CompilerNotFound(lang) => {
                write!(f, "Compiler for {:?} not found", lang)
            }
            LoadError::CompilationFailed(lang, msg) => {
                write!(f, "Compilation of {:?} failed: {}", lang, msg)
            }
            LoadError::Io { path, error } => {
                write!(f, "IO error for {}: {}", path.display(), error)
            }
            LoadError::BindingFailed(msg) => {
                write!(f, "Binding generation failed: {}", msg)
            }
            LoadError::ExportFailed(msg) => {
                write!(f, "Export discovery failed: {}", msg)
            }
            LoadError::ImportFailed(msg) => {
                write!(f, "Import generation failed: {}", msg)
            }
        }
    }
}

impl std::error::Error for LoadError {}

fn sibling_header(source: &Path) -> Option<PathBuf> {
    let header = source.with_extension("h");
    header.is_file().then_some(header)
}

fn emit_cargo_link(
    language: Language,
    output_path: &Path,
    link_args: &[String],
    warnings: &mut Vec<String>,
) -> Result<(), LoadError> {
    if std::env::var_os("CARGO").is_none() || std::env::var_os("OUT_DIR").is_none() || cfg!(test) {
        return Ok(());
    }
    if !output_path.is_file() {
        warnings.push(format!(
            "link requested but output is missing: {}",
            output_path.display()
        ));
        return Ok(());
    }
    for arg in link_args {
        if !extra_link_arg_allowed(arg) {
            return Err(LoadError::CompilationFailed(
                language,
                format!("rejected extra link argument: {arg}"),
            ));
        }
    }
    let ext = output_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    match ext {
        "o" | "obj" => {
            println!("cargo:rustc-link-arg={}", output_path.display());
        }
        "a" | "lib" => {
            if let (Some(dir), Some(stem)) = (output_path.parent(), output_path.file_stem()) {
                let stem = stem.to_string_lossy();
                let libname = stem.strip_prefix("lib").unwrap_or(&stem);
                println!("cargo:rustc-link-search=native={}", dir.display());
                println!("cargo:rustc-link-lib=static={libname}");
            }
        }
        "so" | "dylib" | "dll" => {
            if let (Some(dir), Some(stem)) = (output_path.parent(), output_path.file_stem()) {
                let stem = stem.to_string_lossy();
                let libname = stem.strip_prefix("lib").unwrap_or(&stem);
                println!("cargo:rustc-link-search=native={}", dir.display());
                println!("cargo:rustc-link-lib=dylib={libname}");
            }
        }
        _ => {
            println!("cargo:rustc-link-arg={}", output_path.display());
        }
    }
    for arg in link_args {
        println!("cargo:rustc-link-arg={arg}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_c() {
        if find_compiler(Language::C).is_none() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("add.c");
        let header = dir.path().join("add.h");
        std::fs::write(&header, "int add(int a, int b);\n").unwrap();
        std::fs::write(&path, "int add(int a, int b) { return a + b; }\n").unwrap();
        let out = dir.path().join("out");

        let result = load_with_options(&path, LoadOptions::default().output_dir(&out).link(false))
            .expect("load C source");
        assert!(result.output_path.exists());
        assert_eq!(result.output_path.extension().unwrap(), "o");
        let code = result.bindings_code().expect("bindings");
        assert!(code.contains("pub fn add("));
    }

    #[test]
    fn test_load_honors_compile_false() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("add.c");
        let header = dir.path().join("add.h");
        std::fs::write(&header, "int add(int a, int b);\n").unwrap();
        std::fs::write(&path, "int add(int a, int b) { return a + b; }\n").unwrap();
        let out = dir.path().join("out");

        let result = load_with_options(
            &path,
            LoadOptions::default()
                .output_dir(&out)
                .compile(false)
                .link(false),
        )
        .expect("load without compile");
        assert_eq!(
            result.output_path.canonicalize().unwrap(),
            path.canonicalize().unwrap()
        );
        assert!(result.bindings_code().unwrap().contains("pub fn add("));
        assert!(!out.join("add.o").exists());
    }

    #[test]
    fn test_load_options() {
        let opts = LoadOptions::default();
        assert!(opts.generate_bindings);
        assert!(opts.compile);
        assert!(opts.link);
    }
}
