//! Compiler invocation for generating C output.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::detector::{find_compiler, Language};

/// Maximum source file size passed to `compile_to_c` (denial-of-service guard).
const MAX_SOURCE_FILE_BYTES: u64 = 64 * 1024 * 1024;

/// Error during compilation.
#[derive(Debug)]
pub enum CompileError {
    /// Compiler not found on system.
    CompilerNotFound { language: Language },
    /// Compilation failed.
    CompilationFailed {
        stderr: String,
        exit_code: Option<i32>,
    },
    /// IO error.
    Io(std::io::Error),
    /// Language doesn't support C output.
    UnsupportedCOutput { language: Language },
    /// Extra compiler/link argument was rejected.
    InvalidExtraArg { arg: String },
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompileError::CompilerNotFound { language } => {
                write!(f, "Compiler for {:?} not found", language)
            }
            CompileError::CompilationFailed { stderr, exit_code } => {
                write!(f, "Compilation failed (exit {:?}): {}", exit_code, stderr)
            }
            CompileError::Io(e) => write!(f, "IO error: {}", e),
            CompileError::UnsupportedCOutput { language } => {
                write!(f, "{:?} doesn't support direct C output", language)
            }
            CompileError::InvalidExtraArg { arg } => {
                write!(f, "rejected extra compiler argument: {arg}")
            }
        }
    }
}

impl std::error::Error for CompileError {}

impl From<std::io::Error> for CompileError {
    fn from(e: std::io::Error) -> Self {
        CompileError::Io(e)
    }
}

/// Result of a successful compilation.
#[derive(Clone, Debug)]
pub struct CompileResult {
    /// Path to the generated C file or object.
    pub output_path: PathBuf,
    /// Path to the generated header (if any).
    pub header_path: Option<PathBuf>,
    /// The language that was compiled.
    pub language: Language,
    /// Compiler output (stdout).
    pub stdout: String,
    /// Compiler warnings (stderr, if successful).
    pub stderr: String,
}

/// Compile a source file to C intermediate representation.
pub fn compile_to_c(input: &Path, output_dir: &Path) -> Result<CompileResult, CompileError> {
    compile_to_c_with_extra(input, output_dir, &[], &[])
}

/// Compile with extra compiler and link arguments (link args reserved for future link step).
pub fn compile_to_c_with_extra(
    input: &Path,
    output_dir: &Path,
    compile_args: &[String],
    _link_args: &[String],
) -> Result<CompileResult, CompileError> {
    let language = crate::detector::detect_language(input).ok_or_else(|| {
        CompileError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Unknown source language",
        ))
    })?;
    compile_to_c_with_lang_and_extra(input, output_dir, language, compile_args, _link_args)
}

/// Compile a source file to C with explicit language.
pub fn compile_to_c_with_lang(
    input: &Path,
    output_dir: &Path,
    language: Language,
) -> Result<CompileResult, CompileError> {
    compile_to_c_with_lang_and_extra(input, output_dir, language, &[], &[])
}

