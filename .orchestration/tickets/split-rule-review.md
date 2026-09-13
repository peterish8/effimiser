# Worker ticket: adversarial review of the token-count split rule

ROLE: independent correctness reviewer. You are NOT the author. The author
(Claude) wrote both the implementation and the tests that validate it, so your
job is to find what a self-review would miss.

SCOPE: read-only. Do not edit, create, or delete any file in the repository.

## The claim under review

File: crates/ctx-core/src/tokens.rs

`TokenCounter::count()` speeds up large inputs by splitting the text into
chunks, tokenizing each chunk on a separate thread with
`CoreBPE::encode_ordinary`, and summing the per-chunk token counts.

The claim is that this is EXACT, not approximate: the summed count always
equals the count of encoding the whole text in one call.

The claim rests entirely on the split rule in `safe_split_points` /
`next_boundary_at_or_after`:

    A split at byte offset i is safe iff
      text[i-1] == b'\n'
      AND text[i] < 0x80            (ASCII)
      AND text[i] is not ASCII whitespace
      AND text[i] != b'/'

The supporting argument is that tiktoken pretokenizes with a regex alternation
before any BPE merge, and BPE never merges across a pretokenizer piece, so the
split is safe exactly when no regex match can span offset i.

## The actual regexes (verified from tiktoken-rs 0.6.0 assets)

o200k_base (alternation, leftmost-first, in this order):

    [^\r\n\p{L}\p{N}]?[\p{Lu}\p{Lt}\p{Lm}\p{Lo}\p{M}]*[\p{Ll}\p{Lm}\p{Lo}\p{M}]+(?i:'s|'t|'re|'ve|'m|'ll|'d)?
    [^\r\n\p{L}\p{N}]?[\p{Lu}\p{Lt}\p{Lm}\p{Lo}\p{M}]+[\p{Ll}\p{Lm}\p{Lo}\p{M}]*(?i:'s|'t|'re|'ve|'m|'ll|'d)?
    \p{N}{1,3}
     ?[^\s\p{L}\p{N}]+[\r\n/]*
    \s*[\r\n]+
    \s+(?!\S)
    \s+

cl100k_base:

    '(?i:[sdmt]|ll|ve|re)|[^\r\n\p{L}\p{N}]?+\p{L}+|\p{N}{1,3}| ?[^\s\p{L}\p{N}]++[\r\n]*|\s++$|\s*[\r\n]|\s+(?!\S)|\s+

Engine: fancy-regex 0.13 (leftmost-first alternation, backtracking, Perl-like
semantics — NOT leftmost-longest).

## What I want from you

1. Try to break the claim. Construct a concrete input string where the rule
   permits a split and the summed count differs from the whole-text count.
   Pay particular attention to:
   - the ` ?[^\s\p{L}\p{N}]+[\r\n/]*` branch and its trailing class;
   - the possessive quantifiers and the end-anchored `\s++$` in cl100k_base
     (a chunk's end is not the real end of text);
   - the `(?!\S)` lookahead evaluated at a chunk boundary vs mid-text;
   - whether "BPE never merges across a pretokenizer piece" is actually true
     for these vocabularies, including `byte_pair_encode`'s handling of a
     single-byte piece and of pieces found whole in the encoder map;
   - multi-byte UTF-8 and combining marks (\p{M}) immediately after a newline;
   - \r\n pairs where the split would land between \r and \n (can the rule
     ever produce that?).

2. Separately assess whether the ASCII restriction on text[i] is load-bearing
   or merely conservative, and whether any NON-obvious byte besides '/' also
   needs excluding.

3. State whether the two differential tests in the file
   (`adversarial_strings_tokenize_identically_when_split`,
   `repository_sources_tokenize_identically_when_split`) would actually catch
   the failure modes you identified, or whether they have a blind spot.

## Deliverable

Answer in this exact structure, no preamble:

VERDICT: SOUND | UNSOUND | UNPROVEN
COUNTEREXAMPLE: <the exact Rust string literal, or "none found">
REASONING: <the specific regex branches you checked and why each stops at the
boundary, or why one does not>
TEST_BLIND_SPOTS: <concrete gaps, or "none">
CONFIDENCE: <high|medium|low> and what would raise it

Be concise and technical. Do not summarise the code back to me. If you cannot
establish soundness, say UNPROVEN rather than guessing SOUND.
