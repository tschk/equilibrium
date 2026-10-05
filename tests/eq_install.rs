#![cfg(feature = "cli")]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const CATALOG: &str = r#"
[[compiler]]
id = "fakecc"
lang = "FakeCC"
bin = "fakecc"
[compiler.install]
brew = "fakecc"
manual = "https://example.invalid/fakecc"

[[compiler]]
id = "zig"
lang = "Zig"
bin = "fakezig"
[compiler.install]
brew = "fakezig"
manual = "https://example.invalid/fakezig"
"#;

const STUB: &str = r#"#!/bin/sh
echo "$EQ_STUB_NAME $*" >> "$EQ_TEST_LOG"
mode=$(eval echo "\$EQ_STUB_MODE_$EQ_STUB_NAME")
case "$mode" in
  ok)
    printf '#!/bin/sh\necho fake 1.0\n' > "$EQ_TEST_BIN/$2"
    chmod +x "$EQ_TEST_BIN/$2"
    exit 0 ;;
  lie) exit 0 ;;
  *) exit 1 ;;
esac
"#;

struct Env {
    dir: tempfile::TempDir,
    bin: PathBuf,
    log: PathBuf,
}

impl Env {
    fn new(stubs: &[&str]) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        fs::create_dir_all(&bin).unwrap();
        fs::create_dir_all(dir.path().join("home")).unwrap();
        fs::write(dir.path().join("catalog.toml"), CATALOG).unwrap();
        for name in stubs {
            let script = bin.join(name);
            let body = STUB
                .replacen("$EQ_STUB_NAME", name, 1)
                .replace("$EQ_STUB_NAME", name);
            fs::write(&script, body).unwrap();
            fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let log = dir.path().join("log");
        Env { dir, bin, log }
    }

