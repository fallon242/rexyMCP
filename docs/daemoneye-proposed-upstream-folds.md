# Proposed upstream folds from the DaemonEye project

**Written:** 2026-08-09, at the close of DaemonEye's M12 milestone.
**Audience:** whoever does the analysis and the folding in this repo. You do
not need the conversation that produced this — everything is cited to a file
you can read.

**Source of truth for all proposed text:** `/home/matt/src/daemoneye/docs/dev/WORKFLOW.md`.
**Target:** `/home/matt/src/rexyMCP/plugin/templates/WORKFLOW.md`.

This is a **proposal, not a patch**. Read the source sections and decide what
generalises. Several are DaemonEye-flavoured and need the specifics filed off.

---

## 1. The situation, measured

Run this yourself before trusting any of it — line counts and headings drift:

```bash
L=/home/matt/src/daemoneye/docs/dev/WORKFLOW.md
T=/home/matt/src/rexyMCP/plugin/templates/WORKFLOW.md
wc -l $L $T
echo "== LOCAL-ONLY (candidate pushes) =="
comm -23 <(grep -E "^#{2,3} " $L | sed 's/[[:space:]]*$//' | sort -u) \
         <(grep -E "^#{2,3} " $T | sed 's/[[:space:]]*$//' | sort -u)
echo "== UPSTREAM-ONLY (lessons DaemonEye may be missing) =="
comm -13 <(grep -E "^#{2,3} " $L | sed 's/[[:space:]]*$//' | sort -u) \
         <(grep -E "^#{2,3} " $T | sed 's/[[:space:]]*$//' | sort -u)
```

As of 2026-08-09 that reported:

- **1611 lines local vs 848 upstream** — the template is ~760 lines behind.
- **14 local-only `##`/`###` headings.** M12's seven folds are a *subset* of a
  backlog that predates M12.
- **2 upstream-only headings**, `## How to fix` and `## Verification`. These
  are **deliberately superseded** in DaemonEye — do not pull them back, and do
  not treat their absence as drift.

Heading comparison understates it. Sections that exist in both can still be
wildly different sizes: § "End-to-end verification" is **11 lines upstream and
~180 lines locally**.

### A correction, stated plainly because it was asserted the other way first

An earlier note in DaemonEye's `NEXT.md` claimed the template "still tells
architects to write mutation pairs as `sed -i`" and called the push an urgent
correctness fix affecting every repo. **That was wrong.** `grep -n 'sed -i'`
against the template returns nothing; its E2E section is the bare original and
never carried mutation-pair guidance at all. The template misinstructs nobody.

The real argument for pushing is weaker but still real: **absence, not error.**
An architect given no guidance on how to express a mutation pair invents one,
and `sed -i` is the natural reflex — which is precisely how DaemonEye acquired
it and then had to unpick it across three phases. Prioritise accordingly; this
is a backlog to work through, not a fire.

---

## 2. The one thing to fold first, if you fold only one

**The executor cannot make in-place shell edits.** The contract in this repo
(`executor/templates/executor_contract.md`, the do-not list) bans `sed -i`,
`perl -i`, and `>`/`tee` redirects into a source file, and `bash` refuses them.
Nothing in the *template* tells architects this, so architects who need a
mutation applied and reverted reach for the obvious shell one-liner and write
an unrunnable block.

In DaemonEye this went unnoticed for three phases because the failure is
silent and grades green: the executor substitutes `patch`, the run passes, and
the only visible symptom is that marker `echo`s sitting *between* the banned
commands go missing from the transcript.

**Source:** `docs/dev/WORKFLOW.md` § "End-to-end verification", the paragraph
beginning *"Concretely, mutation pairs belong in `## Spec` as tasks"* and its
fold note *(Folded 2026-08-08 after M12 phases 03–05)*.

**Shape of the rule:** each mutation pair becomes three numbered tasks in
`## Spec` — a `patch` apply quoting exact `old_str`/`new_str`, the inverse
`patch` restore, and a `grep -c` of the mutated text after *each* direction.
Marker `echo`s and test runs stay as ordinary shell; only the *edit* moves to
the `patch` tool. `git checkout` is never the restore for a file holding the
round's own uncommitted work.

