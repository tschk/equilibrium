# equilibrium.nim

Equilibrium FFI helpers for Nim.

## Installation

Place `equilibrium.nim` in your project or Nim's lib path.

## Usage

```nim
import equilibrium

proc add*(a, b: cint): cint {.exportc, cdecl.} =
  return a + b

proc multiply*(a, b: cint): cint {.exportc, cdecl.} =
  return a * b
```

## Type Helpers

```nim
import equilibrium

let nimInt = 42
let cInt = nimInt.toCInt()

let cStr = "Hello".toCString()
```

## Why?

Nim's `{.exportc.}` pragma makes functions callable from C, which equilibrium can then bind to Rust.

## Importing Another Target

```bash
eq generate build/math.h --consumer nim -o src/math_bindings.nim
```

```nim
import math_bindings

discard add(2, 3)
```
