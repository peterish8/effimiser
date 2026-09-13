# Reference matrix

## aider

**Problem solved**

Builds a compact repository map so an agent can navigate relevant definitions without reading every file. The implementation is in `aider/repomap.py`.

**Architecture**

`aider/repomap.py` (867 lines) extracts tags with Tree-sitter `.scm` queries, ranks them, and renders them with `to_tree`; entry points include `get_repo_map`, `get_ranked_tags`, and `get_ranked_tags_map`.

**Strongest implementation idea**

Ranks a `networkx.MultiDiGraph` with `networkx.pagerank` and a query-derived `personalization` vector, then renders only the selected tags.

**Weak points**

The map depends on language-specific query files and on tags being extracted correctly; unsupported or failed parsing falls back to less structured behavior in the implementation.

**Performance strategy**

Tags are cached on disk by file mtime; map and tree results also have in-process caches. This avoids re-parsing unchanged files and repeated rendering.

**Key data structures**

The central structures are tag records, a `MultiDiGraph`, personalization scores, cache dictionaries, and tree rendering inputs.

**Indexing strategy**

`get_scm_fname(lang)` resolves the language query file; queries live under `aider/queries/tree-sitter-language-pack/` and `aider/queries/tree-sitter-languages/`.

**Agent integration strategy**

Python library and CLI integration through Aider’s repository/model workflow; no MCP server is exposed by the files read.

**Token-saving technique**

Return ranked definitions and signatures in a tree-shaped repo map instead of whole-file contents.

**Tool-call-saving technique**

One `get_repo_map` result replaces repeated file listing, search, and definition reads for navigation.

**Primary language + notable dependencies**

Apache-2.0; Python; `pyproject.toml` declares a dynamically loaded dependency set, and `repomap.py` imports `diskcache`, `networkx`, and Tree-sitter-related helpers.

**What we should adopt**

The query-personalized graph ranking, mtime-keyed cache, and compact tree renderer.

**What we should NOT adopt**

Do not make the runtime’s core correctness depend on one language-query pack or on a heavyweight graph library for every request.

**Evidence**

- `aclones/aider/LICENSE.txt`
- `aclones/aider/pyproject.toml`
- `aclones/aider/aider/repomap.py`
- `aclones/aider/aider/repo.py`
- `aclones/aider/aider/run_cmd.py`
- `aclones/aider/aider/queries/tree-sitter-languages/python-tags.scm`

## serena

**Problem solved**

Provides semantic code retrieval and editing through language-server capabilities, plus project memories.

**Architecture**

Tools are registered by `src/serena/tools/tools_base.py`; `src/serena/tools/` includes `FindSymbolTool`, `FindReferencingSymbolsTool`, `GetSymbolsOverviewTool`, `ReplaceSymbolBodyTool`, `InsertAfterSymbolTool`, and `SearchForPatternTool`, while file operations are in `file_tools.py`, memory operations in `memory_tools.py`, and MCP wiring in `src/serena/mcp.py`.

**Strongest implementation idea**

Use LSP symbol locations and references as the semantic boundary: `FindSymbolTool`, `FindReferencingSymbolsTool`, `GetSymbolsOverviewTool`, and symbol editing tools return or mutate bounded symbols rather than files.

**Weak points**

Results depend on a configured language server; `tools_base.py` asserts that LSP-backed symbol retrieval requires an LSP language backend.

**Performance strategy**

Long-lived language-server processes and per-project managers avoid reparsing on each tool call; exact scheduling and cache behavior beyond these modules is unverified.

**Key data structures**

LSP `UnifiedSymbolInformation`, `LanguageServerSymbolLocation`, symbol name paths, registered-tool records, and Markdown memory files.

**Indexing strategy**

Delegates symbol indexing and reference resolution to language servers; `symbol.py` wraps the returned symbol tree and matches name-path suffixes.

**Agent integration strategy**

MCP server using FastMCP, with tool registry metadata and process-isolated agent/server setup in `src/serena/mcp.py`.

**Token-saving technique**

Fetch symbol overviews, definitions, references, or bodies directly instead of reading full files.

**Tool-call-saving technique**

One semantic tool call can return a symbol or its references; optional tool markers allow the exposed surface to be reduced.

**Primary language + notable dependencies**

MIT; Python; FastMCP/MCP, LSP support, Pydantic-style configuration, and language-server adapters.

**What we should adopt**

The symbol-level API, explicit tool registry, diagnostics around edits, and project/global memory naming rules.

**What we should NOT adopt**

Do not require a language server for every language or make tool availability silently depend on a running IDE backend.

**Evidence**

- `aclones/serena/LICENSE`
- `aclones/serena/pyproject.toml`
- `aclones/serena/src/serena/mcp.py`
- `aclones/serena/src/serena/tools/tools_base.py`
- `aclones/serena/src/serena/tools/symbol_tools.py`
- `aclones/serena/src/serena/tools/file_tools.py`
- `aclones/serena/src/serena/tools/memory_tools.py`
- `aclones/serena/src/serena/symbol.py`

## ast-grep

**Problem solved**

Searches and rewrites source by syntax-tree structure rather than text spelling.

**Architecture**

