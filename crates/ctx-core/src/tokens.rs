//! Token accounting.
//!
//! Anthropic does not publish the Claude tokenizer, so we cannot count Claude
//! tokens exactly offline. Rather than silently guess, every [`TokenCount`]
//! records *which* tokenizer produced it, and reports carry that label. The
//! default is OpenAI's `o200k_base` BPE, used as a documented proxy.
//!
//! What we refuse to do: report `bytes / 4` as a token count.
//!
//! # Why counting is parallel
//!
//! Iteration 3 measured single-threaded counting at ~2.5 MiB/s warm, which
//! puts a 40 MiB build log — the exact workload this runtime exists to
//! compress — at roughly 16 seconds of local CPU just to *measure* it. Local
//! CPU is not free, and a runtime that cannot afford to count honestly is a
//! runtime that will eventually be tempted to estimate. So counting is
//! parallelised, under a split rule that is exact rather than approximate.
//! See [`safe_split_points`].

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tiktoken_rs::CoreBPE;

/// Which tokenizer produced a count. Carried into every benchmark artifact so
/// no number is ever ambiguous about its provenance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tokenizer {
    /// OpenAI `o200k_base`. Proxy for Claude; exact for GPT-4o/5-family.
    O200kBase,
    /// OpenAI `cl100k_base`. Proxy; exact for GPT-4/3.5.
    Cl100kBase,
}

impl Tokenizer {
    pub fn as_str(self) -> &'static str {
        match self {
            Tokenizer::O200kBase => "o200k_base",
            Tokenizer::Cl100kBase => "cl100k_base",
        }
    }

    /// True when this tokenizer is an approximation for the model in question
    /// rather than its real tokenizer. Used to label reports.
    pub fn is_proxy_for_claude(self) -> bool {
        true
    }
}

/// A token count that knows how it was produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenCount {
    pub tokens: usize,
    pub tokenizer: Tokenizer,
}

impl TokenCount {
    pub fn new(tokens: usize, tokenizer: Tokenizer) -> Self {
        Self { tokens, tokenizer }
    }
}

/// Inputs at or below this size are counted on the calling thread. Below it
/// the work is smaller than the cost of handing it to other threads.
const PARALLEL_THRESHOLD: usize = 128 * 1024;

/// Smallest slice worth giving to a worker thread.
const MIN_CHUNK: usize = 64 * 1024;

/// Upper bound on worker threads, so counting a large log cannot monopolise a
/// big machine while the agent is waiting on something else.
const MAX_WORKERS: usize = 16;

/// Counts tokens with a real BPE. Construction loads the vocabulary, which is
/// not free (~350 ms measured), so hold one counter and reuse it.
#[derive(Clone)]
pub struct TokenCounter {
    bpe: Arc<CoreBPE>,
    tokenizer: Tokenizer,
}

impl std::fmt::Debug for TokenCounter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenCounter")
            .field("tokenizer", &self.tokenizer)
            .finish_non_exhaustive()
    }
}

impl TokenCounter {
    pub fn new(tokenizer: Tokenizer) -> anyhow::Result<Self> {
        let bpe = match tokenizer {
            Tokenizer::O200kBase => tiktoken_rs::o200k_base()?,
            Tokenizer::Cl100kBase => tiktoken_rs::cl100k_base()?,
        };
        Ok(Self {
            bpe: Arc::new(bpe),
            tokenizer,
        })
    }

    /// The default counter used across the runtime.
    pub fn default_counter() -> anyhow::Result<Self> {
        Self::new(Tokenizer::O200kBase)
    }

    pub fn tokenizer(&self) -> Tokenizer {
        self.tokenizer
    }

    /// Count tokens in `text`.
    ///
    /// Exact: for large inputs the work is split across threads, but only at
    /// boundaries where the tokenizer provably cannot produce a token that
    /// spans the split, so the result is bit-identical to [`Self::count_serial`].
    /// That equivalence is asserted by differential tests over this repository's
    /// own sources and over adversarial strings built for each branch of the
    /// pretokenizer regex.
    pub fn count(&self, text: &str) -> TokenCount {
        if text.len() <= PARALLEL_THRESHOLD {
            return self.count_serial(text);
        }

        let workers = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
            .min(MAX_WORKERS)
            .min(text.len() / MIN_CHUNK)
            .max(1);
        if workers < 2 {
            return self.count_serial(text);
        }

        let points = safe_split_points(text, text.len().div_ceil(workers));
        if points.is_empty() {
            // No safe boundary anywhere — e.g. a minified single-line bundle.
            // Correctness wins: count it serially and take the slower path
            // rather than risk a boundary-crossing token.
            return self.count_serial(text);
        }

        let mut chunks: Vec<&str> = Vec::with_capacity(points.len() + 1);
        let mut start = 0usize;
        for &p in &points {
            chunks.push(&text[start..p]);
            start = p;
        }
        chunks.push(&text[start..]);

        let total: usize = std::thread::scope(|scope| {
            let handles: Vec<_> = chunks
                .iter()
                .map(|chunk| scope.spawn(move || self.encode_len(chunk)))
                .collect();
            handles
                .into_iter()
                .map(|h| h.join().expect("token counting worker panicked"))
                .sum()
        });

        TokenCount::new(total, self.tokenizer)
    }

