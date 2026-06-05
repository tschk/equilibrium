//! Language detection for source files.

use std::path::Path;

/// Supported languages that can be compiled to C.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    /// V language (vlang.io)
    V,
    /// Zig language
    Zig,
    /// C (already native)
    C,
    /// C++
    Cpp,
    /// C#
    CSharp,
    /// Rust (for cbindgen)
    Rust,
    /// D language
    D,
    /// Nim language
    Nim,
    /// Odin language
    Odin,
    /// Hare language
    Hare,
}

/// Information about a detected language.
#[derive(Clone, Debug)]
pub struct LanguageInfo {
    pub language: Language,
    pub compiler: Option<String>,
    pub version: Option<String>,
}

impl Language {
    pub fn cli_name(&self) -> &'static str {
        match self {
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

    pub fn from_cli_name(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "v" | "vlang" => Some(Language::V),
            "zig" => Some(Language::Zig),
            "c" => Some(Language::C),
            "cpp" | "c++" | "cxx" => Some(Language::Cpp),
            "csharp" | "c#" | "cs" | "dotnet" => Some(Language::CSharp),
            "rust" | "rs" => Some(Language::Rust),
            "d" => Some(Language::D),
            "nim" => Some(Language::Nim),
            "odin" => Some(Language::Odin),
            "hare" => Some(Language::Hare),
            _ => None,
        }
    }

    /// Get the file extensions for this language.
    pub fn extensions(&self) -> &[&str] {
        match self {
            Language::V => &["v"],
            Language::Zig => &["zig"],
            Language::C => &["c", "h"],
            Language::Cpp => &["cpp", "cxx", "cc", "hpp", "hxx"],
            Language::CSharp => &["cs"],
            Language::Rust => &["rs"],
            Language::D => &["d", "di"],
            Language::Nim => &["nim", "nims"],
            Language::Odin => &["odin"],
            Language::Hare => &["ha"],
        }
    }

    /// Get the typical compiler command for this language.
    pub fn default_compiler(&self) -> &str {
        match self {
            Language::V => "v",
            Language::Zig => "zig",
            Language::C => "clang",
            Language::Cpp => "clang++",
            Language::CSharp => "csc",
            Language::Rust => "rustc",
            Language::D => "ldc2", // or dmd, gdc
            Language::Nim => "nim",
            Language::Odin => "odin",
            Language::Hare => "hare",
        }
    }

    /// Get alternative compiler names to try.
    pub fn alternative_compilers(&self) -> &[&str] {
        match self {
            Language::D => &["dmd", "gdc"],
            Language::C => &["gcc", "cc"],
            Language::Cpp => &["g++", "c++"],
            _ => &[],
        }
    }

    /// Get the command to compile to C intermediate.
    pub fn to_c_args(&self, input: &str, output: &str) -> Vec<String> {
        match self {
            Language::V => vec![
                "-o".to_string(),
                output.to_string(),
                "-backend".to_string(),
                "c".to_string(),
                input.to_string(),
            ],
            Language::Zig => {
                // Zig doesn't have direct C output, but we can use translate-c for headers
                // For actual code, we emit object files
                vec![
                    "build-obj".to_string(),
                    format!("-femit-bin={output}"),
                    input.to_string(),
                ]
            }
            Language::C => {
                // C is already C, just preprocess
                vec![
                    "-E".to_string(),
                    "-o".to_string(),
                    output.to_string(),
                    input.to_string(),
                ]
            }
            Language::Cpp => {
                // Compile to object, we'll need headers separately
                vec![
                    "-c".to_string(),
                    "-o".to_string(),
                    output.to_string(),
                    input.to_string(),
                ]
            }
            Language::CSharp => {
                // C# to native requires AOT compilation
                vec![
                    "-target:library".to_string(),
                    format!("-out:{output}"),
                    input.to_string(),
                ]
            }
            Language::Rust => {
                // Rust uses cbindgen for headers + normal compilation
                vec![
                    "--crate-type=cdylib".to_string(),
                    "-o".to_string(),
                    output.to_string(),
                    input.to_string(),
                ]
            }
            Language::D => {
                // D can emit C headers with -HC flag (LDC2)
                vec![
                    "-c".to_string(),
                    "-of".to_string(),
                    output.to_string(),
                    "-HC".to_string(), // Generate C header
                    input.to_string(),
                ]
            }
            Language::Nim => {
                // Nim compiles to C by default
                vec![
                    "c".to_string(),
                    "--nimcache:.".to_string(),
                    format!("-o:{output}"),
                    input.to_string(),
                ]
            }
            Language::Odin => {
                // Odin compiles to object files
                vec![
                    "build".to_string(),
                    input.to_string(),
                    "-out:".to_string() + output,
                    "-build-mode:obj".to_string(),
                ]
            }
            Language::Hare => {
                // Hare compiles to object files via QBE
                vec![
                    "build".to_string(),
                    "-o".to_string(),
                    output.to_string(),
                    input.to_string(),
                ]
            }
        }
    }

