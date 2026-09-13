# Context Runtime

A local-first runtime that sits between AI coding agents and the resources they
touch — source, files, shell, tests, MCP servers, git, project memory — and
returns the smallest high-quality evidence set needed for the task, while
keeping **100% of the raw material recoverable** behind stable handles.

Not an agent. Not a RAG package. Not a bag of fifty MCP tools.

> **Status: early. Phase 1 of 14.** The foundation is built and tested. The
> context compiler, code index, MCP gateway and agent integrations are not.
>
> This README contains **no token-savings, tool-call or context-size
> percentages**, because the suites that would produce them need a model in the
> loop and have not been run. The deterministic microbenchmarks *have* run —
> token counting, store I/O, and end-to-end `ctx run` latency — and their raw
> artifacts are under [`benchmarks/results/`](benchmarks/results/), with the
> reasoning and the negative results in
> [LOOP_LOG.md](docs/benchmarks/LOOP_LOG.md) and
> [FAILED_EXPERIMENTS.md](docs/benchmarks/FAILED_EXPERIMENTS.md). Those measure
> the plumbing, not the product claim. See
> [docs/benchmark-plan.md](docs/benchmark-plan.md) for why that distinction
> matters here.

## The idea

A tool that produces 500 KB does not mean the model should receive 500 KB.
Store the output in full locally; give the model a small representation plus a
recovery handle:

```
$ ctx run cargo tree
$ cargo tree
exit 0 in 555 ms
captured 6718 bytes of output (2599 o200k_base tokens) — not shown
raw: output://run/a0b2ee373e8d

$ ctx output output://run/a0b2ee373e8d --grep tiktoken
stdout:59: │   ├── tiktoken-rs v0.6.0
```

The full 6,718 bytes are still there, byte-exact, forever. Compression is
progressive disclosure, never data loss — if a handle cannot be minted, the
compression does not happen.

## What is built

| Component | State | What it does |
|---|---|---|
| `ctx-core` | working | handles, provenance, token accounting against a real BPE |
| `ctx-store` | working | SQLite metadata + content-addressed blobs, run capture, file snapshots |
| `ctx-cli` | working | `ctx init`, `ctx stats`, `ctx run`, `ctx output` |
| `ctx-bench` | working | deterministic microbenchmarks with interleaved A/B arms |

49 tests, all passing. Verified: captured output recovers byte-identically,
handles survive process restarts, identical output from two runs stores one
copy, and an unchanged file is detectable without re-reading it.

Token counting is parallelised across threads at split points where the
tokenizer provably cannot emit a token spanning the split; the equivalence is
asserted on full token sequences, not counts, over this repository's own
sources against both vocabularies, and was validated by planting an unsafe
split rule and confirming the tests fail.

## What is not built

The context packet compiler, tree-sitter code index, retrieval router, smart
and delta reads, per-command shell parsers, MCP schema virtualisation,
symbol-level editing, impact graph, typed project memory, and every agent
integration. See [docs/implementation-roadmap.md](docs/implementation-roadmap.md).

## Try it

```bash
cargo build --release
./target/release/ctx init
./target/release/ctx run <your command>
./target/release/ctx output <the handle it printed> --grep error
```

Requires Rust 1.80+. No network, no API keys, no cloud services — by design.

## Design commitments

These are constraints, not aspirations, and each one is testable.

- **Correctness outranks token savings.** A run that uses 80% fewer tokens and
  fails the task is recorded as a failure, not a qualified win.
- **Raw evidence is always recoverable.** Every lossy transform carries a
  handle, or it is rejected.
- **Cheap retrieval before expensive.** Exact path → symbol → graph → AST →
  lexical → semantic. Embeddings are the fallback, never the foundation.
- **Zero mandatory cloud services.** No hosted vector DB, no Redis, no Postgres,
  no embedding API.
- **Token numbers come from a tokenizer.** Never `bytes / 4`. A unit test
  enforces this, and every count is labelled with the tokenizer that produced
  it — currently `o200k_base`, a documented proxy, since Claude's own tokenizer
  is not public.
- **No claim without an artifact.** No percentage ships unless a reproducible
  run under `benchmarks/results/` supports that exact number.

## Documentation

| Document | Contents |
|---|---|
| [vision.md](docs/vision.md) | the problem, the thesis, what makes it defensible |
| [architecture-v0.md](docs/architecture-v0.md) | components, marked `[built]` or `[planned]` |
| [benchmark-plan.md](docs/benchmark-plan.md) | targets, anti-gaming rules, suites, gates |
| [implementation-roadmap.md](docs/implementation-roadmap.md) | 14 phases with exit criteria |
| [research/LICENSE_MATRIX.md](docs/research/LICENSE_MATRIX.md) | compliance record for 17 reference projects |
| [benchmarks/FAILED_EXPERIMENTS.md](docs/benchmarks/FAILED_EXPERIMENTS.md) | what did not work, with measurements |

## Provenance

This project studied 17 open-source reference implementations and borrowed
architectural ideas. **No source code was copied from any of them.** Four are
blocked from reuse outright on license grounds. Details in
[LICENSE_MATRIX.md](docs/research/LICENSE_MATRIX.md).

Apache-2.0. See [LICENSE](LICENSE) and [NOTICE](NOTICE).
