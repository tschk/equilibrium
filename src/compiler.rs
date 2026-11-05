//! Compiler invocation for generating C output.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::detector::{find_compiler, Language};

/// Maximum source file size passed to `compile_to_c` (denial-of-service guard).
const MAX_SOURCE_FILE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_PREPROCESSED_BYTES: u64 = 64 * 1024 * 1024;

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
    /// `equilibrium.toml` (or the ABI it declares) is unusable.
    InvalidConfig { message: String },
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
            CompileError::UnsupportedCOutput {
                language: Language::CSharp,
            } => write!(
                f,
                "C# sources cannot be loaded: csc produces a managed .NET assembly, not a C ABI \
                 library, and Native AOT publishing is not implemented. Build a native library \
                 with `dotnet publish -p:PublishAot=true` yourself, or use C# as a consumer \
                 (P/Invoke imports via `--consumer csharp`)"
            ),
            CompileError::UnsupportedCOutput { language } => {
                write!(f, "{:?} doesn't support direct C output", language)
            }
            CompileError::InvalidExtraArg { arg } => {
                write!(f, "rejected extra compiler argument: {arg}")
            }
            CompileError::InvalidConfig { message } => {
                write!(f, "invalid configuration: {message}")
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

/// Options for [`compile_to_c_with_options`].
#[derive(Clone, Debug, Default)]
pub struct CompileOptions {
    /// `equilibrium.toml` to read language settings from. When unset the config
    /// beside the source, then `CARGO_MANIFEST_DIR`, is used.
    pub config_path: Option<PathBuf>,
    /// Extra compiler arguments appended to the language's own flags.
    pub compile_args: Vec<String>,
    /// Reserved for a future link step (stored, not applied).
    pub link_args: Vec<String>,
}

impl CompileOptions {
    pub fn config_path<P: AsRef<Path>>(mut self, path: P) -> Self {
        self.config_path = Some(path.as_ref().to_path_buf());
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

    pub fn link_args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.link_args = args.into_iter().map(Into::into).collect();
        self
    }
}

/// Result of a successful compilation.
///
/// Artifact and header paths are not immutable snapshots: a later compilation into the same
/// output location can replace them. Callers that need a consistent pair must coordinate builds
/// and consumption, or use separate output directories.
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
    /// Exports left out of a synthesized header, with the reason.
    pub warnings: Vec<String>,
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
    compile_inner(
        input,
        output_dir,
        language,
        &CompileOptions::default().compile_args(compile_args.iter().cloned()),
    )
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
    compile_inner(
        input,
        output_dir,
        language,
        &CompileOptions::default().compile_args(compile_args.iter().cloned()),
    )
}

/// Compile with explicit options.
pub fn compile_to_c_with_options(
    input: &Path,
    output_dir: &Path,
    language: Language,
    options: &CompileOptions,
) -> Result<CompileResult, CompileError> {
    compile_inner(input, output_dir, language, options)
}

