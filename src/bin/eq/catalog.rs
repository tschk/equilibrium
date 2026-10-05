//! Load `compilers.toml` once for the `eq` CLI.

use std::sync::OnceLock;

use serde::Deserialize;

#[derive(Clone, Debug)]
pub struct Install {
    pub brew: String,
    pub apt: String,
    pub dnf: String,
    pub pacman: String,
    pub npm: String,
    pub winget: String,
    pub scoop: String,
    pub scoop_bucket: String,
    pub manual: String,
}

#[derive(Clone, Debug)]
pub struct Compiler {
    pub id: String,
    pub lang: String,
    pub bin: String,
    pub extra_paths: Vec<String>,
    pub version_args: Vec<String>,
    pub install: Install,
    pub supported: bool,
}

#[derive(Deserialize)]
struct File {
    compiler: Vec<CompilerEntry>,
}

#[derive(Deserialize)]
struct CompilerEntry {
    id: String,
    lang: String,
    bin: String,
    #[serde(default)]
    extra_paths: Vec<String>,
    #[serde(default = "default_version_args")]
    version_args: Vec<String>,
    install: InstallEntry,
    #[serde(default)]
    linux_only: bool,
}

#[derive(Deserialize)]
struct InstallEntry {
    #[serde(default)]
    brew: String,
    #[serde(default)]
    apt: String,
    #[serde(default)]
    dnf: String,
    #[serde(default)]
    pacman: String,
    #[serde(default)]
    npm: String,
    #[serde(default)]
    winget: String,
    #[serde(default)]
    scoop: String,
    #[serde(default)]
    scoop_bucket: String,
    #[serde(default)]
    manual: String,
}

fn default_version_args() -> Vec<String> {
    vec!["--version".to_string()]
}

fn load_compilers() -> Vec<Compiler> {
    const BUNDLED: &str = include_str!("compilers.toml");
    let raw = match std::env::var_os("EQ_COMPILERS_FILE") {
        Some(path) => std::fs::read_to_string(&path).unwrap_or_else(|e| {
            eprintln!(
                "eq: cannot read EQ_COMPILERS_FILE {}: {e}",
                path.to_string_lossy()
            );
            std::process::exit(2);
        }),
        None => BUNDLED.to_string(),
    };
    let file: File = toml::from_str(&raw).unwrap_or_else(|e| {
        eprintln!("eq: invalid compiler catalogue: {e}");
        std::process::exit(2);
    });
    file.compiler
        .into_iter()
        .map(|e| {
            let supported = !e.linux_only || cfg!(target_os = "linux");
            Compiler {
                id: e.id,
                lang: e.lang,
                bin: e.bin,
                extra_paths: e.extra_paths,
                version_args: e.version_args,
                install: Install {
                    brew: e.install.brew,
                    apt: e.install.apt,
                    dnf: e.install.dnf,
                    pacman: e.install.pacman,
                    npm: e.install.npm,
                    winget: e.install.winget,
                    scoop: e.install.scoop,
                    scoop_bucket: e.install.scoop_bucket,
                    manual: e.install.manual,
                },
                supported,
            }
        })
        .collect()
}

static CATALOG: OnceLock<Vec<Compiler>> = OnceLock::new();

pub fn compilers() -> &'static [Compiler] {
    CATALOG.get_or_init(load_compilers).as_slice()
}

pub fn extra_path_refs(paths: &[String]) -> Vec<&str> {
    paths.iter().map(String::as_str).collect()
}

pub fn version_arg_refs(args: &[String]) -> Vec<&str> {
    args.iter().map(String::as_str).collect()
}