The Rust workspace contains `core`, `config`, `language`, `cli`, `lsp`, `dynamic`, `napi`, `pyo3`, `wasm`, and `outline` crates. Core matching and edits are in `crates/core/src/node.rs`, `replacer.rs`, and `replacer/structural.rs`.

**Strongest implementation idea**

Patterns match Tree-sitter nodes with meta-variable environments; structural replacement collects edits in post-order and merges them into document edits.

**Weak points**

Pattern behavior is language- and grammar-dependent; the files read do not establish whole-repository indexing, call-graph construction, or persistent result storage.

**Performance strategy**

Potential node kinds are precomputed for compound matchers, and traversal exposes `find_all`, `dfs`, ancestors, and field access on wrapped nodes.

**Key data structures**

Generic `Root<D>`, `Node<'r, D>`, `NodeMatch`, `MetaVarEnv`, matcher combinators, and `Edit<D>`.

**Indexing strategy**

Per-document parsing and tree traversal; no persistent cross-file index was verified.

**Agent integration strategy**

Rust library plus CLI, LSP, dynamic, N-API, PyO3, WASM, and outline surfaces from the workspace manifests.

**Token-saving technique**

Return matching syntax nodes or outlines rather than unrelated source text.

**Tool-call-saving technique**

Structural search and replacement combine discovery and edit intent in one operation.

**Primary language + notable dependencies**

MIT; Rust; Tree-sitter and workspace crates for language, CLI, LSP, dynamic, N-API, PyO3, WASM, and outline support.

**What we should adopt**

The generic node wrapper, meta-variable environment, post-order edit collection, and explicit edit ranges.

**What we should NOT adopt**

Do not treat a syntax pattern match as proof of semantic correctness across types, generated code, or unresolved names.

**Evidence**

- `aclones/ast-grep/LICENSE`
- `aclones/ast-grep/Cargo.toml`
- `aclones/ast-grep/crates/core/src/node.rs`
- `aclones/ast-grep/crates/core/src/replacer.rs`
- `aclones/ast-grep/crates/core/src/replacer/structural.rs`
- `aclones/ast-grep/crates/core/src/ops.rs`

## tree-sitter

**Problem solved**

Provides fast incremental parsing and syntax-tree queries for source languages.

**Architecture**

The C runtime is under `lib/src/`; parser state and reuse are in `parser.c`, changed-range computation is in `get_changed_ranges.c`, and the Rust tags crate is under `crates/tags/src/`.

**Strongest implementation idea**

`ts_parser_parse` accepts an old tree and the runtime computes changed ranges after edits, allowing unaffected subtrees to be reused.

**Weak points**

The parser produces syntax structure, not type resolution or a project-wide reference graph; those capabilities were not found in the files read.

**Performance strategy**

Reusable parse state, cached tokens, subtree reuse, compact C data structures, and incremental changed-range traversal are visible in `parser.c` and `get_changed_ranges.c`.

**Key data structures**

`TSParser`, `TSTree`, subtrees, parse stacks, `TSInputEdit`, `TSRange`, tree cursors, and queries.

**Indexing strategy**

No persistent index in the runtime files read; `crates/tags` provides a tag-oriented consumer.

**Agent integration strategy**

C library with Rust workspace bindings and CLI/config/highlight/loader/tags crates; no agent-specific integration was verified.

**Token-saving technique**

Enables callers to extract syntax nodes, tags, and changed regions instead of sending whole files.

**Tool-call-saving technique**

Incremental reparsing lets a caller update local structure after an edit without rebuilding the complete tree.

**Primary language + notable dependencies**

MIT; C runtime plus Rust bindings/crates; workspace includes parser, tags, loader, highlight, config, and CLI components.

**What we should adopt**

Old-tree parsing, changed-range calculation, and a reusable parser pool per language.

**What we should NOT adopt**

Do not expose raw syntax nodes as if they were stable semantic symbols; add file/version provenance around them.

**Evidence**

- `aclones/tree-sitter/LICENSE`
- `aclones/tree-sitter/Cargo.toml`
- `aclones/tree-sitter/lib/src/parser.c`
- `aclones/tree-sitter/lib/src/get_changed_ranges.c`
- `aclones/tree-sitter/lib/src/tree.c`
- `aclones/tree-sitter/crates/tags/src/tags.rs`

## scip

**Problem solved**

Defines a portable serialized representation for precise code intelligence indexes.

**Architecture**

The schema is in `scip.proto`; its top-level `Index` contains `Metadata`, `Document`, and external symbols, while documents contain occurrences and symbol information.

**Strongest implementation idea**

Separate symbol identity, occurrence ranges, relationships, package descriptors, signatures, and diagnostics into typed protobuf messages.

**Weak points**

This clone’s evidence is the schema and Go module, not a complete indexer or query engine; incremental update and ranking behavior are unverified.

**Performance strategy**

`Occurrence` retains compact packed range forms and typed ranges; the schema comments describe compact encoding options, but runtime performance is unverified here.

**Key data structures**

`Index`, `Metadata`, `ToolInfo`, `Document`, `Symbol`, `Package`, `Descriptor`, `Signature`, `SymbolInformation`, `Relationship`, `Occurrence`, `Diagnostic`, `SingleLineRange`, and `MultiLineRange`.

