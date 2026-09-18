# Phase 3: harden the vault container

**Milestone:** F05 — Privacy and security hardening
**Status:** todo
**Depends on:** phase 02 (done) — it set the owner-only pattern for the session
log; this phase applies the same idea to the vault.
**Estimated diff:** ~250 lines, over half of it tests
**Tags:** language=rust, kind=security, size=m

> **Dispatch on a LOCAL executor only.** This phase edits
> `executor/src/privacy/**`, the plumbing a cloud executor is itself subject to.

## Goal

`seal.rs` sets `0600` on the vault **key** and stops there. The vault
directory, the encrypted vault, the encrypted PII index and the registry are
created with no explicit mode — `0755` and `0644` in practice. Every account on
the host can read them (F05 README, finding 3).

The documentation calls the vault a honeypot, because it concentrates every
original value the tokenizer ever replaced. Protecting only the key is half a
job: the key file is `0600`, and the ciphertext it decrypts sits world-readable
beside it.

The default location is inside the repository (`<repo>/.rexymcp/vault`). That
stays — moving it would orphan every existing vault, and a vault whose key is
gone is unreadable, not merely relocated. Instead, this phase **verifies** what
the default has always assumed: that git ignores the vault. `Vault::open` and
`PiiIndex::save` already write a `.gitignore` holding `*` into the directory,
but nothing checks that it took effect. A vault configured into a tracked
directory, or a repo whose rules force-include it, is a vault about to be
committed.

After this phase:

1. The vault directory is `0700`, and every file rexymcp writes into it is
   `0600`: the key (already), `vault.enc`, `egress-index.enc`,
   `egress-registry.json` and the `.gitignore`.
2. Opening or saving into a vault that sits inside a git work tree **and is not
   ignored** fails with `Error::Privacy` naming the directory.
3. A vault outside any work tree, or one git already ignores, is untouched.

## Architecture references

- `executor/src/privacy/seal.rs:18-39` — `load_or_create_key`, and
  `set_owner_only` at line 68 (`0600`, `#[cfg(unix)]`, with a no-op
  `#[cfg(not(unix))]` twin at line 75). This is the helper to extend.
- `executor/src/privacy/vault.rs:26-45` — `Vault::open`: `create_dir_all`, the
  `.gitignore`, the key, then the sealed map. `save` is at line 60, writing
  `vault.enc.tmp` then renaming.
- `executor/src/privacy/prescan.rs:48-60` — `PiiIndex::save`: `create_dir_all`,
  `.gitignore`, `egress-index.enc.tmp` → rename.
- `executor/src/privacy/registry.rs:60-66` — `Registry::save`, writing
  `egress-registry.json` through a `.tmp` rename.
- `executor/src/store/sessions/jsonl.rs:56-63` — phase 02's `set_mode`, the
  precedent. **Do not refactor it into this phase's helper**; it lives in
  another module and is already approved.

## Pre-flight

1. `git status --short` is clean. If it is not, stop and file a blocker.
2. `cargo test -p rexymcp-executor privacy` passes. Record the count.
3. `git --version` prints a version.

## Current state

```rust
// executor/src/privacy/seal.rs:68-78
#[cfg(unix)]
fn set_owner_only(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_owner_only(_path: &Path) -> Result<()> {
    Ok(())
}
```

```rust
// executor/src/privacy/vault.rs:29-34
    pub fn open(dir: &Path) -> Result<Self> {
        fs::create_dir_all(dir)?;
        // The vault and its key must never be committed.
        fs::write(dir.join(".gitignore"), "*\n")?;

        let key = seal::load_or_create_key(dir)?;
```

```rust
// executor/src/privacy/prescan.rs:49-59 (PiiIndex::save)
        fs::create_dir_all(dir)?;
        fs::write(dir.join(".gitignore"), "*\n")?;
        let key = seal::load_or_create_key(dir)?;
        let plaintext = serde_json::to_vec(self)
            .map_err(|e| Error::Privacy(format!("serialize egress index: {e}")))?;
        let blob = seal::seal(&key, &plaintext)?;
        let tmp = dir.join("egress-index.enc.tmp");
        fs::write(&tmp, &blob)?;
        fs::rename(&tmp, dir.join(INDEX_FILE))?;
```

## Spec

### 1. Two helpers in `seal.rs`, shared across the vault writers

Make the existing helper visible to the module's siblings and add a directory
twin. Keep both `#[cfg]` pairs.

```rust
/// `0600` on unix; a no-op elsewhere. Every file rexymcp writes into the vault
/// goes through this — the key, the sealed vault, the sealed index, the
/// registry, the `.gitignore`.
pub(crate) fn set_owner_only(path: &Path) -> Result<()>       // was private

/// `0700` on unix; a no-op elsewhere. The vault directory itself.
pub(crate) fn set_dir_owner_only(path: &Path) -> Result<()>
```

