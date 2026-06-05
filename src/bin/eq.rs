//! eq — the equilibrium-ffi CLI
//!
//! Commands:
//!   eq check              — show which compilers are installed
//!   eq install            — interactive multi-select installer
//!   eq install zig nim … — install specific compilers directly
//!   eq build [ARGS…]      — cargo build with compilers on PATH
//!   `eq generate <HEADER>`  — emit Rust FFI bindings from a C header

use clap::{Parser, Subcommand};
use console::{style, Style, Term};
use dialoguer::{theme::ColorfulTheme, MultiSelect};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

// ── CLI definition ────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(name = "eq", about = "equilibrium-ffi — polyglot FFI toolkit", version)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Show which compilers are installed (and their versions)
    Check,
    /// Interactively install missing compilers
    Install {
        /// Install these specific compilers instead of showing a selector
        names: Vec<String>,
    },
    /// Run `cargo build` with all compilers added to PATH
    Build {
        /// Extra arguments forwarded to cargo
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Generate Rust FFI bindings from a C header
    Generate {
        /// Path to the C header
        header: PathBuf,
        /// Write output to this file (default: stdout)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
}

// ── Compiler catalogue ────────────────────────────────────────────────────────

struct Compiler {
    /// Short id used on the command line
    id: &'static str,
    /// Human-readable language name
    lang: &'static str,
    /// Binary to look for
    bin: &'static str,
    /// Extra PATH entries to search beyond $PATH
    extra_paths: &'static [&'static str],
    /// Args to pass for version query
    version_args: &'static [&'static str],
    /// Install recipe
    install: Install,
    /// Whether this compiler is supported on this platform at all
    supported: bool,
}

/// Per-package-manager package names.
/// Empty string means "not available via this manager".
struct Install {
    /// brew / wax package name (same namespace)
    brew: &'static str,
    /// apt package name (Debian/Ubuntu)
    apt: &'static str,
    /// dnf package name (Fedora/RHEL)
    dnf: &'static str,
    /// pacman package name (Arch)
    pacman: &'static str,
    /// winget package ID (used with `winget install -e --id <id>`)
    winget: &'static str,
    /// scoop package name
    scoop: &'static str,
    /// scoop bucket to add before installing (empty = main, no add needed)
    scoop_bucket: &'static str,
    /// Manual install URL (always shown as last resort)
    manual: &'static str,
}

const COMPILERS: &[Compiler] = &[
    Compiler {
        id: "zig",
        lang: "Zig",
        bin: "zig",
        extra_paths: &[
            "/usr/local/sbin",
            "/usr/local/bin",
            "/home/linuxbrew/.linuxbrew/bin",
            "/opt/homebrew/bin",
        ],
        version_args: &["version"],
        install: Install {
            brew: "zig",
            apt: "zig",
            dnf: "zig",
            pacman: "zig",
            winget: "zig.zig",
            scoop: "zig",
            scoop_bucket: "",
            manual: "https://ziglang.org/download/",
        },
        supported: true,
    },
    Compiler {
        id: "nim",
        lang: "Nim",
        bin: "nim",
        extra_paths: &[
            "/home/linuxbrew/.linuxbrew/bin",
            "/opt/homebrew/bin",
            "/usr/local/bin",
        ],
        version_args: &["--version"],
        install: Install {
            brew: "nim",
            apt: "nim",
            dnf: "nim",
            pacman: "nim",
            winget: "nim.nim",
            scoop: "nim",
            scoop_bucket: "",
            manual: "https://nim-lang.org/install.html",
        },
        supported: true,
    },
    Compiler {
        id: "v",
        lang: "V",
        bin: "v",
        extra_paths: &[
            "/home/linuxbrew/.linuxbrew/bin",
            "/opt/homebrew/bin",
            "/usr/local/bin",
        ],
        version_args: &["version"],
        install: Install {
            brew: "vlang",
            apt: "",
            dnf: "",
            pacman: "vlang",
            winget: "", // not in winget catalog
            scoop: "v", // scoop main bucket: package is named "v"
            scoop_bucket: "",
            manual: "https://vlang.io/",
        },
        supported: true,
    },
    Compiler {
        id: "d",
        lang: "D (ldc2)",
        bin: "ldc2",
        extra_paths: &[
            "/home/linuxbrew/.linuxbrew/bin",
            "/opt/homebrew/bin",
            "/usr/local/bin",
        ],
        version_args: &["--version"],
        install: Install {
            brew: "ldc",
            apt: "ldc",
            dnf: "ldc",
            pacman: "ldc",
            winget: "",   // not in winget catalog
            scoop: "ldc", // scoop main bucket
            scoop_bucket: "",
            manual: "https://github.com/ldc-developers/ldc/releases",
        },
        supported: true,
    },
    Compiler {
        id: "odin",
        lang: "Odin",
        bin: "odin",
        extra_paths: &[
            "/usr/local/bin",
            "/usr/local/odin",
            "/home/linuxbrew/.linuxbrew/bin",
            "/opt/homebrew/bin",
        ],
        version_args: &["version"],
        install: Install {
            brew: "odin",
            apt: "",
            dnf: "",
            pacman: "odin",
            winget: "odin-lang.Odin",
            scoop: "odin", // scoop versions bucket
            scoop_bucket: "versions",
            manual: "https://odin-lang.org/docs/install/",
        },
        supported: true,
    },
    Compiler {
        id: "hare",
        lang: "Hare",
        bin: "hare",
        extra_paths: &["/usr/local/bin", "/usr/bin"],
        version_args: &["version"],
        install: Install {
            brew: "",
            apt: "hare",
            dnf: "hare",
            pacman: "hare",
            winget: "",
            scoop: "",
            scoop_bucket: "",
            manual: "https://harelang.org/",
        },
        supported: cfg!(target_os = "linux"),
    },
    Compiler {
        id: "dotnet",
        lang: "C# (dotnet)",
        bin: "dotnet",
        extra_paths: &[
            "/home/linuxbrew/.linuxbrew/bin",
            "/opt/homebrew/bin",
            r"C:\Program Files\dotnet",
        ],
        version_args: &["--version"],
        install: Install {
            brew: "dotnet",
            apt: "dotnet-sdk-9.0",
            dnf: "dotnet-sdk-9.0",
            pacman: "dotnet-sdk",
            winget: "Microsoft.DotNet.SDK.9",
            scoop: "dotnet-sdk",
            scoop_bucket: "",
            manual: "https://dot.net/",
        },
        supported: true,
    },
];