**Indexing strategy**

A producer writes protobuf documents with symbols and occurrences; no producer implementation was verified in this clone.

**Agent integration strategy**

Language/tool-neutral schema and bindings, not an MCP server or agent hook.

**Token-saving technique**

Consumers can request symbol/range metadata rather than source files; this is an implication of the schema, not an implemented retrieval policy here.

**Tool-call-saving technique**

One serialized document can carry many occurrences and relationships.

**Primary language + notable dependencies**

Apache-2.0; protobuf schema with Go module and generated/binding packages visible in the repository.

**What we should adopt**

Use typed symbol IDs, occurrence ranges, relationship kinds, and diagnostics as the interchange model for our indexes.

**What we should NOT adopt**

Do not assume SCIP alone supplies parsing, storage, ranking, or freshness tracking.

**Evidence**

- `aclones/scip/LICENSE`
- `aclones/scip/scip.proto`
- `aclones/scip/go.mod`
- `aclones/scip/bindings/typescript/package.json`
- `aclones/scip/bindings/rust/Cargo.toml`

## probe

**Problem solved**

Provides fully local semantic code search, language-aware extraction, AST matching, and ranked results for large codebases.

**Architecture**

`src/` has `ranking.rs`, `simd_ranking.rs`, `bert_reranker.rs`, `search/`, `extract/`, `language/`, and `lsp_integration/`; search and ranking are split across those modules, with extraction and Tree-sitter parser caches in their respective directories.

**Strongest implementation idea**

Combine lexical/BM25-style ranking, SIMD paths, language-aware block extraction, AST queries, and optional BERT reranking behind both library and CLI APIs.

**Weak points**

LICENSE is Apache-2.0 but `Cargo.toml` declares `license = "MIT"`; this discrepancy must be resolved before reuse. Semantic/reranker quality and index durability beyond the read files are unverified.

**Performance strategy**

Uses `rayon`, `ahash`, `DashMap`, SIMD similarity/string helpers, parser pools, tree caches, and release LTO/size optimization.

**Key data structures**

`SearchOptions`, `QueryOptions`, `CodeBlock`, `SearchResult`, `LimitedSearchResults`, query-token maps, TF/DF results, sparse document matrices, and cached syntax trees.

**Indexing strategy**

The read implementation is search-time over ignored filesystem paths and parsed blocks; a persistent inverted index was not verified.

**Agent integration strategy**

Rust library and CLI; `src/lib.rs` exposes search, extraction, and AST query functions. MCP integration was not verified.

**Token-saving technique**

Extracts relevant code blocks/outlines and caps result bytes/tokens instead of returning full files.

**Tool-call-saving technique**

Natural-language search, AST query, and extraction combine discovery and context shaping in a single invocation.

**Primary language + notable dependencies**

Rust; Tree-sitter grammars, ast-grep, Tokio, Rayon, `tiktoken-rs`, Turso, SIMD helpers, and optional Candle/Hugging Face reranker dependencies.

**What we should adopt**

Language-aware extraction, result limits, pooled parsers, and a staged lexical-to-reranker pipeline.

**What we should NOT adopt**

Do not ship the license ambiguity or make optional model downloads a hard dependency for local search.

**Evidence**

- `aclones/probe/LICENSE`
- `aclones/probe/Cargo.toml`
- `aclones/probe/src/lib.rs`
- `aclones/probe/src/ranking.rs`
- `aclones/probe/src/simd_ranking.rs`
- `aclones/probe/src/bert_reranker.rs`
- `aclones/probe/src/search/early_ranker.rs`
- `aclones/probe/src/extract/symbols.rs`
- `aclones/probe/src/language/parser_pool.rs`

## repomix

**Problem solved**

Packs repository contents into a bounded, AI-consumable output with parsing, filtering, metrics, and security checks.

**Architecture**

`src/core/` contains `treeSitter/` (languageParser, parseStrategies, queries), `tokenCount/`, `security/`, `git/`, `metrics/`, and `packager/`; `src/core/file/` collects and reads files, while output modules render formats.

**Strongest implementation idea**

Use a bounded concurrent file collector plus language-specific parse strategies and output styles, while skipping binary, oversized, and unsafe inputs.

**Weak points**

The packager’s primary unit is still file content; the files read do not establish symbol handles, cross-session raw-output recovery, or a persistent code index.

**Performance strategy**

`fileCollect.ts` uses a 50-worker promise pool, `fileRead.ts` avoids a separate `stat` call and lazy-loads encoding dependencies, and language resources are cached per language.

**Key data structures**

`RawFile`, skipped-file records/reasons, language resource bundles, parse strategies, metrics/token counters, and git diff/log records.

**Indexing strategy**

On-demand repository walk and parse; Tree-sitter queries extract structure for compression, not a durable symbol index.

**Agent integration strategy**

TypeScript CLI/library and an MCP package entry in `package.json`; direct agent hook behavior was not verified.

**Token-saving technique**

Filters files, strips or restructures content through Tree-sitter strategies, and emits XML/Markdown/plain outputs with metrics.

**Tool-call-saving technique**

One pack operation returns a whole filtered repository snapshot plus metrics.