    /// Single-threaded count. Kept public because it is the reference the
    /// parallel path is tested against, and the A/B arm in the benchmark.
    pub fn count_serial(&self, text: &str) -> TokenCount {
        TokenCount::new(self.encode_len(text), self.tokenizer)
    }

    /// The token sequence, not just its length. Tests use this to assert the
    /// stronger property: splitting preserves the *tokenization*, not merely
    /// the count. Two different tokenizations can coincidentally share a
    /// length, so a count-only check could pass over a real defect — a blind
    /// spot found by independent review in iteration 3.
    #[cfg(test)]
    fn encode_tokens(&self, text: &str) -> Vec<u32> {
        self.bpe.encode_ordinary(text)
    }

    fn encode_len(&self, text: &str) -> usize {
        // encode_ordinary: no special-token handling, so arbitrary file
        // content (which may contain "<|endoftext|>") cannot panic or be
        // miscounted.
        self.bpe.encode_ordinary(text).len()
    }
}

/// Byte offsets at which `text` may be split without changing its tokenization.
///
/// # Why these boundaries are safe
///
/// Both supported vocabularies pretokenize with a regex alternation before any
/// BPE merging happens, and BPE never merges across a pretokenizer piece. So a
/// split is safe exactly when no regex match can span it. Offset `i` qualifies
/// when `text[i-1] == '\n'` and `text[i]` is ASCII, non-whitespace, and not
/// `/`. Taking the `o200k_base` alternation in order:
///
/// - The two letter branches begin with `[^\r\n\p{L}\p{N}]?` and then require
///   letters; none of their elements match `\n`, so no such match contains the
///   newline at `i-1` at all.
/// - `\p{N}{1,3}` matches digits only.
/// - ` ?[^\s\p{L}\p{N}]+[\r\n/]*` can absorb the newline after punctuation, and
///   its trailing class would keep going into `text[i]` if that byte were
///   `\r`, `\n` or `/`. Excluding those three is what makes this branch stop
///   exactly at `i` — the reason `/` is in the rule and the reason a rule
///   written only in terms of whitespace would be wrong.
/// - `\s*[\r\n]+` stops at `i` because `text[i]` is not whitespace.
/// - `\s+(?!\S)` and `\s+` match whitespace only, so they too stop at `i`.
///
/// `cl100k_base`, as vendored in tiktoken-rs 0.6.0, is
/// `(?i:'s|'t|'re|'ve|'m|'ll|'d)|[^\r\n\p{L}\p{N}]?\p{L}+|\p{N}{1,3}|`
/// ` ?[^\s\p{L}\p{N}]+[\r\n]*|\s*[\r\n]+|\s+(?!\S)|\s+`. It is structurally the
/// same argument with a weaker punctuation tail (`[\r\n]*`, no `/`), so the
/// rule is strictly conservative for it. Note this differs from the spelling
/// published for upstream Python tiktoken, which uses possessive quantifiers
/// and an end-anchored `\s++$`; the pattern that matters is the one actually
/// compiled by the version we link, and that is the one quoted here.
///
/// Restricting `text[i]` to ASCII avoids reasoning about Unicode whitespace
/// such as U+00A0 and about combining marks, and costs nothing in practice
/// since it only skips a candidate boundary.
///
/// Returns an empty vector when no boundary exists, which the caller must
/// treat as "count serially" rather than as "split anywhere".
pub fn safe_split_points(text: &str, target_chunk: usize) -> Vec<usize> {
    if target_chunk == 0 || text.len() <= target_chunk {
        return Vec::new();
    }
    let bytes = text.as_bytes();
    let mut points = Vec::new();
    let mut cursor = target_chunk;

    while cursor < bytes.len() {
        match next_boundary_at_or_after(bytes, cursor) {
            Some(p) => {
                points.push(p);
                // Advance a full chunk past the boundary we just took, so
                // chunks stay comparable in size even when newlines are sparse.
                cursor = p.saturating_add(target_chunk);
            }
            None => break,
        }
    }
    points
}

