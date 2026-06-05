# Equilibrium

**Load foreign code with one call**

Equilibrium auto-detects source files in various programming languages, compiles them to C intermediate representation, and loads the result into a Rust-friendly module handle. Binding generation is available when you need it, but `load()` is the primary path. Generated consumer wrappers can target the same C ABI surface for other supported languages.

## `eq` CLI

The `eq` CLI manages compilers and builds polyglot projects.

```bash
# Build
cargo install --path . --features cli

# Check which compilers are installed
eq check

# Install missing compilers (interactive multi-select, parallel)
eq install

# Install specific compilers
eq install zig nim d odin

# Build a project with all compilers on PATH
eq build --release --bin my-app

# Generate Rust FFI bindings from a C header (optional)
eq generate mylib.h -o src/mylib_ffi.rs

# Generate imports for another language
eq generate mylib.h --consumer zig -o src/mylib.zig
eq generate mylib.h --consumer all --out-dir generated-imports
```

**Install order per platform:**
- **Linux**: wax → brew/linuxbrew → apt / dnf / pacman
- **macOS**: wax → brew
- **Windows**: winget → scoop

Multiple compilers install in parallel.

## Quick Start

```rust
use equilibrium_ffi::load;

let lib = load("examples/c-ffi/mathlib.c")?;
println!("{}", lib.output_path.display());
```

`load()` compiles the source when needed, then gives you a loaded module wrapper you can inspect, reuse, or turn into generated bindings.

```rust
use equilibrium_ffi::{Language, LoadOptions, load_with_options};

let lib = load_with_options(
    "examples/c-ffi/mathlib.c",
    LoadOptions::default().consumer_languages([Language::Zig, Language::Nim]),
)?;

for generated in lib.imports {
    println!("{:?}: {}", generated.language, generated.code);
}
```

## How It Works

### 1. Load a source file

```rust
use equilibrium_ffi::load;

let lib = load("math.v")?;
println!("loaded: {}", lib.output_path.display());
```

## Quick Start: Using in Your Project

### 1. Add as a dependency

```toml
[dependencies]
equilibrium-ffi = "0.1"
```

### 2. Use in build.rs

```rust
// build.rs
use equilibrium_ffi::load;

fn main() {
    let _lib = load("src/native/math.v").unwrap();
    println!("cargo:rerun-if-changed=src/native/*");
}
```

### 3. Call from Rust

```rust
fn main() {
    let lib = equilibrium_ffi::load("src/native/math.v").unwrap();
    println!("{}", lib.output_path.display());
}
```

### Full Example

Use `load()` for the smallest path. Reach for `generate_bindings()` only when you already have a C header and want explicit Rust `extern` declarations:

```rust
let lib = equilibrium_ffi::load("native/math.c")?;
println!("{}", lib.output_path.display());
```

```bash
eq generate mylib.h -o src/mylib_ffi.rs
eq generate mylib.h --consumer csharp -o src/mylib.cs
```

## Supported Languages

| Language | Compiler | Notes |
|----------|----------|-------|
| **V (Vlang)** | `v` | `-backend c` outputs C |
| **Zig** | `zig` | `build-obj -OReleaseFast -fPIC` |
| **C** | `clang`/`gcc` | Already C (preprocessed) |
| **C++** | `clang++`/`g++` | Compiled to object files |
| **C#** | `dotnet` | Native AOT |
| **Rust** | `rustc` | cbindgen for header generation |
| **D** | `ldc2`/`dmd`/`gdc` | `-HC` flag for C headers |
| **Nim** | `nim` | Compiles to C by default, `--mm:none --app:staticlib` |
| **Odin** | `odin` | `-build-mode:obj -reloc-mode:pic` |
| **Hare** | `hare` | QBE backend (Linux only) |

## Installation

```toml
[dependencies]
equilibrium-ffi = { git = "https://github.com/tschk/equilibrium" }
```

For the `eq` CLI:
```toml
[dependencies]
equilibrium-ffi = { git = "https://github.com/tschk/equilibrium", features = ["cli"] }
```

Or install globally:
```bash
cargo install --git https://github.com/tschk/equilibrium --features cli
```

## Architecture

```
┌─────────────────┐
│  Source Files   │
│  (.v, .zig, .d) │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│   Detector      │ ◄─── Auto-detect language + compiler
│  detector.rs    │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│   Compiler      │ ◄─── Invoke with language-specific flags
│  compiler.rs    │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  C Output       │
│  (.c, .h, .o)   │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│   Bindings      │ ◄─── Parse C headers → Rust FFI
│  bindings.rs    │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  Rust Code      │
│  (ready to use) │
└─────────────────┘
```

## Helper Libraries

| Language | Library | Description |
|----------|---------|-------------|
| **Rust** | `equilibrium-rust` | `#[ffi]` proc macro for automatic `extern "C"` |
| **Nim** | `equilibrium.nim` | Type conversion helpers and export utilities |
| **D** | `equilibrium.d` | `@ffi` UDA and `extern(C)` helpers |
| **Zig** | `equilibrium.zig` | Comptime FFI helpers and type conversions |

## Polyglot Demo

`examples/polyglot-gui/` is the live demo. It loads C via `load()` and shows the rest of the compilers it can find.

```bash
cd examples/polyglot-gui

# TUI (works everywhere including WSL2)
cargo build --bin polyglot-tui
./target/debug/polyglot-tui

# GUI
cargo build --bin polyglot-gui
./target/debug/polyglot-gui
```

Or use `eq build` to ensure all compilers are on PATH:
```bash
cd examples/polyglot-gui
eq build --bin polyglot-tui
```

## Testing

```bash
cargo test
```

## CI/CD

- `.github/workflows/ci.yml` — tests on Linux/macOS/Windows
- `.github/actions/setup-equilibrium/` — reusable action for your projects

```yaml
- uses: tschk/equilibrium/.github/actions/setup-equilibrium@main
  with:
    install-zig: true
    install-nim: true
```

## License

MPL-2.0
