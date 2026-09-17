// Cloud-executor sandbox.
//
// Wraps a `bash` command in bubblewrap (`bwrap`) so a cloud executor cannot
// read the host's `$HOME` (vault keys, other projects' config) or touch the
// repo's `.rexymcp/` state: the host is read-only, `/home`, `/tmp` and the
// (possibly non-`/home`) home directory are empty tmpfs mounts, only the
// Rust toolchain under `$HOME` is visible read-only, the repo is writable,
// and the repo's `.rexymcp/` is an empty scratch directory. A failing probe
// before turn 1 makes `run_phase` refuse a cloud dispatch (see `runner`).

use std::path::{Path, PathBuf};

/// How the repo root holds its git metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitDir {
    /// `<root>/.git` is a directory (a normal clone).
    Dir,
    /// `<root>/.git` is a file (a git worktree).
    File,
}

#[derive(Debug, Clone)]
pub struct Sandbox {
    program: String,
    root: PathBuf,
    home: Option<PathBuf>,
    git: Option<GitDir>,
}

impl Sandbox {
    /// `home` is the value of `$HOME`, or `None` when unset. `program` = `"bwrap"`.
    pub fn new(root: &std::path::Path, home: Option<PathBuf>) -> Self {
        Self {
            program: "bwrap".to_string(),
            root: root.to_path_buf(),
            home,
            git: None,
        }
    }

    /// A sandbox for the repo at `root` (canonical), with its git layout
    /// detected. Fails when the sandbox cannot protect the repo's git metadata.
    pub fn for_repo(root: &Path, home: Option<PathBuf>) -> Result<Self, String> {
        let git = root.join(".git");
        let meta = std::fs::symlink_metadata(&git).map_err(|_| {
            format!(
                "{} has no .git; a cloud dispatch needs the repo root to be a git work-tree root",
                root.display()
            )
        })?;
        if meta.file_type().is_symlink() {
            return Err(format!(
                "{}.git is a symlink; refusing to sandbox a symlinked git directory",
                git.display()
            ));
        }
        if meta.is_dir() {
            let config = git.join("config");
            if !config.is_file() {
                return Err(format!(
                    "{} is not a file; the sandbox cannot protect a .git directory without its config",
                    config.display()
                ));
            }
            let hooks = git.join("hooks");
            std::fs::create_dir_all(&hooks)
                .map_err(|e| format!("cannot create .git/hooks: {e}"))?;
            Ok(Self::new(root, home).with_git(GitDir::Dir))
        } else {
            Ok(Self::new(root, home).with_git(GitDir::File))
        }
    }

    /// Replace the program name. Tests use a name that does not exist.
    pub fn with_program(mut self, program: &str) -> Self {
        self.program = program.to_string();
        self
    }

    /// Set the repo's git layout, adding the matching `.git` mount rows.
    pub fn with_git(mut self, git: GitDir) -> Self {
        self.git = Some(git);
        self
    }

