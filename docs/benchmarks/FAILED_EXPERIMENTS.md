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

---

## 003 — Sequential A/B benchmark arms

**Date:** 2026-09-13
**Hypothesis:** Measuring a baseline arm to completion, then the treatment arm,
is good enough for a deterministic microbenchmark in a single process.

**Measured:** It is not. Single-threaded 10 MiB token counting recorded
**1,668 ms** on its first execution in the process and a **sustained 3,457 ms**
across the ten iterations that followed — a 2x degradation with a *tight*
p95/p50 of 1.1, so a systematic shift rather than a transient spike. The
degradation landed entirely on whichever arm ran first, which was the baseline.

The resulting speedups were:

| input | sequential arms | interleaved arms |
|---|---|---|
| 1 MiB | 5.93x | 4.12x |
| 10 MiB | 3.84x | 4.21x |
| 40 MiB | 1.76x | 5.06x |

**Result:** The sequential numbers are not merely noisy, they are *shaped
wrong*: the apparent speedup fell as the input grew, which is backwards for a
parallel decomposition across a fixed worker count. That inversion was the tell
that the measurement, not the code, was broken. Had the benchmark reported only
the 1 MiB case, it would have published 5.93x for a change whose honest figure
is around 4x.

**Decision:** Rejected. `Runner::measure_interleaved` alternates A, B, A, B and
swaps which arm leads on each round. All A/B comparisons use it, and results
carry an `interleaved` note so they are never compared against
sequentially-measured ones.

**Root cause, unresolved:** the underlying 2x degradation is not explained. It
is reproducible and correlates with repeated allocation of a multi-megabyte
`Vec<u32>` on Windows. Interleaving neutralises its effect on the *ratio*, but
the absolute serial numbers should not be treated as this machine's best case.
Recorded as an open question rather than a solved one.

---

## 004 — Antigravity headless dispatch for a build-and-test ticket

**Date:** 2026-09-13
**Hypothesis:** `agy -p ... --sandbox --mode accept-edits` can run a bounded
implementation ticket that requires `cargo build` and `cargo test`.

**Measured:** Returned `"status": "SUCCESS"` with an **empty response**, one
turn, 34,220 tokens, and `"denied_actions": [{"action": "command"}]`. The
worktree was untouched. stderr explained it: a tool required the `command`
permission, which headless mode cannot prompt for, so it was auto-denied.

**Result:** The ticket never ran. Note that the exit code was 0 and the status
field said SUCCESS — a worker self-report that would have been entirely wrong to
trust. The only evidence that nothing happened was `denied_actions` and an
unchanged worktree.

**Decision:** For tickets needing command execution, either grant a scoped
`permissions.allow` rule in agy's settings first, or route to a runtime whose
sandbox already permits scoped writes. `--dangerously-skip-permissions` is not
an acceptable workaround. This ticket was rerouted to `codex exec -s
workspace-write`.

**Worth recording generally:** always inspect `denied_actions` and the actual
diff. Exit code zero does not prove the requested work ran.