**Generalisation note:** the `grep -c` applied-check is the load-bearing half
and is easy to drop as noise. A `patch` whose `old_str` does not match fails
loudly, but one that matches the *wrong* line does not, and a mutation that
silently did not apply certifies a vacuous guard.

---

## 3. The two folds with the strongest evidence

### 3a. Give the executor a condition it can check, not an instruction it can agree with

**Source:** `docs/dev/WORKFLOW.md`, the section of that exact name (a new
top-level `###`, so it can be lifted whole).

When a requirement keeps going unmet, the reflex is to state it more clearly.
That reflex is wrong often enough to name: if the executor can satisfy every
tracked task and still miss the requirement, no rewording closes the gap,
because nothing in its own loop evaluates the requirement. Two shapes that
work — make it a seeded `## Spec` task, or give it a self-check with a
falsifiable output (`PASS`/`FAIL`) and make that output an acceptance
criterion. And run the check against a known-bad input before speccing it.

**Why this one is worth the space** — the evidence is a controlled contrast,
not an anecdote:

| Attempt | Result |
|---|---|
| Three rounds of increasingly specific prose about how to write the E2E block | entry still missing |
| Make the capture a numbered `## Spec` task | worked immediately, both times used |
| Shrink the artifact 2,555 lines → 56 so it *could* be pasted whole | still retyped from memory |
| Add a `PASTE MATCH` self-check the executor runs on its own output | byte-identical, first try |

Both "better wording" attempts failed; both structural attempts worked first
try. **This is the most transferable thing DaemonEye learned in M12** and is
not model-specific reasoning — it is about what the harness tracks.

**Depends on a rexyMCP implementation fact** you should verify before folding:
only a heading of *exactly* `## Spec` is seeded into the task list
(`executor/src/agent/tasks.rs` — DaemonEye cited `if line.trim() == "## Spec"`
around lines 52–55; confirm it still holds). If that changes, the first shape
above changes with it.

### 3b. A completion summary is a claim too, including its "deviations" line

**Source:** `docs/dev/WORKFLOW.md` § "A pasted transcript is a claim, not
evidence" — the paragraph beginning *"A completion summary is a claim too"*.
Note the parent section is itself local-only (see §4), so this may fold as part
of it rather than separately.

Read the diff, not the narrative. In M12 the deviations line was wrong in both
directions: one phase reported "Deviations from spec: None" while having
rewritten an unrelated tool's user-visible text, and another reported removing
an unused binding **that never existed in the file**. Neither caused a
regression; neither was catchable by reading the summary carefully.

Recorded against *three accurate* self-reports in the same milestone — the
point is not that executors lie, it is that accuracy is unknowable from the
text and cheap to establish from the diff.

---

## 4. Sections that exist only locally — the wider backlog

Each of these is a `###` under the calibration part of DaemonEye's
`WORKFLOW.md` and can be read there in full. They are listed in rough
descending order of how general they look; the judgement is yours.

| Section | One line | Notes for folding |
|---|---|---|
| `A pasted transcript is a claim, not evidence` | capture mechanically; reviewer re-runs and diffs | Carries 3a's paragraph and the paste-fidelity self-check. Largest and most reusable of the group. |
| `Coverage claims are inadmissible without mutation proof` | a test not seen to fail proves nothing | Pairs with §2 — the mutation machinery is only worth specifying because of this rule. |
| `Every acceptance criterion must be satisfiable, and its mechanics pinned` | re-read each criterion against the rest of your own spec | DaemonEye added a reinforcement note after two further breaches, one of which was a rule in these very docs. |
| `Run every count criterion; never derive it` | run the number, don't compute it | Small, general. |
| `A NoProgressStall is usually a nearly-finished phase — diagnose the tree before choosing a lever` | check the working tree before picking an escalation lever | General; M12 adds a counter-case — see §5. |
| `The bounce sequence — four steps, in order, none optional` | a bounce isn't done when the bug doc is written | Includes "stale criteria certify the phase as finished", which is broadly true. |
| `State the symptom, the root cause and the DoD — not the fix` | bug-report discipline | Check against upstream's `## How to fix` / `## Verification`, which it supersedes. |
| `A sweep's scope is its convertible sites, not its matches` | sizing refactor phases | Moderately general. |
| `Executor self-sabotage on delete-heavy rewrites is a runtime concern` | | Possibly rexyMCP-runtime rather than template material. |
| `A phase that exhausts a trait's uses must say what happens to its import` | | Rust-specific; probably **do not** fold, or fold as a generic "last-use" note. |
| `Task N — Capture the end-to-end evidence` | the template task text itself | Part of the phase-doc template, not the prose. |
| `Definition of done`, `Root cause` | bug-report template subheadings | Structural; fold with the bug-report template or not at all. |

