# Building the GPUI app on Windows

GPUI uses Direct3D 11 + DirectWrite on Windows — no Vulkan, no GPU issues.
Any Windows 10/11 machine with a GPU (or the WARP software renderer) works.

## Prerequisites (one-time)

1. **Rust** — https://rustup.rs (install the MSVC toolchain, not GNU)
2. **Visual Studio Build Tools** — https://aka.ms/vs/17/release/vs_BuildTools.exe
   - Select "Desktop development with C++" workload
   - (Or a full Visual Studio install)
3. **C/C++ compiler** — already included in VS Build Tools (cl.exe)
4. **Zig** *(optional)* — https://ziglang.org/download/ — adds the Zig FFI module
5. **scriptc** *(optional)* — https://scriptc.dev/quickstart — adds the TypeScript FFI module
   (needs Node.js 24+; its library lane also needs clang or zig, and equilibrium
   selects `SCRIPTC_CC=zigcc` when zig is on `PATH`)

## Build

Open **PowerShell** or **Developer Command Prompt**, navigate to this directory,
then:

```powershell
# From WSL2 you can reach this at:
#   \\wsl$\<distro>\home\undivisible\equilibrium\examples\polyglot-gui
# Copy it to a Windows path or just cd to the UNC path.

cd \path\to\equilibrium\examples\polyglot-gui

cargo build --release --bin polyglot-gui
```

The binary lands at `target\release\polyglot-gui.exe`.

## Run

```powershell
.\target\release\polyglot-gui.exe
```

A native Windows window opens. Click **−**, **+**, **×2**, or **reset**
to change n; live FFI results from C, C++, Zig (if installed), TypeScript
(if scriptc is installed), and Rust update instantly on every click.

## TUI fallback (works in any terminal, including WSL2)

```powershell
cargo build --release --bin polyglot-tui
.\target\release\polyglot-tui.exe
```
