# Benchmark plan

This document is the project's contract with itself. Its purpose is to make
optimisation claims falsifiable, and to make gaming them inconvenient.

**Standing rule: no percentage appears in any README, commit message, or
release note unless a reproducible artifact under `benchmarks/results/`
supports that exact number.**

## 1. What counts as a valid result

A benchmark result is valid only when all of the following hold:

1. the expected code change is correct;
2. the repository still builds;
3. relevant tests pass;
4. no hidden regression was introduced;
5. retrieved evidence was sufficient for the task;
6. raw information remains recoverable where promised.

A run that saves 80% of tokens and fails the task is recorded as a **failure**.
It is not a token-savings win with an asterisk.

## 2. Baseline first

The baseline is established *before* the optimisation exists, never after.

```
Baseline:   agent + repository + its normal tools
Treatment:  same agent, same model, same effort, same repo commit,
            same task, same environment, runtime enabled
```

Only the runtime changes. Explicitly forbidden comparisons:

- different models (GPT vs Claude, Opus vs Sonnet)
- different reasoning effort or thinking budgets
- different repository states or commits
- an intentionally weakened baseline

## 3. Anti-gaming rules

Benchmark numbers must not be improved by any of:

removing hard cases · shortening inputs · skipping failed runs · reporting only
the best run · changing the benchmark after seeing results (unless the
benchmark is genuinely invalid) · excluding cold-start cost without labelling
it · excluding indexing time when indexing is part of the user experience ·
pretending local CPU is free · hardcoding answers · caching expected outputs
for test cases · truncating context until accuracy drops · hiding failed tool
calls · ignoring retries or timeouts · counting compressed **bytes** as saved
**tokens** · inventing API prices.

Two of these have code-level defences already:

- **Token counts** come from a real BPE (`o200k_base`), and a unit test asserts
  that dense-punctuation text counts higher than `bytes / 4`. The estimator
  shortcut cannot silently return.
- **Every count is labelled** with its tokenizer, and the label records that it
  is a proxy for Claude rather than Claude's own tokenizer.

If provider pricing is unknown, report **tokens**, not guessed money.

## 4. Reproducibility record

Every run records: git commit · runtime version · OS · CPU · RAM · repository ·
task · model · effort setting · context window · agent version · runtime
configuration · cold or warm · index state · repetitions · input tokens ·
output tokens · cached tokens · tool calls · **failed** tool calls · retries ·
file reads · **repeated** file reads · bytes of raw tool output · bytes and
tokens exposed to the model · retrieval latency · wall-clock time · task
success · test results.

Stored as JSONL under `benchmarks/results/`. Raw run artifacts are retained —
a summarised result with the raw discarded is not a result.

## 5. Repetitions and statistics

- Deterministic microbenchmarks: enough repetitions for stable measurement.
- Expensive agent benchmarks: at least 3 comparable runs where practical.
- Report median, and preferably p50 / p95 / min / max.
- **Never** report only the fastest run.

## 6. Targets

Engineering goals to validate, **not** claims to publish.

| # | Metric | Target |
|---|---|---|
| 1 | Context packet, focused task | ≤4,000 tokens (stretch 2,500) |
| 2 | Model-facing tools | ≤6 |
| 3 | Warm symbol lookup | p50 <50 ms, p95 <150 ms |
| 4 | Warm multi-source context query | p50 <300 ms (early: <500 ms) |
| 5 | Unchanged files reparsed on 1-file edit | 0 |
| 6 | Repeated full-file reads | ~0 |
| 7 | Terminal output injected into context | 60–90% reduction |
| 8 | Tool round trips, navigation tasks | 30–60% fewer |
| 9 | Irrelevant MCP schema hidden | >80%, *with total task tokens down* |
| 10 | Targeted test verification time | materially lower on large projects |
| 11 | Raw recoverability | 100% |
| 12 | Mandatory cloud services | 0 |
| 13 | Correctness regression vs baseline | none statistically meaningful |

Target 1 never justifies cutting required evidence. Target 8 is not met by
returning one 50,000-token tool result — the metric that matters is *useful
evidence per round trip*. Target 9 is not met by schema reduction alone.

## 7. Suites