    /// The prefix of the full argv to spawn: element 0 is `program`, then rows
    /// 1-9 of the mount table, then `--chdir`, `chdir`. Append a program and its
    /// arguments to run it in the sandbox. bwrap applies mounts in order, so a
    /// later mount covers an earlier one — the order below is the security
    /// property.
    pub fn command_prefix(&self, chdir: &std::path::Path) -> Vec<String> {
        let p = |path: &std::path::Path| path.to_string_lossy().into_owned();
        let root = self.root.clone();
        let home = self.home.clone();
        let mut a: Vec<String> = vec![self.program.clone()];

        // 1. killing bwrap on timeout kills the whole process tree.
        a.push("--die-with-parent".into());
        a.push("--unshare-pid".into());
        // 2. host filesystem read-only; toolchains stay usable.
        a.extend(["--ro-bind", "/", "/"].iter().map(|s| s.to_string()));

        // 3. a working /dev/null and a /proc for the new pid namespace.
        a.extend(
            ["--dev", "/dev", "--proc", "/proc"]
                .iter()
                .map(|s| s.to_string()),
        );

        // 4-5. hide other processes' temp files and every home directory.
        a.extend(["--tmpfs", "/tmp"].iter().map(|s| s.to_string()));
        a.extend(["--tmpfs", "/home"].iter().map(|s| s.to_string()));

        // 6. a home outside /home (e.g. /root, /var/home/x) gets its own tmpfs.
        if let Some(h) = &home
            && !h.starts_with("/home")
        {
            a.extend(["--tmpfs".to_string(), p(h)]);
        }

        // 7. Rust toolchain under $HOME, individually — never ~/.cargo as a whole
        // (it can hold credentials.toml). --ro-bind-try skips missing paths.
        if let Some(h) = home {
            let rel = [
                ".cargo/bin",
                ".cargo/registry",
                ".cargo/git",
                ".cargo/config.toml",
                ".rustup",
            ];
            for r in rel {
                let path = h.join(r);
                a.push("--ro-bind-try".into());
                a.push(p(&path));
                a.push(p(&path));
            }
        }

        // 8. the repo is writable.
        a.push("--bind".into());
        a.push(p(&root));
        a.push(p(&root));

        // 9. the repo's .rexymcp/ is an empty scratch dir: sessions, vault and
        // keys are hidden and writes do not reach the host.
        let state = root.join(".rexymcp");
        a.push("--tmpfs".into());
        a.push(p(&state));

        // 9a/9b. Protect the repo's git metadata: bind .git onto itself first
        // (that bind is what makes `mv .git` fail with EBUSY), then pin the
        // files git reads at commit time read-only.
        let git = root.join(".git");
        match self.git {
            Some(GitDir::Dir) => {
                a.push("--bind".into());
                a.push(p(&git));
                a.push(p(&git));
                let config = git.join("config");
                a.push("--ro-bind".into());
                a.push(p(&config));
                a.push(p(&config));
                let hooks = git.join("hooks");
                a.push("--ro-bind".into());
                a.push(p(&hooks));
                a.push(p(&hooks));
            }
            Some(GitDir::File) => {
                a.push("--ro-bind".into());
                a.push(p(&git));
                a.push(p(&git));
            }
            None => {}
        }
        // 9c. the config the next dispatch loads; --ro-bind-try skips it when absent.
        let toml = root.join("rexymcp.toml");
        a.push("--ro-bind-try".into());
        a.push(p(&toml));
        a.push(p(&toml));

        // 10-11.
        a.push("--chdir".into());
        a.push(p(chdir));

        a
    }

    /// `command_prefix(root)` followed by `"sh"`, `"-c"`, `command`: the full
    /// argv to spawn, element 0 is `program`, the last three are `"sh"`, `"-c"`,
    /// `command`.
    pub fn argv(&self, command: &str) -> Vec<String> {
        let mut a = self.command_prefix(&self.root);
        a.push("sh".into());
        a.push("-c".into());
        a.push(command.to_string());
        a
    }
}