pub fn compile_to_c_with_lang_and_extra(
    input: &Path,
    output_dir: &Path,
    language: Language,
    compile_args: &[String],
    _link_args: &[String],
) -> Result<CompileResult, CompileError> {
    if !input.is_file() {
        return Err(CompileError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "compile_to_c input must be a regular file",
        )));
    }
    let meta = std::fs::metadata(input)?;
    if meta.len() > MAX_SOURCE_FILE_BYTES {
        return Err(CompileError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "source file too large ({} bytes; max {} bytes)",
                meta.len(),
                MAX_SOURCE_FILE_BYTES
            ),
        )));
    }

    std::fs::create_dir_all(output_dir)?;
    validate_extra_args(compile_args)?;

    // Find compiler
    let info = find_compiler(language).ok_or(CompileError::CompilerNotFound { language })?;

    let compiler = info
        .compiler_path
        .clone()
        .or_else(|| info.compiler.as_ref().map(PathBuf::from))
        .ok_or(CompileError::CompilerNotFound { language })?;

    // Determine output file name
    let stem = input
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output");

    let c_output = output_dir.join(artifact_filename(language, stem));
    let header_output = output_dir.join(format!("{stem}.h"));

    // Build command
    let input_str = input.to_string_lossy();
    let output_str = c_output.to_string_lossy();

    let mut args = language.to_c_args(&input_str, &output_str);
    args.extend(compile_args.iter().cloned());

    let output = Command::new(&compiler)
        .args(&args)
        .current_dir(input.parent().unwrap_or(Path::new(".")))
        .output()?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() {
        return Err(CompileError::CompilationFailed {
            stderr,
            exit_code: output.status.code(),
        });
    }

    let output_path = if c_output.exists() {
        c_output
    } else {
        let alt = output_dir.join(format!("lib{stem}.a"));
        if alt.exists() {
            alt
        } else {
            return Err(CompileError::CompilationFailed {
                stderr: format!(
                    "compiler succeeded but output is missing: {} (stderr: {stderr})",
                    c_output.display()
                ),
                exit_code: output.status.code(),
            });
        }
    };

    // Check if header was generated (language-specific)
    let header_path = if header_output.exists() {
        Some(header_output)
    } else if let Some(copied) = copy_sibling_header(input, output_dir) {
        Some(copied)
    } else {
        // Try to generate header for some languages
        generate_header(input, output_dir, language).ok()
    };

    Ok(CompileResult {
        output_path,
        header_path,
        language,
        stdout,
        stderr,
    })
}

fn artifact_filename(language: Language, stem: &str) -> String {
    match language {
        Language::C
        | Language::Cpp
        | Language::Zig
        | Language::D
        | Language::Odin
        | Language::Hare => format!("{stem}.o"),
        Language::V => format!("{stem}.c"),
        Language::Nim => format!("{stem}.a"),
        Language::CSharp => format!("{stem}.dll"),
        Language::Rust => {
            if cfg!(target_os = "windows") {
                format!("{stem}.dll")
            } else if cfg!(target_os = "macos") {
                format!("{stem}.dylib")
            } else {
                format!("{stem}.so")
            }
        }
    }
}

fn copy_sibling_header(input: &Path, output_dir: &Path) -> Option<PathBuf> {
    let header = input.with_extension("h");
    if !header.is_file() {
        return None;
    }
    let dest = output_dir.join(header.file_name()?);
    std::fs::copy(&header, &dest).ok()?;
    Some(dest)
}

pub(crate) fn validate_extra_args(args: &[String]) -> Result<(), CompileError> {
    for arg in args {
        if !extra_compile_arg_allowed(arg) {
            return Err(CompileError::InvalidExtraArg { arg: arg.clone() });
        }
    }
    Ok(())
}

pub(crate) fn extra_compile_arg_allowed(arg: &str) -> bool {
    if arg.is_empty() || !arg.is_ascii() {
        return false;
    }
    if arg.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return false;
    }
    if arg.starts_with('@') || !arg.starts_with('-') {
        return false;
    }
    if let Some(rest) = arg.strip_prefix("-O") {
        return rest.chars().all(|c| c.is_ascii_alphanumeric());
    }
    let lower = arg.to_ascii_lowercase();
    const DENY: &[&str] = &[
        "-fplugin",
        "-load",
        "-plugin",
        "-wl,",
        "-xlinker",
        "-wrapper",
        "-specs",
        "-b",
        "-femit-bin",
        "--output",
        "-o",
        "-of",
    ];
    for prefix in DENY {
        if lower == *prefix
            || lower.starts_with(&format!("{prefix}="))
            || lower.starts_with(&format!("{prefix}:"))
        {
            return false;
        }
        if *prefix == "-o" && lower.starts_with("-o") && !lower.starts_with("-objc") {
            return false;
        }
        if *prefix == "-of" && lower.starts_with("-of") {
            return false;
        }
        if *prefix == "-b"
            && (lower == "-b" || lower.starts_with("-b/") || lower.starts_with("-b="))
        {
            return false;
        }
    }
    if arg.starts_with("-I") && arg.len() > 2 {
        let path = &arg[2..];
        return !path.contains("..") && path.chars().all(|c| c.is_ascii_graphic());
    }
    extra_flag_syntax(arg)
}

