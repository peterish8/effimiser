Read-only investigation. Do NOT edit, create, or delete any file.

CONTEXT
Repo: C:/Users/nithy/Desktop/Effimiser (Rust). The crate `crates/ctx-index`
builds a tree-sitter symbol index for Rust. `crates/ctx-bench/src/bin/symbols.rs`
benchmarks it against ripgrep and includes an "agreement" check comparing the
set of (file, line) definition sites each one reports.

The agreement check found the index MISSING sites that ripgrep reported:
4 of 40 symbols on ast-grep, 7 of 40 on probe, 2 on Effimiser itself.

Known examples (path:line is what RIPGREP reported):
  Effimiser  crates/ctx-index/src/lib.rs:319   symbol Gamma
  Effimiser  crates/ctx-index/src/lib.rs:321   symbol Epsilon
  ast-grep   crates/language/src/lib.rs:159              symbol ALIAS
  ast-grep   crates/core/src/source.rs:148               symbol Underlying
  ast-grep   crates/outline/tests/typescript_outline_rules.rs:58   symbol readonly
  ast-grep   crates/outline/tests/jvm_swift_outline_rules.rs:165   symbol Protocol
  probe      lsp-daemon/src/indexing/manager.rs:5349     symbol MathOp
  probe      lsp-daemon/src/indexing/manager.rs:5550     symbol MathOp
  probe      tests/go_outline_format_tests.rs:31         symbol CalculatorInterface
  probe      tests/swift_outline_format_tests.rs:2639    symbol AsyncDataIterator
  probe      tests/swift_outline_format_tests.rs:3190    symbol ComplexDataStructure

The other repos are at C:/Users/nithy/Desktop/aclones/ast-grep and
C:/Users/nithy/Desktop/aclones/probe.

YOUR TASK
For EACH example above, open the file at that line, read enough surrounding
context to be sure, and classify it into exactly one of:

  A) RIPGREP_FALSE_POSITIVE - the match is inside a Rust string literal
     (often a raw string r#"..."# holding test-fixture source in another
     language), or inside a comment. It is not a real Rust definition in this
     file, so the index is CORRECT to skip it.

  B) INDEX_BUG_MISSING_NODE_KIND - it is a genuine Rust definition that the
     extractor fails to handle. Name the exact tree-sitter node kind involved
     (e.g. `associated_type`, `const_item` inside a trait, `macro_rules!`,
     a definition inside a macro invocation body). Read
     crates/ctx-index/src/symbols.rs to see which node kinds the walker
     currently matches.

  C) LINE_OFFSET - both found the same definition but report different line
     numbers (e.g. one points at an attribute or doc comment, the other at the
     `fn`/`struct` keyword line). Give both line numbers.

  D) OTHER - explain.

Then answer these two questions:

  Q1: Which node kinds does crates/ctx-index/src/symbols.rs walk(), and which
      Rust definition kinds are therefore NOT indexed? Give a complete list of
      what is missing (associated types, associated consts, trait method
      signatures, macro_rules, items generated inside macro bodies, extern
      blocks, impl blocks themselves, etc).

  Q2: Is the benchmark's agreement metric itself flawed? It compares exact
      (path, line) sets. Would a line-number difference for the same definition
      be miscounted as a miss? Look at how `rg_sites` parses ripgrep output in
      crates/ctx-bench/src/bin/symbols.rs, especially the Windows drive-letter
      handling, and say whether it can mis-parse paths.

OUTPUT FORMAT - be concise and concrete, no preamble:

CLASSIFICATIONS:
<one line per example: path:line symbol -> CATEGORY - one-sentence evidence>

TALLY: A=<n> B=<n> C=<n> D=<n>

Q1_MISSING_NODE_KINDS:
<list>

Q2_METRIC_FLAWED: YES|NO
<explanation>

MOST_IMPORTANT_FIX:
<the single change that would most improve index accuracy>
