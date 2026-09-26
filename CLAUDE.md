# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
# Build library
cargo build
cargo build --release

# Build eq CLI
cargo build --bin eq --features cli --release

# Test
cargo test
cargo test test_detect_v          # Run a single test by name

# Lint / Format — always run cargo fmt before committing
cargo fmt                         # Auto-fix formatting
cargo fmt -- --check              # Check formatting (CI gate)
cargo clippy -- -D warnings       # Lint (warnings as errors)

# Docs
cargo doc --all-features --no-deps

# Run example
cargo run --example using_equilibrium

# Polyglot demo (TUI works everywhere; GUI needs GPU)
cd examples/polyglot-gui
cargo build --bin polyglot-tui
cargo build --bin polyglot-gui
```

## Architecture

Equilibrium is a Rust library that auto-generates C FFI bindings for foreign-language source files. It implements a three-stage pipeline:

1. **Language Detection** (`src/detector.rs`) — Maps file extensions to one of 11 supported languages (V, Zig, C, C++, C#, Rust, D, Nim, Odin, Hare, TypeScript/JavaScript via scriptc). `find_compiler()` uses `which` to locate installed compiler binaries. Each `Language` variant knows its extensions, primary/fallback compiler commands, and the CLI flags needed to emit C-compatible output.

2. **Compilation to C** (`src/compiler.rs`) — Invokes the detected compiler with the appropriate flags to produce a `.c` or preprocessed intermediate file plus an optional `.h` header. `compile_to_c()` auto-detects the language; `compile_batch()` handles multiple files; `generate_header()` produces headers for languages with cbindgen/native support (Rust, V). scriptc is the exception: it has no header emitter and its host-callable ABI is library mode, so `write_scriptc_library_surface()` writes a library profile plus a matching header (both derived from the module's `export function` declarations by `src/scriptc.rs`) before invoking `scriptc build --lib --profile`.

3. **Binding Generation** (`src/bindings.rs`) — Parses a C header (functions, typedefs, structs) and emits Rust `extern "C"` declarations. `BindingOptions` controls the module name, include paths, symbol allowlists, and `#[derive]` attributes. `c_type_to_rust()` handles the type mapping (e.g. `int` → `c_int`, `char*` → `*mut c_char`).

`src/lib.rs` re-exports the public surface: `detect_language`, `compile_to_c`, `generate_bindings`, `find_compiler`.

### `eq` CLI (`src/bin/eq.rs`)

The `eq` binary (feature-gated behind `cli`) provides four subcommands:

- `eq check` — detects all supported compilers and shows versions/paths
- `eq install [names…]` — installs missing compilers via the best available package manager; multiple compilers install in parallel. Install order: **wax → brew/linuxbrew → apt/dnf/pacman** on Linux/macOS, **wax → winget → scoop** on Windows.
- `eq build [args…]` — runs `cargo build` with all known compiler bin dirs prepended to PATH (linuxbrew, homebrew, `/usr/local/sbin`, etc.)
- `eq generate <header> [-o file]` — emits Rust `extern "C"` bindings from a C header via `equilibrium_ffi::generate_bindings`

### Helper Libraries

Language-specific ergonomic crates live in sibling directories:
- `equilibrium-rust/` — proc macro `#[ffi]` attribute
- `equilibrium-nim/` — Nim type conversion helpers
- `equilibrium-d/` — D `@ffi` UDA and `extern(C)` helpers
- `equilibrium-zig/` — Zig comptime FFI helpers

### Examples

