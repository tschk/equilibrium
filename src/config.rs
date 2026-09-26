//! `equilibrium.toml` — per-target language settings.
//!
//! Export discovery and compilation read the same file: the `[target.<name>]`
//! table that governs a source lists its exports and, for scriptc, the ABI
//! surface and emission of the generated library profile.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::detector::Language;
use crate::limits::read_config_text;

#[derive(Deserialize)]
struct EquilibriumConfig {
    target: Option<BTreeMap<String, TargetConfig>>,
}

/// One `[target.<name>]` table.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TargetConfig {
    pub(crate) language: Option<String>,
    pub(crate) sources: Option<Vec<String>>,
    pub(crate) exports: Option<Vec<String>>,
    /// scriptc only: profile emission, `"llvm"` (default) or `"c"`.
    pub(crate) emission: Option<String>,
    /// scriptc only: marshalling overrides, keyed by TypeScript export name.
    pub(crate) signatures: Option<BTreeMap<String, SignatureConfig>>,
}

/// One `[target.<name>.signatures.<export>]` entry.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SignatureConfig {
    /// scriptc marshalling classes for the parameters, in order.
    pub(crate) params: Option<Vec<String>>,
    /// The scriptc marshalling class of the returned value.
    pub(crate) returns: Option<String>,
}

/// A malformed or unreadable config, with the file it came from.
#[derive(Debug)]
pub(crate) struct ConfigError {
    pub(crate) path: PathBuf,
    pub(crate) message: String,
}

/// The `[target.*]` entry governing `source`, if the config names one.
pub(crate) fn target_for(
    source: &Path,
    config_path: Option<&Path>,
    language: Language,
) -> Result<Option<TargetConfig>, ConfigError> {
    for path in candidates(source, config_path) {
        if !path.is_file() {
            continue;
        }
        let text = read_config_text(&path).map_err(|message| ConfigError {
            path: path.clone(),
            message,
        })?;
        let config: EquilibriumConfig = toml::from_str(&text).map_err(|error| ConfigError {
            path: path.clone(),
            message: error.to_string(),
        })?;
        let Some(targets) = config.target else {
            continue;
        };
        let base = path.parent().unwrap_or(Path::new("."));
        for target in targets.into_values() {
            if target_matches(&target, source, base, language) {
                return Ok(Some(target));
            }
        }
    }
    Ok(None)
}

fn candidates(source: &Path, config_path: Option<&Path>) -> Vec<PathBuf> {
    if let Some(path) = config_path {
        return vec![path.to_path_buf()];
    }
    let mut candidates = Vec::new();
    if let Some(parent) = source.parent() {
        candidates.push(parent.join("equilibrium.toml"));
    }
    if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
        candidates.push(PathBuf::from(manifest_dir).join("equilibrium.toml"));
    }
    candidates.dedup();
    candidates
}

fn target_matches(target: &TargetConfig, source: &Path, base: &Path, language: Language) -> bool {
    if let Some(target_language) = &target.language {
        if target_language.to_ascii_lowercase() != language.cli_name() {
            return false;
        }
    }
    let Some(sources) = &target.sources else {
        return false;
    };
    let canonical_source = source
        .canonicalize()
        .unwrap_or_else(|_| source.to_path_buf());
    sources.iter().any(|candidate| {
        let candidate_path = base.join(candidate);
        let canonical_candidate = candidate_path
            .canonicalize()
            .unwrap_or_else(|_| candidate_path.clone());
        canonical_candidate == canonical_source || candidate_path == source
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write_config(dir: &Path, body: &str) -> PathBuf {
        let path = dir.join("equilibrium.toml");
        std::fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn reads_exports_and_scriptc_settings() {
        let dir = tempdir().unwrap();
        let source = dir.path().join("math.ts");
        std::fs::write(&source, "").unwrap();
        write_config(
            dir.path(),
            r#"
[target.math]
language = "scriptc"
sources = ["math.ts"]
exports = ["mix"]
emission = "c"

[target.math.signatures]
mix = { params = ["u32", "u32"], returns = "f64" }
"#,
        );

        let target = target_for(&source, None, Language::ScriptC)
            .unwrap()
            .expect("target matches the source");
        assert_eq!(target.exports.as_deref(), Some(&["mix".to_string()][..]));
        assert_eq!(target.emission.as_deref(), Some("c"));
        let signatures = target.signatures.expect("signatures");
        let mix = &signatures["mix"];
        assert_eq!(mix.params.as_deref().unwrap(), ["u32", "u32"]);
        assert_eq!(mix.returns.as_deref(), Some("f64"));
    }

    #[test]
    fn unknown_keys_are_refused() {
        let dir = tempdir().unwrap();
        let source = dir.path().join("math.ts");
        std::fs::write(&source, "").unwrap();
        write_config(
            dir.path(),
            r#"
[target.math]
language = "scriptc"
sources = ["math.ts"]
emmission = "c"
"#,
        );

        let error = target_for(&source, None, Language::ScriptC).unwrap_err();
        assert!(error.message.contains("emmission"), "{}", error.message);
    }

    #[test]
    fn explicit_config_path_wins() {
        let dir = tempdir().unwrap();
        let source = dir.path().join("math.ts");
        std::fs::write(&source, "").unwrap();
        write_config(
            dir.path(),
            r#"
[target.math]
language = "scriptc"
sources = ["math.ts"]
exports = ["from_default"]
"#,
        );
        let custom = dir.path().join("custom.toml");
        std::fs::write(
            &custom,
            r#"
[target.math]
language = "scriptc"
sources = ["math.ts"]
exports = ["from_custom"]
"#,
        )
        .unwrap();

        let target = target_for(&source, Some(&custom), Language::ScriptC)
            .unwrap()
            .expect("target");
        assert_eq!(
            target.exports.as_deref(),
            Some(&["from_custom".to_string()][..])
        );
    }

    #[test]
    fn sources_are_required_to_match() {
        let dir = tempdir().unwrap();
        let source = dir.path().join("math.ts");
        std::fs::write(&source, "").unwrap();
        write_config(
            dir.path(),
            r#"
[target.math]
language = "scriptc"
exports = ["mix"]
"#,
        );

        assert!(target_for(&source, None, Language::ScriptC)
            .unwrap()
            .is_none());
    }
}
