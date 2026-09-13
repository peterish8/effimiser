# Evaluation — TencentDB-Agent-Memory

**Repo:** https://github.com/TencentCloud/TencentDB-Agent-Memory
**Evaluated:** 2026-09-13 · v2.0.1 · MIT · 26.5k stars · TypeScript 89.6%, Python 3.7%
**Verdict: do not adopt. Borrow two design ideas. Keep as a possible
co-installed neighbour, not a dependency.**

## What it actually is

A **team-level memory hub** for agents. Four asset types — Chat Memory, Skill,
Wiki, CodeGraph — extracted from conversations, documents and repositories,
then governed (owner / version / visibility / ACL) and "equipped" to specific
agents.

Deployment is three services plus a panel (`memory-core`, `memory-hub`,
`memory-proxy`, port 8125), started via `docker compose` from
`deploy/global-images`. Integration is by **HTTP proxy**: you repoint the
agent's base URL at the proxy. No MCP server, no plugin, no hook.

Its memory model is layered and distilled asynchronously:

| Layer | Contents |
|---|---|
| L0 Conversation | raw conversations, full context |
| L1 Atom | extracted facts, preferences, constraints, events |
| L2 Scenario | knowledge blocks per project/scenario |
| L3 Core / Persona | long-term profile and stable patterns |

Retrieval is BM25 + vector + RRF, capped by item count, character budget and
timeout.

## Do we already have this?

**No — and mostly we are not trying to.** Mapping it onto our roadmap:

| Their asset | Our equivalent | State |
|---|---|---|
| CodeGraph (symbols, calls, impact paths) | Phase 3 code intelligence | planned, not built |
| Chat Memory (L0–L3 distillation) | Phase 10 typed project memory | planned, not built |
| Wiki (docs → linked pages) | nothing planned | genuine gap, deliberate |
| Skill (versioned, reusable procedures) | nothing planned | out of scope |
| Hub panel, teams, ACLs | nothing planned | out of scope |

So there are two real overlaps (CodeGraph, Chat Memory) and three things we
have no plans for.

## Why not adopt it

**1. It breaks our hardest constraint.** benchmark-plan.md target #12 says core
functionality must need **0 mandatory cloud services**, and names the exact
list to avoid: hosted vector DB, Redis, Postgres, embedding API, LLM
summarisation service. This project's install step is *"fill in two sets of LLM
parameters (memory group + proxy group)"* — the distillation pipeline is an LLM
summarisation service, and retrieval uses vector search. It also ships a
MongoDB backend. Adopting it would not bend that constraint, it would delete
it, and "runs entirely locally with no API keys" is one of the few things that
currently distinguishes this project.

**2. It solves a different problem.** Ours is *within-session context
efficiency*: terminal output compression, smart and delta reads, MCP schema
virtualisation, raw recoverability behind handles, honest token accounting.
Theirs is *cross-session knowledge accumulation*: what the agent should still
know next week. Almost nothing in their design reduces the tokens burned in the
session you are currently in.

**3. Stack mismatch.** Three TypeScript services in Docker versus one ~2k-line
Rust binary with a SQLite store. There is no version of "integrate this" that
leaves the deployment story intact.

**4. Their headline number does not measure what we optimise.** The only
published benchmark is PersonaMem 48% → 76%. PersonaMem tests whether an agent
correctly applies user information after long interactions. It is a recall
benchmark. There is no published measurement of token reduction, tool-call
reduction, or latency — so the honest answer to "how much benefit would this
bring us" is **unknown, and their evidence does not bear on our targets.**
Quoting +59% as though it were a benefit to this project would be exactly the
kind of borrowed claim the claim policy forbids.

## What is worth stealing

**1. The L0→L3 layered distillation, for Phase 10.** Our roadmap says "typed
project memory" without specifying granularity. Their split is better thought
out than that: keep raw conversations for provenance, extract atomic facts for
precise recall, group into scenarios for fast context restore, and maintain a
stable top layer for cold start. Crucially the *retrieval* is layered too —
serve L2/L3 by default, fall back to L1/L0 only when a specific fact is needed.
That is the same "cheapest sufficient method" principle our router already has,
applied to memory. We can implement this with SQLite FTS5 and no LLM by
extracting facts deterministically rather than by summarisation, which keeps
target #12 intact.

**2. Asset governance as a first-class concern.** Ownership, version, status,
and explicit visibility (private by default, sharing as an explicit action).
Our Phase 10 notes say memory must support supersession; theirs shows the
fuller shape. Worth copying the *model*, even single-user, because "which
version of this decision is current" is the question that makes stale memory
dangerous.

**3. PersonaMem as an eval target** if Phase 10 ever ships — an external,
published benchmark we did not design ourselves is worth more than one we did.

## The non-obvious option

Because they integrate by **base-URL proxy** and we integrate by **MCP tools**,
the two can run simultaneously without touching each other. A user could point
their agent at the memory proxy *and* load our MCP tools; neither intercepts
the other. So this is not a build-or-buy decision at all. If a user wants
cross-session team memory, they can already have both, and we do not need to
grow a Wiki or a Skill library to compete.

That also means the interesting future question is not "should we absorb this"
but "does our CodeGraph-equivalent in Phase 3 duplicate theirs badly enough to
matter" — and the answer depends on whether ours can hit the p50 <50 ms symbol
lookup target that they publish no figure for.

## Decision

Do not adopt, do not vendor, do not depend on. Revisit only if the offline
constraint is ever deliberately relaxed, which would be a change to the
project's identity and not a technical detail. Record the two design ideas
against Phase 10 in the roadmap.
