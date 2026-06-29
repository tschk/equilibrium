//! Automatic library discovery and binding generation.
//!
//! Scans directories for C headers, generates bindings for all discovered
//! libraries, and creates a unified module structure for frictionless imports.

use std::fs;
use std::path::{Path, PathBuf};

use crate::bindings::{generate_bindings, BindingOptions, GeneratedBinding};

/// A discovered C library with its headers.
#[derive(Clone, Debug)]
pub struct LibraryDiscovery {
    /// Library name (e.g., "stm32f4xx_hal")
    pub name: String,
    /// Root directory of the library
    pub root: PathBuf,
    /// All header files found
    pub headers: Vec<PathBuf>,
    /// Main/public header (if identifiable)
    pub main_header: Option<PathBuf>,
}

/// Options for automatic binding generation.
#[derive(Clone, Debug)]
pub struct AutoBindingOptions {
    /// Output directory for generated bindings (defaults to OUT_DIR/bindings)
    pub output_dir: Option<PathBuf>,
    /// Include subdirectories when scanning
    pub recursive: bool,
    /// File patterns to exclude (e.g., ["*_internal.h", "test_*.h"])
    pub exclude_patterns: Vec<String>,
    /// Whether to generate a unified mod.rs that re-exports everything
    pub generate_mod_rs: bool,
    /// Default binding options for all libraries
    pub default_binding_options: BindingOptions,
}

impl Default for AutoBindingOptions {
    fn default() -> Self {
        Self {
            output_dir: None,
            recursive: true,
            exclude_patterns: vec![
                "*_internal.h".to_string(),
                "*_private.h".to_string(),
                "test_*.h".to_string(),
            ],
            generate_mod_rs: true,
            default_binding_options: BindingOptions::default(),
        }
    }
}

/// Scanner for discovering C libraries in a directory tree.
pub struct LibraryScanner {
    root: PathBuf,
    options: AutoBindingOptions,
}

