# equilibrium.d

Equilibrium FFI helpers for D.

## Installation

Import `equilibrium.d` in your D project.

## Usage

```d
import equilibrium;

@ffi
extern(C) export int add(int a, int b)
{
    return a + b;
}

@ffi
extern(C) export int multiply(int a, int b)
{
    return a * b;
}
```

## Why?

D's `extern(C)` makes functions callable from C, which equilibrium can bind to Rust.
The `@ffi` attribute is a marker for documentation and future tooling.

## Importing Another Target

```bash
eq generate build/math.h --consumer d -o source/math_bindings.d
```

```d
import math_bindings;

void main()
{
    add(2, 3);
}
```