**Primary language + notable dependencies**

MIT; TypeScript; `web-tree-sitter`, Tree-sitter WASMs, `gpt-tokenizer`, `globby`, `chokidar`, `Secretlint`, Commander, and MCP SDK.

**What we should adopt**

The bounded collector, fast UTF-8 path, skip reasons, metrics cache, and Secretlint boundary.

**What we should NOT adopt**

Do not make a monolithic full-repository pack the default evidence path for targeted agent tasks.

**Evidence**

- `aclones/repomix/LICENSE`
- `aclones/repomix/package.json`
- `aclones/repomix/src/core/file/fileCollect.ts`
- `aclones/repomix/src/core/file/fileRead.ts`
- `aclones/repomix/src/core/treeSitter/languageParser.ts`
- `aclones/repomix/src/core/treeSitter/parseFile.ts`
- `aclones/repomix/src/core/security/securityCheck.ts`
- `aclones/repomix/src/core/metrics/TokenCounter.ts`
- `aclones/repomix/src/core/git/gitDiffHandle.ts`

## zoekt

**Problem solved**

Indexes large code corpora for fast text, regexp, filename, repository, branch, and symbol-aware search.

**Architecture**

`index/builder.go` builds bounded shards, `index/shard_builder.go` builds postings, `index/read.go` reads B-tree trigram indexes, `index/score.go` scores matches, `query/` parses query ASTs, and `cmd/zoekt-webserver/main.go` serves HTTP/RPC.

**Strongest implementation idea**

Positional trigrams narrow candidates through posting lists before exact matching; optional BM25 scoring is implemented in `index/score.go`.

**Weak points**

Trigram limits can skip binary, oversized, too-small, or too-many-trigram documents; relevance is primarily lexical and the BM25 implementation omits IDF according to `score.go` comments.

**Performance strategy**

Uniform shards, parallel shard creation, pooled postings builders, direct-array ASCII trigram postings, B-trees, Roaring bitmaps, and candidate aggregation reduce search and indexing cost.

**Key data structures**

`Document`, `DocumentSection`, posting lists, B-tree indexes, shard metadata, Roaring bitmaps, query AST nodes, and match trees.

**Indexing strategy**

Builds full or delta shards; changed/removed files are tombstoned in older shards, and ctags/SCIP-ctags can add symbol metadata.

**Agent integration strategy**

Go library plus command-line indexers and HTTP/gRPC web server; no MCP integration was verified.

**Token-saving technique**

Returns search matches, line/chunk context, and symbol sections rather than whole indexed files.

**Tool-call-saving technique**

Query syntax supports filename/content, repo/branch, language, regexp, and symbol filters in one search request.

**Primary language + notable dependencies**

Apache-2.0; Go; Roaring bitmaps, regexp, ctags/SCIP-ctags, HTTP, gRPC, Prometheus, and tracing dependencies.

**What we should adopt**

Positional trigram candidate narrowing, bounded shards, tombstoned delta updates, and explicit skip reasons.

**What we should NOT adopt**

Do not use BM25 as a substitute for semantic ranking, and do not hide documents skipped by index limits.

**Evidence**

- `aclones/zoekt/LICENSE`
- `aclones/zoekt/go.mod`
- `aclones/zoekt/doc/design.md`
- `aclones/zoekt/doc/indexing.md`
- `aclones/zoekt/index/builder.go`
- `aclones/zoekt/index/shard_builder.go`
- `aclones/zoekt/index/document.go`
- `aclones/zoekt/index/read.go`
- `aclones/zoekt/index/score.go`
- `aclones/zoekt/query/query.go`
- `aclones/zoekt/query/parse.go`

## mcp-context-proxy

**Problem solved**

Reduces MCP tool-schema context overhead and optionally compresses large tool results while preserving a proxy path to upstream servers.

**Architecture**

`src/` contains `proxy-server.ts`, `schema-cache.ts`, `response-compress.ts`, `token-estimator.ts`, `metrics.ts`, `cli.ts`, and `types.ts`; the proxy server routes upstream MCP calls, the schema cache persists schemas, response compression summarizes JSON/truncates text, and metrics report savings.

**Strongest implementation idea**

Expose cheap stubs for all tools, lazy-load a full schema on first invocation, persist it with a checksum and 24-hour TTL, and reuse it on later calls.

**Weak points**

`proxy-server.ts` currently accepts only stdio upstream transport; response compression intentionally drops detail from large results, so raw-result recovery is not provided by this proxy.

**Performance strategy**

Schema memory/disk caches, schema deduplication by server/tool storage, and no extra process for the host-facing stdio server reduce repeated setup cost.

**Key data structures**

Upstream maps, tool-name router, `SchemaCache`, `ToolSchema`, `ToolStub`, compression configuration, and context reports.

**Indexing strategy**

No code index; initial `listTools` discovers names and minimal descriptions, while full schemas are fetched on demand.

**Agent integration strategy**

TypeScript MCP server over stdio, using the MCP SDK; it sits between an MCP client and configured upstream MCP servers.

**Token-saving technique**

First-sentence tool stubs, schema caching, rough token estimates, and JSON array/text compression.

**Tool-call-saving technique**

