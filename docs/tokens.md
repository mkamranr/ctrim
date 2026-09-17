# Token counting

`ctrim` reports what it saved. That number has to be trustworthy, and it has to
be cheap enough to compute on a gigabyte of logs.

## The default: a fitted estimator

No vocabulary, no dependency, one pass over the bytes:

```
tokens = 1.05 × letter_runs
       + 0.95 × digits
       + 0.65 × punctuation
       + 1.50 × escape_characters
       + 1.00 × non_ascii_characters
       + 0.35 × newlines
```

The weights come from a least-squares fit against `o200k_base` over this
repository's fixture corpus and source tree. Accuracy on that corpus:

| Input | Error |
| :--- | ---: |
| Docker retry log | +5% |
| `git diff` | +12% |
| `cargo check`, coloured | −12% |
| Jest stack trace | −2% |
| pytest run | +13% |
| Rust source | ±10% |

Worst case 13%, mean 7%. Reported numbers carry a `~` so they never claim more
precision than they have:

```
[ctrim] pytest: ~1,646 -> ~1,145 tokens (-30.4%) | 94 -> 68 lines
```

### Why these features

Vocabularies map common words to one token, so a **letter run** costs about one
token regardless of length. **Digits** tokenize far more densely — timestamps
and durations are expensive. **Punctuation** mostly costs a token each, less
when it merges. An **escape character** drags a whole colour sequence with it,
which is why stripping ANSI saves so much on coloured build output.

### Fixed-point arithmetic

Counting is done in hundredths of a token and rounded once at the end. Rounding
per line instead would drift by thousands of tokens over a large log:

```rust
let mut counter = ctrim::token::Counter::new();
for line in lines { counter.add_line(line); }
counter.tokens()   // matches counting the whole text in one call
```

## Exact counts

```bash
cargo install ctrim --features exact-tokens
```

This links `tiktoken-rs` and counts with `o200k_base`, dropping the `~` from the
summary. It costs roughly 4 MB of binary and a slower first call.

Several vendors keep their tokenizer private, so even these counts approximate
those models. They are exact for OpenAI `o200k` models.

## Refitting the weights

If you change the estimator, refit it rather than guessing:

```bash
cargo run --release --example calibrate --features exact-tokens -- \
  tests/fixtures/*.log src/*.rs src/processors/*.rs README.md
```

The example prints CSV — `chars, words, word_chars, punct, space_runs,
newlines, non_ascii, exact, estimate` — which is enough to run a least-squares
fit over the features and read off new weights.

Two tests hold the line, both under `--features exact-tokens`:

- `heuristic_tracks_exact_on_the_fixture_corpus` fails if the estimator drifts
  past 15% on any fixture.
- `calibration_report` (ignored by default) prints the per-fixture error:

```bash
cargo test --features exact-tokens -- --ignored --nocapture
```