- `examples/using_equilibrium.rs` — demonstrates all three pipeline stages (detect, compile, generate bindings, scan_directory)
- `equilibrium::load()` is the primary one-call entry point; prefer it in docs and demos when the goal is to load a single C source and use generated bindings
- `examples/demo-app/` — minimal end-to-end demo: `build.rs` compiles `math.c` via `cc` and generates bindings with equilibrium-ffi; `main.rs` calls C functions through the generated `include!()`d bindings
- `examples/scriptc-app/` — the same end-to-end shape for TypeScript: `build.rs` calls `load_with_options()` on `foreign-code/math.ts`, which compiles a scriptc library archive and writes bindings for `main.rs`; `equilibrium.toml` supplies the target's marshalling classes
- `examples/full-demo/` — full demo calling a C calculator library from Rust
- `examples/polyglot-gui/` — interactive polyglot dashboard with a ratatui TUI (`polyglot-tui`) and a GPUI GUI (`polyglot-gui`). Calls live FFI into C, C++, Zig, Nim, V, D, Odin, Rust, and TypeScript (scriptc, via `load_with_options` in `build.rs` + `equilibrium.toml` class overrides). `build.rs` uses `find_bin()` with hardcoded linuxbrew fallbacks so compilers are found regardless of the shell PATH that cargo inherits.

### scriptc (TypeScript) FFI notes

`scriptc` is producer-only: it exposes a C ABI through library mode (`scriptc build --lib --profile <profile.json>`), which emits a self-contained static archive and **no header**. Equilibrium therefore generates both build inputs from the module's `export function` declarations (`src/scriptc.rs`): `<stem>.profile.json` names every exported symbol and its marshalling classes, and `<stem>.h` mirrors them so binding generation works unchanged. Type annotations give the common classes (`number` → `f64`, `boolean` → `bool`, `string` → `string`, `Uint8Array` → `bytes`, `void`); the `[target.<name>]` table in `equilibrium.toml` refines them and picks the emission:

```toml
[target.math]
language = "scriptc"
sources = ["native/math.ts"]
emission = "c"                                  # "llvm" (default) or "c"

[target.math.signatures]
mix = { params = ["u32", "u32"], returns = "f64" }
truncate = { returns = "i64" }
```

`config::target_for()` (`src/config.rs`) resolves that table for both export discovery and compilation, so the config governs the ABI the profile declares and the bindings generated from it. Overrides are validated against the annotations (class family, parameter count, inbound-only return classes, unknown export names) and refused with the reason rather than silently changing the ABI. scriptc still enforces its own rules on top: an `i64`/`u64` return must be provably whole and within ±(2^53−1), which the TypeScript function establishes with ordered comparisons (`SC4022`/`SC4023` otherwise).

Every symbol is prefixed with the sanitized module stem (`math.ts` → `math_add`), the archive is named `lib<stem>.a` so rustc's `-l` lookup resolves it, and the profile sets `localize_runtime: true` so several compiled modules can link into one process. Library mode always compiles runtime C, so it needs `clang` or `zig`: equilibrium sets `SCRIPTC_CC=zigcc` when `zig` is installed and the caller left `SCRIPTC_CC` unset. Hosts call `<stem>_init()` before the exports, may register `<stem>_set_panic_sink()` (an unregistered trap aborts), and call `<stem>_collect()` to release buffered `string`/`bytes` results. `scriptc` cannot consume generated wrappers — it calls C through its own `--ffi` manifests — so `Language::supports_imports()` excludes it from `eq generate --consumer all`.

### Zig FFI notes

Zig objects must be compiled with `-fPIC -OReleaseFast` to link cleanly into Rust's PIE binary. `ReleaseFast` removes safety checks that otherwise pull in Zig's stdlib panic infrastructure, which conflicts with the linker. See `examples/polyglot-gui/build.rs`.

### V FFI on Linux

V's runtime cannot link directly into Rust's PIE binary. The polyglot-gui uses a C shim (`v_module_shim.c`) that implements the same exported symbols with identical semantics.

### Windows build notes

The `polyglot-gui` binary targets D3D11 (no Vulkan needed). Build from Windows PowerShell:
```powershell
cargo build --release --bin polyglot-gui
```
The TUI binary works cross-platform. The `eq` CLI on Windows uses `%TEMP%` as the working directory when invoking winget/scoop to avoid UNC path errors from WSL2 filesystem paths.
