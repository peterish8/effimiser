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

---

## Iteration 3 — 2026-09-13 — Parallel token counting

Commits: `03fb7e9` (benchmark methodology), `97183b5` (the optimisation)

Artifacts: `benchmarks/results/micro-97183b5.jsonl`

The artifact records `git_dirty: true`. At the time of the run every *tracked*
file was committed at `97183b5`; the flag reflects untracked files only — the
results directory itself, `.orchestration/`, and two research documents, all
of which are committed alongside this entry. Noted rather than quietly
tidied away, because a dirty flag that turns out to be meaningless is exactly
the kind of thing that erodes trust in the ones that are not.

### What we set out to do

Iteration 2's baseline said token counting ran at ~2.5 MiB/s single-threaded.
Projected onto the workload this runtime exists to serve — capturing a noisy
build or test log — that is ~4 s for a 10 MiB log and ~16 s for a 40 MiB one,
spent purely on *measuring* output before any compression decision is made.

That is not a cosmetic number. "Pretending local CPU work is free" is on this
project's prohibited list, and a runtime that cannot afford to count honestly
is one that will eventually be tempted to estimate. Making honest accounting
cheap is load-bearing for the project's integrity, not a micro-optimisation.

### What improved

Counting is now split across up to 16 threads at boundaries where the
tokenizer provably cannot emit a token spanning the split. Interleaved A/B,
warm p50, on the clean commit:

| input | serial (baseline) | parallel | speedup | throughput |
|---|---|---|---|---|
| 1 KiB | 0.42 ms | 0.42 ms | 1.00x | unchanged, below threshold |
| 100 KiB | 43.67 ms | 43.59 ms | 1.00x | unchanged, below threshold |
| 1 MiB | 383.57 ms | 97.88 ms | **3.92x** | 2.6 → 10.2 MiB/s |
| 10 MiB | 3,859.55 ms | 857.46 ms | **4.50x** | 2.6 → 11.7 MiB/s |
| 40 MiB | 15,656.20 ms | 3,176.67 ms | **4.93x** | 2.6 → 12.6 MiB/s |

n = 100 / 100 / 100 / 10 / 5 respectively. A second independent run of the
same suite gave 4.12x / 4.21x / 5.06x, so these reproduce to within ~5%.

In practical terms: a 40 MiB build log costs ~3.2 s to count instead of
~15.7 s. Counting a 1 MiB log is now cheaper than the one-off cost of loading
the vocabulary.

### What became worse

- **Nothing measurable on the pathological case, but it was worth measuring.**
  A 4 MiB input with no newline anywhere (a minified bundle) has no safe split
  point. The parallel path scans for a boundary, fails, and falls back to
  serial. Measured 1,621.88 ms serial vs 1,515.66 ms through the public API —
  i.e. no penalty above noise, and certainly no gain. The failed scan is cheap
  relative to the tokenization it precedes. This limitation is now a permanent
  benchmark case rather than a footnote.

- **The corpus test got ~20x slower** (roughly 1 s to ~20 s) because it now
  compares full token sequences across both vocabularies over every `.rs`,
  `.toml` and `.md` file in the repository. Accepted deliberately: see below.

- **Complexity.** `tokens.rs` grew from 134 to ~490 lines, most of it the
  correctness argument and its tests. The split rule is now a thing that must
  be re-verified if the tokenizer dependency is upgraded.

### What remains bottlenecked

**`TokenCounter::new` is now the dominant fixed cost: 402.9 ms cold, 249.0 ms
warm p50 (n=10).** It is unchanged this iteration and is paid once per CLI
process. A profiling probe (`crates/ctx-bench/examples/profile_construct.rs`,
kept so this is reproducible) attributed it across three runs:

| stage | ms | needed by a counter? |
|---|---|---|
| whole `o200k_base()` | 313.2 / 376.7 / 397.8 | — |
| base64-decode + build encoder map | 61.8 / 65.8 / 79.5 | **yes** |
| compile pretokenizer regex | 6.9 / 8.2 / 9.2 | **yes** |
| 128 regex clones x2 | 10.5 / 17.5 / 14.9 | no |
| build 200k-entry decoder map | 29.5 / 31.7 / 38.2 | no |
| clone + sort 200k token byte strings | 87.9 / 80.5 / 93.4 | no |