fn compile_inner(
    input: &Path,
    output_dir: &Path,
    language: Language,
    options: &CompileOptions,
) -> Result<CompileResult, CompileError> {
    if language == Language::CSharp {
        return Err(CompileError::UnsupportedCOutput { language });
    }
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

    // Compilers run with the input's directory as their working directory, so
    // every path handed to them (and checked afterwards) has to be absolute.
    let input = std::fs::canonicalize(input)?;
    let output_dir = std::fs::canonicalize(output_dir)?;
    validate_extra_args(&options.compile_args)?;

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
    let sibling_header = input.with_extension("h");
    let managed = ManagedHeader::new(&output_dir, &input, stem)?;
    let header_dir = &managed.stage;
    let header_output = header_dir.join(format!("{stem}.h"));

    // scriptc takes its ABI surface from a JSON profile and emits no header,
    // so both are written next to the archive before the compiler runs.
    if language == Language::ScriptC {
        write_scriptc_library_surface(&input, header_dir, stem, &c_output, options)?;
    }

    // Build command
    let input_str = input.to_string_lossy();
    let output_str = c_output.to_string_lossy();

    let mut args = language.to_c_args(&input_str, &output_str);
    args.extend(options.compile_args.iter().cloned());

    let mut command = Command::new(&compiler);
    command
        .args(&args)
        .current_dir(input.parent().unwrap_or(Path::new(".")));
    if language == Language::ScriptC {
        crate::scriptc::configure_command(&mut command);
    }
    if matches!(language, Language::Nim | Language::Odin) {
        prefer_system_archiver(&mut command, &output_dir)?;
    }
    let output = command.output()?;

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
    let mut synth_warnings = Vec::new();
    let header_path = if language == Language::ScriptC {
        Some(header_output.clone())
    } else if sibling_header.is_file() {
        Some(sibling_header)
    } else if language == Language::D {
        if let Some(header) = crate::header_synth::d_header_from_hc(&stdout) {
            std::fs::write(&header_output, header)?;
            Some(header_output.clone())
        } else {
            None
        }
    } else if let Ok(generated) = generate_header(&input, header_dir, language) {
        Some(generated)
    } else {
        synthesize_header(
            &input,
            &header_output,
            language,
            &compiler,
            &args,
            &mut synth_warnings,
        )?
    };

    let header_path = match header_path {
        Some(path) if path == header_output => Some(managed.publish(&path)?),
        other => other,
    };
    Ok(CompileResult {
        output_path,
        header_path,
        language,
        stdout,
        stderr,
        warnings: synth_warnings,
    })
}

/// Nim and Odin archive static libraries with whatever `ar` is first on `PATH`. On macOS a
/// Homebrew GNU binutils `ar` there writes archives Apple's linker rejects ("member not 8-byte
/// aligned"), so they get a private `PATH` entry whose `ar` is the system one.
fn prefer_system_archiver(command: &mut Command, output_dir: &Path) -> std::io::Result<()> {
    let system_ar = Path::new("/usr/bin/ar");
    if !cfg!(target_os = "macos") || !system_ar.is_file() {
        return Ok(());
    }
    let tools = output_dir.join(".eq-tools");
    std::fs::create_dir_all(&tools)?;
    let link = tools.join("ar");
    if std::fs::read_link(&link).ok().as_deref() != Some(system_ar) {
        // Create under a name unique to this call (process id + counter: `compile_batch` runs
        // several of these on threads of one process) and rename over the target. Rename is
        // atomic, so concurrent builds sharing an output dir never race on a half-made link.
        static STAGED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let call = STAGED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let staged = tools.join(format!("ar.{}.{call}", std::process::id()));
        let _ = std::fs::remove_file(&staged);
        #[cfg(unix)]
        std::os::unix::fs::symlink(system_ar, &staged)?;
        std::fs::rename(&staged, &link)?;
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    let joined = std::env::join_paths(std::iter::once(tools).chain(std::env::split_paths(&path)))
        .map_err(std::io::Error::other)?;
    command.env("PATH", joined);
    Ok(())
}

fn artifact_filename(language: Language, stem: &str) -> String {
    match language {
        Language::C | Language::Cpp | Language::Zig | Language::D | Language::Hare => {
            format!("{stem}.o")
        }
        Language::Odin => format!("lib{stem}.a"),

        // rustc's `-l static=<name>` / `-l dylib=<name>` only finds `lib<name>.<ext>` on Unix, so
        // archives and shared libraries carry the `lib` prefix `emit_cargo_link` strips again.
        Language::Nim => format!("lib{stem}.a"),
        // scriptc's own name is `<stem>.lib.a`; the `lib` prefix keeps rustc's
        // `-l` lookup (`cargo:rustc-link-lib=static=…`) working.
        Language::ScriptC => format!("lib{stem}.a"),
        // Unreachable through `compile_inner`, which refuses C# producers.
        Language::CSharp => format!("{stem}.dll"),
        Language::Rust | Language::V => {
            if cfg!(target_os = "windows") {
                format!("{stem}.dll")
            } else if cfg!(target_os = "macos") {
                format!("lib{stem}.dylib")
            } else {
                format!("lib{stem}.so")
            }
        }
    }
}

/// Write the library profile and the matching C header that scriptc's library
/// mode needs: scriptc emits no header, and the profile is where the module's
/// exported C symbols are declared. `equilibrium.toml` may refine the ABI
/// surface (marshalling classes) and the profile's emission.
fn write_scriptc_library_surface(
    input: &Path,
    output_dir: &Path,
    stem: &str,
    archive: &Path,
    options: &CompileOptions,
) -> Result<(), CompileError> {
    let settings = crate::scriptc::target_settings(input, options.config_path.as_deref())
        .map_err(|message| CompileError::InvalidConfig { message })?;
    let bytes = std::fs::read(input)?;
    let content = String::from_utf8_lossy(&bytes);
    let exports: Vec<crate::scriptc::ScriptcExport> = crate::scriptc::scan_declarations(&content)
        .into_iter()
        .filter_map(|declaration| crate::scriptc::parse_signature(&declaration.signature).ok())
        .collect();
    let exports = crate::scriptc::apply_overrides(exports, &settings)
        .map_err(|message| CompileError::InvalidConfig { message })?;
    let prefix = crate::scriptc::symbol_prefix(stem);
    std::fs::write(
        crate::scriptc::profile_path(input, archive),
        crate::scriptc::profile_json(stem, &prefix, input, &exports, settings.emission),
    )?;
    std::fs::write(
        output_dir.join(format!("{stem}.h")),
        crate::scriptc::header(&prefix, stem, &exports),
    )?;
    Ok(())
}

struct ManagedHeader {
    directory: PathBuf,
    stage: PathBuf,
    stem: String,
    _lock: std::fs::File,
}

fn header_hash(bytes: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hash);
    hash.finish()
}

