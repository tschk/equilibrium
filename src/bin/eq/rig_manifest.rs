//! Read a `rig.toml` and work out which compilers `eq install` needs for it.

use toml::Value;

fn compiler_id(name: &str) -> Result<Option<&'static str>, String> {
    match name.to_ascii_lowercase().as_str() {
        "zig" => Ok(Some("zig")),
        "nim" => Ok(Some("nim")),
        "v" => Ok(Some("v")),
        "d" => Ok(Some("d")),
        "odin" => Ok(Some("odin")),
        "hare" => Ok(Some("hare")),
        "csharp" | "cs" | "dotnet" => Ok(Some("dotnet")),
        "scriptc" | "typescript" | "ts" => Ok(Some("scriptc")),
        "rust" | "cargo" | "c" | "cpp" | "c++" => Ok(None),
        other => Err(format!(
            "unknown language or ecosystem in rig.toml: {other}"
        )),
    }
}

pub fn compilers_for(manifest: &str) -> Result<Vec<String>, String> {
    let doc: Value = toml::from_str(manifest).map_err(|e| format!("invalid rig.toml: {e}"))?;
    let mut names: Vec<String> = Vec::new();
    if let Some(lang) = doc
        .get("host")
        .and_then(|h| h.get("language"))
        .and_then(Value::as_str)
    {
        names.push(lang.to_string());
    }
    if let Some(deps) = doc.get("dependencies").and_then(Value::as_table) {
        for (pkg, dep) in deps {
            let eco = dep
                .get("ecosystem")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("dependency {pkg} has no ecosystem"))?;
            names.push(eco.to_string());
        }
    }
    let mut out: Vec<String> = Vec::new();
    for name in names {
        if let Some(id) = compiler_id(&name)? {
            if !out.iter().any(|x| x == id) {
                out.push(id.to_string());
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_language_and_dependency_ecosystems_map_to_compilers() {
        let toml = r#"
schema_version = 1
[host]
language = "zig"
[dependencies.rx4]
ecosystem = "cargo"
[dependencies.jester]
ecosystem = "nim"
[dependencies.json]
ecosystem = "csharp"
"#;
        assert_eq!(compilers_for(toml).unwrap(), ["zig", "nim", "dotnet"]);
    }

    #[test]
    fn rust_and_c_hosts_need_no_installable_compiler_and_duplicates_collapse() {
        let toml = r#"
[host]
language = "rust"
[dependencies.a]
ecosystem = "cargo"
[dependencies.b]
ecosystem = "d"
[dependencies.c]
ecosystem = "D"
"#;
        assert_eq!(compilers_for(toml).unwrap(), ["d"]);
        assert!(compilers_for("[host]\nlanguage = \"c\"\n")
            .unwrap()
            .is_empty());
    }

    #[test]
    fn unknown_names_and_malformed_input_are_errors_not_silently_ignored() {
        assert!(compilers_for("[host]\nlanguage = \"cobol\"\n")
            .unwrap_err()
            .contains("cobol"));
        assert!(compilers_for("[dependencies.x]\nversion = \"1\"\n")
            .unwrap_err()
            .contains("no ecosystem"));
        assert!(compilers_for("not toml [")
            .unwrap_err()
            .contains("invalid rig.toml"));
    }
}
