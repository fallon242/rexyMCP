# rexyMCP privacy — the PII ingestion gate (M44)

rexyMCP can anonymize personally identifiable information (PII) before it reaches
a **cloud model** — Claude (the architect) or a cloud executor endpoint such as
DeepSeek — and reconstitute it on demand from a **local, encrypted, git-ignored
vault**. Detection runs on a **local** model (Qwen on your LAN), so the detection
step itself never leaves your network.

> **Read this honestly.** This is a *risk-reduction* tool, not a guarantee.
> Deterministic detectors (email, phone, SSN, credit card, IP, MAC) are reliable.
> The NER model that catches names / addresses / organizations is **best-effort**
> and **will miss some** — and every miss is exactly the leak the gate exists to
> stop. The reversible vault is a PII honeypot; it is encrypted at rest and never
> committed, but its existence concentrates risk. Treat this as defense-in-depth,
> not a compliance boundary.

## Where PII can leak, and what the gate does

```
your prompt ─▶ Claude architect (CLOUD ①) ─▶ execute_phase ─▶ DeepSeek executor (CLOUD ②)
                  ▲                                               │ reads target repo
                  └──── PhaseResult / diff / briefing ◀───────────┘  (return path, CLOUD ①)
        Qwen (LAN) = local PII engine, detection only, never on any cloud path
```

- **① Return path to Claude** — every `execute_phase` / `continue_phase`
  `PhaseResult` is scrubbed of **structured** PII (deterministic detectors) before
  it crosses the MCP boundary to Claude. Automatic when `[privacy].enabled = true`.
- **② Executor egress to a cloud model** — anonymizing the executor's outbound
  prompts and reconstituting token→original on writes is **deferred** (see
  `docs/dev/milestones/M44-pii-ingestion-gate/phase-06b-executor-egress.md`): a
  model pass per turn is unbounded, and echo-back robustness is unproven. Until
  it lands, use a **local** executor for PII-bearing repos, or pre-scrub inputs
  with the CLI.
- **Your typed prompt** — scrub it before Claude sees it with the CLI (reliable)
  or the `UserPromptSubmit` hook (best-effort; see below).

## How it works

| Piece | What it does |
|---|---|
| Deterministic detectors | Regex + validators for email, phone, SSN, credit card (Luhn), IPv4, MAC. Reliable, scale to any size. |
| NER engine (Qwen, LAN) | Names, street addresses, organizations. Best-effort; thinking disabled so it returns direct JSON. |
| Tokenizer | Stable, reversible pseudonyms (`Person_1`, `Email_2`) — same original → same token, no collisions, boundary-safe. |
| Vault | The reversible token↔original map, **encrypted** (XChaCha20-Poly1305) under a local `0600` key, in a git-ignored dir. |
| Registry | Content-hash tracking so the model re-runs only on new/changed sources; tokens stay stable across edits. |

## CLI

Configure `[privacy]` in `rexymcp.toml` (see `rexymcp init` output), pointing
`engine_base_url` / `engine_model` at your local NER model, then:

```bash
# Anonymize a file or stdin → tokenized text on stdout; mapping saved to the vault
printf '%s' "John Smith emailed jane@acme.com" | rexymcp anonymize
# → Person_1 emailed Email_1

# Reverse it (reads the same vault)
printf '%s' "Person_1 emailed Email_1" | rexymcp reconstitute
# → John Smith emailed jane@acme.com

# Inspect the vault — counts per PII kind, never the originals
rexymcp vault
```

Flags: `--config <path>` (default `rexymcp.toml`), `--repo <dir>` (default `.`,
used only for the default vault location), `--vault <dir>` (override the vault
directory). Input is a positional file path, or `-` / omitted for stdin.

**Reliable workflow for a PII-bearing prompt:** run `rexymcp anonymize` on your
text, paste the tokenized output into Claude, and `rexymcp reconstitute` anything
you need to read back in real terms. The vault stays on your machine.

## The `UserPromptSubmit` hook (opt-in safety net)

**Honest constraint:** Claude Code's `UserPromptSubmit` hook **cannot rewrite the
prompt** — the contract only allows *allow*, *add-context*, or *block*. So there
is no way to silently scrub-and-forward your prompt. A hook can only **block** a
prompt it doesn't like.

`plugin/hooks/pii-guard.sh` is therefore a **block-on-PII safety net**: it detects
obvious *structured* PII (email / SSN / phone / card) with fast local regex — no
model, no network, so it never fails open — and blocks the prompt with a message
telling you to run `rexymcp anonymize` and paste the tokenized text. It does
**not** catch names or addresses (those need the model); for full anonymization,
use the CLI.

Enable it (opt-in) by copying the script into your project and registering it in
`.claude/settings.json`:

```json
{
  "hooks": {
    "UserPromptSubmit": [
      { "type": "command",
        "command": "${CLAUDE_PROJECT_DIR}/.claude/hooks/pii-guard.sh",
        "timeout": 30 }
    ]
  }
}
```

Requires `jq`. **The reliable, comprehensive path remains the CLI**: run
`rexymcp anonymize` on your text and paste the result. The hook is a backstop for
the times you forget.

## Configuration (`[privacy]`)

```toml
[privacy]
enabled = false                                 # opt-in; the gate is inert until true
# engine_base_url = "http://localhost:8080/v1"  # local NER endpoint (detection only)
# engine_model = "qwen3.5-9b"                    # thinking must be off
# vault_dir = ".rexymcp/vault"                   # default: <repo>/.rexymcp/vault
```

## Limitations (do not skip)

- **Best-effort NER.** Structured PII is caught reliably; names/addresses via the
  model are not guaranteed. Bias is toward over-matching (over-redaction is safe;
  a miss is a leak).
- **Vault = honeypot.** Encrypted and git-ignored, but it concentrates every
  original. Protect the vault dir and its key like a secret.
- **Executor egress (②) is not yet automatic** — deferred (phase-06b). A cloud
  executor over a PII-bearing repo is not protected by the return-path scrub
  alone.
- **This cannot retro-protect a chat.** PII already sent to Claude is already in
  the cloud. Scrub *before* sending.