    fn run(&self, managers: &str, modes: &[(&str, &str)], args: &[&str]) -> Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_eq"));
        cmd.env_clear()
            .env("PATH", format!("{}:/usr/bin:/bin", self.bin.display()))
            .env("HOME", self.dir.path().join("home"))
            .env("EQ_COMPILERS_FILE", self.dir.path().join("catalog.toml"))
            .env("EQ_INSTALL_MANAGERS", managers)
            .env("EQ_TEST_BIN", &self.bin)
            .env("EQ_TEST_LOG", &self.log)
            .current_dir(self.dir.path())
            .args(args)
            .stdin(Stdio::null());
        for (name, mode) in modes {
            cmd.env(format!("EQ_STUB_MODE_{name}"), mode);
        }
        cmd.output().unwrap()
    }

    fn log(&self) -> Vec<String> {
        fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    fn write(&self, name: &str, body: &str) -> PathBuf {
        let p = self.dir.path().join(name);
        fs::write(&p, body).unwrap();
        p
    }
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

#[test]
fn installs_through_the_first_manager_and_verifies_the_binary() {
    let env = Env::new(&["wax"]);
    let out = env.run("wax", &[("wax", "ok")], &["install", "fakecc"]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(env.log(), ["wax install fakecc"]);
    assert!(env.bin.join("fakecc").exists());
    assert!(text(&out).contains("FakeCC installed"));
}

#[test]
fn already_installed_compilers_run_no_package_manager() {
    let env = Env::new(&["wax"]);
    let existing = env.bin.join("fakecc");
    fs::write(&existing, "#!/bin/sh\necho fake\n").unwrap();
    fs::set_permissions(&existing, fs::Permissions::from_mode(0o755)).unwrap();
    let out = env.run("wax", &[("wax", "ok")], &["install", "fakecc"]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(env.log().is_empty());
    assert!(text(&out).contains("already installed"));
}

#[test]
fn duplicate_names_install_once() {
    let env = Env::new(&["wax"]);
    let out = env.run(
        "wax",
        &[("wax", "ok")],
        &["install", "fakecc", "FakeCC", "fakecc"],
    );
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(env.log(), ["wax install fakecc"]);
}

#[test]
fn a_manager_that_reports_success_without_installing_is_a_failure() {
    let env = Env::new(&["wax"]);
    let out = env.run("wax", &[("wax", "lie")], &["install", "fakecc"]);
    assert!(!out.status.success());
    assert!(text(&out).contains("reported success but `fakecc` was not found"));
    assert!(text(&out).contains("FakeCC failed"));
    assert_eq!(env.log(), ["wax install fakecc"]);
}

#[test]
fn a_failing_manager_falls_back_to_the_next_in_order() {
    let env = Env::new(&["wax", "brew"]);
    let out = env.run(
        "wax,brew",
        &[("wax", "fail"), ("brew", "ok")],
        &["install", "fakecc"],
    );
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(env.log(), ["wax install fakecc", "brew install fakecc"]);
}

#[test]
fn managers_outside_the_allow_list_are_never_used() {
    let env = Env::new(&["wax", "brew"]);
    let out = env.run(
        "wax",
        &[("wax", "fail"), ("brew", "ok")],
        &["install", "fakecc"],
    );
    assert!(!out.status.success());
    assert_eq!(env.log(), ["wax install fakecc"]);
    assert!(!env.bin.join("fakecc").exists());
}

#[test]
fn an_empty_allow_list_means_no_manager_and_prints_the_manual_hint() {
    let env = Env::new(&["wax", "brew"]);
    let out = env.run("", &[("wax", "ok"), ("brew", "ok")], &["install", "fakecc"]);
    assert!(!out.status.success());
    assert!(text(&out).contains("No supported package manager found"));
    assert!(text(&out).contains("https://example.invalid/fakecc"));
    assert!(env.log().is_empty());
}

#[test]
fn a_lying_manager_also_falls_through_to_the_next() {
    let env = Env::new(&["wax", "brew"]);
    let out = env.run(
        "wax,brew",
        &[("wax", "lie"), ("brew", "ok")],
        &["install", "fakecc"],
    );
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(env.log(), ["wax install fakecc", "brew install fakecc"]);
    assert!(text(&out).contains("trying the next manager"));
}

#[test]
fn every_manager_failing_is_a_failure_with_the_manual_hint() {
    let env = Env::new(&["wax", "brew"]);
    let out = env.run(
        "wax,brew",
        &[("wax", "fail"), ("brew", "fail")],
        &["install", "fakecc"],
    );
    assert!(!out.status.success());
    assert!(text(&out).contains("https://example.invalid/fakecc"));
}

#[test]
fn no_names_and_no_terminal_fails_instead_of_silently_succeeding() {
    let env = Env::new(&["wax"]);
    let out = env.run("wax", &[("wax", "ok")], &["install"]);
    assert!(!out.status.success());
    assert!(text(&out).contains("no terminal"));
    assert!(text(&out).contains("fakecc"));
    assert!(env.log().is_empty());
}

#[test]
fn unknown_compilers_fail_without_running_a_manager() {
    let env = Env::new(&["wax"]);
    let out = env.run("wax", &[("wax", "ok")], &["install", "nosuch"]);
    assert!(!out.status.success());
    assert!(text(&out).contains("Unknown compiler: nosuch"));
    assert!(env.log().is_empty());
}

#[test]
fn an_unknown_name_does_not_stop_valid_ones_but_still_fails_overall() {
    let env = Env::new(&["wax"]);
    let out = env.run("wax", &[("wax", "ok")], &["install", "nosuch", "fakecc"]);
    assert!(!out.status.success());
    assert_eq!(env.log(), ["wax install fakecc"]);
}

#[test]
fn from_rig_installs_what_the_manifest_needs() {
    let env = Env::new(&["wax"]);
    let rig = env.write(
        "rig.toml",
        "schema_version = 1\n[host]\nlanguage = \"zig\"\n[dependencies.rx4]\necosystem = \"cargo\"\n",
    );
    let out = env.run(
        "wax",
        &[("wax", "ok")],
        &["install", "--from-rig", rig.to_str().unwrap()],
    );
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(env.log(), ["wax install fakezig"]);
}

#[test]
fn from_rig_defaults_to_rig_toml_in_the_working_directory() {
    let env = Env::new(&["wax"]);
    env.write("rig.toml", "[host]\nlanguage = \"zig\"\n");
    let out = env.run("wax", &[("wax", "ok")], &["install", "--from-rig"]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(env.log(), ["wax install fakezig"]);
}

#[test]
fn from_rig_rejects_unknown_ecosystems_before_installing_anything() {
    let env = Env::new(&["wax"]);
    let rig = env.write(
        "rig.toml",
        "[host]\nlanguage = \"zig\"\n[dependencies.x]\necosystem = \"cobol\"\n",
    );
    let out = env.run(
        "wax",
        &[("wax", "ok")],
        &["install", "--from-rig", rig.to_str().unwrap()],
    );
    assert!(!out.status.success());
    assert!(text(&out).contains("cobol"));
    assert!(env.log().is_empty());
}

#[test]
fn from_rig_with_a_missing_file_fails_clearly() {
    let env = Env::new(&["wax"]);
    let missing: &Path = &env.dir.path().join("nope.toml");
    let out = env.run(
        "wax",
        &[("wax", "ok")],
        &["install", "--from-rig", missing.to_str().unwrap()],
    );
    assert!(!out.status.success());
    assert!(text(&out).contains("cannot read"));
}

#[test]
fn from_rig_for_a_rust_only_project_needs_nothing() {
    let env = Env::new(&["wax"]);
    let rig = env.write("rig.toml", "[host]\nlanguage = \"rust\"\n");
    let out = env.run(
        "wax",
        &[("wax", "ok")],
        &["install", "--from-rig", rig.to_str().unwrap()],
    );
    assert!(out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("needs no installable compilers"));
    assert!(env.log().is_empty());
}