---

## 5. One thing that argues *against* a rule you already have

Fold nothing here — this is a data point to weigh.

Upstream's `A NoProgressStall is usually a nearly-finished phase — diagnose the
tree before choosing a lever` says to inspect the tree and prefer a cheap
lever. M12 phase-06a produced **two consecutive `NoProgressStall` hard-fails of
the same shape**: a mutating `patch` lands, then ~60 consecutive
`search`/`read_file` calls against the *same file* with no edit, until the
governor fires. The second round's spec carried an explicit
`Notes for executor` block naming that exact pathology, and it did not help.
It was resolved by architect takeover.

If the upstream rule reads as "a stall means you are nearly done, so resume",
consider adding the counter-case: **a stall that recurs within the same phase,
in the same shape, is a takeover signal rather than a re-dispatch one.** That
is now three data points in DaemonEye's history.

---

## 6. Suggested order of work

1. Re-run the drift commands in §1 — these numbers are from 2026-08-09.
2. Fold §2 (the in-place-edit prohibition). Highest value per line, and it
   documents a hard constraint of this repo's own executor contract that the
   template currently leaves unstated.
3. Fold §3a. Verify the `## Spec` seeding fact first.
4. Decide on § "A pasted transcript is a claim, not evidence" as a whole,
   which brings §3b and the paste-fidelity check with it.
5. Work the §4 table at whatever pace suits, skipping the Rust-specific one.
6. Weigh §5 against the existing stall guidance.
7. Leave `## How to fix` and `## Verification` alone.

Where a fold lands, keep DaemonEye's `*(Folded <date> after <what>, on PE
sign-off …)*` note form but **rewrite the specifics** — upstream readers have
no idea what "M12 phase-06a" is. What generalises is the mechanism and the
occurrence count, not the phase numbers.

---

## Disposition — 2026-08-09, decided with the project owner

Analysis re-verified every cited fact (drift numbers, `tasks.rs:55` seeding,
`executor_contract.md:160-164` shell-edit ban) before folding. Decisions:

**Folded into `plugin/templates/WORKFLOW.md`:** §2 (mutation pairs as `patch`
Spec tasks + the seeded capture task, as calibration § "The E2E block" plus a
tightened template E2E section); §3a verbatim-condensed; § "A pasted
transcript is a claim" whole, including §3b and the paste-fidelity check;
the bounce sequence; § "State the symptom, the root cause and the DoD — not
the fix" — **and, going further than this proposal's step 7, the bug-report
template's `## How to fix`/`## Verification` sections were replaced by
`## Root cause`/`## Definition of done`** (owner decision: transfer authority
to the executor; `How to fix` is now optional and admissible only when the
architect has run the fix). Also folded condensed: satisfiable-criteria,
count-criteria, coverage-mutation-proof, sweep-scope (carrying a generic
last-use-import note in place of the Rust-specific trait section), and the
delete-heavy/additive phase split. All fold notes rewritten to mechanism +
occurrence count, DaemonEye phase numbers filed off.

**Folded into `plugin/skills/escalate/SKILL.md`:** the NoProgressStall
diagnose-the-tree-first rule (the acting agent at stall time reads the skill,
not the target repo's WORKFLOW).

**Declined (owner decision):** §5's recurring-stall takeover counter-case —
left as-is for now; it is the one item that reduces executor opportunity, and
the preferred long-term fix is runtime loop-breaking, not policy.

**Deferred to M45** (`docs/dev/milestones/M45-executor-work-preservation-guards/`):
the runtime feature requests from § "Executor self-sabotage" — git self-revert
hard-block (the unblocked `checkout <file>` / `HEAD --` / `restore` / `stash`
forms) and identical-call normalization. The read-only-stall ask already
landed in M37 and is noted as such there.

**Left alone, as proposed:** upstream's `## How to fix`/`## Verification`
headings are gone by supersession, not by pull-back; the Rust trait-import
section was not folded standalone.