/// Run `<program> --ro-bind / / --dev /dev true` with stdin, stdout and stderr
/// null. `Ok(())` on exit 0; otherwise `Err` with a one-line reason that starts
/// with the program name (the spawn error or the exit status).
pub fn probe_with(program: &str) -> Result<(), String> {
    let out = std::process::Command::new(program)
        .args(["--ro-bind", "/", "/", "--dev", "/dev", "true"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output();

    match out {
        Ok(o) if o.status.success() => Ok(()),
        Ok(o) => Err(format!(
            "{program}: sandbox probe exited with status {:?}",
            o.status.code()
        )),
        Err(e) => Err(format!("{program}: {e}")),
    }
}

/// `probe_with("bwrap")`.
pub fn probe() -> Result<(), String> {
    probe_with("bwrap")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn argv_contains_sequence(a: &[String], seq: &[&str]) -> Option<usize> {
        let n = seq.len();
        a.windows(n)
            .position(|w| w.iter().zip(seq).all(|(x, y)| x == y))
    }

    #[test]
    fn argv_orders_mounts_so_repo_state_is_hidden() {
        let sb = Sandbox::new(Path::new("/srv/repo"), Some(PathBuf::from("/home/u")));
        let a = sb.argv("echo hi");

        assert_eq!(a[0], "bwrap");
        assert_eq!(&a[a.len() - 3..], &["sh", "-c", "echo hi"]);

        let ro_bind = argv_contains_sequence(&a, &["--ro-bind", "/", "/"]).expect("ro-bind / /");
        let tmpfs_home = argv_contains_sequence(&a, &["--tmpfs", "/home"]).expect("tmpfs /home");
        let bind_root = argv_contains_sequence(&a, &["--bind", "/srv/repo", "/srv/repo"])
            .expect("bind repo root");
        let tmpfs_state =
            argv_contains_sequence(&a, &["--tmpfs", "/srv/repo/.rexymcp"]).expect("tmpfs .rexymcp");

        assert!(
            ro_bind < tmpfs_home && tmpfs_home < bind_root && bind_root < tmpfs_state,
            "mount order is wrong: ro-bind={ro_bind} tmpfs_home={tmpfs_home} bind={bind_root} tmpfs_state={tmpfs_state}"
        );

        assert!(a.contains(&"--die-with-parent".to_string()));
        assert!(a.contains(&"--unshare-pid".to_string()));
        assert!(
            argv_contains_sequence(&a, &["--chdir", "/srv/repo"]).is_some(),
            "--chdir /srv/repo must be present"
        );
    }

    #[test]
    fn argv_binds_toolchain_dirs_but_not_cargo_home() {
        let sb = Sandbox::new(Path::new("/srv/repo"), Some(PathBuf::from("/home/u")));
        let a = sb.argv("echo hi");

        assert!(
            a.windows(3).any(|w| w
                == [
                    "--ro-bind-try",
                    "/home/u/.cargo/registry",
                    "/home/u/.cargo/registry"
                ]),
            "registry ro-bind-try must be present"
        );
        assert!(
            a.windows(3)
                .any(|w| w == ["--ro-bind-try", "/home/u/.rustup", "/home/u/.rustup"]),
            "rustup ro-bind-try must be present"
        );
        assert!(
            !a.iter().any(|x| x == "/home/u/.cargo"),
            "must not bind ~/.cargo as a whole (credentials.toml)"
        );
    }

    #[test]
    fn argv_hides_home_outside_slash_home() {
        let sb = Sandbox::new(Path::new("/srv/repo"), Some(PathBuf::from("/root")));
        let a = sb.argv("echo hi");

        let tmpfs_root = argv_contains_sequence(&a, &["--tmpfs", "/root"]).expect("tmpfs /root");
        let bind = argv_contains_sequence(&a, &["--bind", "/srv/repo", "/srv/repo"]).unwrap();
        assert!(
            tmpfs_root < bind,
            "/root tmpfs must come before the repo bind"
        );
    }

    #[test]
    fn argv_without_home_has_no_toolchain_binds() {
        let sb = Sandbox::new(Path::new("/srv/repo"), None);
        let a = sb.argv("echo hi");
        assert!(
            !a.iter().any(|x| x == "/home/u/.cargo/registry"),
            "no toolchain binds without a home"
        );
    }

    #[test]
    fn argv_protects_git_dir_config_and_hooks() {
        let sb = Sandbox::new(Path::new("/srv/repo"), None)
            .with_program("true")
            .with_git(GitDir::Dir);
        let a = sb.argv("true");
        let idx = |needle: &[&str]| -> usize {
            a.windows(needle.len())
                .position(|w| w.iter().zip(needle).all(|(x, s)| x == s))
                .expect("row missing")
        };
        let repo = idx(&["--bind", "/srv/repo", "/srv/repo"]);
        let git = idx(&["--bind", "/srv/repo/.git", "/srv/repo/.git"]);
        let config = idx(&[
            "--ro-bind",
            "/srv/repo/.git/config",
            "/srv/repo/.git/config",
        ]);
        let hooks = idx(&["--ro-bind", "/srv/repo/.git/hooks", "/srv/repo/.git/hooks"]);
        let toml = idx(&[
            "--ro-bind-try",
            "/srv/repo/rexymcp.toml",
            "/srv/repo/rexymcp.toml",
        ]);
        let chdir = idx(&["--chdir", "/srv/repo"]);
        assert!(
            repo < git && git < config,
            "mount order must be repo, .git bind, then .git/config ro-bind: {a:?}"
        );
        assert!(
            config < hooks && hooks < toml && toml < chdir,
            "git mounts and the toml must precede --chdir: {a:?}"
        );
    }

    #[test]
    fn argv_binds_git_file_read_only() {
        let sb = Sandbox::new(Path::new("/srv/repo"), None)
            .with_program("true")
            .with_git(GitDir::File);
        let a = sb.argv("true");
        let ro = a.windows(3).any(|w| {
            w == [
                "--ro-bind".to_string(),
                "/srv/repo/.git".to_string(),
                "/srv/repo/.git".to_string(),
            ]
        });
        assert!(ro, "a worktree .git must be ro-bound: {a:?}");
        let rw = a.windows(3).any(|w| {
            w == [
                "--bind".to_string(),
                "/srv/repo/.git".to_string(),
                "/srv/repo/.git".to_string(),
            ]
        });
        assert!(!rw, "a worktree .git must not be rw-bound: {a:?}");
        assert!(
            !a.iter().any(|el| el.ends_with(".git/config")),
            "no .git/config mounts for a file .git: {a:?}"
        );
    }

    #[test]
    fn argv_without_git_has_no_git_mounts() {
        let sb = Sandbox::new(Path::new("/srv/repo"), None).with_program("true");
        let a = sb.argv("true");
        assert!(
            !a.iter().any(|el| el.contains("/.git")),
            "no .git mounts without with_git: {a:?}"
        );
        let toml = a.windows(3).any(|w| {
            w == [
                "--ro-bind-try".to_string(),
                "/srv/repo/rexymcp.toml".to_string(),
                "/srv/repo/rexymcp.toml".to_string(),
            ]
        });
        assert!(
            toml,
            "the ro-bind-try for rexymcp.toml is always present: {a:?}"
        );
    }

    #[test]
    fn for_repo_detects_git_layout() {
        let missing = tempfile::TempDir::new().unwrap();
        let err = Sandbox::for_repo(missing.path(), None).unwrap_err();
        assert!(err.contains(".git"), "missing .git must be named: {err}");

        let dir_repo = tempfile::TempDir::new().unwrap();
        let git = dir_repo.path().join(".git");
        std::fs::create_dir_all(&git).unwrap();
        std::fs::write(git.join("config"), "[core]\n").unwrap();
        let sb = Sandbox::for_repo(dir_repo.path(), None).unwrap();
        let a = sb.argv("true");
        let git_str = git.to_string_lossy().into_owned();
        let expected: [&str; 3] = ["--bind", git_str.as_str(), git_str.as_str()];
        assert!(
            a.windows(3)
                .any(|w| w.iter().zip(expected.iter()).all(|(x, s)| x == s)),
            "a .git directory must be bound onto itself: {a:?}"
        );
        assert!(
            git.join("hooks").is_dir(),
            "for_repo must create a missing .git/hooks"
        );

        let file_repo = tempfile::TempDir::new().unwrap();
        std::fs::write(file_repo.path().join(".git"), "gitdir: /x\n").unwrap();
        let sb = Sandbox::for_repo(file_repo.path(), None).unwrap();
        let git_file = file_repo.path().join(".git");
        let git_str = git_file.to_string_lossy().into_owned();
        let expected: [&str; 3] = ["--ro-bind", git_str.as_str(), git_str.as_str()];
        let a = sb.argv("true");
        assert!(
            a.windows(3)
                .any(|w| w.iter().zip(expected.iter()).all(|(x, s)| x == s)),
            "a .git file must be ro-bound: {a:?}"
        );

        let no_config = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(no_config.path().join(".git")).unwrap();
        let err = Sandbox::for_repo(no_config.path(), None).unwrap_err();
        assert!(
            err.contains(".git/config"),
            "a .git without a config file must be named: {err}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn for_repo_refuses_symlinked_git() {
        let dir = tempfile::TempDir::new().unwrap();
        let target = tempfile::TempDir::new().unwrap();
        std::fs::write(target.path().join("config"), "[core]\n").unwrap();
        std::os::unix::fs::symlink(target.path(), dir.path().join(".git")).unwrap();
        let err = Sandbox::for_repo(dir.path(), None).unwrap_err();
        assert!(
            err.contains("symlink"),
            "a symlinked .git must be refused: {err}"
        );
    }

    #[tokio::test]
    #[ignore = "needs bwrap and user namespaces"]
    async fn sandbox_protects_git_and_config() {
        let dir = tempfile::TempDir::new().unwrap();
        let repo = dir.path();
        let out = std::process::Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(repo)
            .output()
            .unwrap();
        assert!(out.status.success(), "git init on the host must succeed");
        std::fs::write(repo.join("rexymcp.toml"), "x").unwrap();

        let canonical = repo.canonicalize().unwrap();
        let sb = Sandbox::for_repo(&canonical, std::env::var_os("HOME").map(PathBuf::from))
            .expect("a real repo must produce a sandbox");

        let run = |cmd: &str| -> std::process::Output {
            let argv = sb.argv(cmd);
            std::process::Command::new(&argv[0])
                .args(&argv[1..])
                .output()
                .expect("spawning the sandbox")
        };
        let fails = |cmd: &str| -> std::process::Output {
            let out = run(cmd);
            assert!(!out.status.success(), "{cmd} must fail inside the sandbox");
            out
        };

        fails("printf '#!/bin/sh\\n' > .git/hooks/pre-commit");
        fails("git config core.fsmonitor evil");
        fails("mv .git g2");
        fails("echo y >> rexymcp.toml");
        fails("sed -i s/x/z/ rexymcp.toml");
        fails("rm -f rexymcp.toml");

        // Positive control: commits still work inside the sandbox.
        let commit = run("git -c user.name=t -c user.email=t@t commit --allow-empty -qm t");
        assert!(
            commit.status.success(),
            "git commit must work in the sandbox: {}",
            String::from_utf8_lossy(&commit.stderr)
        );

        assert_eq!(
            std::fs::read_to_string(repo.join("rexymcp.toml")).unwrap(),
            "x",
            "rexymcp.toml on the host must be unchanged"
        );
        assert!(
            !repo.join(".git/hooks/pre-commit").exists(),
            "a hook must not appear in .git/hooks"
        );
        let config = std::fs::read_to_string(repo.join(".git/config")).unwrap();
        assert!(
            !config.contains("fsmonitor"),
            ".git/config must not have been rewritten: {config}"
        );
        let log = std::process::Command::new("git")
            .args(["log", "--oneline"])
            .current_dir(repo)
            .output()
            .unwrap();
        let log_stdout = String::from_utf8_lossy(&log.stdout).to_string();
        let lines: Vec<&str> = log_stdout.lines().filter(|l| !l.is_empty()).collect();
        assert_eq!(
            lines.len(),
            1,
            "exactly one commit must exist: {log_stdout:?}"
        );
    }

    #[test]
    fn argv_is_prefix_plus_shell() {
        let sb = Sandbox::new(Path::new("/srv/repo"), Some("/home/u".into()));
        let prefix = sb.command_prefix(Path::new("/srv/repo"));
        let mut expected = prefix;
        expected.push("sh".into());
        expected.push("-c".into());
        expected.push("echo hi".into());
        assert_eq!(sb.argv("echo hi"), expected);
    }

    #[test]
    fn command_prefix_ends_with_chdir() {
        let sb = Sandbox::new(Path::new("/srv/repo"), None);
        let prefix = sb.command_prefix(Path::new("/srv/repo/sub"));
        let tail: Vec<&str> = prefix.iter().rev().take(2).map(|s| s.as_str()).collect();
        assert_eq!(tail, vec!["/srv/repo/sub", "--chdir"]);
        assert_eq!(prefix[0], "bwrap");
    }

    #[tokio::test]
    #[ignore = "needs bwrap and user namespaces"]
    async fn sandbox_blocks_home_and_state_dir() {
        let dir = tempfile::TempDir::new().unwrap();
        let repo = dir.path();
        std::fs::create_dir_all(repo.join(".rexymcp/vault")).unwrap();
        std::fs::write(repo.join(".rexymcp/vault/key"), "k").unwrap();

        let canonical = repo.canonicalize().unwrap();
        let run = |sb: &Sandbox, cmd: &str| -> std::process::Output {
            let argv = sb.argv(cmd);
            let (first, rest) = argv
                .split_first()
                .expect("argv always starts with the program name");
            std::process::Command::new(first)
                .args(rest)
                .output()
                .unwrap_or_else(|e| panic!("spawn failed: {e}"))
        };
        // Real $HOME (no override): the hidden home must be the one production
        // hides, so `--tmpfs <home>` (or `--tmpfs /home`) covers it.
        let real_home = std::env::var_os("HOME").map(PathBuf::from);
        let sb = Sandbox::new(&canonical, real_home.clone());

        // $HOME/.config must not be visible. The host has ~/.config; inside the
        // sandbox the home tmpfs is empty, so the check must fail.
        if real_home
            .as_deref()
            .is_some_and(|h| h.join(".config").exists())
        {
            let check = run(&sb, "test -e \"$HOME/.config\"");
            assert!(
                !check.status.success(),
                "$HOME/.config must not be visible in the sandbox; got exit {:?}",
                check.status.code()
            );
        }

        // A write under $HOME must not reach the host. The touch may succeed in
        // the sandbox (bwrap recreates the home dir inside the home tmpfs via
        // the toolchain mounts) — the property is host-side.
        let touch = run(&sb, "touch \"$HOME/.sandbox-probe\"");
        assert!(
            touch.status.success(),
            "touch inside the sandbox should succeed: {}",
            String::from_utf8_lossy(&touch.stderr)
        );
        if let Some(home) = &real_home {
            assert!(
                !home.join(".sandbox-probe").exists(),
                "a write under $HOME must not reach the host"
            );
        }

        // The repo's .rexymcp/ must be hidden (cat fails) and deletions inside
        // the sandbox must not reach the host.
        let cat = run(&sb, "cat .rexymcp/vault/key");
        assert!(
            !cat.status.success(),
            "repo .rexymcp/vault/key must not be visible in the sandbox"
        );
        let rm = run(&sb, "rm -rf .rexymcp/vault");
        assert!(
            rm.status.success(),
            "rm inside the sandbox should succeed against the empty tmpfs: {}",
            String::from_utf8_lossy(&rm.stderr)
        );
        assert_eq!(
            std::fs::read_to_string(repo.join(".rexymcp/vault/key")).unwrap(),
            "k",
            "host .rexymcp/vault/key must survive the sandboxed rm"
        );

        // The repo itself stays writable and writes reach the host.
        let write = run(&sb, "echo ok > written.txt");
        assert!(write.status.success(), "writing into the repo must work");
        assert!(
            repo.join("written.txt").exists(),
            "written.txt must reach the host"
        );

        // Control for the .config check: a sandbox with home=None (no
        // toolchain mounts, no per-home tmpfs) must also hide a real
        // $HOME/.config via the /home tmpfs, so the check above is not
        // passing only because the per-home tmpfs replaced it.
        let sb_no_home = Sandbox::new(&canonical, None);
        if real_home
            .as_deref()
            .is_some_and(|h| h.join(".config").exists())
        {
            let check = run(&sb_no_home, "test -e \"$HOME/.config\"");
            assert!(
                !check.status.success(),
                "control: $HOME/.config must not be visible without a home mount either"
            );
        }
    }

    #[test]
    #[ignore = "needs bwrap and user namespaces"]
    fn probe_succeeds_where_bwrap_works() {
        assert!(probe().is_ok());
    }
}
