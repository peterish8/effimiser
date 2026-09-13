//! Throwaway profiling probe for iteration 3. Breaks `TokenCounter::new` into
//! its constituent stages so the dominant cost is measured, not guessed.
//!
//! Not part of the benchmark suite: it reimplements the tiktoken-rs loader
//! stages purely to attribute time to each. Delete after use.

use base64::{engine::general_purpose, Engine};
use rustc_hash::FxHashMap;
use std::time::Instant;

const PATTERN: &str = r"[^\r\n\p{L}\p{N}]?[\p{Lu}\p{Lt}\p{Lm}\p{Lo}\p{M}]*[\p{Ll}\p{Lm}\p{Lo}\p{M}]+(?i:'s|'t|'re|'ve|'m|'ll|'d)?|[^\r\n\p{L}\p{N}]?[\p{Lu}\p{Lt}\p{Lm}\p{Lo}\p{M}]+[\p{Ll}\p{Lm}\p{Lo}\p{M}]*(?i:'s|'t|'re|'ve|'m|'ll|'d)?|\p{N}{1,3}| ?[^\s\p{L}\p{N}]+[\r\n/]*|\s*[\r\n]+|\s+(?!\S)|\s+";

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

fn main() -> anyhow::Result<()> {
    // Stage 0: the whole thing, for reference against the recorded baseline.
    let t = Instant::now();
    let bpe = tiktoken_rs::o200k_base()?;
    let whole = ms(t);
    std::hint::black_box(&bpe);
    drop(bpe);

    // Stage 1: obtain the vocabulary text. The crate `include_str!`s it, so
    // this disk read is an upper bound on a cost the crate does not pay.
    let asset = std::env::var("O200K_ASSET").expect("set O200K_ASSET to the .tiktoken file");
    let t = Instant::now();
    let text = std::fs::read_to_string(&asset)?;
    let read = ms(t);

    // Stage 2: base64-decode every line and build the encoder map.
    let t = Instant::now();
    let mut encoder: FxHashMap<Vec<u8>, u32> = FxHashMap::default();
    for line in text.lines() {
        let mut parts = line.split(' ');
        let raw = parts.next().unwrap();
        let token = general_purpose::STANDARD.decode(raw)?;
        let rank: u32 = parts.next().unwrap().parse().unwrap();
        encoder.insert(token, rank);
    }
    let build_encoder = ms(t);
    let n = encoder.len();

    // Stage 3: compile the pretokenizer regex once.
    let t = Instant::now();
    let re = fancy_regex::Regex::new(PATTERN).unwrap();
    let compile_regex = ms(t);

    // Stage 4: the 128 thread-local clones CoreBPE::new makes (twice: the
    // pretokenizer regex and the special-token regex).
    let t = Instant::now();
    let clones: Vec<fancy_regex::Regex> = (0..128).map(|_| re.clone()).collect();
    let clone_regex = ms(t);
    std::hint::black_box(&clones);

    // Stage 5: the decoder map CoreBPE::new builds. A counter never decodes.
    let t = Instant::now();
    let decoder: FxHashMap<u32, Vec<u8>> = encoder.iter().map(|(k, v)| (*v, k.clone())).collect();
    let build_decoder = ms(t);
    std::hint::black_box(&decoder);

    // Stage 6: sorted_token_bytes, used only by _encode_unstable.
    let t = Instant::now();
    let mut sorted: Vec<Vec<u8>> = encoder.keys().cloned().collect();
    sorted.sort();
    let sort_tokens = ms(t);
    std::hint::black_box(&sorted);

    println!("vocab entries          {n}");
    println!("--- attributed stages (single run, ms) ---");
    println!("whole o200k_base()     {whole:8.1}");
    println!("read asset from disk   {read:8.1}   (include_str! avoids this)");
    println!("decode+build encoder   {build_encoder:8.1}   REQUIRED");
    println!("compile regex          {compile_regex:8.1}   REQUIRED");
    println!("regex clones (128 x2)  {:8.1}   avoidable", clone_regex * 2.0);
    println!("build decoder map      {build_decoder:8.1}   avoidable");
    println!("sorted_token_bytes     {sort_tokens:8.1}   avoidable");
    let required = build_encoder + compile_regex;
    let avoidable = clone_regex * 2.0 + build_decoder + sort_tokens;
    println!("--- required {required:.1} ms, avoidable {avoidable:.1} ms ---");
    Ok(())
}
