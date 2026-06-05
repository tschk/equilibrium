# equilibrium.zig

Equilibrium FFI helpers for Zig.

## Installation

Place `equilibrium.zig` in your project.

## Usage

```zig
const eq = @import("equilibrium.zig");

pub export fn add(a: i32, b: i32) i32 {
    return a + b;
}

pub export fn multiply(a: i32, b: i32) i32 {
    return a * b;
}
```

## Type Helpers

```zig
const eq = @import("equilibrium.zig");

const value = eq.FFI.toInt(42);
const ptr = eq.FFI.toPtr(slice);
```

## Why?

Zig's `export` keyword makes functions callable from C with C ABI, which equilibrium can bind to Rust.

## Importing Another Target

```bash
eq generate build/math.h --consumer zig -o src/math_bindings.zig
```

```zig
const math = @import("math_bindings.zig");

pub fn main() void {
    _ = math.add(2, 3);
}
```