| Suite | Content | Primary metrics |
|---|---|---|
| **A. Code navigation** | find definition, find references, understand a feature, trace route→DB, locate tests | tool calls, tokens, latency, accuracy |
| **B. Large files** | 500 / 2,000 / 10,000-line and generated files; first read, unchanged re-read, modified read, symbol read, range read | tokens, bytes, latency, correctness |
| **C. Terminal output** | large test output, compiler errors, docker logs, repeated stack traces, lint warnings | errors preserved, noise removed, raw recoverable |
| **D. MCP catalog** | 10 / 50 / 100 / 300 / 500 tools, eager vs virtualised | startup context, **total** task tokens, tool-selection accuracy |
| **E. Project memory** | session 1 discovers a decision, session 2 continues | rediscovery cost vs retrieval; supersession works |
| **F. Incremental indexing** | 1 file / 10 files / branch switch / mass refactor | files reparsed, symbols reindexed, latency, memory |
| **G. Full coding tasks** | real bugs and features in OSS repos with test suites | task solved, tests pass, tokens, tool calls, time |

Suite G uses a SWE-bench-style harness rather than only synthetic tasks, so
results are comparable to external work.

## 8. Context quality — not just packet size

For every packet benchmark, record alongside size:

- Did the packet contain the file that mattered?
- Did it contain the relevant symbol?
- Did it contain the relevant test?
- Did it contain misleading **stale** information?
- Did the model need an emergency fallback search?

Tracked as **precision**, **recall**, **fallback rate**, **packet size**.
Optimising size alone is prohibited; a size win with a recall loss is rejected.

## 9. Failure analysis loop

When a benchmark misses target, classify before touching code:

`RETRIEVAL_FAILURE` · `RANKING_FAILURE` · `INDEX_FAILURE` · `CACHE_MISS` ·
`TOOL_ROUTING_FAILURE` · `TOOL_SCHEMA_OVERHEAD` · `EXCESSIVE_TOOL_CALLS` ·
`EXCESSIVE_CONTEXT` · `MISSING_CONTEXT` · `STALE_CONTEXT` · `SLOW_PARSE` ·
`SLOW_DATABASE` · `SLOW_SERIALIZATION` · `BAD_COMPRESSION` ·
`CORRECTNESS_REGRESSION` · `MEMORY_REGRESSION` · `INTEGRATION_LIMITATION`

Write a root-cause note, then fix the dominant cause. Profile before
optimising; do not rewrite systems based on guesses.

## 10. Regression budget

A change may improve one metric and harm another. Evaluate at minimum:
correctness, tokens, tool calls, latency, memory, startup cost, complexity.

A 40% token reduction that raises latency 800% is **rejected** absent a
documented compelling reason.

## 11. Gates

**Merge gate** for a major optimisation: all unit tests pass · all integration
tests pass · no known correctness regression · relevant benchmark improves or
is neutral · no severe latency regression · no raw-evidence loss · no
license or security regression.

**Release gate** before calling anything production ready: core tests pass ·
benchmark suite passes · security tests pass · fresh install works · uninstall
works · at least two agent integrations work · large-repo indexing works ·
recovery handles work · results reproducible · documented limitations accurate.

## 12. Stop conditions

Do not loop forever on a target. Stop when any of these is true, and document
which:

- **A.** target reached reliably with no meaningful regression;
- **B.** sharply diminishing returns;
- **C.** remaining cost is in an external component we do not control;
- **D.** reaching it would harm correctness or usability;
- **E.** measurements show the target was unrealistic.

For B–E, record: current result, target, reason for stopping, evidence,
recommended future work. This is good engineering, not failure.

## 13. Priority order

1. correctness → 2. task success → 3. evidence quality → 4. fewer round trips →
5. fewer model-visible tokens → 6. latency → 7. resource efficiency →
8. architectural elegance.

Never reversed to make numbers look better.

## 14. Claim language

| Not allowed | Allowed |
|---|---|
| "10x faster" | "warm symbol lookup moved from 182 ms to 41 ms median over 100 runs" |
| "90% cheaper" | "model-visible output fell 84% on the log-heavy suite with all expected error groups still recoverable" |

The words *revolutionary*, *10x*, *industry-leading*, *cheapest* and *fastest*
are banned unless a current artifact supports the exact claim.

## 15. Failed experiments are kept

`docs/benchmarks/FAILED_EXPERIMENTS.md` records ideas that did not work, with
measurements. Negative results are not quietly deleted — knowing that
semantic-search-first costs latency without accuracy gain is worth as much as
knowing what did work.