Transparent routing preserves the agent’s existing tool names, so the agent need not manually configure each upstream path.

**Primary language + notable dependencies**

MIT; TypeScript; `@modelcontextprotocol/sdk`, Express, WebSocket types, and Node filesystem/crypto APIs.

**What we should adopt**

Lazy schema stubs, checksum-backed persistent cache, and compression thresholds with an explicit error-result exception.

**What we should NOT adopt**

Do not discard large raw responses without storing a recoverable handle, and do not assume `bytes / 4` is model-accurate tokenization.

**Evidence**

- `aclones/mcp-context-proxy/LICENSE`
- `aclones/mcp-context-proxy/package.json`
- `aclones/mcp-context-proxy/src/proxy-server.ts`
- `aclones/mcp-context-proxy/src/schema-cache.ts`
- `aclones/mcp-context-proxy/src/response-compress.ts`
- `aclones/mcp-context-proxy/src/token-estimator.ts`
- `aclones/mcp-context-proxy/src/cli.ts`
- `aclones/mcp-context-proxy/src/types.ts`

## dynamic-discovery-mcp

**Problem solved**

Reduces MCP tool-manifest cost and defers connection to low-priority upstream MCP servers.

**Architecture**

`src/proxy/server.ts` owns host-facing MCP handlers, `tool-catalog.ts` builds compact descriptions and full schema details, `lazy-registry.ts` tracks deferred servers, `orchestrator.ts` manages upstream clients, and `upstream-client.ts` forwards MCP features.

**Strongest implementation idea**

Expose exactly two meta-tools, `discover_tool` and `use_tool`; tool schemas are retrieved only when named, and lazy MCPs are loaded only after `load_mcp`.

**Weak points**

Loading is permanent for the remainder of a session and lazy mode is unavailable in single-MCP non-namespaced mode, as documented in `orchestrator.ts`. `package.json` declares MIT; no LICENSE file is present in this clone.

**Performance strategy**

Maps for tool catalogs and lazy entries avoid repeated scans; eager and lazy upstreams are separated, and catalog rebuilds respond to tool-list changes.

**Key data structures**

`Map<string, UpstreamTool>`, grouped tool maps, `LazyEntry`, `LazyRegistry`, orchestrator state, and namespaced tool names.

**Indexing strategy**

Indexes tool metadata, not source code: eager upstreams are listed, lazy upstreams are represented by descriptions, and full input/output schemas are fetched by name.

**Agent integration strategy**

TypeScript MCP gateway/proxy with stdio host transport and forwarding for tools, resources, prompts, logging, roots, progress, and cancellation.

**Token-saving technique**

Compact catalog descriptions and deferred full schemas/servers keep the host manifest small.

**Tool-call-saving technique**

One `use_tool` call routes a namespaced tool to its upstream after one discovery call.

**Primary language + notable dependencies**

MIT; TypeScript; MCP SDK, FastMCP, Commander, Zod, YAML, keyring, and Node transports.

**What we should adopt**

The stable two-meta-tool contract, namespaced catalogs, lazy registry lifecycle, and capability forwarding.

**What we should NOT adopt**

Do not require a discovery call for local file evidence where a direct stable handle is cheaper and more deterministic.

**Evidence**

- `aclones/dynamic-discovery-mcp/package.json`
- `aclones/dynamic-discovery-mcp/src/index.ts`
- `aclones/dynamic-discovery-mcp/src/proxy/server.ts`
- `aclones/dynamic-discovery-mcp/src/proxy/tool-catalog.ts`
- `aclones/dynamic-discovery-mcp/src/proxy/lazy-registry.ts`
- `aclones/dynamic-discovery-mcp/src/proxy/orchestrator.ts`
- `aclones/dynamic-discovery-mcp/src/proxy/upstream-client.ts`

## mini-swe-agent

**Problem solved**

Runs a minimal software-engineering agent loop with configurable models and local/container environments.

**Architecture**

`src/minisweagent/agents/default.py` is ~190 lines and implements query, action execution, limits, errors, and serialization; `environments/local.py` executes commands; `run/mini.py` wires config, model, agent, and environment.

**Strongest implementation idea**

Keep the default control loop small: query the model, execute returned actions, append observations, and stop on submission or configured limits.

**Weak points**

The default local environment executes shell commands directly on the local machine; no evidence of raw-output handles, semantic indexing, or automatic safety rewriting was found.

**Performance strategy**

The implementation favors low orchestration overhead and configurable step, cost, time, and format-error limits; caching/indexing behavior is unverified.

**Key data structures**

Pydantic `AgentConfig`, message lists, action dictionaries, serialized trajectory dictionaries, environment config, and subprocess results.

**Indexing strategy**

None verified; the agent navigates through model-generated actions.

**Agent integration strategy**

Python library/CLI with pluggable model, agent, and environment classes selected through configuration and Typer options.

**Token-saving technique**

Configurable limits and optional output/trajectory serialization bound runaway sessions; explicit context compression was not verified.

**Tool-call-saving technique**

`execute_actions` can execute the list of actions returned in one model message.

**Primary language + notable dependencies**

MIT; Python; Pydantic, Typer, subprocess/runtime adapters, model backends, and Rich-oriented CLI support.

**What we should adopt**