fn extra_flag_syntax(arg: &str) -> bool {
    let bytes = arg.as_bytes();
    if bytes.len() < 2 || bytes[0] != b'-' {
        return false;
    }
    let rest = if bytes[1] == b'-' {
        &arg[2..]
    } else {
        &arg[1..]
    };
    if rest.is_empty() {
        return false;
    }
    let mut chars = rest.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    let remaining: String = chars.collect();
    if remaining.is_empty() {
        return true;
    }
    remaining
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '+' | '-' | '=' | ':' | '/'))
}

pub(crate) fn extra_link_arg_allowed(arg: &str) -> bool {
    extra_compile_arg_allowed(arg) && (arg.starts_with("-l") || arg.starts_with("-L"))
}

/// Generate a C header file for the compiled code.
fn generate_header(
    input: &Path,
    output_dir: &Path,
    language: Language,
) -> Result<PathBuf, CompileError> {
    let stem = input
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output");

    let header_path = output_dir.join(format!("{stem}.h"));

    match language {
        Language::Rust => {
            // Use cbindgen for Rust
            if which::which("cbindgen").is_ok() {
                let output = Command::new("cbindgen")
                    .args([
                        "--lang",
                        "c",
                        "--output",
                        header_path.to_string_lossy().as_ref(),
                        input
                            .parent()
                            .unwrap_or(Path::new("."))
                            .to_string_lossy()
                            .as_ref(),
                    ])
                    .output()?;

                if output.status.success() && header_path.exists() {
                    return Ok(header_path);
                }
            }
            Err(CompileError::UnsupportedCOutput { language })
        }
        Language::V => {
            // V generates headers automatically with -backend c
            // The header should be alongside the C file
            if header_path.exists() {
                Ok(header_path)
            } else {
                Err(CompileError::UnsupportedCOutput { language })
            }
        }
        _ => Err(CompileError::UnsupportedCOutput { language }),
    }
}

