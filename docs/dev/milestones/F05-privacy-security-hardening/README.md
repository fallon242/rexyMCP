# F05 — Privacy and security hardening

**Goal:** close the gaps a real deployment found between what the privacy
features promise and what they do. Eleven findings, each with evidence, most
small. Together they are the difference between a safety net and a belief.

**Status:** done — opened 2026-09-16 on human sign-off, closed 2026-09-18 at
eleven phases. All eleven findings are closed; see § F05 retrospective.

**Depends on:** F02 (ingestion gate), F03 (egress protection), F04 (pre-scan
efficiency). All done.

**Source:** the CRS modernisation deployment, 2026-09. Every finding below was
observed there, not imagined.

**Exit criteria:**

- [x] A repo can declare literal terms that must never reach a cloud model, and
      they are redacted on the existing chokepoint.
- [x] A dispatch leaves no unredacted record of what the executor read.
- [x] The vault's container is protected like its key.
- [x] The prompt guard cannot fail open, and is installed rather than copied.
- [x] No privacy setting is inert while appearing to work.
- [x] `docs/privacy.md` states one thing about egress protection, not two.
- [x] A failed PII pre-scan stops a cloud dispatch instead of running it with
      reduced protection.
- [x] Code a cloud executor writes never runs on the host outside the sandbox:
      gates, hooks, the verifier and the bookkeeping commit are covered.
- [x] All four gates pass; each mechanism has a test that fails when reverted.

## The findings

### 1. Literal terms are invisible to the pre-scan

The dictionary is built from what the NER engine *recognises*. Short site codes
and system acronyms are returned by no NER pass; a product name that is also an
ordinary English word cannot be masked blindly without corrupting prose; and a
system name that prefixes legacy program identifiers appears inside identifiers
far more often than in prose — 207 of 216 occurrences in the deployment. A flat
`Vec<(String, PiiKind)>` matched by substring cannot express any of that.

**Phase 01.**

### 2. Session logs store unredacted tool output

`.rexymcp/sessions/*.jsonl` records each `tool_result` with an
`output_preview`. In the deployment, five previews from the turns that read a
private extract were stored raw; none carried a redaction marker. The directory
is `0755` and the files `0644`.

So redaction protects the wire to the model and leaves a world-readable
plaintext record of the same content on disk. An operator who trusts
`[privacy]` has no reason to expect that.

**Phase 02** — redact previews with the same chokepoint, and create the session
directory `0700`, files `0600`.

### 3. The vault is half-hardened

`seal.rs` sets `0600` on the key. The vault directory, the encrypted index and
the registry are created with no explicit mode — `0755` and `0644` in practice.
The default location is inside the repository, for a store the documentation
itself calls a honeypot because it concentrates every original.

**Phase 03** — create the vault `0700` and its contents `0600`; default it
outside the repo, or refuse to run when it sits inside one that is a git work
tree without being ignored.

### 4. The prompt guard fails open three ways

`plugin/hooks/pii-guard.sh` is the last line before a typed prompt reaches the
architect. As shipped it: reads only `.user_input`, so a payload-key rename
silently disables it; exits non-zero when `jq` is absent, which the harness
treats as an error rather than a block; and matches line by line, so PII pasted
across a line break is missed. Its card pattern matches 16 digits only (AmEx is
15, Diners 14) and its SSN and phone patterns require punctuation, which raw
report extracts do not carry. It is also not installed by the plugin — the
operator must find and copy it.

All five were fixed downstream in the deployment; the fixes are small.

**Phase 04.**

### 5. `privacy.kinds` is inert

Declared on `PrivacyConfig`, referenced only by a test asserting it is empty.
An operator who sets it believes they have scoped detection. Nothing reads it.
Implement it or delete it; either is safer than a setting that looks like a
control.

**Phase 05.**

### 6. The privacy doc contradicts itself

`docs/privacy.md` describes executor egress as automated under F03, and its
Limitations section still says egress "is not automated … a documented residual
risk; use a local executor or pre-scrub with the CLI". A reader acts on
whichever they find first. One of them is two milestones stale.

**Phase 05**, with finding 5 — both are documentation-truth fixes.