The small state machine, explicit limits, serializable trajectory, and replaceable environment interface.

**What we should NOT adopt**

Do not run arbitrary local commands by default in a shared runtime without policy, provenance, and output retention.

**Evidence**

- `aclones/mini-swe-agent/pyproject.toml`
- `aclones/mini-swe-agent/LICENSE.md`
- `aclones/mini-swe-agent/src/minisweagent/agents/default.py`
- `aclones/mini-swe-agent/src/minisweagent/environments/local.py`
- `aclones/mini-swe-agent/src/minisweagent/run/mini.py`

## SWE-agent

**Problem solved**

Runs configurable software-engineering agents in managed environments with repository setup, shell tools, history processing, and submission review.

**Architecture**

`sweagent/agent/agents.py` owns configuration and the agent lifecycle, `environment/swe_env.py` wraps deployment/runtime commands and file operations, `environment/repo.py` copies/resets repositories, `tools/tools.py` validates and parses actions, and `agent/history_processors.py` controls retained observations.

**Strongest implementation idea**

Make environment, tool bundles, history processors, model, templates, and review loops configuration-driven and typed with Pydantic models.

**Weak points**

History elision can break prompt caching because the prompt changes on each update; local repository setup rejects dirty repositories outside tests.

**Performance strategy**

Observation length limits, last-N observation processors, command timeouts, action retries, environment reuse, and deployment abstractions bound long runs.

**Key data structures**

Template/config models, typed trajectories, tool bundles, command patterns, repository configs, action/observation history, and review submissions.

**Indexing strategy**

The default configuration enables a file map, but a code index implementation was not verified in the files read.

**Agent integration strategy**

Python CLI/library with shell tools, managed local/container deployments, hooks, model adapters, and configurable prompt/action parsers.

**Token-saving technique**

Last-N observation elision, maximum observation length, and instructions to use narrower shell commands reduce prompt history.

**Tool-call-saving technique**

Tool bundles and parser-driven actions let one model response contain multiple configured commands/actions.

**Primary language + notable dependencies**

MIT; Python; Pydantic, Jinja2, PyYAML, GitPython, Swerex runtime, Tenacity, Unidiff, and model/tool adapters.

**What we should adopt**

Typed command/tool contracts, environment abstraction, explicit repository reset, bounded observations, and review-aware retries.

**What we should NOT adopt**

Do not silently elide evidence without a stable handle; do not use destructive reset commands outside a clearly isolated environment.

**Evidence**

- `aclones/SWE-agent/LICENSE`
- `aclones/SWE-agent/pyproject.toml`
- `aclones/SWE-agent/config/default.yaml`
- `aclones/SWE-agent/sweagent/agent/agents.py`
- `aclones/SWE-agent/sweagent/agent/history_processors.py`
- `aclones/SWE-agent/sweagent/tools/tools.py`
- `aclones/SWE-agent/sweagent/environment/swe_env.py`
- `aclones/SWE-agent/sweagent/environment/repo.py`

## context-mode

**Problem solved**

Keeps large tool output and session state out of the active context while making exact content recoverable through local storage and search.

**Architecture**

`src/server.ts` wires MCP tools, `src/store.ts` stores heading/code chunks in SQLite FTS5, `src/session/db.ts` stores events and snapshots, `src/search/unified.ts` merges content/session/auto-memory results, `src/executor.ts` runs code, and `src/runPool.ts` caps parallel work.

**Strongest implementation idea**

Sandbox execution and capture raw output locally, return a compact result plus a stable recovery path, and use BM25/FTS5 retrieval when exact text is needed.

**Weak points**

The project is Elastic-2.0 (ELv2) — NOT permissive; the license restricts hosted/managed service use and requires notices. SQLite search is lexical; semantic retrieval and impact analysis were not verified.

**Performance strategy**

SQLite WAL/base helpers, chunk caps, stopword/query sanitization, a shared in-flight-capped worker pool, and lazy/bundled runtime paths reduce overhead.

**Key data structures**

Content chunks, FTS5 search rows, typed session events, project/session attribution, snapshots, fetch-cache keys, and settled pool results.

**Indexing strategy**

Chunks Markdown/code by headings and bounded paragraphs into FTS5; unified search can scope results to project-attributed session IDs.

**Agent integration strategy**

MCP server plus platform adapters/hooks for Claude Code, Gemini CLI, Copilot, OpenCode, Codex, and others in the snapshot/configs.

**Token-saving technique**

`ctx_execute`/batch tools compute summaries locally; FTS5/BM25 returns only matching chunks, and hooks capture output across compaction.

**Tool-call-saving technique**

Batch execution uses `runPool` and preserves input order; code-mode scripts replace repeated raw reads with one aggregate call.

**Primary language + notable dependencies**

Elastic-2.0 (ELv2) — NOT permissive; TypeScript; `better-sqlite3`, MCP SDK, Zod, Turndown, esbuild, and Vitest.

**What we should adopt**

Local raw store plus handles, FTS5/BM25 exact retrieval, session attribution, batch worker pool, and pre/post tool lifecycle capture.

**What we should NOT adopt**

Do not copy ELv2 code into a permissively licensed runtime; reimplement interfaces and behavior clean-room unless legal review approves another path.

