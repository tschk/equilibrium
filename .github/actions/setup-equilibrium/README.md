# Example Usage: Equilibrium Action

To use the equilibrium setup action in your own polyglot project:

```yaml
name: Build

on: [push, pull_request]

jobs:
  build:
    runs-on: ${{ matrix.os }}
    strategy:
      matrix:
        os: [ubuntu-latest, macos-latest]
    
    steps:
      - uses: actions/checkout@v4
      
      - name: Setup Equilibrium
        uses: tschk/equilibrium/.github/actions/setup-equilibrium@main
        with:
          rust-version: stable
          install-zig: true
          install-nim: true
          install-d: true
          cache: true
      
      - name: Build Rust + foreign code
        run: cargo build --release
      
      - name: Run tests
        run: cargo test
```

## Inputs

| Input | Description | Default |
|-------|-------------|---------|
| `rust-version` | Rust toolchain version | `stable` |
| `install-zig` | Install Zig compiler | `false` |
| `install-nim` | Install Nim compiler | `false` |
| `install-d` | Install D compiler (LDC) | `false` |
| `cache` | Enable cargo caching | `true` |

## What it does

1. Installs Rust toolchain
2. Optionally installs language compilers (Zig, Nim, D)
3. Configures cargo caching
4. Verifies all installations

## Supported Compilers

- **Zig**: Installed via goto-bus-stop/setup-zig@v2
- **Nim**: Installed via apt (Ubuntu) or brew (macOS)
- **D (LDC)**: Installed via apt (Ubuntu) or brew (macOS)
- **C/C++**: Already available on all platforms
- **Rust**: Managed by dtolnay/rust-toolchain
