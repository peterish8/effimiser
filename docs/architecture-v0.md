# Architecture v0

Status: **v0, partially implemented.** Sections marked `[built]` exist and are
tested. Sections marked `[planned]` are design intent and may change once
benchmarked. Nothing here should be read as a description of shipped behaviour
unless it says `[built]`.

## 1. Position in the stack

The runtime is a library plus a binary that sits *below* the coding agent.

```
Claude Code │ Codex │ Gemini CLI │ OpenCode │ Cursor │ generic MCP client
─────────────────────────┬──────────────────────────────────────────────
                         │  six stable tools
                ┌────────▼────────┐
                │ Context Runtime │
                │  ┌───────────┐  │
                │  │  Router   │  │   picks the cheapest sufficient method
                │  └─────┬─────┘  │
                │  ┌─────▼─────┐  │
                │  │ Compiler  │  │   budgeted evidence selection
                │  └─────┬─────┘  │
                └────────┼────────┘
     ┌──────────┬────────┼────────┬──────────┬──────────┐
     ▼          ▼        ▼        ▼          ▼          ▼
   code      search    files    shell      tools     memory
  (AST,     (trigram, (smart   (parsers,  (MCP      (typed
   symbols,  BM25,     reads,   capture)   gateway)   facts)
   graph)    semantic) deltas)
     └──────────┴────────┼────────┴──────────┴──────────┘
                         ▼
                 ┌───────────────┐
                 │  local store  │  SQLite + content-addressed blobs
                 └───────────────┘
```

## 2. Context as a cache hierarchy

Formalising the tiers makes the promotion rule explicit.

| Tier | Contents | Size | Promotion rule |
|---|---|---|---|
| **L0** | Active model context | 1–8K tokens | only what the task needs now |
| **L1** | Context packets | ≤4K tokens | compiled per task |
| **L2** | Project knowledge | MBs | symbols, graph, decisions |
| **L3** | Raw session store | GBs | every tool output, byte-exact |
| **L4** | Repository | — | source of truth |

The rule is **promote upward only when useful**, replacing the default
behaviour of *everything flows into the model until compaction*.

## 3. Components

### 3.1 Local store `[built]`

`crates/ctx-store`. SQLite for metadata, content-addressed blobs for raw bytes.

- `blobs` — blake3-addressed raw bytes. Writes are idempotent, so capturing the
  same 40 MB log twice costs one copy. Written to a temp sibling then renamed,
  so a crash cannot leave a truncated blob under a hash that promises full
  content.
- `runs` — one row per captured command; stdout and stderr stored in full.
- `snapshots` — `(path, content_hash)` of every file content the agent has been
  shown. This is what makes "unchanged" answerable without a re-read, and what
  makes a delta computable when the file *has* changed.

Handles are `scheme://path` strings: `output://run/<id>`,
`file-snapshot://<hash>`. They survive process restarts — verified by test.

### 3.2 Token accounting `[built]`

`crates/ctx-core/src/tokens.rs`. Anthropic does not publish the Claude
tokenizer, so exact offline Claude counts are impossible. Rather than guess,
every `TokenCount` records *which* tokenizer produced it, and reports carry
that label. Default is `o200k_base`, used as a documented proxy.

A test asserts that dense-punctuation text counts *higher* than `bytes / 4`,
which locks out the estimator shortcut that would make every later savings
number unfalsifiable.

### 3.3 Retrieval router `[planned]`

Routing is by query shape, and embeddings are deliberately last:

| Query shape | Method | Tier |
|---|---|---|
| exact filename | path index | 0 |
| symbol name | symbol index | 1 |
| "who calls this" | reference graph | 2 |
| code structure | AST / tree-sitter pattern | 3 |
| exact string | trigram | 4 |
| conceptual | BM25 | 5 |
| "what breaks if" | dependency graph | 6 |
| ambiguous concept | semantic (optional) | 7 |
| nothing matched | bounded raw scan | 8 |

`RetrievalMethod::cost_tier()` encodes this ordering, and a test asserts cheap
tiers sort before expensive ones — so "did the router try the cheap thing
first?" is a checkable property, not a claim.

### 3.4 Context packet compiler `[planned]`

The centrepiece. Input: intent, repository state, branch, changed files, token
budget. Output: a bounded packet where every item carries provenance.

Scoring, conceptually:

```
value = relevance × confidence × structural_importance × recency × task_dependency
cost  = estimated_tokens + retrieval_latency + stale_probability
score = value / cost
```

Then: select the highest-scoring evidence subject to `Σ tokens ≤ budget`. This
is a knapsack, and it is the reason the project is described as a *compiler* —
the repository is the source language, the packet is the optimised IR.

**The trap to avoid:** optimising packet size alone. A tiny packet that omits
the one file that mattered is worse than a large one. Therefore packet
benchmarks measure precision, recall, and fallback rate *alongside* size, and a
size win with a recall loss is rejected. See benchmark-plan.md §"Context
quality".