impl LibraryScanner {
    /// Create a new scanner for the given directory.
    pub fn new<P: AsRef<Path>>(root: P) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
            options: AutoBindingOptions::default(),
        }
    }

    /// Configure scanning options.
    pub fn with_options(mut self, options: AutoBindingOptions) -> Self {
        self.options = options;
        self
    }

    /// Scan and discover all C libraries.
    pub fn scan(&self) -> Result<Vec<LibraryDiscovery>, String> {
        if !self.root.exists() {
            return Err(format!("Directory not found: {}", self.root.display()));
        }

        let mut discoveries = Vec::new();
        self.scan_directory(&self.root, &mut discoveries)?;

        Ok(discoveries)
    }

    fn scan_directory(
        &self,
        dir: &Path,
        discoveries: &mut Vec<LibraryDiscovery>,
    ) -> Result<(), String> {
        let entries = fs::read_dir(dir)
            .map_err(|e| format!("Failed to read directory {}: {}", dir.display(), e))?;

        let mut headers = Vec::new();

        for entry in entries {
            let entry = entry.map_err(|e| format!("Failed to read entry: {}", e))?;
            let path = entry.path();

            if path.is_file() {
                if let Some(ext) = path.extension() {
                    if ext == "h" && !self.should_exclude(&path) {
                        headers.push(path);
                    }
                }
            } else if path.is_dir() && self.options.recursive {
                // Check if this directory contains headers (it's a library)
                let subheaders: Vec<PathBuf> = fs::read_dir(&path)
                    .ok()
                    .map(|entries| {
                        entries
                            .filter_map(|e| e.ok())
                            .filter(|e| {
                                e.path().extension().is_some_and(|ext| ext == "h")
                                    && !self.should_exclude(&e.path())
                            })
                            .map(|e| e.path())
                            .collect()
                    })
                    .unwrap_or_default();

                if !subheaders.is_empty() {
                    // This subdirectory is a library
                    let lib_name = path
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("unknown")
                        .to_string();

                    discoveries.push(LibraryDiscovery {
                        name: lib_name,
                        root: path.clone(),
                        headers: subheaders.clone(),
                        main_header: self.find_main_header(&subheaders),
                    });
                }

                // Continue scanning subdirectories
                self.scan_directory(&path, discoveries)?;
            }
        }

        // If we found headers directly in this directory, create a discovery
        if !headers.is_empty() {
            let lib_name = dir
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("root")
                .to_string();

            discoveries.push(LibraryDiscovery {
                name: lib_name,
                root: dir.to_path_buf(),
                headers: headers.clone(),
                main_header: self.find_main_header(&headers),
            });
        }

        Ok(())
    }

    fn should_exclude(&self, path: &Path) -> bool {
        let filename = path.file_name().and_then(|s| s.to_str()).unwrap_or("");

        self.options.exclude_patterns.iter().any(|pattern| {
            if let Some(suffix) = pattern.strip_prefix('*') {
                filename.ends_with(suffix)
            } else if let Some(prefix) = pattern.strip_suffix('*') {
                filename.starts_with(prefix)
            } else {
                filename == pattern
            }
        })
    }

    fn find_main_header(&self, headers: &[PathBuf]) -> Option<PathBuf> {
        // Heuristics for finding the main header:
        // 1. Exact match with directory name
        // 2. Contains "api", "public", or library name
        // 3. Shortest name (likely the main one)

        headers
            .iter()
            .min_by_key(|h| {
                let filename = h.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                let score = if filename.contains("api") || filename.contains("public") {
                    0
                } else if filename.len() < 15 {
                    1
                } else {
                    2
                };
                (score, filename.len())
            })
            .cloned()
    }

    /// Generate bindings for all discovered libraries.
    pub fn generate_all(&self) -> Result<GenerationResult, String> {
        let discoveries = self.scan()?;
        let output_dir = self.options.output_dir.clone().unwrap_or_else(|| {
            PathBuf::from(std::env::var("OUT_DIR").unwrap_or_else(|_| ".".to_string()))
                .join("bindings")
        });

        fs::create_dir_all(&output_dir)
            .map_err(|e| format!("Failed to create output dir: {}", e))?;

        let mut results = Vec::new();
        let mut mod_declarations = Vec::new();

        for discovery in discoveries {
            let binding_opts = self.options.default_binding_options.clone();

            // Generate bindings for EACH header file (not just the main one)
            for header in &discovery.headers {
                let header_stem = header
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown");

                match generate_bindings(header, &binding_opts) {
                    Ok(binding) => {
                        // Use header filename to avoid duplicates
                        let safe_name = header_stem.replace(['-', '.'], "_");
                        let output_file = output_dir.join(format!("{}.rs", safe_name));

                        fs::write(&output_file, &binding.code).map_err(|e| {
                            format!("Failed to write {}: {}", output_file.display(), e)
                        })?;

                        mod_declarations.push(format!("pub mod {};", safe_name));

                        results.push(LibraryBindingResult {
                            library: discovery.clone(),
                            output_file,
                            binding,
                        });
                    }
                    Err(e) => {
                        eprintln!(
                            "Warning: Failed to generate bindings for {}: {}",
                            header.display(),
                            e
                        );
                    }
                }
            }
        }

        // Generate mod.rs that re-exports everything
        if self.options.generate_mod_rs && !mod_declarations.is_empty() {
            // Deduplicate module declarations
            mod_declarations.sort();
            mod_declarations.dedup();

            let mod_rs_content = format!(
                "// Auto-generated by equilibrium-ffi\n// Re-exports all discovered C libraries\n\n{}\n",
                mod_declarations.join("\n")
            );

            let mod_rs = output_dir.join("mod.rs");
            fs::write(&mod_rs, mod_rs_content)
                .map_err(|e| format!("Failed to write mod.rs: {}", e))?;
        }

        Ok(GenerationResult {
            output_dir,
            libraries: results,
        })
    }
}

/// Result of generating bindings for multiple libraries.
#[derive(Debug)]
pub struct GenerationResult {
    /// Output directory where bindings were written
    pub output_dir: PathBuf,
    /// Results for each library
    pub libraries: Vec<LibraryBindingResult>,
}

