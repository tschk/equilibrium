# One-Call Polyglot Exports Design

## Goal

Equilibrium should make polyglot FFI feel like one library call. A user should be able to point Equilibrium at a source file and get a C ABI artifact plus usable bindings without needing to hand-write a build graph or run the CLI.

The default API remains library-first:

```rust
let module = equilibrium_ffi::load("src/native/math.rs")?;
```

That call should detect the source language, build the C ABI output, generate bindings or import metadata, and return a module handle that tells the caller what was produced.

## Export Discovery

Equilibrium should prefer explicit export markers when they exist:

- Rust functions marked with the `equilibrium-rust` macro.
- Zig functions declared with `pub export`.
- Nim procedures using C export pragmas or Equilibrium helpers.
- D functions declared with `extern(C) export` or marked by the helper attribute.
- C and C++ declarations already present in a header.

If no explicit markers are found and the caller did not provide an export list in the API call or `equilibrium.toml`, Equilibrium should export every top-level function in that source file.

That fallback should be intentionally simple. It is a convenience path for small files and demos, not a promise to infer complex language-specific visibility rules across an entire project.

## API Shape

The one-call path should support three levels of control:

```rust
equilibrium_ffi::load("src/native/math.zig")?;
```

This uses detection, explicit markers if present, and all-functions fallback when no markers exist.

```rust
equilibrium_ffi::load_with_options(
    "src/native/math.zig",
    LoadOptions::default().exports(["add", "multiply"]),
)?;
```

This uses caller-provided exports and skips implicit all-function export.

```toml
[target.math]
language = "zig"
sources = ["src/native/math.zig"]
exports = ["add", "multiply"]
```

This gives repeatable project configuration when the caller wants it.

## Artifact Model

Every supported language should lower to the same artifact model:

- source language
- generated C ABI library or object
- generated or discovered C header
- generated consumer bindings
- discovered exports
- warnings

Rust remains the first-class consumer because this crate is Rust, but the artifact model should not assume Rust is the only consumer. Zig, Nim, D, and future helpers can import the same C ABI surface through generated wrappers.

## CLI Role

The CLI should stay optional. It can generate bindings, inspect projects, or precompute a build graph, but the default workflow should not require it.

Useful CLI command surfaces:

```bash
eq generate src/native/math.zig
eq graph
eq build-graph
```

These commands should expose the same behavior as the library, not define a separate architecture.

## Build Time and Runtime

The same export discovery and artifact generation should work from `build.rs` and at runtime:

- In `build.rs`, generated artifacts should land in Cargo's output directory or a caller-provided directory.
- At runtime, generated artifacts should land in a cache directory or caller-provided directory.
- Outputs should be deterministic enough that repeated calls can reuse existing artifacts when inputs have not changed.

## Error Handling

Errors should say which stage failed:

- language detection
- export discovery
- compiler discovery
- C ABI artifact generation
- header generation
- binding generation
- dynamic loading

Implicit all-function export should produce warnings when a function cannot be represented safely in the C ABI instead of silently creating invalid bindings.

## Testing

The implementation should add focused coverage for:

- explicit export marker detection
- all-functions fallback when no markers are present
- API-provided export list overriding fallback
- `equilibrium.toml` export list overriding fallback
- generated artifact metadata
- at least one Rust-to-Zig or Zig-to-Rust consumer wrapper path through the C ABI

Compiler-dependent integration tests should skip cleanly when a host compiler is missing.