/// First index `>= from` that satisfies the boundary rule, if any.
fn next_boundary_at_or_after(bytes: &[u8], from: usize) -> Option<usize> {
    let mut i = from.max(1);
    while i < bytes.len() {
        if bytes[i - 1] == b'\n' {
            let next = bytes[i];
            let safe_next =
                next < 0x80 && !(next as char).is_ascii_whitespace() && next != b'/';
            if safe_next {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_are_not_byte_quarters() {
        let c = TokenCounter::default_counter().unwrap();
        // A string where bytes/4 is badly wrong: dense punctuation tokenizes
        // far more finely than 4 bytes per token.
        let text = "!@#$%^&*()_+-=[]{};':\",./<>?".repeat(4);
        let got = c.count(&text).tokens;
        let naive = text.len() / 4;
        assert!(
            got > naive,
            "expected real tokenizer ({got}) to exceed bytes/4 ({naive}) on dense punctuation"
        );
    }

    #[test]
    fn empty_is_zero() {
        let c = TokenCounter::default_counter().unwrap();
        assert_eq!(c.count("").tokens, 0);
    }

    #[test]
    fn tokenizer_label_round_trips() {
        let c = TokenCounter::new(Tokenizer::Cl100kBase).unwrap();
        assert_eq!(c.count("hello world").tokenizer.as_str(), "cl100k_base");
    }

    #[test]
    fn special_token_text_does_not_panic() {
        let c = TokenCounter::default_counter().unwrap();
        assert!(c.count("<|endoftext|> in a source file").tokens > 0);
    }

    // --- split-rule properties ---------------------------------------------

    #[test]
    fn split_points_land_only_after_a_newline() {
        let text = "alpha beta\ngamma delta\n  indented\nend\n";
        for p in safe_split_points(text, 8) {
            assert_eq!(text.as_bytes()[p - 1], b'\n', "boundary {p} not after a newline");
            let next = text.as_bytes()[p];
            assert!(!(next as char).is_ascii_whitespace());
            assert_ne!(next, b'/');
        }
    }

    #[test]
    fn slash_after_newline_is_not_a_boundary() {
        // The `[\r\n/]*` tail of the punctuation branch can reach across a
        // newline into a following `/`. If this rule ever regresses, the
        // differential tests below would start failing intermittently on real
        // source files, so pin it directly.
        let text = "value;\n/comment\n";
        let nl = text.find('\n').unwrap();
        assert!(
            !safe_split_points(text, nl + 1).contains(&(nl + 1)),
            "must not split before a '/'"
        );
    }

    #[test]
    fn no_boundary_yields_no_split() {
        let text = "a".repeat(4096); // single line, no newline at all
        assert!(safe_split_points(&text, 512).is_empty());
    }

    // --- differential exactness --------------------------------------------

    /// The core claim: splitting never changes the *tokenization*. Checked by
    /// forcing splits at *every* legal boundary, which is far more aggressive
    /// than the production path, on inputs chosen to exercise each regex
    /// branch.
    ///
    /// Compares full token sequences rather than counts. Counts are what the
    /// API returns, but two different tokenizations can share a length, so a
    /// count-only assertion could stay green over a real defect.
    fn assert_split_invariant(c: &TokenCounter, text: &str) {
        let reference = c.encode_tokens(text);
        for chunk in [1usize, 2, 3, 7, 16, 64, 1000] {
            let points = safe_split_points(text, chunk);
            let mut joined: Vec<u32> = Vec::with_capacity(reference.len());
            let mut start = 0usize;
            for p in points {
                joined.extend(c.encode_tokens(&text[start..p]));
                start = p;
            }
            joined.extend(c.encode_tokens(&text[start..]));
            assert_eq!(
                joined,
                reference,
                "chunk={chunk} changed the tokenization ({} tokens vs {}) for {:?}",
                joined.len(),
                reference.len(),
                &text[..text.len().min(120)]
            );
        }
    }

    #[test]
    fn adversarial_strings_tokenize_identically_when_split() {
        let cases = [
            // punctuation tail reaching across the newline into '/'
            "let x = 1;\n/// doc comment\nfn f() {}\n",
            "a;\n/b;\n/c;\n/d\n",
            // whitespace runs spanning newlines
            "a\n\n\n\nb\n   \n\t\nc\n",
            "x\n \n  \n   \ny\n",
            // CRLF
            "line one\r\nline two\r\n\r\nline three\r\n",
            // newline immediately followed by punctuation, digits, letters
            "end.\n#include <stdio.h>\n42\nName\n",
            // trailing and leading newlines
            "\n\n\nalpha\n\n\n",
            // unicode right after a newline (boundary must be skipped safely)
            "alpha\nβγδ\nemoji 🙂\nnbsp\u{a0}here\ndone\n",
            // no trailing newline
            "no trailing newline at all\nsecond line",
            // markdown-ish and log-ish shapes
            "| a | b |\n|---|---|\n| 1 | 2 |\n",
            "ERROR: failed\n  at foo.rs:1\n  at bar.rs:2\nERROR: failed\n",
            // Combining marks (\p{M}) directly after a newline. The ASCII
            // restriction should refuse these as boundaries; if it were ever
            // relaxed, a mark could be separated from its base character.
            "base\n\u{0301}combining acute\ne\u{0301}\n\u{0300}\u{0301}\u{0302}\n",
            "a\u{0301}\nb\u{0327}\nc\n",
            // Unicode whitespace after a newline: U+00A0, U+2028, U+3000,
            // U+200B. `\s` in fancy-regex is Unicode-aware, so admitting these
            // as boundaries would be unsound.
            "x\n\u{a0}y\n\u{2028}z\n\u{3000}w\n\u{200b}v\n",
            // Unicode punctuation after a newline, exercising the punctuation
            // branch with a non-ASCII lead.
            "quote\n«guillemet»\n…ellipsis\n—dash\n",
            // RTL, CJK and emoji sequences with ZWJ, immediately after newlines.
            "ltr\nالعربية\n中文字符\n👨‍👩‍👧‍👦\n🇯🇵\n",
            // A '/' after a newline in the shapes it actually occurs in.
            "path\n/usr/bin/env\n//comment\n/* block */\n",
        ];
        for tok in [Tokenizer::O200kBase, Tokenizer::Cl100kBase] {
            let c = TokenCounter::new(tok).unwrap();
            for case in cases {
                assert_split_invariant(&c, case);
            }
        }
    }

    #[test]
    fn repository_sources_tokenize_identically_when_split() {
        // Real input beats synthetic input for this property. Walk the whole
        // repository, not just this crate: sources contain regexes, doc
        // comments, unicode and odd punctuation, and the markdown carries
        // tables, code fences and em-dashes. That is precisely the risky
        // material. Both tokenizers are checked — reviewing this in iteration 3
        // found cl100k_base was only ever exercised on short synthetic strings.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .expect("repository root")
            .to_path_buf();

        let mut corpus: Vec<String> = Vec::new();
        let mut bytes = 0usize;
        let mut stack = vec![root];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let name = entry.file_name();
                let name = name.to_string_lossy();
                // Skip build output and VCS internals; they are large and add
                // no linguistic variety.
                if path.is_dir() {
                    if !matches!(name.as_ref(), "target" | ".git" | "node_modules") {
                        stack.push(path);
                    }
                } else if path
                    .extension()
                    .is_some_and(|e| e == "rs" || e == "toml" || e == "md")
                {
                    if let Ok(text) = std::fs::read_to_string(&path) {
                        bytes += text.len();
                        corpus.push(text);
                    }
                }
            }
        }

        assert!(
            corpus.len() >= 10 && bytes >= 50_000,
            "expected a real corpus, got {} files / {bytes} bytes",
            corpus.len()
        );

        for tok in [Tokenizer::O200kBase, Tokenizer::Cl100kBase] {
            let c = TokenCounter::new(tok).unwrap();
            for text in &corpus {
                assert_split_invariant(&c, text);
            }
        }
    }

    #[test]
    fn parallel_path_matches_serial_on_a_large_input() {
        // Above PARALLEL_THRESHOLD, so this exercises the production path
        // (thread spawning included), not just the split helper. Both
        // tokenizers: the production path was previously only covered for
        // o200k_base.
        let unit = "fn handler(req: Request) -> Result<Response> {\n    let token = \
                    req.header(\"authorization\")?;\n    // note: refresh if expired\n    \
                    validate(token)?;\n}\n\n/// Résumé: naïve café — 中文 🙂\n";
        let mut text = String::with_capacity(PARALLEL_THRESHOLD * 3);
        while text.len() < PARALLEL_THRESHOLD * 2 {
            text.push_str(unit);
        }
        for tok in [Tokenizer::O200kBase, Tokenizer::Cl100kBase] {
            let c = TokenCounter::new(tok).unwrap();
            assert_eq!(
                c.count(&text).tokens,
                c.count_serial(&text).tokens,
                "production path diverged for {}",
                tok.as_str()
            );
        }
    }

    #[test]
    fn large_input_without_newlines_still_counts_correctly() {
        // Worst case for the split rule: no boundary exists, so the parallel
        // path must fall back rather than split unsafely.
        let c = TokenCounter::default_counter().unwrap();
        let text = "minified,".repeat(PARALLEL_THRESHOLD / 4);
        assert!(text.len() > PARALLEL_THRESHOLD);
        assert!(safe_split_points(&text, MIN_CHUNK).is_empty());
        assert_eq!(c.count(&text).tokens, c.count_serial(&text).tokens);
    }
}
