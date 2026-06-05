# equilibrium-rust

Ergonomic FFI exports for Rust code consumed by equilibrium-ffi.

## Installation

```toml
[dependencies]
equilibrium-rust = { git = "https://github.com/tschk/equilibrium", subdir = "equilibrium-rust" }
```

## Usage

### Simple Function Export

```rust
use equilibrium_rust::ffi;

#[ffi]
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

// Expands to:
// #[no_mangle]
// pub extern "C" fn add(a: i32, b: i32) -> i32 {
//     a + b
// }
```

### Struct Export

```rust
use equilibrium_rust::ffi_struct;

#[ffi_struct]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

// Expands to:
// #[repr(C)]
// pub struct Point { ... }
```

## Why?

Without `equilibrium-rust`:
```rust
#[no_mangle]
pub extern "C" fn calculate(x: i32, y: i32) -> i32 {
    // implementation
}
```

With `equilibrium-rust`:
```rust
#[ffi]
pub fn calculate(x: i32, y: i32) -> i32 {
    // implementation
}
```

Much cleaner and less error-prone!
