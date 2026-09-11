# Failed experiments

Negative results, kept deliberately. An idea that did not work is worth as much
as one that did, provided the measurement is recorded. Nothing is removed from
this file once added.

Format per entry: hypothesis, what was measured, result, decision.

---

## 001 — Bash heredocs for authoring Rust source

**Date:** 2026-09-12
**Hypothesis:** Writing `.rs` files via `cat > file <<'EOF'` is faster than a
dedicated file-writing tool.

**Result:** Failed on the first non-trivial file. Rust raw-string syntax
(`r#"..."#`) combined with mixed quotes inside a heredoc produced
`unexpected EOF while looking for matching quote` at line 134, losing the whole
write. Retry cost exceeded any saving.

**Decision:** Rejected for source files. Bash heredocs remain fine for
markdown and simple config. SQL schema constants in Rust now use plain `"`
strings rather than raw strings, which also removes the fragility at the source.

---

## 002 — Codex CLI with the user's ambient configuration

**Date:** 2026-09-12
**Hypothesis:** Delegated research tasks can run via `codex exec` using the
machine's existing Codex configuration.

**Measured:** Three consecutive invocations aborted with
`memory allocation of 1073741824 bytes failed` (1 GiB) during startup, after
loading MCP servers, hooks and the skill catalogue. One run additionally
reported `Exceeded skills context budget of 2%. All skill descriptions were
removed and 467 additional skills were not included`. A trivial prompt
("reply CODEX_OK") consumed **10,382 tokens**.

**Result:** Unusable. With `--ignore-user-config` the same prompt succeeded.

**Decision:** All delegated Codex runs pass `--ignore-user-config`.

**Worth recording for the product:** this is the failure mode Context Runtime
exists to prevent, observed in the wild on the development machine itself. An
eagerly-loaded tool and skill catalogue consumed a gigabyte of memory and ten
thousand tokens before any work began. It is direct evidence for the MCP
schema-virtualisation design in architecture-v0.md §3.8 — and a caution that
the virtualisation layer must itself stay cheap at startup.