    /// Get all supported languages.
    pub fn all() -> &'static [Language] {
        &[
            Language::V,
            Language::Zig,
            Language::C,
            Language::Cpp,
            Language::CSharp,
            Language::Rust,
            Language::D,
            Language::Nim,
            Language::Odin,
            Language::Hare,
        ]
    }
}

/// Detect the language of a source file based on extension.
pub fn detect_language(path: &Path) -> Option<Language> {
    let ext = path.extension()?.to_str()?.to_lowercase();

    for lang in Language::all() {
        if lang.extensions().contains(&ext.as_str()) {
            return Some(*lang);
        }
    }

    None
}

/// Check if a compiler is available on the system.
pub fn find_compiler(language: Language) -> Option<LanguageInfo> {
    let compiler_name = language.default_compiler();

    // Check primary compiler
    if which::which(compiler_name).is_ok() {
        return Some(LanguageInfo {
            language,
            compiler: Some(compiler_name.to_string()),
            version: get_compiler_version(compiler_name),
        });
    }

    // Try alternatives
    for alt in language.alternative_compilers() {
        if which::which(alt).is_ok() {
            return Some(LanguageInfo {
                language,
                compiler: Some((*alt).to_string()),
                version: get_compiler_version(alt),
            });
        }
    }

    None
}

fn get_compiler_version(compiler: &str) -> Option<String> {
    let output = std::process::Command::new(compiler)
        .arg("--version")
        .output()
        .ok()?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        // Get first line
        stdout.lines().next().map(|s| s.to_string())
    } else {
        None
    }
}