### 3.5 Provenance `[built]`

Every packet item records path, line range, symbol, content hash at retrieval
time, retrieval method, and a raw handle. Without this a packet is an
unfalsifiable claim; with it, any line is traceable and staleness is detectable
by hash comparison.

### 3.6 Smart and delta reads `[planned]`

`context_read` behaves by situation:

| Situation | Response |
|---|---|
| small file | return it |
| huge file | structure + relevant spans |
| already seen, unchanged | `UNCHANGED` + snapshot reference |
| already seen, changed | diff against the seen hash |
| generated / vendored | metadata unless raw explicitly requested |
| log file | grouped errors and warnings |
| binary | metadata only |

The `snapshots` table already supports the unchanged and changed cases.

### 3.7 Shell engine `[partially built]`

`ctx run <cmd>` spawns the child directly (`Command::args`, never a shell, so
arguments cannot be reinterpreted as shell syntax), captures stdout and stderr
in full, and prints a summary plus handle. `ctx output <handle> --grep` reads
the raw bytes back.

Per-command structured parsers (cargo, pytest, npm, go test, tsc, eslint,
docker, git) are `[planned]`. Deterministic parsing comes before any
LLM summarisation.

**Lossy vs lossless must be labelled.** Deduplication, progress-bar stripping,
AST extraction and diff generation are lossless. Natural-language summarisation
is lossy, and lossy output must always carry a raw handle.

### 3.8 MCP gateway `[planned]`

Connect many upstream servers; expose six tools. Keep a local registry of
server, tool, one-line purpose, full schema, permissions, side effects, latency
history and error rate — loading full schemas only when needed.

Critically, there is both a **fast path** and a **discovery path**. A
discovery layer that adds a model turn can *increase* total task tokens even
while shrinking the schema catalogue. So when router confidence exceeds a
threshold, invoke directly; use discovery only for genuinely ambiguous
requests; and measure **total task tokens**, never schema bytes alone.

### 3.9 Impact graph `[planned]`

Relate symbols, files, modules, tests and commands, so that changing
`validate_token()` yields "3 affected suites" instead of a 3,000-test run.
Targeted tests run first; full verification still runs before any release
claim — targeted testing never replaces required final verification.

### 3.10 Project memory `[planned]`

Typed facts, not a chat dump: `decision`, `finding`, `failure`, `dead_end`,
`convention`, `architecture`, `task`, `dependency`, `symbol`, `test`, `change`.
Each carries id, content, source, confidence, timestamps, `superseded_by`, and
typed edges (`depends_on`, `caused_by`, `contradicts`, `tested_by`, …).

Memory retrieval obeys the token budget like everything else. Memories are
**not** injected wholesale at session start.

## 4. The model-facing surface

Six tools, regardless of internal complexity:

```
context_query   compile an evidence packet for an intent
context_read    smart / delta read of a path or symbol
context_edit    symbol- or diff-level edit, transactional
context_exec    run a command, capture fully, return a summary
context_tool    route to an upstream MCP tool
context_memory  read and write typed project facts
```

Internally there may be hundreds of implementations. The lesson taken from
mini-swe-agent is that elaborate agent interfaces are not automatically
better — a deliberately small surface performs strongly, so surface size is a
design constraint rather than an afterthought.

## 5. Crate layout

```
crates/
  ctx-core      [built]   handles, provenance, token accounting
  ctx-store     [built]   SQLite + blobs, runs, snapshots
  ctx-cli       [built]   the `ctx` binary
  ctx-code      [planned] tree-sitter index, symbols, graph
  ctx-search    [planned] trigram, BM25, AST patterns
  ctx-compiler  [planned] packet scoring and packing
  ctx-shell     [planned] per-command output parsers
  ctx-tools     [planned] MCP registry and gateway
  ctx-memory    [planned] typed project facts
  ctx-bench     [planned] benchmark harness
```

Crates are added when implemented, not scaffolded empty in advance.

## 6. Why Rust

One binary, no Node or Python needed at runtime, and incremental parsing via
tree-sitter is available natively. Cold start matters because the runtime is on
the critical path of every agent tool call.

## 7. Security posture

The runtime sits between a capable agent and real tools, so it is a control
point and must behave like one: secret-file exclusion, `.gitignore` awareness,
command permission policy, MCP allow/deny lists, tool-schema integrity hashes,
defence against malicious tool descriptions, path-traversal and project-root
boundaries, audit logs, and an optional read-only mode. Repository source is
never sent to an external service unless the user explicitly enables that
provider.

## 8. Open questions

1. Does the packet compiler beat plain symbol lookup often enough to justify
   its complexity? Unknown until benchmarked.
2. What budget maximises task success? 4K is an initial guess, not a finding.
3. Does MCP discovery reduce *total* tokens, or only schema tokens?
4. Is SQLite FTS5 sufficient for BM25, or is a native index needed?
5. How much does tree-sitter indexing cost on a large monorepo, cold?

Each is a benchmark, not an argument to settle in review.