/// Compile multiple files to C (parallel when `files.len() > 1`).
pub fn compile_batch(
    files: &[(PathBuf, Language)],
    output_dir: &Path,
) -> Vec<Result<CompileResult, CompileError>> {
    if files.is_empty() {
        return Vec::new();
    }
    if files.len() == 1 {
        return vec![compile_to_c_with_lang(&files[0].0, output_dir, files[0].1)];
    }

    let output_dir = output_dir.to_path_buf();
    std::thread::scope(|scope| {
        let handles: Vec<_> = files
            .iter()
            .map(|(path, lang)| {
                let path = path.clone();
                let output_dir = output_dir.clone();
                let lang = *lang;
                scope.spawn(move || compile_to_c_with_lang(&path, &output_dir, lang))
            })
            .collect();
        handles
            .into_iter()
            .map(|h| {
                h.join().unwrap_or_else(|_| {
                    Err(CompileError::Io(std::io::Error::other(
                        "compile_batch worker thread panicked",
                    )))
                })
            })
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::find_compiler;
    use tempfile::tempdir;

    #[test]
    fn test_compile_error_display() {
        let err = CompileError::CompilerNotFound {
            language: Language::V,
        };
        assert!(err.to_string().contains("V"));
    }

    #[test]
    fn test_compile_error_failed_display() {
        let err = CompileError::CompilationFailed {
            stderr: "syntax error".to_string(),
            exit_code: Some(1),
        };
        let msg = err.to_string();
        assert!(msg.contains("syntax error"));
        assert!(msg.contains('1'));
    }

    #[test]
    fn test_compile_nonexistent() {
        let dir = tempdir().unwrap();
        let result = compile_to_c(Path::new("/nonexistent/file.v"), dir.path());
        assert!(result.is_err());
    }

    #[test]
    fn test_compile_unknown_extension() {
        let dir = tempdir().unwrap();
        // .py is not a supported language
        let result = compile_to_c(Path::new("/nonexistent/file.py"), dir.path());
        assert!(result.is_err());
    }

    #[test]
    fn test_compile_batch_empty() {
        let dir = tempdir().unwrap();
        let results = compile_batch(&[], dir.path());
        assert!(results.is_empty());
    }

    #[test]
    fn test_compile_c_file() {
        // Skip if no C compiler is available
        if find_compiler(Language::C).is_none() {
            return;
        }

        let dir = tempdir().unwrap();
        let c_file = dir.path().join("test.c");
        std::fs::write(&c_file, "int add(int a, int b) { return a + b; }\n").unwrap();

        let output_dir = dir.path().join("out");
        std::fs::create_dir(&output_dir).unwrap();

        let result = compile_to_c(&c_file, &output_dir).unwrap();
        assert!(result.output_path.exists());
        assert_eq!(result.language, Language::C);
    }

    #[test]
    fn test_compile_c_missing_include() {
        // Skip if no C compiler is available
        if find_compiler(Language::C).is_none() {
            return;
        }

        let dir = tempdir().unwrap();
        let c_file = dir.path().join("bad.c");
        // A missing #include fails even with -E (preprocessor-only mode)
        std::fs::write(
            &c_file,
            "#include <this_header_does_not_exist_equilibrium_test.h>\n",
        )
        .unwrap();

        let output_dir = dir.path().join("out");
        std::fs::create_dir(&output_dir).unwrap();

        let result = compile_to_c(&c_file, &output_dir);
        assert!(result.is_err());
        if let Err(CompileError::CompilationFailed { stderr, .. }) = result {
            assert!(!stderr.is_empty());
        }
    }

    #[test]
    fn test_compile_rejects_plugin_arg() {
        if find_compiler(Language::C).is_none() {
            return;
        }
        let dir = tempdir().unwrap();
        let c_file = dir.path().join("test.c");
        std::fs::write(&c_file, "int add(int a, int b) { return a + b; }\n").unwrap();
        let output_dir = dir.path().join("out");
        let err = compile_to_c_with_extra(
            &c_file,
            &output_dir,
            &["-fplugin=/tmp/x.so".to_string()],
            &[],
        )
        .unwrap_err();
        assert!(err.to_string().contains("rejected extra compiler argument"));
    }

    #[test]
    fn test_rejects_dangerous_compile_args() {
        assert!(!extra_compile_arg_allowed("-fplugin=/tmp/x.so"));
        assert!(!extra_compile_arg_allowed("@response"));
        assert!(!extra_compile_arg_allowed("-o"));
        assert!(!extra_compile_arg_allowed("-ofoo.o"));
        assert!(!extra_compile_arg_allowed("-Wl,-rpath,/tmp"));
        assert!(!extra_compile_arg_allowed("-fPIC\n-o/tmp/x"));
        assert!(extra_compile_arg_allowed("-fPIC"));
        assert!(extra_compile_arg_allowed("-O2"));
        assert!(extra_compile_arg_allowed("-std=c11"));
        assert!(extra_compile_arg_allowed("-I/usr/include"));
        assert!(!extra_compile_arg_allowed("-I../secret"));
    }

    #[test]
    fn test_compile_batch_c_files() {
        // Skip if no C compiler is available
        if find_compiler(Language::C).is_none() {
            return;
        }

        let dir = tempdir().unwrap();
        let c1 = dir.path().join("a.c");
        let c2 = dir.path().join("b.c");
        std::fs::write(&c1, "int foo(void) { return 1; }\n").unwrap();
        std::fs::write(&c2, "int bar(void) { return 2; }\n").unwrap();

        let output_dir = dir.path().join("out");
        std::fs::create_dir(&output_dir).unwrap();

        let files = vec![(c1, Language::C), (c2, Language::C)];
        let results = compile_batch(&files, &output_dir);

        assert_eq!(results.len(), 2);
        for r in &results {
            assert!(r.is_ok(), "batch compile failed: {:?}", r);
        }
    }
}