/// Scan a directory and detect all source files with their languages.
pub fn scan_directory(dir: &Path) -> Vec<(std::path::PathBuf, Language)> {
    let mut results = Vec::new();

    fn visit(dir: &Path, results: &mut Vec<(std::path::PathBuf, Language)>) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    // Skip common non-source directories
                    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    if !matches!(
                        name,
                        "target"
                            | "node_modules"
                            | ".git"
                            | "build"
                            | "dist"
                            | "zig-cache"
                            | "nimcache"
                    ) {
                        visit(&path, results);
                    }
                } else if let Some(lang) = detect_language(&path) {
                    results.push((path, lang));
                }
            }
        }
    }

    visit(dir, &mut results);
    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_detect_v() {
        let path = Path::new("mylib.v");
        assert_eq!(detect_language(path), Some(Language::V));
    }

    #[test]
    fn test_detect_zig() {
        let path = Path::new("mylib.zig");
        assert_eq!(detect_language(path), Some(Language::Zig));
    }

    #[test]
    fn test_detect_cpp() {
        assert_eq!(detect_language(Path::new("foo.cpp")), Some(Language::Cpp));
        assert_eq!(detect_language(Path::new("foo.cxx")), Some(Language::Cpp));
        assert_eq!(detect_language(Path::new("foo.cc")), Some(Language::Cpp));
    }

    #[test]
    fn test_detect_d() {
        assert_eq!(detect_language(Path::new("foo.d")), Some(Language::D));
        assert_eq!(detect_language(Path::new("foo.di")), Some(Language::D));
    }

    #[test]
    fn test_detect_nim() {
        assert_eq!(detect_language(Path::new("foo.nim")), Some(Language::Nim));
    }

    #[test]
    fn test_detect_odin() {
        assert_eq!(detect_language(Path::new("foo.odin")), Some(Language::Odin));
    }

    #[test]
    fn test_detect_hare() {
        assert_eq!(detect_language(Path::new("foo.ha")), Some(Language::Hare));
    }

    #[test]
    fn test_detect_c_and_header() {
        assert_eq!(detect_language(Path::new("main.c")), Some(Language::C));
        assert_eq!(detect_language(Path::new("lib.h")), Some(Language::C));
    }

    #[test]
    fn test_detect_rust() {
        assert_eq!(detect_language(Path::new("main.rs")), Some(Language::Rust));
    }

    #[test]
    fn test_detect_csharp() {
        assert_eq!(
            detect_language(Path::new("Program.cs")),
            Some(Language::CSharp)
        );
    }

    #[test]
    fn test_detect_unknown() {
        assert_eq!(detect_language(Path::new("foo.py")), None);
        assert_eq!(detect_language(Path::new("foo.js")), None);
        assert_eq!(detect_language(Path::new("Makefile")), None);
    }

    #[test]
    fn test_detect_case_insensitive_extension() {
        // Extensions are lowercased before matching
        assert_eq!(detect_language(Path::new("FOO.C")), Some(Language::C));
        assert_eq!(detect_language(Path::new("main.RS")), Some(Language::Rust));
    }

    #[test]
    fn test_all_languages() {
        assert_eq!(Language::all().len(), 10);
    }

    #[test]
    fn test_find_compiler_c_available() {
        // clang or gcc is expected in any dev environment
        let info = find_compiler(Language::C);
        assert!(
            info.is_some(),
            "expected a C compiler (clang/gcc) to be on PATH"
        );
        let info = info.unwrap();
        assert!(info.compiler.is_some());
    }

    #[test]
    fn test_find_compiler_returns_version() {
        if let Some(info) = find_compiler(Language::C) {
            // version is best-effort; just ensure the field exists (may be None on exotic setups)
            let _ = info.version;
        }
    }

    #[test]
    fn test_to_c_args_c_preprocess() {
        let args = Language::C.to_c_args("foo.c", "foo.i");
        assert!(args.contains(&"-E".to_string()));
        assert!(args.contains(&"foo.c".to_string()));
        assert!(args.contains(&"foo.i".to_string()));
    }

    #[test]
    fn test_to_c_args_zig_no_duplicate_flag() {
        let args = Language::Zig.to_c_args("foo.zig", "foo.o");
        assert!(args.contains(&"build-obj".to_string()));
        let femit_count = args.iter().filter(|a| a.starts_with("-femit-bin")).count();
        assert_eq!(femit_count, 1, "should have exactly one -femit-bin flag");
    }

    #[test]
    fn test_scan_directory_empty() {
        let dir = tempdir().unwrap();
        let results = scan_directory(dir.path());
        assert!(results.is_empty());
    }

    #[test]
    fn test_scan_directory_finds_sources() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("lib.c"), "").unwrap();
        std::fs::write(dir.path().join("lib.v"), "").unwrap();
        std::fs::write(dir.path().join("README.md"), "").unwrap(); // not a source file

        let results = scan_directory(dir.path());
        assert_eq!(results.len(), 2);
        let langs: Vec<Language> = results.iter().map(|(_, l)| *l).collect();
        assert!(langs.contains(&Language::C));
        assert!(langs.contains(&Language::V));
    }

    #[test]
    fn test_scan_directory_skips_target() {
        let dir = tempdir().unwrap();
        let target_dir = dir.path().join("target");
        std::fs::create_dir(&target_dir).unwrap();
        std::fs::write(target_dir.join("generated.c"), "").unwrap(); // should be skipped
        std::fs::write(dir.path().join("main.rs"), "").unwrap();

        let results = scan_directory(dir.path());
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1, Language::Rust);
    }

    #[test]
    fn test_scan_directory_recurses() {
        let dir = tempdir().unwrap();
        let sub = dir.path().join("src");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("lib.zig"), "").unwrap();

        let results = scan_directory(dir.path());
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1, Language::Zig);
    }
}
