//! Compile the TypeScript module with scriptc and generate Rust bindings.
//!
//! `load_with_options` compiles `foreign-code/math.ts` into a self-contained
//! scriptc library archive, writes the generated C header beside it (scriptc
//! emits none of its own), emits the Rust bindings, and prints the cargo link
//! directives for the archive. `equilibrium.toml` supplies the target's
//! marshalling classes.

use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());

    let module = equilibrium_ffi::load_with_options(
        manifest.join("foreign-code/math.ts"),
        equilibrium_ffi::LoadOptions::default()
            .output_dir(out_dir.join("scriptc"))
            .config_path(manifest.join("equilibrium.toml")),
    )
    .expect("load the scriptc module (is `scriptc` installed and on PATH?)");

    std::fs::write(
        out_dir.join("math_bindings.rs"),
        module.bindings_code().expect("bindings"),
    )
    .expect("write bindings");

    for warning in &module.warnings {
        println!("cargo:warning={warning}");
    }
    println!(
        "cargo:warning=scriptc module compiled to {}",
        module.output_path.display()
    );
    println!("cargo:rerun-if-changed=foreign-code/math.ts");
    println!("cargo:rerun-if-changed=equilibrium.toml");
}
