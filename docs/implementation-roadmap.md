# Implementation roadmap

Ordering principle: **a measurement exists before the thing it measures is
optimised.** Each phase ends with tests green and, where the phase claims an
improvement, a benchmark artifact under `benchmarks/results/`.

Phases are sequential because the loop depends on causal understanding. Do not
rewrite indexer, router and storage simultaneously — isolate one bottleneck,
optimise, verify, commit, move on.

## Status

| Phase | Deliverable | State |
|---|---|---|
| 0 | Reference research, license matrix, feature matrix | **done** |
| 1 | Rust CLI, SQLite + raw store, token accounting | **done** |
| 2 | Benchmark harness + vanilla baselines | next |
| 3 | Incremental tree-sitter index, symbols, lexical search | planned |
| 4 | Context packet compiler + budgeted ranking | planned |
| 5 | Smart reads, hash snapshots, delta reads | planned |
| 6 | Shell capture, per-command parsers, raw querying | planned |
| 7 | MCP registry, schema virtualisation, discovery | planned |
| 8 | Symbol-aware transactional editing | planned |
| 9 | Impact graph + targeted testing | planned |
| 10 | Typed project memory | planned |
| 11 | Agent integrations | planned |
| 12 | Optional embeddings and reranking | planned |
| 13 | Provider prompt-cache adapters | planned |
| 14 | Full benchmarks, profiling, security review | planned |

## Phase 0 — Research `done`

- `docs/research/LICENSE_MATRIX.md` — 17 repos, four reuse blockers identified
  (context-mode ELv2; caveman MIT/BSL ambiguity; probe LICENSE/Cargo.toml
  conflict; dynamic-discovery-mcp missing LICENSE file).
- `docs/research/REFERENCE_MATRIX.md`, `FEATURE_MATRIX.md` — per-repo analysis
  and capability gaps.

Outcome that shaped the design: the four blockers mean the core is a clean-room
implementation, not a merge. Architectural ideas are borrowed; source is not.

## Phase 1 — Foundation `done`

Built and tested (26 tests):

- `ctx-core` — handles, provenance with retrieval-cost tiers, token accounting
  against a real BPE.
- `ctx-store` — SQLite metadata, content-addressed blobs, run capture,
  file snapshots.
- `ctx-cli` — `ctx init`, `ctx stats`, `ctx run`, `ctx output`.

Verified end to end: `ctx run cargo tree` captured 6,718 bytes / 2,599
`o200k_base` tokens, printed a four-line summary plus a handle, and
`ctx output <handle>` returned the bytes **identically**.

No performance claim is attached to this yet — one measured example is an
existence proof, not a benchmark. Phase 2 turns it into one.

## Phase 2 — Benchmark harness `next`

The gate for everything after it. Deliverables:

1. Harness that runs a task against baseline and treatment under identical
   model, effort, repo commit and environment.
2. JSONL result schema covering every field in benchmark-plan.md §4.
3. Vanilla baselines for suites A (navigation), B (large files) and
   C (terminal output).
4. Microbenchmarks for store write, blob read, handle resolution and token
   counting — these are deterministic and can run on every commit.

Exit criteria: baselines recorded with p50/p95 over ≥3 runs; a deliberately
broken treatment is correctly reported as a regression rather than a win.

## Phase 3 — Code intelligence

Incremental tree-sitter indexing; symbols, imports, exports, references where
obtainable; source↔test relationships. Fallback chain is explicit:
**SCIP/LSP → tree-sitter → lexical → grep.** Running the core must never
require a language server.

Exit criteria: one-file edit reparses exactly one file (target 5 in
benchmark-plan.md); warm symbol lookup p50 <50 ms on a medium repo, with
scaling behaviour reported honestly if not.

## Phase 4 — Context packet compiler

Scoring, ranking, packing under budget, provenance on every item.

Exit criteria: packet ≤4K tokens **and** precision / recall / fallback-rate
recorded. A size win with a recall loss is rejected. If the compiler does not
beat plain symbol lookup often enough to justify its complexity, that finding
is published in FAILED_EXPERIMENTS.md and the design is cut back.

## Phase 5 — Smart and delta reads

Unchanged re-read returns a marker; changed file returns a diff against the
hash the agent last saw. The `snapshots` table already supports both.

Exit criteria: repeated full-file reads ~0 across suite B, with no case where
the agent lacked content it needed.

## Phase 6 — Shell engine

Deterministic per-command parsers before any LLM summarisation. Every lossy
transform labelled and carrying a raw handle.

Exit criteria: 60–90% reduction on suite C with **all** expected error groups
still recoverable from the handle.

## Phase 7 — MCP gateway

Registry, lazy schema loading, and both a fast path and a discovery path.

Exit criteria: >80% of irrelevant schema hidden **and total task tokens down**.
If discovery adds enough turns to raise total tokens, routing is fixed or
discovery is dropped. Schema reduction alone is not a result.

## Phase 8 — Edit transactions

Precondition hash → patch → parse → format → syntax check → targeted lint →
targeted tests → rollback on structural failure. Prefer symbol and diff edits
over whole-file rewrites.

Exit criteria: a syntactically broken edit never reaches the working tree.

## Phase 9 — Impact graph

Symbol/file/module/test relationships; affected-test selection.

Exit criteria: materially lower verification time on a large project, with
targeted runs never substituted for required final verification.

## Phase 10 — Project memory

Typed facts with confidence, supersession and typed edges. Budget-obeying
retrieval; no wholesale injection at session start.

Exit criteria: suite E shows lower rediscovery cost in session 2, and a
superseded decision is not resurfaced as current.

## Phase 11 — Integrations

Priority: Claude Code → Codex → Gemini CLI → OpenCode → generic MCP. For each,
document MCP support, hooks, shell interception, session and compaction hooks,
config format, and **limitations**. Integrations do not pretend to capabilities
a client does not expose.

`ctx init` must be reversible; `ctx uninstall` must cleanly remove everything
it added.

## Phase 12 — Optional semantics

Embeddings and reranking only after deterministic retrieval is benchmarked, so
their marginal value is measurable rather than assumed.

## Phase 13 — Prompt-cache adapters

Only where the runtime owns the provider request. Stable prefix first,
semi-stable task state next, dynamic content last. We do not claim cache
control inside closed clients we do not proxy.

## Phase 14 — Hardening

Full benchmark suite, profiling, security review against architecture-v0.md §7,
fresh-install and uninstall verification, and an accuracy pass over every
documented limitation.

## Per-phase loop

Every phase runs the same cycle:

```
inspect → find the bottleneck → baseline → tests first → smallest change
→ unit + integration + regression + benchmark → compare → fix correctness
→ profile → refactor only with evidence → benchmark again → keep or revert
→ record → next bottleneck
```

Closing each phase with a short self-critique: what improved (with numbers),
what got worse (with numbers), what is still bottlenecked (with evidence), what
was learned, and the single next highest-value experiment — stated concretely,
not as "optimise performance further".
