# scriptc (TypeScript) Demo

A demonstration app showing equilibrium compiling TypeScript with
[scriptc](https://scriptc.dev) and calling it from Rust.

## What it does

1. Reads `foreign-code/math.ts`, a module of `export function`s.
2. Derives a scriptc **library profile** and a matching C header from those
   exports, using the marshalling classes in `equilibrium.toml`.
3. Runs `scriptc build --lib --profile …` to produce a self-contained static
   archive (`libmath.a`).
4. Generates Rust `extern "C"` bindings from the header and links the archive.
5. Calls the TypeScript functions from `src/main.rs`.

## Running

Requires `scriptc` (Node.js 24+) and a C compiler for its library lane —
`zig` is used automatically when it is on `PATH` and `SCRIPTC_CC` is unset.

```bash
cargo run
```

## Output

```
=== TypeScript (scriptc) FFI demo ===

math_add(0.1, 0.2)         = 0.30000000000000004
math_greet("world", true)  = "WORLD"
math_sum([1, 2, 3, 4])     = 10
math_mix(7, 12)            = 7012
math_truncate(-12.75)      = -12

✓ TypeScript (scriptc) FFI round-trip OK
```

## Where the pieces come from

| Piece | Produced by |
|-------|-------------|
| `math.profile.json` | `src/scriptc.rs` — the C ABI scriptc compiles against |
| `math.h` | `src/scriptc.rs` — scriptc emits no header, so binding generation parses this |
| `libmath.a` | `scriptc build --lib` — self-contained, private scriptc runtime |
| `math_bindings.rs` | `src/bindings.rs`, written to `$OUT_DIR` by `build.rs` |

`equilibrium.toml` refines the ABI: `mix` takes `u32` parameters and `truncate`
returns an `i64`, so the host passes integers in their C-native widths. See the
scriptc notes in the repository README for the full class list and the rules
scriptc enforces on integer returns.
