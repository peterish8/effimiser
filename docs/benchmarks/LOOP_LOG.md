# Engineering loop log

One entry per iteration of BUILD → TEST → BENCHMARK → ANALYZE → FIX. Numbers
here are measurements, not estimates. Where a number does not exist yet, the
entry says so rather than guessing.

---

## Iteration 1 — 2026-09-12 — Phase 0 + Phase 1

Commit: `d802b96`

### What improved

- 26 tests, 0 failures. `cargo build --workspace` clean.
- Raw recoverability demonstrated end to end: `ctx run cargo tree` captured
  6,718 bytes / 2,599 `o200k_base` tokens; `ctx output <handle>` returned the
  bytes **byte-identically** (verified by diff against a fresh `cargo tree`).
- Content-addressed dedup verified: two runs with identical output store one
  copy of it (2 blobs total, counting the shared empty stderr).
- Handle durability verified across a store reopen, which is the property that
  makes cross-session evidence recovery possible.
- License risk pinned down before any code was written: four of 17 reference
  projects are blocked from source reuse.

### What became worse

Nothing measured. There is no prior state to regress from.

### What remains bottlenecked

**No benchmark harness exists.** Every target in benchmark-plan.md is currently
unmeasured. The single end-to-end example above is an existence proof, not a
benchmark: one run, one repository, no baseline, no repetitions, no p50/p95.
It must not be cited as a savings figure, and the README deliberately carries
no percentages.

Also unmeasured: store write throughput, blob read latency, token-counting cost
on large inputs. `TokenCounter::new` loads a full BPE vocabulary and is likely
the dominant cold-start cost, but this is a hypothesis, not a profile result.

### What we learned

1. **Token accounting had to be settled first.** Had `bytes / 4` been used
   anywhere, every later savings number would have been unfalsifiable. Pinning
   it to a real BPE with a unit test that rejects the estimator makes the
   anti-gaming rule structural instead of aspirational.

2. **Content addressing pays for itself immediately.** Idempotent writes mean
   repeated capture of the same large log is free, with no dedup logic layered
   on top.

3. **The problem is real and observable on this machine.** The delegated Codex
   CLI aborted three times with a 1 GiB allocation failure while loading its
   ambient MCP, hook and skill catalogue, and reported dropping 467 skill
   descriptions for exceeding a 2% context budget — spending 10,382 tokens to
   answer "reply CODEX_OK". Recorded in FAILED_EXPERIMENTS.md #002. This is
   direct field evidence for the schema-virtualisation design, and a warning
   that the virtualisation layer must itself stay cheap at startup.

### Next highest-value experiment

Phase 2, and specifically the part that can be wrong in a detectable way:

> Build the microbenchmark harness and record p50/p95 over ≥100 runs for
> (a) `TokenCounter::new` cold construction, (b) `count()` on 1 KB / 100 KB /
> 10 MB inputs, (c) `put_run` write throughput at 1 KB / 1 MB / 40 MB, and
> (d) `run_output` read-back latency at the same sizes. Emit JSONL to
> `benchmarks/results/` with the full reproducibility record.

Then a negative control: feed the harness a deliberately broken treatment and
confirm it is reported as a regression rather than a win. A harness that cannot
detect a planted regression cannot validate a real improvement.