### 7. A failed pre-scan runs the cloud dispatch anyway

Found 2026-09-16 during F06 phase-01. The NER engine was unreachable
(`curl` to `engine_base_url` exits 7, "couldn't connect"), `build_egress_index`
failed (`executor/src/privacy/egress.rs:114-124`), and the run continued:

```rust
// mcp/src/runner.rs:402-408
Err(e) => {
    egress_warning = Some(format!(
        "executor-egress redaction engaged but the PII pre-scan failed ({e}); only \
         structured PII is redacted live and the write-guard is off"
    ));
}
```

The run made 48 turns against `api.deepseek.com` with no dictionary redaction
and no write-guard. The only signal was a line in `PhaseResult.warnings`. The
comment at `runner.rs:385` says this fallback is deliberate. The stated privacy
rule says the opposite: "When the detection engine is unavailable, cloud work
stops rather than proceeding unprotected"
(`plugin/skills/architect/SKILL.md:177`).

The warning is also hard to diagnose. `scrub_phase_result`
(`mcp/src/server.rs:248`) redacts the engine's IP address in the error, so it
reads `http://Ip_1:8080`. That is redaction working correctly, not a bug, but
the message does not say which setting to check.

**Phase 06** — a failed pre-scan fails the dispatch before turn 1, and the
error names `privacy.engine_base_url`. Whether to keep the reduced mode behind
an explicit opt-in is a question for drafting. Small: one match arm plus a test.

### 8. A reply cut off at the token limit counts as "no PII"

Found 2026-09-16 during the phase-01 pre-scan. `NerEngine::from_config` caps
each reply at `max_tokens: 1024` (`executor/src/privacy/ner.rs:63`). A reply
that hits the cap ends before its JSON array closes. `parse_items`
(`ner.rs:113`) then returns an empty list, with no error. Out of 589 replies to
the full-repo scan, 56 were cut off (vLLM `finished_reason="length"`). Each of
those files is recorded as containing no PII.

Related: `build_pii_index` sends each file whole (`prescan.rs:114`). A file
larger than the model's window makes the request fail, and since phase 06 that
stops the dispatch.

**Fix direction:** treat a cut-off or unparseable reply as an error, and split
large files into chunks. A config key for the reply cap is optional.

### 9. Substring redaction hides the project's own name

Found 2026-09-16, phase-01 dispatch (`budget_exceeded`, 200 turns, no tests
run). The pre-scan tagged `rexymcp` as an organisation. `redact_pii` matches
dictionary terms as plain substrings (`executor/src/privacy/redact.rs:18`), so
every path and crate name the executor saw read `[REDACTED:org]`, for example
`[REDACTED:org].toml.example` and `cargo test -p [REDACTED:org]-executor`.
The executor spent its turns guessing names (`rexy_mcp.toml.example`,
`rexy-executor`).

**Fix direction:** keep the project's own identifiers (crate names, repo
directory name, config file names) out of the dictionary, or never apply a
dictionary term inside a path or identifier. The executor must be able to name
the files it is asked to edit.

### 10. The executor's `bash` tool reaches outside the repo, and did

Found 2026-09-16, same run. `bash` is cwd-pinned but not path-confined
(`executor/src/tools/bash.rs:5-7`: "defense-in-depth, not a jail"). Once the
executor could not read the project name, it went looking for the redaction
dictionary outside the repo:

- listed `$HOME` and `~/.config`, globbed `~/.config/*/vault/*/egress-registry*`,
  and ran a script that opened `~/.config/*/mask-table.json`. The file exists
  and holds the real reversible mask table. Anything printed went to
  `api.deepseek.com`, and names that were not in this repo's dictionary were not
  masked;
- listed `~/.config/rexymcp`, which holds the DeepSeek API key in `env`;
- ran `rm -f .re*/output/cmd-output-*.log`. `.rexymcp/sessions` was emptied at
  13:12, which deleted this run's session log and the phase-06 log. rexymcp's
  own cleanup (`mcp/src/jobs.rs`, `mcp/src/sweep.rs`) does not delete session
  logs;
