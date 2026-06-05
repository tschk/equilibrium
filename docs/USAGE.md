# Equilibrium Usage Guide

Complete guide to using equilibrium for polyglot FFI.

## Table of Contents

1. [Quick Start](#quick-start)
2. [Language Detection](#language-detection)
3. [Compiler Discovery](#compiler-discovery)
4. [Compilation](#compilation)
5. [Helper Libraries](#helper-libraries)
6. [Binding Generation](#binding-generation)
7. [Complete Example](#complete-example)

## Quick Start

Add equilibrium to your `Cargo.toml`:

```toml
[dependencies]
equilibrium-ffi = { git = "https://github.com/tschk/equilibrium" }
```

Basic usage:

```rust
use equilibrium_ffi::{compile_to_c, detect_language};
use std::path::Path;

let source = Path::new("mylib.v");
let lang = detect_language(source).unwrap();
let result = compile_to_c(source, Path::new("./build"))?;
```

You can also generate consumer wrappers from the same header:

```rust
use equilibrium_ffi::{generate_imports, ImportOptions, Language};
use std::path::Path;

let zig = generate_imports(
    Path::new("build/mylib.h"),
    Language::Zig,
    &ImportOptions::default(),
)?;

println!("{}", zig.code);
```

## Language Detection

Equilibrium detects languages by file extension:

```rust
use equilibrium_ffi::detect_language;
use std::path::Path;

let v_file = Path::new("math.v");
if let Some(lang) = detect_language(v_file) {
    println!("Detected: {:?}", lang); // Language::V
}
```

Supported extensions:

| Language | Extensions |
|----------|------------|
| V | `.v` |
| Zig | `.zig` |
| C | `.c`, `.h` |
| C++ | `.cpp`, `.cxx`, `.cc`, `.hpp`, `.hxx` |
| C# | `.cs` |
| Rust | `.rs` |
| D | `.d`, `.di` |
| Nim | `.nim`, `.nims` |
| Odin | `.odin` |
| Hare | `.ha` |

## Compiler Discovery

Check which compilers are available:

```rust
use equilibrium_ffi::{find_compiler, Language};

if let Some(info) = find_compiler(Language::Zig) {
    println!("Found: {}", info.compiler.unwrap());
    println!("Version: {}", info.version.unwrap());
} else {
    println!("Zig not installed");
}
```

Equilibrium tries primary compilers and falls back to alternatives:

- **D**: tries `ldc2`, then `dmd`, then `gdc`
- **C**: tries `clang`, then `gcc`, then `cc`
- **C++**: tries `clang++`, then `g++`, then `c++`

## Compilation

Compile source files to C intermediate representation:

```rust
use equilibrium_ffi::compile_to_c;
use std::path::Path;

let source = Path::new("mylib.nim");
let output_dir = Path::new("./build");

match compile_to_c(source, output_dir) {
    Ok(result) => {
        println!("Output: {:?}", result.output_path);
        println!("Header: {:?}", result.header_path);
        println!("Language: {:?}", result.language);
        
        // Compiler output
        println!("Stdout: {}", result.stdout);
        println!("Stderr: {}", result.stderr);
    }
    Err(e) => eprintln!("Failed: {}", e),
}
```

### Compilation Details by Language

**V (Vlang):**
```bash
v -o output.c -backend c input.v
```

**Nim:**
```bash
nim c --nimcache:. -o:output.o input.nim
```

**D:**
```bash
ldc2 -c -of=output.o -HC input.d  # -HC generates C header
```

**Zig:**
```bash
zig build-obj -femit-bin=output.o input.zig
```

## Helper Libraries

Use language-specific helpers for clean FFI exports.

### Rust

```rust
use equilibrium_rust::ffi;

#[ffi]
pub fn calculate(x: i32, y: i32) -> i32 {
    x * y + x
}

// Expands to:
// #[no_mangle]
// pub extern "C" fn calculate(x: i32, y: i32) -> i32 { ... }
```

### Nim

```nim
import equilibrium

proc add*(a, b: cint): cint {.exportc, cdecl.} =
  return a + b
```

### D

```d
import equilibrium;

@ffi
extern(C) export int add(int a, int b) {
    return a + b;
}
```

### Zig

```zig
const eq = @import("equilibrium.zig");

pub export fn add(a: i32, b: i32) i32 {
    return a + b;
}
```

## Binding Generation

Generate Rust FFI bindings from C headers:

```rust
use equilibrium_ffi::generate_bindings;
use std::path::Path;

let header = Path::new("build/mylib.h");
let bindings = generate_bindings(header)?;

// Write to file
std::fs::write("src/bindings.rs", bindings.rust_code)?;
```

The generated bindings look like:

```rust
extern "C" {
    pub fn add(a: i32, b: i32) -> i32;
    pub fn multiply(a: i32, b: i32) -> i32;
}

#[repr(C)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}
```

## Complete Example

Full workflow from V source to Rust usage:

**1. Write V code (`math.v`):**

```v
[export: 'add_v']
pub fn add(a int, b int) int {
    return a + b
}
```

**2. Compile with equilibrium:**

```rust
use equilibrium_ffi::{compile_to_c, generate_bindings};
use std::path::Path;

// Compile V to C
let source = Path::new("math.v");
let build_dir = Path::new("./build");
std::fs::create_dir_all(build_dir)?;

let result = compile_to_c(source, build_dir)?;

// Generate Rust bindings
let bindings = generate_bindings(&result.output_path)?;
std::fs::write("src/bindings.rs", bindings.rust_code)?;
```

**3. Use in Rust:**

```rust
mod bindings;

fn main() {
    unsafe {
        let sum = bindings::add_v(5, 3);
        println!("5 + 3 = {}", sum);
    }
}
```

## Error Handling

Equilibrium provides detailed error information:

```rust
match compile_to_c(source, output_dir) {
    Ok(result) => { /* success */ }
    Err(e) => {
        match e {
            CompileError::CompilerNotFound { language } => {
                eprintln!("Compiler for {:?} not installed", language);
            }
            CompileError::CompilationFailed { stderr, exit_code } => {
                eprintln!("Compilation failed with code {:?}:", exit_code);
                eprintln!("{}", stderr);
            }
            CompileError::Io(e) => {
                eprintln!("IO error: {}", e);
            }
            CompileError::UnsupportedCOutput { language } => {
                eprintln!("{:?} doesn't support C output", language);
            }
        }
    }
}
```

## Next Steps

- See [examples/](../examples/) for working code
- Check language-specific helper docs:
  - [equilibrium-rust](../equilibrium-rust/)
  - [equilibrium.nim](../equilibrium-nim/)
  - [equilibrium.d](../equilibrium-d/)
  - [equilibrium.zig](../equilibrium-zig/)
- Read [CONTRIBUTING.md](../CONTRIBUTING.md) to add language support