**Evidence**

- `aclones/context-mode/LICENSE`
- `aclones/context-mode/package.json`
- `aclones/context-mode/README.md`
- `aclones/context-mode/src/server.ts`
- `aclones/context-mode/src/store.ts`
- `aclones/context-mode/src/search/unified.ts`
- `aclones/context-mode/src/session/db.ts`
- `aclones/context-mode/src/runPool.ts`
- `aclones/context-mode/src/security.ts`
- `aclones/_research/mksglu_context-mode.md`

## rtk

**Problem solved**

Rewrites supported shell commands through hooks and emits compact command output before it reaches an agent context.

**Architecture**

`src/core/runner.rs` executes and guards filters, `src/parser/mod.rs` defines three-tier output parsing, `src/cmds/` contains per-command adapters, `src/hooks/` decides rewrites and installs integrations, and `src/core/tee.rs` routes recovery storage.

**Strongest implementation idea**

The `ParseResult` contract has Full, Degraded, and Passthrough tiers; `never_worse` prevents filtered output from exceeding raw output, while tee/retriever stores failure/truncation evidence.

**Weak points**

The rewrite hook only sees intercepted shell calls; the snapshot explicitly notes that Claude built-in Read/Grep/Glob bypass it. Token estimates are bytes divided by four, not tokenizer counts.

**Performance strategy**

Single Rust binary, streamed/captured execution, pure filters, release LTO, and command-specific parsers. The snapshot reports sub-10ms overhead, but that measurement was not independently reproduced here.

**Key data structures**

`RunOptions`, `RunMode`, `ParseResult`, `OutputParser`, filter strategies, hook decisions, and recovery records keyed by hashes/slugs.

**Indexing strategy**

No code index; command registry and filter tables map command names to parsers/rewriters.

**Agent integration strategy**

CLI plus native/plugin/hook adapters for multiple agents; `src/hooks/decision.rs` centralizes allow/ask/defer/deny behavior.

**Token-saving technique**

Per-command parsers emit structured summaries, strip boilerplate/comments, and truncate only in passthrough mode.

**Tool-call-saving technique**

PreToolUse rewrite turns ordinary commands into compact equivalents without requiring the model to call a second tool.

**Primary language + notable dependencies**

Apache-2.0; Rust; Clap, Serde, SQLite, TOML, regex, quick-XML, compression, and platform command/runtime libraries.

**What we should adopt**

The three-tier parser API, never-worse guard, command-specific parser registry, permission-aware rewrites, and recoverable tee path.

**What we should NOT adopt**

Do not rely on post-tool compression to shrink the current turn, and do not make raw-output recovery legacy-only.

**Evidence**

- `aclones/rtk/LICENSE`
- `aclones/rtk/Cargo.toml`
- `aclones/rtk/src/core/runner.rs`
- `aclones/rtk/src/core/filter.rs`
- `aclones/rtk/src/parser/mod.rs`
- `aclones/rtk/src/core/tee.rs`
- `aclones/rtk/src/hooks/decision.rs`
- `aclones/rtk/src/cmds/system/read.rs`
- `aclones/_research/rtk-ai_rtk.md`

## caveman

**Problem solved**

Shrinks agent output and stores original compressed bytes for recovery, with skills, hooks, proxy, MCP, and multi-agent setup surfaces.

**Architecture**

The supplied snapshot describes a local proxy, SQLite-backed recovery store, output shrinkers, context packing, MCP tools, hooks, and agent-specific plugins; exact source module boundaries were not relied on for this section.

**Strongest implementation idea**

Pair output-side compression with a recovery handle so reduced context does not destroy access to original bytes; pack candidates by BM25/relevance/recency/error signals within a token budget.

**Weak points**

Licensing is ambiguous: dual — `LICENSE` says MIT, and a separate `LICENSE.BSL` (Business Source License) exists. The snapshot also says the engine-linked runtime is BSL-1.1 and the CLI sends anonymous usage statistics by default.

**Performance strategy**

The snapshot describes one local process, SQLite storage, command-specific shrinkers, and concurrency tests; quantitative claims are reported by the snapshot and not independently benchmarked here.

**Key data structures**

SQLite recovery records/handles, compressed observations, BM25-packed candidates, hook configuration, skills, and agent adapter settings.

**Indexing strategy**

Snapshot evidence describes context packing and learning from local agent history; a durable source-code symbol index was unverified.

**Agent integration strategy**

Hooks, skills, CLI, MCP tools, provider proxy, OpenCode plugin, and wrappers for Claude, Gemini, Codex, Cursor, and other agents are described in the snapshot.

**Token-saving technique**

Command/output shrinkers, compact skills, BM25 budget packing, and image rendering for skill text reduce visible context or generated prose.

**Tool-call-saving technique**

The proxy and five MCP tools combine compression, retrieval, stats, and encoding; exact call routing was not verified in source.

**Primary language + notable dependencies**

Dual MIT/BSL; repository is multi-language; the snapshot identifies SQLite, proxy/MCP, Go core/platform, TypeScript plugins, and shell/Python hooks.

**What we should adopt**

The invariant that every compressed output has a recoverable original, plus token-budget packing and explicit savings accounting.