- deleted vault keys. `load_or_create_key` (`executor/src/privacy/seal.rs:18`)
  only creates a key when none exists. The key of another project's vault,
  `~/.config/rexymcp/vault/FG_CRS_F/key`, was recreated at 12:59:18. That
  vault's `vault.enc` (09:16) is now unreadable. The repo vault's key was
  recreated at 13:16:39, the second the run ended (run record `ts`). The human
  confirmed that no other rexymcp session was running and that they did not
  touch the key.

Because the session log is gone, what was printed to the cloud cannot be
reconstructed. Treat the mask table as disclosed. The DeepSeek key should be
rotated.

**Fix direction:** confine `bash` for a cloud executor, so that it cannot read
outside the repo, and cannot delete or write under `.rexymcp/` or any vault
directory. The executor contract must also forbid hunting for redaction
dictionaries or vaults. Until this lands, no phase goes to a cloud executor,
in any project.

### 11. Gate and verifier commands run model-written code outside the sandbox

Found 2026-09-17, in the local vs. cloud benchmark that ran after phase 07.
Phase 07 put the model's `bash` tool in bwrap. Its Out of scope says the final
command set is left alone because "rexymcp runs those commands from the phase
doc's config, not the model". The command is rexymcp's, but the code it builds
and runs is the model's. Three host-side paths run that code with nothing
around it:

- **Gates and hooks.** `RealCommandRunner::run`
  (`executor/src/agent/command.rs:32-38`) runs `sh -c <command>` on the host,
  with no sandbox, no `env_clear` and the full inherited environment. In this
  session that environment holds `DEEPSEEK_API_KEY` and `REXYMCP_API_KEY`.
  `run_command_set` (`executor/src/agent/mod.rs:789`, `:1167`) and
  `run_post_write_hooks` (`:1309`, `format_fix`/`lint_fix`) go through it.
- **Gate output goes back to the model.** `gate_failure_feedback`
  (`executor/src/agent/command.rs:109-139`) puts the output of every failed
  gate into the next prompt. A cloud model can write a test that fails with
  `std::fs::read_to_string("<home>/.config/rexymcp/env")` or
  `std::env::var("DEEPSEEK_API_KEY")` in its message, and the next turn sends
  that text to the cloud endpoint.
- **The per-write verifier.** `executor/src/governor/verifier.rs:264` runs
  `cargo check` on the host after writes. That runs any `build.rs` and proc
  macro the model wrote, with the same access. `:497` (a configured program)
  and `:589` (`ruff`) run on the host as well.
- **Git hooks.** The repo is writable inside the sandbox, `.git/` included. A
  model can write `.git/hooks/pre-commit`. `git_commit_docs`
  (`mcp/src/finalize.rs:247-257`) then runs `git commit` on the host, and git
  runs that hook with no sandbox.

Nothing in this run exploited any of these. Both benchmark implementations were
checked for `std::fs`/`env`/`process`/`net` use before their tests ran on the
host, and neither had a `build.rs`. What the benchmark shows is that the
sandbox does not cover these paths.

Side effect seen in the same run: bwrap hides `~/.gitconfig`, so a commit from
the model's `bash` has no identity when the repo sets none. DeepSeek worked
around it with `git -c user.name=…`, which made up an author.

**Fix direction:** run every command that builds or runs repo code for a cloud
executor through the same `Sandbox`. That covers the gates, the hooks, the
verifier's `cargo check`/`ruff`/configured program, and the bookkeeping
`git commit`, or the commit must run with `core.hooksPath=/dev/null`. Clear
the environment for these commands the same way `bash` does. Decide whether
`.git/` should be read-only inside the sandbox. Until this lands, no phase on a
real project goes to a cloud executor. Finding 10's halt is carried forward.

**Phases 10 and 11.** Phase 10 moves the gate, hook and verifier commands into
the sandbox. Phase 11 protects `.git/` and `rexymcp.toml`. Per-file read-only
mounts are not enough on their own: tested by hand on bubblewrap 0.12.0,
`mv .git .git2` succeeds inside the sandbox and renames the directory on the
host. Bind-mounting `.git` onto itself first makes the rename fail with
`EBUSY`. After that, read-only mounts of `.git/config` and `.git/hooks` stop
config and hook edits, and `git commit` still works. Phase 11 also needs the
file tools (`Scope`) to refuse `.git/` and `rexymcp.toml`, and a cloud
dispatch to fail when the repo root has no `.git`.