// ── Compiler detection ────────────────────────────────────────────────────────

fn find_bin(bin: &str, extra_paths: &[&str]) -> Option<PathBuf> {
    which::which(bin).ok().or_else(|| {
        extra_paths
            .iter()
            .map(|p| PathBuf::from(p).join(bin))
            .find(|p| p.exists())
    })
}

fn compiler_version(path: &Path, version_args: &[&str]) -> Option<String> {
    let out = Command::new(path).args(version_args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let text = if stdout.trim().is_empty() {
        stderr
    } else {
        stdout
    };
    let line = text.lines().next()?.trim();
    // Strip a leading absolute-path token some compilers (odin) emit
    let line = if line.starts_with('/') || line.starts_with(r"C:\") {
        line.split_once(' ').map(|x| x.1).unwrap_or(line).trim()
    } else {
        line
    }
    .to_string();
    if line.is_empty() {
        None
    } else {
        Some(line)
    }
}

struct Status<'a> {
    compiler: &'a Compiler,
    path: Option<PathBuf>,
    version: Option<String>,
}

fn check_all() -> Vec<Status<'static>> {
    COMPILERS
        .iter()
        .map(|c| {
            let path = find_bin(c.bin, c.extra_paths);
            let version = path
                .as_deref()
                .and_then(|p| compiler_version(p, c.version_args));
            Status {
                compiler: c,
                path,
                version,
            }
        })
        .collect()
}

// ── Subcommand: check ─────────────────────────────────────────────────────────

fn cmd_check() -> ExitCode {
    let statuses = check_all();
    let term = Term::stdout();
    let _ = term.write_line("");
    let _ = term.write_line(&format!(
        "  {}",
        style("equilibrium-ffi — compiler status").bold()
    ));
    let _ = term.write_line("");

    for s in &statuses {
        let (marker, name_style) = if !s.compiler.supported {
            (style("~").dim(), Style::new().dim())
        } else if s.path.is_some() {
            (style("✓").green().bold(), Style::new().green())
        } else {
            (style("✗").red(), Style::new().dim())
        };

        let name = name_style.apply_to(format!("{:<12}", s.compiler.lang));

        let detail = match (&s.path, &s.version) {
            (Some(p), Some(v)) => format!("{v}  ({})", p.display()),
            (Some(p), None) => format!("installed  ({})", p.display()),
            (None, _) if !s.compiler.supported => "not supported on this platform".to_string(),
            (None, _) => format!("not found  — run: eq install {}", s.compiler.id),
        };

        let _ = term.write_line(&format!("  {marker}  {name}  {}", style(detail).dim()));
    }
    let _ = term.write_line("");
    ExitCode::SUCCESS
}