`set_dir_owner_only` is the same shape with `0o700`. Both keep returning
`Result` and propagating a real `set_permissions` error with `?` — unlike the
session log, a vault whose mode cannot be set is worth failing on.

### 2. Verify the vault is ignored

Add to `seal.rs`, next to the helpers:

```rust
/// Fail when `dir` sits inside a git work tree that does **not** ignore it.
/// A vault is a honeypot; one about to be committed is worse than no vault.
///
/// `git check-ignore -q` is the ground truth (checked by hand 2026-09-18):
/// exit 0 = ignored, exit 1 = inside a work tree and not ignored, exit 128 =
/// not a work tree. A missing `git`, or any other status, leaves the vault
/// alone: this check reports a certainty, never a suspicion.
pub(crate) fn ensure_git_ignored(dir: &Path) -> Result<()> {
    let probe = dir.join("vault.enc");
    let status = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .arg("check-ignore")
        .arg("-q")
        .arg(&probe)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    if let Ok(s) = status
        && s.code() == Some(1)
    {
        return Err(Error::Privacy(format!(
            "{} is inside a git work tree and is not ignored — the vault holds every \
             original value the tokenizer replaced, so committing it would publish them. \
             Add it to .gitignore, or set [privacy] vault_dir to a path outside the repo.",
            dir.display()
        )));
    }
    Ok(())
}
```

The probe path need not exist: `check-ignore` answers on the path, not the file.
`git -C <dir>` means no repo root has to be threaded in.

### 3. Apply them at the three writers

Call order matters: create the directory, tighten it, write the `.gitignore`,
**then** check. The check must run after the `.gitignore` exists, or the very
file that makes the vault safe would be missing when it is judged.

- **`vault.rs`, `Vault::open`:**
  ```rust
          fs::create_dir_all(dir)?;
          seal::set_dir_owner_only(dir)?;
          // The vault and its key must never be committed.
          fs::write(dir.join(".gitignore"), "*\n")?;
          seal::set_owner_only(&dir.join(".gitignore"))?;
          seal::ensure_git_ignored(dir)?;
  ```
- **`vault.rs`, `save`:** after the `fs::rename` that puts `vault.enc` in place,
  `seal::set_owner_only(&self.dir.join(VAULT_FILE))?;`. Tighten after the
  rename, not the temp file — the rename replaces the inode.
- **`prescan.rs`, `PiiIndex::save`:** the same five lines as `open` above, then
  after its `fs::rename`, `seal::set_owner_only(&dir.join(INDEX_FILE))?;`.
- **`registry.rs`, `Registry::save`:** after its `fs::rename`,
  `seal::set_owner_only(&self.path)?;`. The registry is not encrypted and holds
  a hash per scanned path, so its mode matters as much as the sealed files'.

`load_or_create_key` already tightens the key; leave it as it is.

## Acceptance criteria

- [ ] After `Vault::open` on a fresh directory: the directory is `0700`, and
      `.gitignore` and `key` are `0600`.
- [ ] After a `Vault::save`, `vault.enc` is `0600`.
- [ ] After `PiiIndex::save`, the directory is `0700` and `egress-index.enc` is
      `0600`.
- [ ] After `Registry::save`, `egress-registry.json` is `0600`.
- [ ] A vault directory inside a git work tree whose `.gitignore` does not cover
      it makes `Vault::open` return `Err(Error::Privacy(m))` with `m` naming the
      directory.
- [ ] **Must NOT fail:** a vault outside any git work tree, and a vault inside
      one that ignores it (the default layout).
- [ ] Each new non-ignored test fails when its fix is reverted (show one in the
      Update Log).
- [ ] `cargo fmt --all --check`, `cargo build` (zero warnings),
      `cargo clippy --all-targets --all-features -- -D warnings` and
      `cargo test` all pass.

## Test plan

Hermetic: a `tempfile::TempDir` per case, no network. Tests that read a mode are
`#[cfg(unix)]`, matching `key_is_owner_only` in `seal.rs:98`. Read a mode the
way that test does:

```rust
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
```

Creating a git repo in a test is already done elsewhere in this crate — see
`executor/src/tools/find_files.rs`, which runs
`std::process::Command::new("git").args(["init", "--quiet"])`.

### `executor/src/privacy/vault.rs`

1. **`vault_dir_and_files_are_owner_only`** — `#[cfg(unix)]`. `Vault::open` on a
   fresh `TempDir` subdirectory, then insert something and `save`. The directory
   is `0700`; `.gitignore`, `key` and `vault.enc` are each `0600`.
2. **`vault_inside_an_unignored_repo_is_refused`** — `git init --quiet` in a
   `TempDir`, then **delete** the `.gitignore` that `Vault::open` wrote and call
   `Vault::open` again on the same directory. The second call is
   `Err(Error::Privacy(m))` and `m` contains the directory path.
   (Deleting it is the realistic shape: the first open creates the file, and a
   repo rule or a stray `git add -f` is what removes the protection later.)
