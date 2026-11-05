//! Exercise the synthesized C# ABI with a native Windows consumer.
#![cfg(windows)]

use equilibrium_ffi::{compile_to_c, find_compiler, generate_bindings, Language, LoadOptions};
use std::process::Command;

#[test]
#[ignore = "requires the Windows .NET Native AOT and MSVC C toolchains"]
fn windows_native_aot_synthesized_header_and_refusal() {
    assert!(
        find_compiler(Language::CSharp).is_some(),
        "dotnet is required"
    );
    let dir = tempfile::tempdir().unwrap();
    let sdk = Command::new("dotnet").arg("--version").output().unwrap();
    assert!(sdk.status.success(), "dotnet --version failed");
    let version = String::from_utf8(sdk.stdout).unwrap();
    std::fs::write(
        dir.path().join("global.json"),
        format!(
            "{{\"sdk\":{{\"version\":\"{}\",\"rollForward\":\"disable\"}}}}",
            version.trim()
        ),
    )
    .unwrap();
    let source = dir.path().join("csmath.cs");
    std::fs::write(
        &source,
        r#"using System.Runtime.InteropServices;
public static class CsMath {
    [UnmanagedCallersOnly(EntryPoint = "csmath_add")]
    public static int Add(int a, int b) => a + b;
}
"#,
    )
    .unwrap();
    let output_dir = dir.path().join("out");
    let result = compile_to_c(&source, &output_dir).expect("Native AOT compilation");
    assert!(result.output_path.is_file());
    assert_eq!(result.output_path.extension().unwrap(), "dll");
    let header = result.header_path.expect("synthesized header");
    let contents = std::fs::read_to_string(&header).unwrap();
    assert!(contents.contains("csmath_add"), "{contents}");
    assert!(contents.contains("#include <stdint.h>"), "{contents}");

    let error = generate_bindings(&header, &Default::default()).unwrap_err();
    assert!(
        error.contains("without preprocessing") && error.contains("#include <stdint.h>"),
        "{error}"
    );
    let error =
        equilibrium_ffi::load_with_options(&source, LoadOptions::default().output_dir(&output_dir))
            .unwrap_err();
    assert!(
        matches!(error, equilibrium_ffi::LoadError::BindingFailed(ref reason)
        if reason.contains("without preprocessing") && reason.contains("#include <stdint.h>")),
        "{error}"
    );

    // Resolve the real DLL export at runtime; no assumptions about an import library's name.
    let consumer = dir.path().join("consumer.c");
    std::fs::write(
        &consumer,
        r#"#include <windows.h>
#include "csmath.h"
typedef int32_t (__cdecl *add_fn)(int32_t, int32_t);
_Static_assert(_Generic(&csmath_add, add_fn: 1, default: 0), "header signature");
int main(int argc, char **argv) {
    if (argc != 2) return 1;
    HMODULE module = LoadLibraryA(argv[1]);
    if (!module) return 2;
    add_fn add = (add_fn)GetProcAddress(module, "csmath_add");
    if (!add) return 3;
    return add(20, 22) == 42 && add(-10, 3) == -7 ? 0 : 4;
}
"#,
    )
    .unwrap();
    let executable = dir.path().join("consumer.exe");
    assert!(header.is_file(), "synthesized header disappeared");
    let canonical_dir = std::fs::canonicalize(dir.path()).unwrap();
    let include_dir = header
        .parent()
        .unwrap()
        .strip_prefix(&canonical_dir)
        .unwrap();
    let compile = Command::new("cl.exe")
        .current_dir(dir.path())
        .arg("/nologo")
        .arg("/std:c11")
        .arg(format!("/I{}", include_dir.display()))
        .arg(format!("/Fe:{}", executable.display()))
        .arg(&consumer)
        .output()
        .expect("MSVC C compiler is required");
    assert!(
        compile.status.success(),
        "C compiler failed:\n{}\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = Command::new(&executable)
        .arg(&result.output_path)
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "native consumer failed: {:?}\n{}\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
}