// ── Subcommand: install ───────────────────────────────────────────────────────

/// Detect which package managers are available, in preference order.
#[derive(Clone, Copy, PartialEq)]
enum PkgMgr {
    Wax,
    Brew,
    Apt,
    Dnf,
    Pacman,
    Winget,
    Scoop,
}

impl PkgMgr {
    fn cmd(self) -> &'static str {
        match self {
            PkgMgr::Wax => "wax",
            PkgMgr::Brew => "brew",
            PkgMgr::Apt => "apt-get",
            PkgMgr::Dnf => "dnf",
            PkgMgr::Pacman => "pacman",
            PkgMgr::Winget => "winget",
            PkgMgr::Scoop => "scoop",
        }
    }

    fn pkg_name(self, install: &Install) -> &'static str {
        match self {
            PkgMgr::Wax | PkgMgr::Brew => install.brew,
            PkgMgr::Apt => install.apt,
            PkgMgr::Dnf => install.dnf,
            PkgMgr::Pacman => install.pacman,
            PkgMgr::Winget => install.winget,
            PkgMgr::Scoop => install.scoop,
        }
    }

    fn install_args(self, pkg: &str) -> Vec<String> {
        match self {
            PkgMgr::Wax | PkgMgr::Brew => vec!["install".into(), pkg.into()],
            PkgMgr::Apt => vec!["install".into(), "-y".into(), pkg.into()],
            PkgMgr::Dnf => vec!["install".into(), "-y".into(), pkg.into()],
            PkgMgr::Pacman => vec!["-S".into(), "--noconfirm".into(), pkg.into()],
            // -e = exact match; --id avoids interactive prompts
            PkgMgr::Winget => vec!["install".into(), "-e".into(), "--id".into(), pkg.into()],
            PkgMgr::Scoop => vec!["install".into(), pkg.into()],
        }
    }

    /// Whether this manager needs sudo on Linux
    fn needs_sudo(self) -> bool {
        matches!(self, PkgMgr::Apt | PkgMgr::Dnf | PkgMgr::Pacman)
    }
}

fn has_wax() -> bool {
    if which::which("wax").is_ok() {
        return true;
    }
    let home = std::env::var("HOME").unwrap_or_default();
    let candidates = [
        format!("{home}/.local/bin/wax"),
        "/usr/local/bin/wax".to_string(),
        r"C:\ProgramData\wax\bin\wax.exe".to_string(),
    ];
    candidates.iter().any(|p| PathBuf::from(p).exists())
}

fn available_managers() -> Vec<PkgMgr> {
    let mut v = vec![];

    // wax is tried first on all platforms
    if has_wax() {
        v.push(PkgMgr::Wax);
    }

    if cfg!(target_os = "windows") {
        // scoop has broader coverage than winget for dev tools
        if which::which("scoop").is_ok() {
            v.push(PkgMgr::Scoop);
        }
        if which::which("winget").is_ok() {
            v.push(PkgMgr::Winget);
        }
        return v;
    }

    // homebrew / linuxbrew
    if which::which("brew").is_ok()
        || PathBuf::from("/home/linuxbrew/.linuxbrew/bin/brew").exists()
        || PathBuf::from("/opt/homebrew/bin/brew").exists()
    {
        v.push(PkgMgr::Brew);
    }
    if cfg!(target_os = "linux") {
        if which::which("apt-get").is_ok() {
            v.push(PkgMgr::Apt);
        }
        if which::which("dnf").is_ok() {
            v.push(PkgMgr::Dnf);
        }
        if which::which("pacman").is_ok() {
            v.push(PkgMgr::Pacman);
        }
    }
    v
}