/// Result of generating bindings for a single library.
#[derive(Debug)]
pub struct LibraryBindingResult {
    /// The discovered library
    pub library: LibraryDiscovery,
    /// Path to the generated .rs file
    pub output_file: PathBuf,
    /// The generated binding
    pub binding: GeneratedBinding,
}

/// Scan a directory for C libraries and generate bindings.
///
/// This is the main convenience function for frictionless binding generation.
///
/// # Example
///
/// ```rust,no_run
/// // In build.rs:
/// equilibrium_ffi::scan_c_libraries("libs/stm32")
///     .with_options(equilibrium_ffi::AutoBindingOptions {
///         recursive: true,
///         ..Default::default()
///     })
///     .generate_all()
///     .expect("Failed to generate bindings");
/// ```
///
/// Then in your Rust code:
/// ```rust,ignore
/// mod bindings {
///     include!(concat!(env!("OUT_DIR"), "/bindings/mod.rs"));
/// }
///
/// unsafe {
///     bindings::stm32f4xx_hal::HAL_Init();
/// }
/// ```
pub fn scan_c_libraries<P: AsRef<Path>>(root: P) -> LibraryScanner {
    LibraryScanner::new(root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_scan_simple_library() {
        let dir = tempdir().unwrap();
        let lib_dir = dir.path().join("mylib");
        fs::create_dir(&lib_dir).unwrap();

        fs::write(lib_dir.join("api.h"), "int add(int a, int b);").unwrap();
        fs::write(lib_dir.join("utils.h"), "void cleanup(void);").unwrap();

        let scanner = LibraryScanner::new(dir.path());
        let discoveries = scanner.scan().unwrap();

        assert!(!discoveries.is_empty());
        let mylib = discoveries.iter().find(|d| d.name == "mylib").unwrap();
        assert_eq!(mylib.headers.len(), 2);
    }

    #[test]
    fn test_scan_nested_libraries() {
        let dir = tempdir().unwrap();

        // Create structure: libs/hal/gpio.h, libs/drivers/uart.h
        let hal_dir = dir.path().join("hal");
        let drivers_dir = dir.path().join("drivers");
        fs::create_dir(&hal_dir).unwrap();
        fs::create_dir(&drivers_dir).unwrap();

        fs::write(hal_dir.join("gpio.h"), "void GPIO_Init(void);").unwrap();
        fs::write(drivers_dir.join("uart.h"), "void UART_Init(void);").unwrap();

        let scanner = LibraryScanner::new(dir.path()).with_options(AutoBindingOptions {
            recursive: true,
            ..Default::default()
        });

        let discoveries = scanner.scan().unwrap();
        assert!(discoveries.len() >= 2);
    }

    #[test]
    fn test_exclude_patterns() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("api.h"), "int add(int a, int b);").unwrap();
        fs::write(
            dir.path().join("api_internal.h"),
            "int internal_func(void);",
        )
        .unwrap();

        let scanner = LibraryScanner::new(dir.path());
        let discoveries = scanner.scan().unwrap();

        let headers = &discoveries[0].headers;
        assert_eq!(headers.len(), 1);
        assert!(headers[0].ends_with("api.h"));
    }

    #[test]
    fn test_generate_all_creates_mod_rs() {
        let dir = tempdir().unwrap();
        let lib_dir = dir.path().join("testlib");
        fs::create_dir(&lib_dir).unwrap();
        fs::write(lib_dir.join("test.h"), "int test(void);").unwrap();

        let output = tempdir().unwrap();

        let scanner = LibraryScanner::new(dir.path()).with_options(AutoBindingOptions {
            output_dir: Some(output.path().to_path_buf()),
            generate_mod_rs: true,
            ..Default::default()
        });

        let result = scanner.generate_all().unwrap();
        assert!(!result.libraries.is_empty());

        let mod_rs = output.path().join("mod.rs");
        assert!(mod_rs.exists());

        let content = fs::read_to_string(mod_rs).unwrap();
        // Module name is based on header filename, not directory
        assert!(content.contains("pub mod test;"));
    }
}
