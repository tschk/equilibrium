//! **equilibrium-ffi** — Automatic C FFI generation
//!
//! This crate auto-detects C-compiling languages (V, Zig, C++, C#, etc.),
//! compiles them to C intermediate representation, and generates Rust bindings
//! so you can call foreign code like native modules.
//!
//! # Quick Start
//!
//! ```rust,ignore
//! use equilibrium_ffi::load;
//!
//! let lib = load("native/math.c")?;
//! println!("{}", lib.output_path.display());
//! ```
//!
//! # Supported Languages
//!
//! | Language | Compiler | C Backend |
//! |----------|----------|-----------|
//! | V (Vlang) | `v` | `v -o output.c -backend c` |
//! | Zig | `zig` | `zig build-obj -femit-asm` or C export |
//! | C/C++ | `clang`/`gcc` | Native |
//! | C# | `csc`/`mono` | P/Invoke + Native AOT |
//! | Rust | `rustc` | cbindgen |

mod bindings;
mod c_header;
mod compiler;
mod detector;
mod exports;
mod imports;
mod limits;
mod loader;
mod scanner;

pub use bindings::{
    generate_bindings, generate_bindings_from_content, BindingOptions, GeneratedBinding,
};
pub use compiler::{
    compile_batch, compile_to_c, compile_to_c_with_extra, CompileError, CompileResult,
};
pub use detector::{
    compiler_version_at, detect_language, find_binary, find_compiler, scan_directory, Language,
    LanguageInfo,
};
pub use exports::{
    discover_exports_with_options, ExportDiscovery, ExportError, ExportOptions, ExportSource,
};
pub use imports::{generate_imports, GeneratedImport, ImportOptions};
pub use loader::{load, load_with_options, LoadError, LoadOptions, LoadedModule};
pub use scanner::{
    scan_c_libraries, AutoBindingOptions, GenerationResult, LibraryBindingResult, LibraryDiscovery,
    LibraryScanner,
};
