# License matrix — reference repos

Compliance record for Context Runtime (Apache-2.0). Reference repos live under `C:\Users\nithy\Desktop\aclones`. We must never absorb copyleft or source-available code into the permissive core.

## Summary table

| Repo | Declared license | Evidence file(s) | OSI-approved? | Safe to copy code into an Apache-2.0 core? | Attribution required if reused | Risk level |
|---|---|---|---|---|---|---|
| aider | Apache-2.0 | LICENSE.txt; pyproject classifier | Yes | Yes | Yes (NOTICE) | none |
| serena | MIT | LICENSE; pyproject classifier | Yes | Yes | Yes (retain MIT notice) | none |
| ast-grep | MIT | LICENSE; Cargo.toml `license = "MIT"` | Yes | Yes | Yes (retain MIT notice) | none |
| tree-sitter | MIT | LICENSE; Cargo.toml | Yes | Yes | Yes (retain MIT notice) | none |
| scip | Apache-2.0 | LICENSE | Yes | Yes | Yes (NOTICE) | none |
| probe | CONFLICT (LICENSE Apache-2.0 vs Cargo.toml MIT) | LICENSE; Cargo.toml | Unclear | No — resolve conflict first | N/A until resolved | blocker |
| repomix | MIT | package.json | Yes | Yes | Yes (retain MIT notice) | low |
| zoekt | Apache-2.0 | LICENSE | Yes | Yes | Yes (NOTICE) | none |
| mcp-context-proxy | MIT | LICENSE | Yes | Yes | Yes (retain MIT notice) | none |
| dynamic-discovery-mcp | MIT (weak evidence) | package.json only; no LICENSE file | Claimed; grant unverifiable | No — unverifiable grant | N/A until LICENSE present | blocker |
| mini-swe-agent | MIT | LICENSE.md | Yes | Yes | Yes (retain MIT notice) | none |
| SWE-agent | MIT | LICENSE | Yes | Yes | Yes (retain MIT notice) | none |
| context-mode | Elastic-2.0 (ELv2) | package.json `"license": "Elastic-2.0"` | No (source-available) | No | N/A — do not reuse | blocker |
| rtk | Apache-2.0 | LICENSE; Cargo.toml | Yes | Yes | Yes (NOTICE) | none |
| caveman | AMBIGUOUS (MIT + BSL) | LICENSE (MIT); LICENSE.BSL | Unclear | No — resolve ambiguity first | N/A until resolved | blocker |
| claude-context | MIT | package.json | Yes | Yes | Yes (retain MIT notice) | low |
| claude-token-optimizer | MIT | LICENSE | Yes | Yes | Yes (retain MIT notice) | none |

## Compatibility rules we follow

**Apache-2.0 inbound into an Apache-2.0 project.** Permitted. Keep copyright and NOTICE attribution for any reused files. Preserve existing NOTICE entries when combining.

**MIT inbound into Apache-2.0.** Permitted. Retain the MIT copyright notice and permission text for any reused files. Do not strip upstream copyright headers.

**ELv2 (context-mode).** Elastic License 2.0 is source-available, not OSI-approved. It restricts providing the software as a managed service. It is not compatible with our permissive Apache-2.0 core. Read for ideas only. Clean-room reimplementation only. Never copy code, comments, tests, or fixtures.

**BSL (caveman's LICENSE.BSL).** Business Source License is time-delayed and use-restricted. Treat as a blocker until the MIT-vs-BSL ambiguity is resolved upstream. Do not copy any caveman source until that is clear.

**Ideas vs expression.** Architectural ideas and observable behaviour are not copyrightable; specific source expression is. Our default is clean-room reimplementation from documented behaviour, not transcription of upstream source.

## Blockers

- **context-mode** — ELv2 source-available; not compatible with a permissive Apache-2.0 core.
- **caveman** — LICENSE.BSL present alongside MIT; ambiguity unresolved; treat as blocker.
- **probe** — LICENSE file is Apache-2.0, Cargo.toml declares MIT; resolve conflict before any reuse.
- **dynamic-discovery-mcp** — no LICENSE file; package.json MIT claim alone is an unverifiable grant.

## Clean-room protocol

When a reference repo inspired a feature:

1. Read the reference and take notes on observable behaviour only.
2. Close the upstream source before writing any Context Runtime code. Do not keep it open side-by-side.
3. Describe the desired behaviour in an issue (inputs, outputs, edge cases). No pasted upstream snippets.
4. Implement from that description alone.
5. Cite the inspiration in the commit message (repo name and what was learned).
6. Never paste upstream code, comments, test fixtures, or identifiers into this tree.

Even permissively-licensed test fixtures and data files are avoided, because they carry their own provenance.

## Attribution obligations

If we reuse code (not merely learn from behaviour) from a permissive repo:

- **Apache-2.0 sources:** record copyright holders and any required NOTICE text in our top-level `NOTICE` file; keep file-level license headers where present.
- **MIT sources:** retain the MIT copyright notice and permission text with the reused material; record the source in `NOTICE` when the reuse is non-trivial.

As of this document we reuse none of the reference repos' code.

## Open questions

- Resolve the **probe** LICENSE (Apache-2.0) vs Cargo.toml (`MIT`) conflict with upstream before any code reuse.
- Resolve the **caveman** MIT vs LICENSE.BSL ambiguity with upstream before any code reuse.
- Confirm whether **dynamic-discovery-mcp**'s package.json `"license": "MIT"` is a sufficient grant without a LICENSE file (default: no).
- This document is engineering analysis, not legal advice. A human should confirm the classifications above before shipping any reused third-party code.

Last verified: 2026-09-12. This is engineering analysis, not legal advice.
