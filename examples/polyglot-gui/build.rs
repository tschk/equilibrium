use std::path::PathBuf;
use std::process::Command;

fn raw_string_literal(source: &str) -> String {
    for hashes in 0..16 {
        let marker = "#".repeat(hashes);
        let terminator = format!("\"{marker}");
        if !source.contains(&terminator) {
            return format!("r{marker}\"{source}\"{marker}");
        }
    }
    panic!("crepus template contains too many raw string delimiters");
}

/// Try `which` first, then fall back to known installation paths.
fn find_bin(name: &str, fallbacks: &[&str]) -> Option<PathBuf> {
    // First try current PATH (the extra_paths from earlier in main())
    if let Ok(p) = which::which(name) {
        return Some(p);
    }
    // Then try fallbacks
    fallbacks.iter().map(PathBuf::from).find(|p| p.exists())
}

fn main() {
    // Fix gpui build on macOS - set SDK path for bindgen
    let sdk_path = std::env::var("SDKROOT")
        .or_else(|_| std::env::var("MACOSX_SDK_PATH"))
        .ok()
        .filter(|sdk_path| !sdk_path.is_empty())
        .or_else(|| {
            Command::new("xcrun")
                .arg("--show-sdk-path")
                .output()
                .ok()
                .and_then(|output| {
                    if output.status.success() {
                        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
                    } else {
                        None
                    }
                })
        });
    if let Some(ref sdk_path) = sdk_path {
        if !sdk_path.is_empty() {
            let clang_args = format!("-isysroot {}", sdk_path);
            println!("cargo:rustc-env=BINDGEN_EXTRA_CLANG_ARGS={}", clang_args);
        }
    }

    // Prepend known package-manager bin dirs to PATH so child compiler
    // processes can find each other (e.g. nim calling gcc) even when cargo
    // is invoked from a shell that doesn't include linuxbrew in PATH.
    let extra_paths = [
        "/home/linuxbrew/.linuxbrew/bin",
        "/home/linuxbrew/.linuxbrew/sbin",
        "/opt/homebrew/bin",
        "/opt/homebrew/sbin",
        "/usr/local/sbin",
        "/usr/local/bin",
    ];
    let current_path = std::env::var("PATH").unwrap_or_default();
    let new_path = extra_paths
        .iter()
        .filter(|p| !current_path.contains(*p))
        .cloned()
        .collect::<Vec<_>>()
        .join(":")
        + ":"
        + &current_path;
    // SAFETY: build scripts are single-threaded at the point this runs.
    unsafe { std::env::set_var("PATH", &new_path) };

    // Declare custom cfg keys so rustc doesn't warn about unknown cfgs
    println!("cargo::rustc-check-cfg=cfg(has_c)");
    println!("cargo::rustc-check-cfg=cfg(has_cpp)");
    println!("cargo::rustc-check-cfg=cfg(has_zig)");
    println!("cargo::rustc-check-cfg=cfg(has_nim)");
    println!("cargo::rustc-check-cfg=cfg(has_v)");
    println!("cargo::rustc-check-cfg=cfg(has_d)");
    println!("cargo::rustc-check-cfg=cfg(has_odin)");

    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let foreign = manifest.join("foreign-code");
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    crepuscularity_core::build::compile_crepus("templates").expect("compile crepus templates");
    let gui_template = manifest.join("templates/polyglot.crepus");
    let gui_template_source =
        std::fs::read_to_string(&gui_template).expect("read polyglot crepus template");
    let gui_template_literal = raw_string_literal(&gui_template_source);
    let gui_template_rs = format!(
        "fn render_crepus_shell<R>(ui: PolyglotTemplate<R>) -> impl IntoElement where R: Iterator<Item = (String, bool, String, &'static str, &'static str)> + 'static {{ let PolyglotTemplate {{ parts, n, is_gui, is_tui, linked_count, missing_count, gui_rows, pipeline_scroll: _pipeline_scroll }} = ui; let tui_rows = std::iter::empty::<(String, bool, String, &'static str, &'static str)>(); view! {{{gui_template_literal}}} }}\n"
    );
    std::fs::write(out_dir.join("polyglot_gui_template.rs"), gui_template_rs)
        .expect("write polyglot GUI template");

    // ── C module (always available) ─────────────────────────────────────────
    cc::Build::new()
        .file(foreign.join("c_module.c"))
        .compile("c_module");
    println!("cargo:rustc-cfg=has_c");
    println!("cargo:rerun-if-changed=foreign-code/c_module.c");
    println!("cargo:rerun-if-changed=foreign-code/c_module.h");

    // Generate C bindings via equilibrium-ffi
    emit_bindings(&foreign.join("c_module.h"), &out_dir, "c_bindings.rs");

    // ── C++ module (always available) ──────────────────────────────────────
    cc::Build::new()
        .cpp(true)
        .file(foreign.join("cpp_module.cpp"))
        .compile("cpp_module");
    println!("cargo:rustc-cfg=has_cpp");
    println!("cargo:rerun-if-changed=foreign-code/cpp_module.cpp");
    println!("cargo:rerun-if-changed=foreign-code/cpp_module.h");

    emit_bindings(&foreign.join("cpp_module.h"), &out_dir, "cpp_bindings.rs");

    // ── Zig module (when zig is on PATH) ───────────────────────────────────
    if let Some(zig) = find_bin(
        "zig",
        &[
            "/usr/local/sbin/zig",
            "/usr/local/bin/zig",
            "/home/linuxbrew/.linuxbrew/bin/zig",
            "/opt/homebrew/bin/zig",
        ],
    ) {
        let obj = out_dir.join("zig_module.o");
        let zig_cache = out_dir.join("zig-cache");
        let status = Command::new(&zig)
            .env("ZIG_LOCAL_CACHE_DIR", &zig_cache)
            .env("ZIG_GLOBAL_CACHE_DIR", &zig_cache)
            .args([
                "build-obj",
                "-fPIC",
                "-OReleaseFast", // No safety checks → no panic/stdlib linkage
                &format!("-femit-bin={}", obj.display()),
                foreign.join("zig_module.zig").to_str().unwrap(),
            ])
            .status();

        if status.map(|s| s.success()).unwrap_or(false) && obj.exists() {
            cc::Build::new().object(&obj).compile("zig_module");
            println!("cargo:rustc-cfg=has_zig");
        }
    }
    println!("cargo:rerun-if-changed=foreign-code/zig_module.zig");

    // ── Nim module (when nim is on PATH) ─────────────────────────────────────
    if let Some(nim) = find_bin(
        "nim",
        &[
            "/home/linuxbrew/.linuxbrew/bin/nim",
            "/usr/local/bin/nim",
            "/opt/homebrew/bin/nim",
        ],
    ) {
        let lib = out_dir.join("nim_module.a");
        let nimcache = out_dir.join("nim_cache");
        let mut cmd = Command::new(&nim);
        cmd.args([
            "c",
            &format!("--nimcache:{}", nimcache.display()),
            "--noMain",
            "--app:staticlib",
            "--mm:none",
            "--passC:-fPIC",
            &format!("-o:{}", lib.display()),
        ]);
        if let Some(ref sdk_path) = sdk_path {
            cmd.arg(format!("--passC:-isysroot {}", sdk_path));
        }
        let status = cmd
            .arg(foreign.join("nim_module.nim").to_str().unwrap())
            .status();

        if status.map(|s| s.success()).unwrap_or(false) && lib.exists() {
            let lib_renamed = out_dir.join("libnim_module.a");
            let _ = std::fs::rename(&lib, &lib_renamed);
            println!("cargo:rustc-link-search=native={}", out_dir.display());
            println!("cargo:rustc-link-lib=static=nim_module");
            println!("cargo:rustc-cfg=has_nim");
        }
    }
    println!("cargo:rerun-if-changed=foreign-code/nim_module.nim");

    // ── V module (when v is on PATH) ───────────────────────────────────────
    // V's runtime-heavy object cannot link cleanly into Rust's PIE binary on
    // Linux. We detect V availability and compile a C shim with the same
    // exported symbols so the FFI calls work and has_v is set.
    if find_bin(
        "v",
        &[
            "/home/linuxbrew/.linuxbrew/bin/v",
            "/usr/local/bin/v",
            "/opt/homebrew/bin/v",
        ],
    )
    .is_some()
    {
        cc::Build::new()
            .file(foreign.join("v_module_shim.c"))
            .compile("v_module");
        println!("cargo:rustc-cfg=has_v");
    }
    println!("cargo:rerun-if-changed=foreign-code/v_module.v");
    println!("cargo:rerun-if-changed=foreign-code/v_module_shim.c");

    // ── D module (when ldc2 is on PATH) ───────────────────────────────────
    if let Some(ldc2) = find_bin(
        "ldc2",
        &[
            "/home/linuxbrew/.linuxbrew/bin/ldc2",
            "/usr/local/bin/ldc2",
            "/opt/homebrew/bin/ldc2",
        ],
    ) {
        let obj = out_dir.join("d_module.o");
        // Detect the ldc2 include dir from the binary path
        let ldc2_dir = ldc2
            .parent()
            .and_then(|p| p.parent())
            .map(|p| p.join("include/dlang/ldc"));
        let mut cmd = Command::new(&ldc2);
        cmd.args([
            "-c",
            "--relocation-model=pic",
            &format!("-of={}", obj.display()),
        ]);
        if let Some(ref inc) = ldc2_dir {
            if inc.exists() {
                cmd.arg(format!("-I{}", inc.display()));
            }
        }
        cmd.arg(foreign.join("d_module.d").to_str().unwrap());
        let status = cmd.status();

        if status.map(|s| s.success()).unwrap_or(false) && obj.exists() {
            cc::Build::new().object(&obj).compile("d_module");
            println!("cargo:rustc-cfg=has_d");
        }
    }
    println!("cargo:rerun-if-changed=foreign-code/d_module.d");

    // ── Odin module ─────────────────────────────────────────────────────────
    let odin_found = find_bin(
        "odin",
        &[
            "/usr/local/bin/odin",
            "/usr/local/odin/odin",
            "/home/linuxbrew/.linuxbrew/bin/odin",
            "/opt/homebrew/bin/odin",
        ],
    );
    if let Some(odin) = odin_found {
        let odin_root = std::env::var("ODIN_ROOT")
            .ok()
            .filter(|p| std::path::Path::new(p).exists())
            .or_else(|| {
                Command::new(&odin)
                    .arg("root")
                    .output()
                    .ok()
                    .and_then(|out| {
                        if out.status.success() {
                            Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
                        } else {
                            None
                        }
                    })
            });

        if let Some(odin_root) = odin_root {
            let odin_out = out_dir.join("odin_module.o");
            let status = Command::new(&odin)
                .env("ODIN_ROOT", &odin_root)
                .args([
                    "build",
                    foreign.join("odin_module.odin").to_str().unwrap(),
                    "-file",
                    &format!("-out:{}", odin_out.display()),
                    "-build-mode:obj",
                    "-reloc-mode:pic",
                ])
                .status();

            if status.map(|s| s.success()).unwrap_or(false) {
                let stem = odin_out.file_stem().unwrap().to_string_lossy().to_string();
                let mut any_linked = false;
                if let Ok(rd) = std::fs::read_dir(&out_dir) {
                    for entry in rd.flatten() {
                        let name = entry.file_name().to_string_lossy().to_string();
                        if name.starts_with(&stem) && name.ends_with(".o") {
                            cc::Build::new()
                                .object(entry.path())
                                .compile(&format!("odin_{}", name.trim_end_matches(".o")));
                            any_linked = true;
                        }
                    }
                }
                if any_linked {
                    println!("cargo:rustc-cfg=has_odin");
                }
            }
        }
    }
    println!("cargo:rerun-if-changed=foreign-code/odin_module.odin");
}

fn emit_bindings(header: &std::path::Path, out_dir: &std::path::Path, filename: &str) {
    let opts = equilibrium_ffi::BindingOptions::default();
    match equilibrium_ffi::generate_bindings(header, &opts) {
        Ok(binding) => {
            let _ = std::fs::write(out_dir.join(filename), &binding.code);
        }
        Err(e) => {
            eprintln!(
                "cargo:warning=equilibrium-ffi binding failed for {:?}: {}",
                header, e
            );
        }
    }
}