3. **`vault_outside_a_repo_opens`** — a `TempDir` with no `git init` anywhere
   above it inside the test's own tree. `Vault::open` is `Ok`. Note that the
   `TempDir` root is `/tmp` on most systems, which is not a work tree, so
   `check-ignore` exits 128 here.
4. **`vault_inside_an_ignored_repo_opens`** — `git init --quiet`, then
   `Vault::open` twice. Both are `Ok`: the first writes the `.gitignore`, and the
   second sees the vault already ignored. This is the default layout and must
   keep working.

### `executor/src/privacy/prescan.rs`

5. **`index_files_are_owner_only`** — `#[cfg(unix)]`. `PiiIndex::save` into a
   fresh directory; the directory is `0700` and `egress-index.enc` is `0600`.

### `executor/src/privacy/registry.rs`

6. **`registry_file_is_owner_only`** — `#[cfg(unix)]`. Load a registry at
   `<TempDir>/egress-registry.json`, mark something, `save`, and assert the file
   is `0600`.

## End-to-end verification

The real artifact is the permissions on a vault this repo's own binary creates,
and the refusal. Run as **one** bash command and paste the output:

```bash
T=$(mktemp -d)
cargo build -q -p rexymcp
BIN="$PWD/target/debug/rexymcp"
mkdir -p "$T/repo"; (cd "$T/repo" && git init -q)
printf '[project]\nid = "x"\n\n[executor]\nprovider = "openai"\nmodel = "m"\nbase_url = "http://localhost:9/v1"\n\n[privacy]\nenabled = true\nengine_base_url = "http://192.168.50.138:8000/v1"\nengine_model = "RedHatAI/Qwen3.8-27B-INT4"\n' > "$T/repo/rexymcp.toml"
printf 'John Smith emailed jane@acme.com\n' | timeout 180 "$BIN" anonymize --config "$T/repo/rexymcp.toml" --repo "$T/repo" > "$T/out.txt" 2>&1
echo "scrub_exit=$?"; cat "$T/out.txt"
V="$T/repo/.rexymcp/vault"
echo "dir=$(stat -c %a "$V")"
for f in .gitignore key vault.enc; do [ -e "$V/$f" ] && echo "$f=$(stat -c %a "$V/$f")"; done
rm -f "$V/.gitignore"
printf 'Maria Gonzalez called\n' | timeout 180 "$BIN" anonymize --config "$T/repo/rexymcp.toml" --repo "$T/repo" > "$T/refused.txt" 2>&1
echo "unignored_exit=$?"; head -2 "$T/refused.txt"
```

`anonymize` is the CLI path that opens a vault (`rexymcp anonymize --help`);
there is no `privacy` subcommand. The engine values above are this machine's
live NER engine — the architect ran exactly this on 2026-09-18 **before** the
phase, and it printed:

```
scrub_exit=0
Person_1 emailed Email_1
dir=755
.gitignore=644
key=600
vault.enc=644
```

That is the defect, measured. After this phase the same run must print
`dir=700` and `600` for all three files, with `scrub_exit=0` and
`Person_1 emailed Email_1` unchanged — the tokenizer still works.

The second run is the refusal: with the `.gitignore` deleted, `unignored_exit`
is non-zero and the message names the vault directory. The first run is its
positive control — the same command, same repo, succeeding while the vault is
ignored.

If the engine at that address is unreachable when you run this, `anonymize`
exits 1 **after** creating the vault directory, key and `.gitignore` (the
architect confirmed that too). The mode checks still work in that case;
`vault.enc` will simply not exist. Say which case you got in the Update Log.

## Authorizations

- [x] May edit `executor/src/privacy/seal.rs`, `vault.rs`, `prescan.rs` and
      `registry.rs`, including their test modules.
- [x] May add `#[cfg(unix)]` to the new mode-reading tests.
- [ ] May change the default vault location — **no.** See Goal.
- [ ] May refactor `executor/src/store/sessions/jsonl.rs`'s `set_mode` — **no.**
- [ ] May add a dependency — **no.** `std::process::Command` is enough.
- [ ] May edit `mcp/**` — **no**, beyond reading it to find the CLI invocation
      for the E2E.

## Out of scope

- **Tightening an existing vault's files on open.** This phase sets modes as it
  writes. A vault created before it keeps its modes until rexymcp rewrites each
  file. Say so in the Update Log; a sweep can be its own phase.
- **Windows.** Both helpers are no-ops there, like `seal.rs`'s today.
- **The session log's `set_mode`.** Phase 02 owns it.
- **A vault on a filesystem without unix modes** (NFS, a mounted share). Out of
  reach of `set_permissions`.

## Update Log

(Filled in by the executor. See WORKFLOW.md § "Update Log entries".)

<!-- entries appended below this line -->