struct HeaderInitialization(PathBuf);

impl Drop for HeaderInitialization {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(self.0.join("owner"));
        let _ = std::fs::remove_file(self.0.join("lock"));
        let _ = std::fs::remove_dir(&self.0);
    }
}

impl ManagedHeader {
    fn new(output: &Path, input: &Path, stem: &str) -> std::io::Result<Self> {
        use std::io::Write;
        let directory = output.join(format!(".equilibrium-header-{stem}"));
        let identity = format!("equilibrium-header-v1\n{}\n", input.display());
        if !directory.exists()
            && std::fs::symlink_metadata(&directory)
                .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
        {
            static NEXT_INIT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let staging = loop {
                let path = output.join(format!(
                    ".equilibrium-init-{}-{}",
                    std::process::id(),
                    NEXT_INIT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                ));
                match std::fs::create_dir(&path) {
                    Ok(()) => break HeaderInitialization(path),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(error) => return Err(error),
                }
            };
            {
                let mut owner = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(staging.0.join("owner"))?;
                owner.write_all(identity.as_bytes())?;
                std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(staging.0.join("lock"))?;
            }
            // Cooperative initializers publish nonempty directories; an existing path is
            // validated below rather than deliberately replaced.
            if std::fs::symlink_metadata(&directory)
                .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
            {
                if let Err(error) = std::fs::rename(&staging.0, &directory) {
                    if std::fs::symlink_metadata(&directory).is_err() {
                        return Err(error);
                    }
                }
            }
        }
        if !std::fs::symlink_metadata(&directory)?.file_type().is_dir()
            || !std::fs::symlink_metadata(directory.join("owner"))?
                .file_type()
                .is_file()
            || std::fs::read_to_string(directory.join("owner"))? != identity
        {
            return Err(std::io::Error::other("unowned generated-header directory"));
        }
        let lock_path = directory.join("lock");
        let metadata = std::fs::symlink_metadata(&lock_path)?;
        if !metadata.file_type().is_file() || metadata.len() != 0 {
            return Err(std::io::Error::other("unowned generated-header lock"));
        }
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(lock_path)?;
        lock.try_lock().map_err(|error| {
            std::io::Error::other(format!("generated-header lock unavailable: {error}"))
        })?;
        static NEXT_STAGE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let stage = loop {
            let stage = directory.join(format!(
                "stage-{}-{}",
                std::process::id(),
                NEXT_STAGE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            match std::fs::create_dir(&stage) {
                Ok(()) => break stage,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        };
        Ok(Self {
            stage,
            directory,
            stem: stem.to_string(),
            _lock: lock,
        })
    }

    fn publish(&self, staged: &Path) -> std::io::Result<PathBuf> {
        let target = self.directory.join(format!("{}.h", self.stem));
        match std::fs::symlink_metadata(&target) {
            Ok(meta) => {
                if !meta.file_type().is_file() {
                    return Err(std::io::Error::other("unowned generated-header path"));
                }
                let old = std::fs::read(&target)?;
                let Some(end) = old.iter().position(|b| *b == b'\n') else {
                    return Err(std::io::Error::other("unowned generated-header file"));
                };
                let expected = format!(
                    "/* equilibrium-managed {:016x} */",
                    header_hash(&old[end + 1..])
                );
                if old[..end] != *expected.as_bytes() {
                    return Err(std::io::Error::other(
                        "modified or unowned generated-header file",
                    ));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        let contents = std::fs::read(staged)?;
        let mut marked = format!(
            "/* equilibrium-managed {:016x} */\n",
            header_hash(&contents)
        )
        .into_bytes();
        marked.extend_from_slice(&contents);
        std::fs::write(staged, marked)?;
        std::fs::rename(staged, &target)?;
        Ok(target)
    }
}

impl Drop for ManagedHeader {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(self.stage.join(format!("{}.h", self.stem)));
        let _ = std::fs::remove_dir(&self.stage);
        let _ = self._lock.unlock();
    }
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

/// Write a header synthesized from the source's exported functions (`header_synth`), for
/// languages whose compiler writes none. Warnings name exports that could not be mapped.
fn synthesize_header(
    input: &Path,
    header_output: &Path,
    language: Language,
    compiler: &Path,
    compile_args: &[String],
    warnings: &mut Vec<String>,
) -> Result<Option<PathBuf>, CompileError> {
    let (source, includes) = if matches!(language, Language::C | Language::Cpp) {
        let mut preprocess_args = compile_args.to_vec();
        preprocess_args[0] = "-E".to_string();
        let output_index = preprocess_args
            .iter()
            .position(|arg| arg == "-o")
            .expect("C compiler output flag");
        preprocess_args.drain(output_index..=output_index + 1);
        preprocess_args.push("-dI".to_string());
        preprocess_args.push("-dD".to_string());
        let mut command = Command::new(compiler);
        command
            .args(&preprocess_args)
            .current_dir(input.parent().unwrap_or(Path::new(".")));
        let output = bounded_output(&mut command, MAX_PREPROCESSED_BYTES)?;
        if !output.status.success() {
            return Err(CompileError::CompilationFailed {
                stderr: format!(
                    "header synthesis preprocessing failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                ),
                exit_code: output.status.code(),
            });
        }
        warnings.push("C/C++ header synthesis excludes system-header declarations and may not represent complex declarations safely; supply a header for a guaranteed ABI.".to_string());
        let preprocessed = String::from_utf8(output.stdout).map_err(|e| {
            CompileError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e))
        })?;
        filter_system_headers(&preprocessed)?
    } else {
        (
            String::from_utf8_lossy(&std::fs::read(input)?).into_owned(),
            Vec::new(),
        )
    };
    let Some(synthesized) = crate::header_synth::synthesize(language, &source) else {
        return Ok(None);
    };
    let header = format!(
        "{}{}",
        includes.join("\n"),
        if includes.is_empty() {
            String::new()
        } else {
            "\n".to_string()
        }
    ) + &synthesized.header;
    std::fs::write(header_output, header)?;
    if matches!(language, Language::C | Language::Cpp) {
        let mut validation_args = compile_args.to_vec();
        validation_args[0] = "-fsyntax-only".to_string();
        let output_index = validation_args
            .iter()
            .position(|arg| arg == "-o")
            .expect("C compiler output flag");
        validation_args.drain(output_index..=output_index + 1);
        let input_index = validation_args
            .iter()
            .position(|arg| arg == &input.to_string_lossy())
            .expect("C compiler input");
        validation_args.remove(input_index);
        let output = Command::new(compiler)
            .args(&validation_args)
            .args([
                "-include",
                &header_output.to_string_lossy(),
                "-x",
                if language == Language::C { "c" } else { "c++" },
                "-",
            ])
            .stdin(std::process::Stdio::null())
            .current_dir(input.parent().unwrap_or(Path::new(".")))
            .output()?;
        if !output.status.success() {
            return Err(CompileError::CompilationFailed {
                stderr: format!(
                    "synthesized header cannot be compiled; supply a header: {}",
                    String::from_utf8_lossy(&output.stderr)
                ),
                exit_code: output.status.code(),
            });
        }
    }
    warnings.extend(synthesized.warnings);
    Ok(Some(header_output.to_path_buf()))
}

fn bounded_output(command: &mut Command, limit: u64) -> std::io::Result<std::process::Output> {
    use std::io::Read;
    use std::process::Stdio;
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stderr = child.stderr.take().expect("piped stderr");
    let reader = std::thread::spawn(move || {
        let mut stderr = stderr;
        let mut bytes = Vec::new();
        stderr.by_ref().take(1024 * 1024).read_to_end(&mut bytes)?;
        std::io::copy(&mut stderr, &mut std::io::sink())?;
        Ok::<_, std::io::Error>(bytes)
    });
    let mut stdout = Vec::new();
    let read = child
        .stdout
        .take()
        .expect("piped stdout")
        .take(limit + 1)
        .read_to_end(&mut stdout);
    if read.is_err() || stdout.len() as u64 > limit {
        let _ = child.kill();
    }
    let status = child.wait()?;
    let stderr = reader
        .join()
        .map_err(|_| std::io::Error::other("stderr reader panicked"))??;
    read?;
    if stdout.len() as u64 > limit {
        return Err(std::io::Error::other(format!(
            "preprocessor output exceeds {limit} bytes"
        )));
    }
    Ok(std::process::Output {
        status,
        stdout,
        stderr,
    })
}

// GCC/Clang line markers identify system-header regions with flag 3. Keep user
// includes, but don't try to reproduce compiler and libc implementation types.
fn filter_system_headers(source: &str) -> Result<(String, Vec<String>), CompileError> {
    let mut system = false;
    let mut saw_marker = false;
    let mut filtered = String::new();
    let mut includes = Vec::new();
    let mut source_macro_directive = false;
    for line in source.lines() {
        let marker = line.strip_prefix("# ").and_then(|line| {
            let (number, rest) = line.split_once(' ')?;
            number.parse::<usize>().ok()?;
            let rest = rest.strip_prefix('"')?;
            let (file, flags) = rest.rsplit_once('"')?;
            Some((file, flags))
        });
        if let Some((file, flags)) = marker {
            saw_marker = true;
            system = file.starts_with('<') || flags.split_whitespace().any(|flag| flag == "3");
        } else if !system {
            let directive = line.trim();
            if directive.starts_with("#define ") || directive.starts_with("#undef ") {
                source_macro_directive = true;
            } else if let Some(rest) = directive.strip_prefix("#include <") {
                if source_macro_directive {
                    return Err(CompileError::InvalidConfig {
                        message: "C/C++ header synthesis cannot safely reproduce source-defined macro directives preceding an active angle-bracket include; supply a handwritten header".to_string(),
                    });
                }
                if let Some((path, _)) = rest.split_once('>') {
                    let directive = format!("#include <{path}>");
                    if !includes.contains(&directive) {
                        includes.push(directive);
                    }
                }
            } else {
                filtered.push_str(line);
                filtered.push('\n');
            }
        }
    }
    if !saw_marker {
        return Err(CompileError::InvalidConfig {
            message: "C/C++ header synthesis requires preprocessor line markers; remove -P or supply a header".to_string(),
        });
    }
    Ok((filtered, includes))
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
    #[cfg(unix)]
    #[test]
    fn preprocessor_output_limit_is_enforced() {
        let mut command = std::process::Command::new("sh");
        command.args(["-c", "printf '123456789'"]);
        let error = super::bounded_output(&mut command, 8).unwrap_err();
        assert!(error.to_string().contains("exceeds 8 bytes"));
    }

    #[test]
    fn scriptc_surface_rebuild_uses_fresh_header_directory() {
        use super::*;
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("math.ts");
        let sibling = dir.path().join("math.h");
        let archive = dir.path().join("libmath.a");
        std::fs::write(&sibling, "caller header").unwrap();
        for (source, expected) in [
            (
                "export function first(x: number): number { return x; }",
                "math_first",
            ),
            (
                "export function second(x: number): number { return x; }",
                "math_second",
            ),
        ] {
            std::fs::write(&input, source).unwrap();
            let managed = ManagedHeader::new(dir.path(), &input, "math").unwrap();
            write_scriptc_library_surface(
                &input,
                &managed.stage,
                "math",
                &archive,
                &CompileOptions::default(),
            )
            .unwrap();
            let header =
                std::fs::read_to_string(managed.publish(&managed.stage.join("math.h")).unwrap())
                    .unwrap();
            assert!(header.contains(expected), "{header}");
            assert_eq!(std::fs::read_to_string(&sibling).unwrap(), "caller header");
        }
    }

    /// `compile_batch` compiles on threads of one process, so archiver setup must not race
    /// between threads sharing an output directory.
    #[cfg(target_os = "macos")]
    #[test]
    fn archiver_setup_is_safe_across_threads_sharing_an_output_dir() {
        use std::sync::{Arc, Barrier};
        for _ in 0..50 {
            let dir = tempfile::tempdir().unwrap();
            let barrier = Arc::new(Barrier::new(8));
            std::thread::scope(|scope| {
                let handles: Vec<_> = (0..8)
                    .map(|_| {
                        let barrier = Arc::clone(&barrier);
                        let dir = dir.path();
                        scope.spawn(move || {
                            barrier.wait();
                            prefer_system_archiver(&mut Command::new("true"), dir)
                        })
                    })
                    .collect();
                for handle in handles {
                    handle
                        .join()
                        .unwrap()
                        .expect("archiver setup should not fail");
                }
            });
            assert_eq!(
                std::fs::read_link(dir.path().join(".eq-tools/ar")).unwrap(),
                Path::new("/usr/bin/ar")
            );
        }
    }
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
    fn csharp_sources_are_refused_instead_of_compiled_to_a_managed_dll() {
        // csc emits a managed assembly, which cannot be linked as a C ABI library; refusing it up
        // front (whatever C# toolchain is installed) beats a confusing compile or link failure.
        let dir = tempdir().unwrap();
        let source = dir.path().join("Lib.cs");
        std::fs::write(
            &source,
            "public static class Lib { public static int Add(int a, int b) => a + b; }\n",
        )
        .unwrap();

        let error = compile_to_c(&source, &dir.path().join("out")).unwrap_err();
        assert!(
            matches!(
                error,
                CompileError::UnsupportedCOutput {
                    language: Language::CSharp
                }
            ),
            "{error}"
        );
        assert!(error.to_string().contains("Native AOT"), "{error}");
    }

    #[test]
    fn test_compile_relative_output_dir_stays_with_the_caller() {
        // Compilers run with the source's directory as their working directory,
        // so a relative output dir must be resolved before it is handed over.
        if find_compiler(Language::C).is_none() {
            return;
        }

        let dir = tempdir().unwrap();
        let c_file = dir.path().join("test.c");
        std::fs::write(&c_file, "int add(int a, int b) { return a + b; }\n").unwrap();

        let output_dir = PathBuf::from(format!("target/relative-output-{}", std::process::id()));
        let result = compile_to_c(&c_file, &output_dir).unwrap();

        assert!(result.output_path.is_absolute());
        assert!(result.output_path.exists());
        assert!(
            !dir.path().join("target").exists(),
            "output landed inside the source directory"
        );
        let _ = std::fs::remove_dir_all(&output_dir);
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