fn install_compiler(c: &Compiler) -> bool {
    // On Windows, if the process cwd is a UNC path (e.g. \\wsl$\...) then
    // cmd.exe subprocesses spawned by winget/scoop will fail. Move to %TEMP%.
    #[cfg(target_os = "windows")]
    {
        let cwd = std::env::current_dir()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        if cwd.starts_with(r"\\") {
            let tmp = std::env::var("TEMP")
                .or_else(|_| std::env::var("TMP"))
                .unwrap_or_else(|_| r"C:\Windows\Temp".to_string());
            let _ = std::env::set_current_dir(&tmp);
        }
    }

    let managers = available_managers();
    if managers.is_empty() {
        println!(
            "  {} No supported package manager found.",
            style("!").yellow()
        );
        if cfg!(target_os = "windows") {
            println!("  Tip: install scoop first — https://scoop.sh");
        }
        println!("  Manual install: {}", c.install.manual);
        return false;
    }

    for mgr in &managers {
        let pkg = mgr.pkg_name(&c.install);
        if pkg.is_empty() {
            continue; // not available via this manager
        }

        // Scoop: add non-main bucket if required (idempotent)
        if *mgr == PkgMgr::Scoop && !c.install.scoop_bucket.is_empty() {
            let bucket = c.install.scoop_bucket;
            let bucket_cmd = format!("scoop bucket add {bucket}");
            println!("  {} {}", style("$").dim(), style(&bucket_cmd).cyan());
            let scoop_bin = find_bin("scoop", &[]).unwrap_or_else(|| PathBuf::from("scoop"));
            let _ = Command::new(&scoop_bin)
                .args(["bucket", "add", bucket])
                .status();
        }

        let args = mgr.install_args(pkg);
        let cmd_display = if mgr.needs_sudo() {
            format!("sudo {} {}", mgr.cmd(), args.join(" "))
        } else {
            format!("{} {}", mgr.cmd(), args.join(" "))
        };
        println!("  {} {}", style("$").dim(), style(&cmd_display).cyan());

        let status = if mgr.needs_sudo() {
            // Use argv directly — never invoke a shell around sudo/package managers.
            Command::new("sudo").arg(mgr.cmd()).args(&args).status()
        } else {
            // Resolve full path for wax/brew/winget in case they aren't on PATH
            let home = std::env::var("HOME").unwrap_or_default();
            let bin = find_bin(
                mgr.cmd(),
                &[
                    "/home/linuxbrew/.linuxbrew/bin",
                    "/opt/homebrew/bin",
                    &format!("{home}/.local/bin"),
                ],
            )
            .unwrap_or_else(|| PathBuf::from(mgr.cmd()));
            Command::new(bin).args(&args).status()
        };

        if status.map(|s| s.success()).unwrap_or(false) {
            return true;
        }
    }

    println!(
        "  {} All package managers failed. Manual: {}",
        style("!").yellow(),
        c.install.manual
    );
    false
}

fn cmd_install(names: Vec<String>) -> ExitCode {
    let statuses = check_all();

    if !names.is_empty() {
        let mut to_install: Vec<&'static Compiler> = vec![];
        let mut had_error = false;

        for name in &names {
            let name_lower = name.to_lowercase();
            if let Some(s) = statuses.iter().find(|s| {
                s.compiler.id == name_lower
                    || s.compiler.bin == name_lower
                    || s.compiler.lang.to_lowercase().contains(&name_lower)
            }) {
                if !s.compiler.supported {
                    println!(
                        "{} {} is not supported on this platform.",
                        style("!").yellow(),
                        s.compiler.lang
                    );
                    had_error = true;
                } else if s.path.is_some() {
                    println!(
                        "{} {} is already installed.",
                        style("✓").green(),
                        s.compiler.lang
                    );
                } else {
                    to_install.push(s.compiler);
                }
            } else {
                println!("{} Unknown compiler: {name}", style("✗").red());
                had_error = true;
            }
        }

        if to_install.is_empty() {
            return if had_error {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            };
        }
        let install_ok = run_installs_parallel(&to_install);
        return if had_error {
            ExitCode::FAILURE
        } else {
            install_ok
        };
    }

    // Interactive multi-select for missing compilers
    let missing: Vec<&Status> = statuses
        .iter()
        .filter(|s| s.compiler.supported && s.path.is_none())
        .collect();

    if missing.is_empty() {
        println!(
            "{} All supported compilers are already installed.",
            style("✓").green()
        );
        return ExitCode::SUCCESS;
    }

    let items: Vec<String> = missing
        .iter()
        .map(|s| format!("{:<14} ({})", s.compiler.lang, s.compiler.id))
        .collect();

    println!();
    let selections = MultiSelect::with_theme(&ColorfulTheme::default())
        .with_prompt("Select compilers to install (space to toggle, enter to confirm)")
        .items(&items)
        .interact_opt();

    match selections {
        Ok(Some(chosen)) if !chosen.is_empty() => {
            let selected: Vec<&'static Compiler> =
                chosen.iter().map(|&i| missing[i].compiler).collect();
            run_installs_parallel(&selected)
        }
        _ => {
            println!("Nothing selected.");
            ExitCode::SUCCESS
        }
    }
}

