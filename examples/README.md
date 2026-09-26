# Equilibrium Examples

This directory contains working examples demonstrating equilibrium's capabilities.

## Examples

### 1. [rust-ffi](rust-ffi/)
Rust library using `equilibrium-rust` helper for clean FFI exports.

```bash
cd rust-ffi
cargo build
cargo test
```

**Demonstrates:**
- `#[ffi]` macro for functions
- `#[ffi_struct]` for structs
- Pointer handling
- Complex logic (factorial)

### 2. [c-ffi](c-ffi/)
Plain C library compiled and consumed via equilibrium.

```bash
cd c-ffi
./build.sh
```

**Demonstrates:**
- C language detection
- Header parsing
- Shared library creation
- String handling

### 3. [using_equilibrium.rs](using_equilibrium.rs)
Example showing the equilibrium API itself.

```bash
cargo run --example using_equilibrium
```

**Demonstrates:**
- `detect_language()`
- `find_compiler()`
- `compile_to_c()`

## Quick Start

```bash
# Run all examples
./run_examples.sh

# Or run individually
cargo run --example using_equilibrium
cd rust-ffi && cargo build
cd c-ffi && ./build.sh
```

## Directory Structure

```
examples/
├── README.md               # This file
├── using_equilibrium.rs    # API usage example
├── rust-ffi/               # Rust FFI example
│   ├── src/lib.rs
│   └── Cargo.toml
└── c-ffi/                  # C FFI example
    ├── mathlib.c
    ├── mathlib.h
    └── build.sh
```

## What You'll Learn

1. **Language Detection** - How equilibrium identifies source languages
2. **Compiler Discovery** - Finding installed compilers on the system
3. **Compilation** - Invoking compilers with correct flags
4. **Helper Libraries** - Using language-specific helpers for clean exports
5. **Binding Generation** - Creating Rust FFI bindings from C headers

## Next Steps

After running these examples:

1. Read the [main README](../README.md) for architecture details
2. Check individual example READMEs for specifics
3. Try creating your own FFI library
4. See [docs/](../docs/) for advanced usage (when created)

### 4. [demo-app](demo-app/)
Demonstrates equilibrium compilation workflow (preprocessing).

```bash
cd demo-app
cargo run
```

**Demonstrates:**
- Compiling C with equilibrium
- Analyzing compiled output
- Understanding the workflow

### 5. [full-demo](full-demo/) ⭐ **ACTUALLY WORKS**
**Complete working example** calling C from Rust!

```bash
cd full-demo
cargo run
```

**Output:**
```
=== Full Equilibrium Demo ===
Calling C functions from Rust!

Arithmetic:
  10 + 5 = 15
  10 - 5 = 5
  10 * 5 = 50
  10.0 / 5.0 = 2.00

Advanced:
  2^8 = 256
  sqrt(144) = 12.00
  sqrt(2) = 1.4142135624

✓ All C functions called successfully!
```

**This proves equilibrium's approach works end-to-end!**

### 6. [scriptc-app](scriptc-app/)
TypeScript compiled with [scriptc](https://scriptc.dev) and called from Rust.

```bash
cd scriptc-app
cargo run
```

**Demonstrates:**
- scriptc **library mode**: a generated profile + header, then `scriptc build --lib`
- `equilibrium.toml` `[target.<name>.signatures]` overrides (`u32` parameters, an `i64` return)
- `string`/`bytes` marshalling through scriptc's `(ptr, len)` and out-parameters
- `load_with_options()` in `build.rs` linking a scriptc archive

Requires `scriptc` (Node.js 24+) and a C compiler for its library lane (`zig` is picked up automatically when `SCRIPTC_CC` is unset).
