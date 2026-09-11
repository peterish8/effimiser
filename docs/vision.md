# Vision

## The problem

An AI coding agent spends most of its context budget on material it did not need.
A single "fix the refresh-token bug" task typically costs ten to twenty model↔tool
round trips: grep, read, grep again, read a test, `git log`, `git diff`, read
`CLAUDE.md`, run the suite, and then absorb three thousand lines of test output —
of which four lines mattered.

Three costs compound:

1. **Tokens.** Raw tool output enters the context window verbatim.
2. **Round trips.** Each retrieval step is a full model turn.
3. **Rediscovery.** The next session relearns everything the last one learned.

Existing tools each attack one slice. Shell-output filters compress `pytest`
dumps. Semantic search indexes narrow what gets read. Output-style skills shorten
replies. Tool proxies hide MCP schemas. Each helps; none composes, and several
actively conflict when installed together.

## The thesis

The agent should not have to know which optimisation applies. A layer beneath it
should choose the cheapest reliable way to obtain useful evidence, and should
never destroy anything it chose not to show.

> **Context Runtime** is a local-first runtime that sits between AI coding agents
> and the resources they touch — source, files, shell, tests, MCP servers, git,
> project memory — and returns the smallest high-quality evidence set needed for
> the task at hand, while keeping 100% of the raw material recoverable behind
> stable handles.

It is not an agent. It is not a RAG package. It is not a bag of fifty MCP tools.

## The central separation

**Data execution is not context injection.**

A command that produces 500 KB does not mean the model should receive 500 KB.
Store the full output locally; give the model an optimised representation plus a
recovery handle:

```
Tests: 922   Passed: 918   Failed: 4

2 unique root failures:
  1. auth timeout        x3
  2. snapshot mismatch   x1

raw: output://run/abc123
```

The model can query that handle for anything the summary dropped. Compression is
**progressive disclosure**, never data loss. If a handle cannot be minted, the
compression does not happen.

## What makes this defensible

Any single piece here already exists somewhere. The combination does not:

- a **context packet compiler** that solves a token-budgeted selection problem
  over multi-source evidence,
- a **code graph** for symbol, reference and impact queries,
- a **lossless raw store** that makes aggressive compression safe,
- **stateful reads** that never retransmit an unchanged file,
- **tool virtualisation** that keeps the model-facing surface at six tools
  regardless of how many MCP servers are connected,
- **impact-aware execution** that runs the three affected test suites before the
  three thousand,
- **cross-agent project intelligence** that survives session boundaries,

all behind an interface small enough that the agent never learns it exists.

## Design commitments

These are constraints, not aspirations. Each is testable.

| Commitment | Consequence |
|---|---|
| Correctness outranks token savings | An 80%-cheaper run that fails the task is a **failure**, not a win |
| Raw evidence is always recoverable | Every lossy transform carries a handle, or is rejected |
| Cheap retrieval before expensive | Exact path → symbol → graph → AST → lexical → semantic |
| Embeddings are optional | The product must be strong with **zero** external API keys |
| No mandatory cloud services | No hosted vector DB, no Redis, no Postgres |
| Nothing is re-indexed unnecessarily | Unchanged files reparsed: zero |
| Token numbers come from a tokenizer | Never `bytes / 4`; the tokenizer is named in every report |
| Claims come from artifacts | No percentage ships without a reproducible run behind it |

## What "done" feels like

A developer installs once, keeps using their preferred agent, and that agent
quietly receives better context while wasting fewer turns and fewer tokens. The
runtime is invisible. The only visible thing is that work goes faster and the
agent stops asking to read the same file for the fourth time.

## Non-goals

- Becoming another coding agent, or competing with Claude Code / Codex / Cursor.
- Summarising aggressively enough to harm task success.
- Controlling prompt caching inside closed clients we do not proxy. Tool-layer
  optimisation is universal; model-layer cache shaping applies only where the
  runtime owns the provider request. We will not claim otherwise.

See [architecture-v0.md](architecture-v0.md) for the structure, and
[benchmark-plan.md](benchmark-plan.md) for how every claim above gets tested.
