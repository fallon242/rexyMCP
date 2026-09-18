# F10 — Remove PII debug print

**Goal:** the privacy module never prints the contents of a PII dictionary.

**Status:** open — opened 2026-09-18 on human go-ahead.

**Depends on:** none.

**Source:** F05 [bug-08-1](../F05-privacy-security-hardening/bugs/bug-08-1.md),
waived 2026-09-17 with the line left in the tree. `NEXT.md` has carried it as
an open item since.

**Exit criteria:**

- [ ] `executor/src/privacy/` contains no `println!` / `eprintln!`.
- [ ] All four gates pass with counts unchanged.

## Phases

| #  | Phase                                                                        | Status |
|----|------------------------------------------------------------------------------|--------|
| 01 | drop-debug-print ([phase-01-drop-debug-print.md](phase-01-drop-debug-print.md)) | todo   |

## Notes

- **Routing: local only.** Privacy code.
