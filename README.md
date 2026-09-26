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
- **Linux**: wax → brew/linuxbrew → apt / dnf / pacman → npm
- **macOS**: wax → brew → npm
- **Windows**: winget → scoop → npm

npm is a fallback for tools the JS ecosystem ships (`scriptc`).

Multiple compilers install in parallel.

### Security (dev tool)

Equilibrium runs **your** compilers on **your** source paths (`load`, `eq generate`, `compile_to_c`). Treat paths like a build script: only trusted trees; shared CI should not point at arbitrary uploads. Compiler binaries come from `PATH` (and `eq`’s extra search dirs)—use a known-good toolchain. `eq install` may invoke `sudo` with apt/dnf/pacman; set `EQ_INSTALL_NO_SUDO=1` to skip those managers. Header/source reads are capped (10 MB headers, 64 MB discovery sources) to limit accidental DoS.

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
| **TypeScript/JavaScript** | `scriptc` | Library mode: `scriptc build --lib --profile` → self-contained static archive |

### scriptc (TypeScript) notes

[scriptc](https://scriptc.dev) compiles ordinary TypeScript/JavaScript to native code, and its only host-callable C ABI is **library mode**. Equilibrium detects `.ts .mts .cts .js .mjs .cjs`, derives a library profile and a matching C header from the module's `export function` declarations, then runs `scriptc build --lib --profile …` and returns the self-contained archive (`lib<stem>.a`) for linking.

```ts
// native/math.ts
export function add(a: number, b: number): number {
  return a + b;
}
export function greet(who: string, loud: boolean): string {
  return loud ? who.toUpperCase() : who;
}
```

```rust
let lib = equilibrium_ffi::load("native/math.ts")?;
// lib.output_path == target/native/scriptc/libmath.a
// lib.header_path == target/native/scriptc/math.h (generated: scriptc emits none)
```

A complete runnable version — `build.rs`, class overrides, string/bytes calls — lives in [`examples/scriptc-app`](examples/scriptc-app).

Requirements and behavior:

- `scriptc` (Node.js 24+) plus a C compiler for its library lane. Equilibrium passes `SCRIPTC_CC=zigcc` when you have `zig` and did not set `SCRIPTC_CC` yourself, because scriptc's default `clang` driver is missing or too old on many Linux hosts.
- Signatures must be C ABI safe: `number` → `f64`, `boolean` → `bool`, `string` → `string`, `Uint8Array` → `bytes`, and `void` returns. Anything else (optional/rest/`any`-typed parameters, unions, generics) is skipped with a warning.
- **Marshalling classes and the profile emission are configurable** through the `[target.<name>]` table in `equilibrium.toml` (beside the source, or at your crate root):

```toml
[target.math]
language = "scriptc"
sources = ["native/math.ts"]
emission = "c"                                  # "llvm" (default) or "c"

[target.math.signatures]
mix = { params = ["u32", "u32"], returns = "f64" }
scale = { params = ["u64", "f64"], returns = "f64" }
truncate = { returns = "i64" }
```

  Overrides are validated against the TypeScript annotations, so a class that cannot describe the annotated value — or the wrong parameter count, an unknown class name, an override for a function that is not part of the ABI — is refused with the reason instead of silently changing the ABI. `u8`/`u32`/`i32` are inbound-only in scriptc's ABI and are rejected as return classes.
- scriptc proves every `i64`/`u64` **return** value whole and in range, so such a function must bound its value with ordered comparisons; otherwise the build fails with scriptc's `SC4022`/`SC4023` diagnostic:

```ts
export function truncate(value: number): number {
  if (value > -9007199254740991 && value < 9007199254740991) {
    return Math.trunc(value);
  }
  return 0;
}
```
- Every exported symbol is prefixed with the module stem (`math_add`), and each module gets a private copy of the scriptc runtime, so several compiled modules can coexist in one process.
- Call `<stem>_init()` once before the exports. Register `<stem>_set_panic_sink()` first if you want trap messages, since an unregistered trap aborts the process, and call `<stem>_collect()` to release buffered `string`/`bytes` results.
- The generated profile/header are build inputs in the output directory. The default `llvm` emission is scriptc's production lane; switch to `emission = "c"` for scriptc's readable C backend (which accepts the same library-mode profile).

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
│ (.v, .zig, .ts) │
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

`examples/polyglot-gui/` is the live demo. It loads C via `load()` and shows the rest of the compilers it can find, including TypeScript through scriptc.

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

ISC