**What we should NOT adopt**

Do not copy engine-linked code until license ownership and applicable MIT/BSL boundaries are resolved; do not enable telemetry by default for a local-first runtime.

**Evidence**

- `aclones/caveman/LICENSE`
- `aclones/caveman/LICENSE.BSL`
- `aclones/caveman/AGENTS.md`
- `aclones/caveman/mcp/package.json`
- `aclones/caveman/proxy/AGENTS.md`
- `aclones/_research/JuliusBrussee_caveman.md`

## claude-context

**Problem solved**

Provides semantic code search for coding agents by indexing code chunks and retrieving related context instead of loading entire directories.

**Architecture**

The snapshot identifies `@zilliz/claude-context-core` for indexing/embeddings/vector DB integration, `packages/mcp` for MCP, and extensions/examples around the core package.

**Strongest implementation idea**

Hybrid search combines BM25 with dense vectors, with incremental indexing described as Merkle-tree based.

**Weak points**

It needs an embedding API plus Milvus/Zilliz vector DB; that is not local-only and introduces service, credential, schema, and embedding-model dependencies.

**Performance strategy**

Chunked vector retrieval, hybrid BM25/dense search, and Merkle-based changed-file indexing are described in the snapshot; benchmark figures were not independently verified.

**Key data structures**

Code chunks, embeddings, Milvus/Zilliz collections, file Merkle state, hybrid search results, and indexing-status records.

**Indexing strategy**

Indexes a codebase into Milvus/Zilliz and incrementally reindexes changed files using Merkle trees.

**Agent integration strategy**

MIT; TypeScript monorepo; MCP server over stdio plus VS Code/other client configuration surfaces.

**Token-saving technique**

Retrieves related chunks rather than entire directories; the snapshot reports a controlled evaluation with token reduction, but it is not verified here.

**Tool-call-saving technique**

`index_codebase`, `search_code`, `clear_index`, and `get_indexing_status` form a direct search/index lifecycle instead of manual file traversal.

**Primary language + notable dependencies**

MIT; TypeScript; OpenAI or another embedding provider and Milvus/Zilliz vector database are required by the snapshot’s setup.

**What we should adopt**

Hybrid lexical/semantic retrieval and Merkle freshness tracking behind a storage-provider interface.

**What we should NOT adopt**

Do not make a cloud embedding API or hosted vector database a prerequisite for Context Runtime’s local-first path.

**Evidence**

- `aclones/claude-context/LICENSE`
- `aclones/claude-context/package.json`
- `aclones/claude-context/packages/core/package.json`
- `aclones/claude-context/packages/mcp/package.json`
- `aclones/_research/zilliztech_claude-context.md`

## claude-token-optimizer

**Problem solved**

Reduces Claude Code startup and project-instruction context by compressing and archiving documentation and injecting topic-specific learnings only when matched.

**Architecture**

`src/lib/scanner.js` finds auto-loaded Markdown using glob and `.claudeignore`; `src/commands/compress.js` applies pure Markdown rules; `prune.js` parses and archives sections; `templates/hooks/` contains lifecycle scripts.

**Strongest implementation idea**

Separate always-loaded core instructions from archived/session/topic files, then use a prompt hook to keyword-match only relevant learning files within file/word caps.

**Weak points**

The optimizer targets Claude Code project docs rather than source-code semantics; the scanner uses filenames and globs, and token estimates are tied to the bundled Claude 2 tokenizer.

**Performance strategy**

Simple filesystem/glob scans, pure string transforms, bounded injected files/words, and no persistent search database keep the tool small.

**Key data structures**

Scanned `{path, content}` records, parsed section objects, prune targets with destinations/token counts, and hook configuration values.

**Indexing strategy**

Glob-based scan of `*.md`, `.claude/*.md`, and `docs/**/*.md`, filtered by `.claudeignore`; no code index.

**Agent integration strategy**

MIT; JavaScript/TypeScript-oriented Node CLI plus Claude Code shell hooks.

**Token-saving technique**

Strip markup, collapse blank lines, shorten code fences, truncate long lists, archive completed/session sections, and inject matched learnings only.

**Tool-call-saving technique**

Prompt hooks inject topic files automatically, avoiding a separate manual read for known learning topics.

**Primary language + notable dependencies**

MIT; JS/TS; Node, Commander, Glob, Chalk, and `@anthropic-ai/tokenizer`.

**What we should adopt**

Topic-scoped memory loading, bounded injection, dry-run/backup behavior, and explicit token deltas.

**What we should NOT adopt**

Do not use filename keyword matching as the runtime’s only relevance mechanism for source code or shell evidence.

**Evidence**

- `aclones/claude-token-optimizer/LICENSE`
- `aclones/claude-token-optimizer/package.json`
- `aclones/claude-token-optimizer/src/lib/scanner.js`
- `aclones/claude-token-optimizer/src/lib/tokenizer.js`
- `aclones/claude-token-optimizer/src/commands/compress.js`
- `aclones/claude-token-optimizer/src/commands/prune.js`
- `aclones/claude-token-optimizer/templates/hooks/user-prompt-inject-context.sh`
- `aclones/_research/nadimtuhin_claude-token-optimizer.md`
