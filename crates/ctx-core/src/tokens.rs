//! Token accounting.
//!
//! Anthropic does not publish the Claude tokenizer, so we cannot count Claude
//! tokens exactly offline. Rather than silently guess, every [`TokenCount`]
//! records *which* tokenizer produced it, and reports carry that label. The
//! default is OpenAI's `o200k_base` BPE, used as a documented proxy.
//!
//! What we refuse to do: report `bytes / 4` as a token count.

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

/// Counts tokens with a real BPE. Construction loads the vocabulary, which is
/// not free, so hold one counter and reuse it.
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

    pub fn count(&self, text: &str) -> TokenCount {
        // encode_ordinary: no special-token handling, so arbitrary file
        // content (which may contain "<|endoftext|>") cannot panic or be
        // miscounted.
        let n = self.bpe.encode_ordinary(text).len();
        TokenCount::new(n, self.tokenizer)
    }
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
}
