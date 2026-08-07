# Phase 1: Endpoint classification + egress config

**Milestone:** M45 — Executor Egress Protection
**Status:** review
**Depends on:** none (M44 config exists)
**Estimated diff:** ~120 lines (module + config field + tests)
**Tags:** language=rust, kind=feature, size=s

## Goal

Decide *when* egress protection engages: classify the executor's `base_url` as a
local/LAN host vs a cloud host, and resolve the engage/skip decision from
`[privacy]` config. Pure logic; the redaction itself is later phases.

## Architecture references

- `docs/dev/milestones/M45-executor-egress-protection/README.md` — the design and
  the `privacy.enabled && !endpoint_is_local(base_url)` engagement rule.
- `executor/src/config.rs` — `PrivacyConfig` (M44); this adds one field.
- `docs/dev/STANDARDS.md` §3.1 (pure fns need tests).

## Design principle

Err toward **cloud** on ambiguity. Misclassifying a cloud host as local skips
redaction → a leak; misclassifying a local host as cloud only over-redacts (safe).
So only clearly-local hosts count as local; unknown hostnames are cloud.

## Spec

1. **`executor/src/privacy/egress.rs`** (new; `pub mod egress;` in `privacy/mod.rs`):
   - `pub fn endpoint_is_local(base_url: &str) -> bool`: extract the host
     (scheme-strip, drop `/path`, `userinfo@`, `:port`, `[ipv6]`), then local iff
     `localhost` (case-insensitive), a loopback/RFC-1918 IP (via
     `std::net::IpAddr::is_loopback` / `Ipv4Addr::is_private`), or a hostname
     ending `.local` / `.lan` / `.internal`. Everything else → cloud.
   - `pub fn should_redact_egress(privacy: &PrivacyConfig, base_url: &str) -> bool`:
     `false` if `!privacy.enabled`; else `privacy.redact_executor_egress` when set
     (force on/off), else `!endpoint_is_local(base_url)`.

2. **Config** — add `pub redact_executor_egress: Option<bool>` to `PrivacyConfig`
   (`None` = auto). Derive `Default` already yields `None`.

## Acceptance criteria

- [ ] `cargo build` / `clippy -D warnings` / `fmt --check` / `test` all pass.
- [ ] `endpoint_is_local` is true for `localhost`, `127.0.0.1`, `::1`,
      `192.168.50.138`, `10.0.0.5`, `172.20.1.1` (incl. with `:port` / scheme /
      `[ipv6]`), and false for `api.deepseek.com`, `8.8.8.8`, `172.32.0.1`.
- [ ] `should_redact_egress`: disabled → false; enabled+cloud → true;
      enabled+local → false; force `Some(true)` overrides local → true; force
      `Some(false)` overrides cloud → false.

## Test plan

`egress.rs` unit tests: `local_hosts_classified_local`,
`cloud_hosts_classified_cloud`, `strips_scheme_port_and_path`, `ipv6_loopback`,
`private_range_boundaries` (172.16 vs 172.32), plus `should_redact_*` cases.

## End-to-end verification

Not applicable — pure library logic, no runtime-loadable artifact. Covered by unit
tests.

## Authorizations

- No new dependencies (stdlib `std::net`). New file `executor/src/privacy/egress.rs`.
- No `docs/architecture.md` edit.

## Out of scope

- The pre-scan, the redaction chokepoint, the write-refuse — phases 02–04.
- Wiring the decision into dispatch — phase-05.

## Update Log

(Filled in by the executor. See WORKFLOW.md § "Update Log entries".)

<!-- entries appended below this line -->

### Update — 2026-08-07 16:39 (complete)

**Summary:** Added `executor/src/privacy/egress.rs` with `endpoint_is_local`
(host extraction + stdlib `IpAddr::is_loopback` / `Ipv4Addr::is_private` + `.local`
/`.lan`/`.internal`, erring toward cloud on ambiguity) and `should_redact_egress`
(disabled → off; explicit `redact_executor_egress` override; else auto = cloud
only). Added the `redact_executor_egress: Option<bool>` field to `PrivacyConfig`
and `pub mod egress;`. Fixed the M44 `ner.rs` live-test `PrivacyConfig` literal to
include the new field. No new dependencies.

**Acceptance criteria:** all ticked above.

**Commands:**

```
$ cargo fmt --all --check      # clean
$ cargo clippy --all-targets --all-features -- -D warnings   # Finished, clean
$ cargo test 2>&1 | grep "^test result"
test result: ok. 691 passed; 0 failed; 0 ignored; ...     (mcp)
test result: ok. 2 passed; 0 failed; 0 ignored; ...       (readme_config_reference)
test result: ok. 1121 passed; 0 failed; 3 ignored; ...    (executor lib: +8 egress)
```

Baseline (M44 merged) was 1806; now 1814 (+8 egress tests).

**End-to-end verification:** Not applicable — pure library logic, no
runtime-loadable artifact. The 8 unit tests cover host classification (localhost /
loopback / RFC-1918 incl. the 172.16–31 boundary / IPv6 / `.local` vs public
domains and IPs, with scheme/port/path stripping) and the `should_redact_egress`
decision matrix (disabled, auto cloud/local, and both explicit overrides).