## Why literal masking is not the abandoned reversible round-trip

Phase-06b proved a *reversible* executor round-trip corrupts files: asked to
normalise a tokenised value, the model replaces the token with fabricated data.
F03's answer was one-way redaction.

This milestone keeps that answer. Literal terms are redacted one-way like every
other dictionary hit, and the executor is never asked to restore one. Where a
human needs real names back — a report, a committee paper — restoration happens
locally from the same term file. The executor boundary gains no reversibility.

## Phases

Expanded on demand, per WORKFLOW. Phase 06 ran first, on human instruction:
until it landed, a dead NER engine let a cloud dispatch run with reduced
protection. Finding 11 (and phase 11) was filed mid-milestone, after phase 10
showed the sandbox did not cover the gate and verifier commands.

| #  | Phase | Status |
|----|-------|--------|
| 01 | terms-file ([phase-01-terms-file.md](phase-01-terms-file.md)) | done        |
| 02 | session-log-privacy ([phase-02-session-log-privacy.md](phase-02-session-log-privacy.md)) | done        |
| 03 | vault-container ([phase-03-vault-container.md](phase-03-vault-container.md)) | done        |
| 04 | prompt-guard ([phase-04-prompt-guard.md](phase-04-prompt-guard.md)) | done        |
| 05 | config-and-doc-truth ([phase-05-config-and-doc-truth.md](phase-05-config-and-doc-truth.md)) | done        |
| 06 | prescan-fails-closed ([phase-06-prescan-fails-closed.md](phase-06-prescan-fails-closed.md)) | done        |
| 07 | bash-confinement ([phase-07-bash-confinement.md](phase-07-bash-confinement.md)) | done        |
| 08 | project-vocabulary ([phase-08-project-vocabulary.md](phase-08-project-vocabulary.md)) | done        |
| 09 | ner-fails-closed ([phase-09-ner-fails-closed.md](phase-09-ner-fails-closed.md)) | done        |
| 10 | sandboxed-commands ([phase-10-sandboxed-commands.md](phase-10-sandboxed-commands.md)) | done        |
| 11 | protect-git-and-config ([phase-11-protect-git-and-config.md](phase-11-protect-git-and-config.md)) | done        |

## Reference implementation

`tools/mask.py` in the CRS modernisation repo implements the phase-01 matcher,
with its limits measured on real documents: restoring a masked document
normalises alias spelling and casing, so a restored copy is not byte-identical;
and an entry not flagged to match inside identifiers is invisible there by
design. Its shell prompt-guard carries the phase-04 fixes.

## Estimate

About **three to four executor days** for all six phases, plus review. Phase 01
is the only one with real design content; 02 and 03 are permissions and one
call to an existing chokepoint; 04 is a shell script; 05 is a deletion and a
paragraph; 06 is one match arm.

## F05 retrospective

**Closed 2026-09-18 at eleven phases**, all eleven findings shut. Six phases
were `approved_first_try` (02, 03, 04, 06, 08, 10) and five `approved_after_1`
(01, 05, 07, 09, 11). Five bugs were filed; four were fixed by the executor and
one — [bug-08-1](bugs/bug-08-1.md), a debug `eprintln!` of the PII dictionary —
was waived by the human rather than spend a cycle on it. Zero session
takeovers: every failure was recovered by resume or refined re-dispatch.

Gates at close: fmt / build / clippy clean with zero warnings, `cargo test`
716 + 2 + 1206 passed (10 ignored), up from 1142 library tests when the
milestone opened.

**Cost.** 23 executor runs, 2 604 turns, about 12.9 hours of executor wall
clock across three calendar days. The Estimate section above guessed "three to
four executor days for all six phases"; the milestone grew to eleven phases and
still finished inside that envelope, so the per-phase estimate was roughly
right and the phase count was not.

### Levers: resume is the answer to `budget_exceeded`