So ~70–90 ms is genuinely required and ~130–145 ms is work a counter never
touches — `CoreBPE::new` builds a decoder and a sorted token table that only
`_encode_unstable` uses. Roughly 120–160 ms remains unattributed and must be
identified before assuming the avoidable stages are the whole story.

Also still unmeasured: everything above the microbenchmark layer. There is
still no index, no query, no retrieval, so none of the targets in
benchmark-plan.md §3–§5 have a number yet.

### What we learned

1. **A wrongly-shaped result is more informative than a noisy one.** The
   sequential benchmark reported speedups *falling* as input grew
   (5.93x → 3.84x → 1.76x). That is backwards for a parallel decomposition
   over a fixed worker count, and the inversion is what exposed the flawed
   methodology. Interleaving the arms produced a coherent 3.92x → 4.50x →
   4.93x. Recorded as FAILED_EXPERIMENTS #003; the underlying 2x baseline
   degradation is neutralised but still unexplained.

2. **Counting tokens is not the same claim as preserving tokenization, and the
   weaker test hid a real case.** The original differential tests compared
   counts. Independent review pointed out that two tokenizations can share a
   length. Switching to sequence comparison and re-planting the unsafe rule
   produced a failure on the string `"a;\n/b;\n/c;\n/d\n"`, reported as
   **8 tokens vs 8** — identical count, different tokenization, which the
   count-only assertion had passed. The blind spot was real, not theoretical.

3. **The `/` exclusion in the split rule is load-bearing, and real source is
   what proves it.** `o200k_base`'s punctuation branch ends in `[\r\n/]*`, so
   a token can start at punctuation, swallow the newline, and continue into a
   following slash. That is the shape of every `//` and `///` comment in this
   repository. When the exclusion was removed as a negative control, the first
   failure came from a real source file, not a synthetic string.

4. **A worker reporting success is not evidence that it ran.** An Antigravity
   dispatch returned `"status": "SUCCESS"` with exit code 0, an empty response
   and an untouched worktree; it had been silently denied command permission.
   Recorded as FAILED_EXPERIMENTS #004.

5. **Independent review earned its cost here.** It confirmed soundness, but
   more usefully it corrected a false premise in the written rationale (the
   `cl100k_base` pattern actually compiled by tiktoken-rs 0.6.0 has no
   end-anchored `\s++$` branch; that is upstream Python's spelling) and found
   the count-vs-sequence gap. Both were things the author could not see.

### Next highest-value experiment

Two candidates, in priority order.

1. **Attempted, not started — cut `TokenCounter::new`.** The hypothesis is
   that a
   counter-only loader — encoder map plus one compiled regex, skipping the
   decoder map, the sorted token table and the 256 regex clones — lands
   construction near the ~70–90 ms of required work. Success is defined as:
   counts bit-identical to tiktoken-rs over a real corpus, all tests green,
   and a measured *first-construction-in-a-fresh-process* figure, since a
   cached singleton would make the 2nd..Nth call free and the 1st exactly as
   slow. The unattributed 120–160 ms must be explained, not ignored.

   This was dispatched to two workers and ran on neither, for reasons
   unrelated to the work itself: Antigravity was silently denied command
   permission in headless mode (FAILED_EXPERIMENTS #004), and Codex exhausted
   its usage quota mid-ticket. Both worktrees came back untouched. The ticket
   text is kept at `.orchestration/tickets/construction-cost.md` so the next
   attempt starts from the same brief rather than a re-derived one.

2. **Then stop optimising this layer.** Once construction is addressed, token
   accounting is no longer on the critical path for any realistic input, and
   further work on it has sharply diminishing returns. The next bottleneck is
   Phase 3: there is still no symbol index, so the p50 <50 ms symbol-lookup
   target has never been measured.
