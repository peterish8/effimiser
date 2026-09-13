# Feature matrix

Cells mean: `yes` = verified implementation; `partial` = narrower, schema-only, optional, or README/snapshot-only support; `no` = the read files show the repository does not provide the capability; `unverified` = the inspected evidence was insufficient.

| Capability | aider | serena | ast-grep | tree-sitter | scip | probe | repomix | zoekt | mcp-context-proxy | dynamic-discovery-mcp | mini-swe-agent | SWE-agent | context-mode | rtk | caveman | claude-context | claude-token-optimizer |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| repo map / graph ranking | yes | partial | no | no | partial | partial | partial | no | no | no | no | no | no | no | partial | no | no |
| symbol index | yes | yes | partial | partial | yes | yes | partial | partial | no | no | no | partial | no | no | unverified | no | no |
| reference-or-call graph | yes | yes | partial | no | yes | partial | no | partial | no | no | no | no | no | no | unverified | no | unverified |
| incremental reindex | partial | partial | no | yes | unverified | partial | no | yes | no | no | no | no | partial | no | partial | yes | no |
| AST structural search | partial | partial | yes | yes | no | yes | yes | no | no | no | no | no | no | no | unverified | no | no |
| AST structural rewrite | no | partial | yes | no | no | no | no | no | no | no | no | no | no | no | unverified | no | no |
| trigram or inverted index | no | no | no | no | no | partial | partial | yes | no | no | no | no | yes | no | unverified | partial | no |
| BM25 ranking | no | no | no | no | no | yes | no | yes | no | no | no | no | yes | no | partial | yes | no |
| embeddings/semantic search | no | partial | no | no | no | partial | no | no | no | no | no | no | no | no | unverified | yes | no |
| reranking | partial | no | no | no | no | yes | no | no | no | no | no | no | no | no | partial | partial | no |
| token counting | partial | no | no | no | no | yes | yes | no | yes | no | no | partial | partial | partial | partial | partial | yes |
| git-aware filtering | partial | partial | no | no | no | yes | yes | partial | no | no | no | yes | partial | yes | partial | partial | no |
| secret scanning | no | no | no | no | no | no | yes | no | no | no | no | no | partial | partial | partial | no | no |
| whole-file-read avoidance | yes | yes | partial | partial | partial | yes | no | yes | no | no | partial | partial | yes | yes | partial | yes | partial |
| delta/diff-aware reads | partial | partial | no | yes | no | partial | yes | yes | no | no | no | partial | partial | partial | yes | partial | yes |
| raw output store with handles | no | no | no | no | no | no | no | no | no | no | no | no | yes | yes | yes | no | no |
| shell output compression | no | no | no | no | no | no | no | no | partial | no | no | no | yes | yes | yes | no | no |
| per-command output parsers | no | no | no | no | no | no | no | no | no | no | partial | yes | partial | yes | partial | no | no |
| MCP server | no | yes | no | no | no | no | partial | no | yes | yes | no | no | yes | no | yes | yes | no |
| MCP gateway/proxy | no | no | no | no | no | no | no | no | yes | yes | no | no | partial | no | yes | no | no |
| lazy tool-schema loading | no | partial | no | no | no | no | no | no | yes | yes | no | no | partial | no | no | no | no |
| tool batching | no | no | no | no | no | no | partial | no | no | partial | yes | partial | yes | no | partial | no | no |
| symbol-level editing | no | yes | yes | no | no | no | no | no | no | no | no | partial | no | no | unverified | no | no |
| edit syntax validation | no | yes | partial | yes | no | partial | no | no | no | no | no | partial | no | no | unverified | no | no |
| impact analysis / test selection | no | partial | no | no | partial | partial | no | no | no | no | no | partial | no | no | partial | no | no |
| typed project memory | no | partial | no | no | no | no | no | no | no | no | no | partial | partial | no | partial | no | no |
| cross-agent memory sharing | no | partial | no | no | no | no | no | no | no | no | no | no | yes | no | partial | no | no |
| local-only operation (no cloud required) | yes | yes | yes | yes | yes | yes | yes | yes | yes | yes | yes | yes | partial | yes | partial | no | yes |

## Capability gaps — owned by nobody

No reference repository scores `yes` for:

- typed project memory;
- impact analysis / test selection.

These are differentiation candidates. Serena, SWE-agent, context-mode, and SCIP provide partial ingredients, but none of the inspected repositories verifies a typed, provenance-linked memory model that drives impact analysis or test selection end to end.

## Overlap clusters

- Code navigation and symbol context: aider, serena, probe, SCIP, tree-sitter, ast-grep, Zoekt, Repomix, and Claude Context. Learn from Serena for semantic symbol operations, Aider for task-personalized ranking, and Zoekt for scalable lexical candidate narrowing.
- Syntax-aware parsing and editing: tree-sitter, ast-grep, probe, Repomix, and Serena. Learn from tree-sitter for incremental trees and ast-grep for structural matching/replacement.
- Shell and tool-output reduction: RTK, context-mode, Caveman, mcp-context-proxy, and claude-token-optimizer. Learn from RTK for parser/guard contracts and context-mode/Caveman for recoverable raw output; reimplement ELv2/BSL ideas clean-room.
- MCP manifest and routing control: mcp-context-proxy, dynamic-discovery-mcp, context-mode, Serena, Repomix, and Claude Context. Learn from dynamic-discovery-mcp for two meta-tools and mcp-context-proxy for persistent lazy schemas.
- Agent loops and environments: mini-swe-agent and SWE-agent. Learn from mini-swe-agent for a small serializable loop and SWE-agent for typed environments, retries, and bounded history.
- Project memory and continuity: Serena, context-mode, Caveman, and claude-token-optimizer. Learn from context-mode for event/session provenance and Serena for explicit project/global memory scope.
- Semantic retrieval: probe and Claude Context, with partial support in Serena/Caveman. Learn from Probe for local staged ranking and Claude Context only as an optional backend pattern because it requires embedding API/vector DB services.

## Adoption shortlist

| Idea | Source repo | Why it wins | Implementation risk | Clean-room reimplementation for license reasons |
|---|---|---|---|---|
| Raw output blob store with stable recovery handles | context-mode; Caveman | Preserves exact evidence while returning compact packets. | high | yes — ELv2/BSL boundaries |
| Bounded concurrent worker pool with input-order results | context-mode | Reusable primitive for batch commands, fetches, and indexing. | low | yes — ELv2 source; reproduce behavior independently |
| Query-personalized PageRank repo map with mtime cache | aider | Produces task-focused navigation context and avoids repeat parsing. | med | no — Apache-2.0 |
| LSP symbol/reference/edit tool boundary with diagnostics | serena | Directly addresses whole-file reads and semantic edits. | high | no — MIT |
| Incremental old-tree parsing and changed ranges | tree-sitter | Keeps source structure current after small edits. | high | no — MIT |
| Meta-variable structural matching and edit generation | ast-grep | Enables syntax-aware search and rewrites without text heuristics. | high | no — MIT |
| Positional trigram postings with delta shards/tombstones | zoekt | Fast lexical candidate narrowing and incremental index updates. | high | no — Apache-2.0 |
| Three-tier output parsers plus never-worse guard | rtk | Makes compression fail-safe and lets parsers degrade explicitly. | med | no — Apache-2.0 |
| Lazy tool schemas with checksum/TTL cache | mcp-context-proxy | Reduces MCP manifest cost without changing upstream tools. | med | no — MIT |
| Two-meta-tool discovery and namespaced lazy MCP catalog | dynamic-discovery-mcp | Keeps tool manifests small while retaining broad MCP reach. | med | no — MIT |
