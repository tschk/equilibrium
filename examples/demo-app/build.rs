use std::path::PathBuf;

fn main() {
    let foreign = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("foreign-code");
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());

    if cfg!(target_os = "macos") && std::path::Path::new("/usr/bin/ar").exists() {
        // SAFETY: build scripts are single-threaded at the point this runs.
        unsafe {
            std::env::set_var("AR", "/usr/bin/ar");
        }
    }
    cc::Build::new()
        .file(foreign.join("math.c"))
        .compile("math");

    let binding = equilibrium_ffi::generate_bindings(
        &foreign.join("math.h"),
        &equilibrium_ffi::BindingOptions::default(),
    )
    .expect("generate bindings");
    std::fs::write(out_dir.join("math_bindings.rs"), binding.code).expect("write bindings");

    println!("cargo:rerun-if-changed=foreign-code/math.c");
    println!("cargo:rerun-if-changed=foreign-code/math.h");
}