Five runs ended `budget_exceeded` at the 200-turn ceiling (phases 01, 02, 03,
07, 10). `continue_phase` recovered **all five**, in 35, 46, 95, 171 and 35
turns — none needed a re-dispatch or a takeover, and none lost committed work.
The ceiling was raised to 220 on 2026-09-18 in response; note that
`rexymcp.toml` is git-ignored, so that change does not travel with the repo.

### Folds landed 2026-09-18 (on human sign-off)

**1. Verify the mechanism before you pin a test to it.** Three occurrences,
all the architect's error, all found by the executor or at review:

- **Phase 07** asserted `touch "$HOME/x"` exits non-zero inside the sandbox. It
  succeeds — the toolchain `--ro-bind-try` mounts create `$HOME` in the tmpfs.
  The check should have been "the write does not reach the host".
- **Phase 03**'s test 2 asked for a scenario the implementation legitimately
  repairs (delete `.gitignore`, reopen — `open` rewrites it). Running git
  instead of reasoning about it produced a better test *and* a stronger claim:
  git reports a **tracked** file as not ignored even when a rule matches, so
  the check now catches an already-committed vault.
- **Phase 11** promised the earlier sandbox tests would pass unchanged after
  adding an always-present mount. They could not. Two of the three bounce items
  were the spec's fault.

`WORKFLOW.md` already says to verify **external APIs** against live docs. The
fold is its sibling for local mechanisms — kernel, git, shell — with the same
rule: run it, then quote the output; never guess a fact the executor will trust.
Landed as `WORKFLOW.md` § "Verify the mechanism before you pin a test to it".

**2. Count the mechanical sites, or thread the value another way.** Phase 02
added one field to `LoopDeps` and paid 21 mechanical test-literal edits; that
churn, not the design, ate a 200-turn budget. The proposed fold is one line in
the phase-doc authoring guidance: when a spec adds a field to a struct that
test literals construct, state the number of construction sites in the Spec.
A phase doc costs an executor context, not turns — concision does not fix
churn, and churn is a spec-*design* problem.

### Held as data, not yet folds

- **Executor claimed a verification it did not run: 2×.** Phase 09 reported
  `complete` without running the required live end-to-end, and the test it
  skipped could never have passed (it counted 15 zipped names against a
  threshold of 270). Phase 06's positive control was misdescribed — the pasted
  FAILED output does not match the revert it claims to describe. Both were
  caught by re-running the control at review. Candidate treatment: state in the
  finish conditions that the *pasted output* is what proves the phase done.
- **The environment can make a good spec look bad: 1×.** Phase 01 ended
  `budget_exceeded` with nothing committed, then completed on the same spec
  once phase 08 stopped the pre-scan from redacting the project's own name. An
  executor that cannot name the files it is asked to edit is not evidence of a
  bad spec.
- **Server-authored completion entries** headed `ts=<epoch-ms>` rather than a
  date: still open from F06 (4×), still waiting on a human go-ahead.

### Local vs cloud executor

Measured this milestone, and the reason for the standing routing rule:

| | local (`RedHatAI/Qwen3.8-27B-INT4`) | cloud (`deepseek-flash`) |
|---|---|---|
| Throughput | ~4 turns/min | ~16 turns/min |
| Greenfield / additive work | fine | phase 06 `approved_first_try`, 66 turns, 4 min |
| In-place edits in dense modules | phase 05 complete, 89 turns | phase 05 `hard_fail` — invented an unrelated edit, truncated a comment mid-word, then oscillated repairing it |

**Rule:** local first; route cloud work to greenfield or additive phases. Any
phase touching `executor/src/privacy/**` or `executor/src/security/**` stays
local regardless. The first real cloud dispatch exercised every protection
layer correctly — 2 239 redaction markers on the wire, a `0700`/`0600` session
log, `bash` in bubblewrap, `.git` untouched.

### Carried forward

- **[bug-08-1](bugs/bug-08-1.md), waived:** the `eprintln!` of the PII
  dictionary in `executor/src/privacy/egress.rs`. Delete it in the next phase
  that touches that file.
- **`docs/dev/NEXT.md` is 326 KB / 4 437 lines** and every session reads it per
  `REXYMCP.md` § "Read these first". It has accumulated closed-milestone
  history since M35. Trimming it to the active pointer plus a short carry-forward
  block is a candidate chore.
