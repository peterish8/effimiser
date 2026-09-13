# Worker ticket: reduce TokenCounter construction cost

You are a bounded implementation worker. The lead (Claude) owns architecture,
integration, and the final completion claim. Do the work described here and
nothing else.

## Context

Repository: Effimiser, a "Context Runtime" for AI coding agents. Rust workspace.
Governing rule for this project: benchmark numbers are never estimated,
massaged, or cherry-picked. Honest measurement outranks a good-looking result.

`crates/ctx-core/src/tokens.rs` defines `TokenCounter`, which counts tokens with
a real BPE (tiktoken-rs 0.6, `o200k_base` by default). It must never report a
`bytes / 4` style estimate.

## The measured problem

`TokenCounter::new()` costs roughly 350 ms, paid on every CLI process start.
A profiling probe attributed the stages (single runs, ms, 3 repetitions):

    whole o200k_base()     313.2 / 376.7 / 397.8
    decode+build encoder    61.8 /  65.8 /  79.5   REQUIRED
    compile regex            6.9 /   8.2 /   9.2   REQUIRED
    regex clones (128 x2)   10.5 /  17.5 /  14.9   avoidable
    build decoder map       29.5 /  31.7 /  38.2   avoidable
    sorted_token_bytes      87.9 /  80.5 /  93.4   avoidable

Read `tiktoken-rs` 0.6.0 source to confirm: `CoreBPE::new` (in
`src/patched_tiktoken.rs`) builds a 200k-entry `decoder` map, clones and sorts
all 200k token byte strings into `sorted_token_bytes`, and clones the compiled
fancy-regex 128 times for each of two regexes. A counter that only ever calls
`encode_ordinary` needs none of that.

Note ~120-160 ms of the total is NOT attributed by the probe above. Find out
what it is before assuming the avoidable stages are the whole story.

## Your task

Make `TokenCounter::new` faster. The likely approach is a counter-only BPE
loader that builds just the encoder map and one compiled regex, vendoring the
minimal encode path from tiktoken-rs. You are free to find a better approach.

## Hard constraints

1. COUNTS MUST NOT CHANGE. Every count your implementation produces must be
   bit-identical to `tiktoken_rs::o200k_base().encode_ordinary(text).len()` and
   the cl100k_base equivalent. Prove it with a differential test that encodes
   real corpus text (walk the repository's own .rs/.md/.toml files) plus
   adversarial strings, comparing your path against tiktoken-rs directly.
   A faster wrong answer is a failed ticket.

2. DO NOT MODIFY these, they are another worker's territory:
   - `safe_split_points`, `next_boundary_at_or_after`, and the chunking logic
     inside `count()` — the parallel split rule is under separate review;
   - the public API/signatures of `TokenCounter`, `TokenCount`, `Tokenizer`;
   - anything under `benchmarks/results/`, `docs/`, or `.orchestration/`.
   You may change the body of `TokenCounter::new` and add new modules/files.

3. A process-wide cached/lazy-static BPE is NOT an acceptable answer on its
   own. The cost that matters is the FIRST construction in a fresh process
   (CLI cold start); a singleton makes the 2nd..Nth construction free and the
   1st exactly as slow. If you add caching, it must be in addition to a real
   reduction, and you must report both numbers separately.

4. If you vendor or adapt any code from tiktoken / tiktoken-rs (both MIT), add
   proper attribution to the existing `NOTICE` file at the repository root.
   Check `docs/research/LICENSE_MATRIX.md` for this project's conventions.

5. `cargo test --workspace` must pass. Do not delete, weaken, skip, or
   `#[ignore]` any existing test to make it pass.

6. Do not add a mandatory network/cloud dependency. Offline operation is a
   project requirement.

## How to measure

Measure the FIRST construction in a FRESH process, several times, and report
min / median / max with the number of repetitions. Do not report only your best
run. A convenient existing harness is `crates/ctx-bench` (see
`tokens.counter_construct` in `crates/ctx-bench/src/main.rs`), but a small
standalone binary that runs construction once and exits is more honest for cold
start; state which you used.

If your change does not actually help, say so with the numbers. A measured
negative result is a successful ticket, not a failure. Do not manufacture an
improvement.

## Deliverable

Report in this structure:

RESULT: IMPROVED | NO_CHANGE | REGRESSED
BEFORE: <min/median/max ms, n=?, how measured>
AFTER:  <min/median/max ms, n=?, how measured>
WHAT_CHANGED: <the mechanism, and what the unattributed 120-160 ms turned out to be>
CORRECTNESS_EVIDENCE: <how you proved counts are identical; test names; corpus size>
TESTS: <cargo test --workspace output summary>
LICENSE: <attribution added, or "no vendored code">
RISKS: <what you are unsure about>

Leave your work committed on the current branch in your working directory.