/// Install multiple compilers in parallel, one thread each.
fn run_installs_parallel(compilers: &[&'static Compiler]) -> ExitCode {
    use std::sync::{Arc, Mutex};
    use std::thread;

    println!(
        "\n{} Installing {} compiler(s) in parallel…\n",
        style("→").cyan(),
        compilers.len()
    );

    // Shared output buffer so lines from different threads don't interleave.
    let log: Arc<Mutex<Vec<(String, bool)>>> = Arc::new(Mutex::new(vec![]));

    let handles: Vec<_> = compilers
        .iter()
        .map(|c| {
            let log = Arc::clone(&log);
            let name = c.lang;
            let compiler: &'static Compiler = c;
            thread::spawn(move || {
                let ok = install_compiler(compiler);
                let msg = if ok {
                    format!("{} {} installed", style("✓").green(), name)
                } else {
                    format!("{} {} failed", style("✗").red(), name)
                };
                log.lock().unwrap().push((msg, ok));
                ok
            })
        })
        .collect();

    let all_ok = handles.into_iter().all(|h| h.join().unwrap_or(false));

    println!();
    for (msg, _) in log.lock().unwrap().iter() {
        println!("{msg}");
    }

    if all_ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

// ── Subcommand: build ─────────────────────────────────────────────────────────

fn cmd_build(args: Vec<String>) -> ExitCode {
    let extra = [
        "/home/linuxbrew/.linuxbrew/bin",
        "/home/linuxbrew/.linuxbrew/sbin",
        "/opt/homebrew/bin",
        "/opt/homebrew/sbin",
        "/usr/local/sbin",
        "/usr/local/bin",
        "/usr/local/odin",
    ];
    let current = std::env::var("PATH").unwrap_or_default();
    let sep = if cfg!(windows) { ";" } else { ":" };
    let prepend: String = extra
        .iter()
        .filter(|p| !current.contains(*p) && PathBuf::from(p).exists())
        .cloned()
        .collect::<Vec<_>>()
        .join(sep);
    let new_path = if prepend.is_empty() {
        current.clone()
    } else {
        format!("{prepend}{sep}{current}")
    };

    let mut cargo_args = vec!["build".to_string()];
    cargo_args.extend(args);

    println!("{} cargo {}", style("→").cyan(), cargo_args.join(" "));

    let status = Command::new("cargo")
        .args(&cargo_args)
        .env("PATH", &new_path)
        .status();

    match status {
        Ok(s) if s.success() => ExitCode::SUCCESS,
        Ok(s) => ExitCode::from(s.code().unwrap_or(1) as u8),
        Err(e) => {
            eprintln!("{} failed to run cargo: {e}", style("✗").red());
            ExitCode::FAILURE
        }
    }
}

// ── Subcommand: generate ──────────────────────────────────────────────────────

fn cmd_generate(header: PathBuf, output: Option<PathBuf>) -> ExitCode {
    let is_header = header
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| matches!(ext, "h" | "hh" | "hpp" | "hxx"));
    if !is_header {
        return match equilibrium_ffi::load(&header) {
            Ok(module) => {
                if let Some(binding) = module.bindings {
                    match output {
                        Some(path) => {
                            if let Err(e) = std::fs::write(&path, &binding.code) {
                                eprintln!("{} write failed: {e}", style("✗").red());
                                return ExitCode::FAILURE;
                            }
                            println!("{} wrote {}", style("✓").green(), path.display());
                        }
                        None => print!("{}", binding.code),
                    }
                    ExitCode::SUCCESS
                } else {
                    eprintln!(
                        "{} no bindings generated; exports: {}",
                        style("✗").red(),
                        module.exports.join(", ")
                    );
                    ExitCode::FAILURE
                }
            }
            Err(e) => {
                eprintln!("{} source generation failed: {e}", style("✗").red());
                ExitCode::FAILURE
            }
        };
    }

    let opts = equilibrium_ffi::BindingOptions::default();
    match equilibrium_ffi::generate_bindings(&header, &opts) {
        Ok(binding) => {
            match output {
                Some(path) => {
                    if let Err(e) = std::fs::write(&path, &binding.code) {
                        eprintln!("{} write failed: {e}", style("✗").red());
                        return ExitCode::FAILURE;
                    }
                    println!("{} wrote {}", style("✓").green(), path.display());
                }
                None => print!("{}", binding.code),
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{} binding generation failed: {e}", style("✗").red());
            ExitCode::FAILURE
        }
    }
}

// ── Entry point ───────────────────────────────────────────────────────────────

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Cmd::Check => cmd_check(),
        Cmd::Install { names } => cmd_install(names),
        Cmd::Build { args } => cmd_build(args),
        Cmd::Generate { header, output } => cmd_generate(header, output),
    }
}
