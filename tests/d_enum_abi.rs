//! Regression for the former D consumer's `alias Color = int` reconstruction.
use equilibrium_ffi::{generate_imports, ImportOptions, Language};

#[test]
fn d_consumer_rejects_enum_for_default_and_short_enum_abis() {
    let dir = tempfile::tempdir().unwrap();
    let header = dir.path().join("packet.h");
    std::fs::write(
        &header,
        "typedef enum Color { SMALL = 1, LARGE = 200 } Color;\n\
         typedef struct Packet { Color color; char marker; int tail; } Packet;\n\
         void receive(Packet packet);\n",
    )
    .unwrap();

    // ImportOptions has no C compiler flags: both native builds below use this same header,
    // and neither context can make the unverified declaration safe to generate.
    let error = generate_imports(&header, Language::D, &ImportOptions::default()).unwrap_err();
    assert!(error.contains("enum representation"), "{error}");

    native_reproduction(dir.path());
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn native_reproduction(dir: &std::path::Path) {
    use equilibrium_ffi::find_compiler;
    use std::process::Command;

    let Some(c) = find_compiler(Language::C).and_then(|info| info.compiler_path) else {
        eprintln!("skipping native enum ABI reproduction: C compiler not installed");
        return;
    };
    let d = find_compiler(Language::D);
    if d.is_none() {
        let required = std::env::var("EQ_REQUIRE_COMPILERS").unwrap_or_default();
        assert!(
            !required.split(',').any(|name| name.trim() == "d"),
            "D compiler required but not installed"
        );
        eprintln!("skipping D call reproduction: D compiler not installed; C layouts still tested");
    }
    std::fs::write(
        dir.join("packet.c"),
        r#"
#include "packet.h"
#include <stddef.h>
#include <stdio.h>
static Packet seen;
void receive(Packet packet) { seen = packet; }
int color_seen(void) { return seen.color; }
int marker_seen(void) { return seen.marker; }
int tail_seen(void) { return seen.tail; }
int main(void) {
    printf("%zu %zu %zu %zu\n", sizeof(Color), sizeof(Packet),
           offsetof(Packet, marker), offsetof(Packet, tail));
    return 0;
}
"#,
    )
    .unwrap();
    std::fs::write(dir.join("consumer.d"), r#"
import std.stdio : writeln;
alias Color = int;
struct Packet { Color color; char marker; int tail; }
extern(C) void receive(Packet);
extern(C) int color_seen();
extern(C) int marker_seen();
extern(C) int tail_seen();
void main() {
    writeln(Color.sizeof, " ", Packet.sizeof, " ", Packet.marker.offsetof, " ", Packet.tail.offsetof);
    receive(Packet(200, 42, 77));
    writeln(color_seen(), " ", marker_seen(), " ", tail_seen());
}
"#).unwrap();
    // The object linked into D must not also define the C probe's main.
    std::fs::write(
        dir.join("library.c"),
        "#define main c_probe_main\n#include \"packet.c\"\n",
    )
    .unwrap();

    for (name, flag, layout, values) in [
        ("default", None, "4 12 4 8", "200 42 77"),
        ("short", Some("-fshort-enums"), "1 8 1 4", "200 0 42"),
    ] {
        let mut compile = Command::new(&c);
        compile
            .current_dir(dir)
            .arg("packet.c")
            .arg("-o")
            .arg(format!("probe-{name}"));
        if let Some(flag) = flag {
            compile.arg(flag);
        }
        let output = compile.output().unwrap();
        assert!(
            output.status.success(),
            "C {name} probe compile: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let output = Command::new(dir.join(format!("probe-{name}")))
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).trim(),
            layout,
            "C {name} layout"
        );
        eprintln!("C {name} layout: {layout}");

        if let Some(info) = &d {
            let mut compile = Command::new(&c);
            compile
                .current_dir(dir)
                .args(["-c", "library.c", "-o", "library.o"]);
            if let Some(flag) = flag {
                compile.arg(flag);
            }
            let output = compile.output().unwrap();
            assert!(
                output.status.success(),
                "C {name} object: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let compiler = info.compiler_path.as_ref().unwrap();
            let mut compile = Command::new(compiler);
            compile.current_dir(dir).args(["consumer.d", "library.o"]);
            if info.compiler.as_deref() == Some("gdc") {
                compile.args(["-o", "consumer"]);
            } else {
                compile.arg("-of=consumer");
            }
            let output = compile.output().unwrap();
            assert!(
                output.status.success(),
                "D {name} compile: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let output = Command::new(dir.join("consumer")).output().unwrap();
            assert!(
                output.status.success(),
                "D {name} run: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let actual = String::from_utf8(output.stdout).unwrap();
            assert_eq!(
                actual.trim(),
                format!("4 12 4 8\n{values}"),
                "D {name} layout and values observed by C"
            );
            eprintln!(
                "D {name} layout and C-observed values: {}",
                actual.trim().replace('\n', "; ")
            );
        }
    }
}

#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
fn native_reproduction(_: &std::path::Path) {
    eprintln!("skipping native enum ABI reproduction: expected layouts and calling convention are specific to x86-64 Linux");
}
