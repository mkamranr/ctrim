//! Prints the features the token estimator uses against exact BPE counts, so
//! [`ctrim::token::heuristic`]'s constants can be refitted on real data.
//!
//! ```sh
//! cargo run --release --example calibrate --features exact-tokens -- tests/fixtures/*.log src/*.rs
//! ```
//!
//! Output is CSV: `file,chars,words,word_chars,punct,space_runs,newlines,non_ascii,exact,estimate`.

fn main() {
    #[cfg(not(feature = "exact-tokens"))]
    {
        eprintln!("build with --features exact-tokens");
        std::process::exit(2);
    }

    #[cfg(feature = "exact-tokens")]
    {
        println!("file,chars,words,word_chars,punct,space_runs,newlines,non_ascii,exact,estimate");
        for path in std::env::args().skip(1) {
            let Ok(text) = std::fs::read_to_string(&path) else {
                eprintln!("skipping {path}");
                continue;
            };
            let f = features(&text);
            println!(
                "{},{},{},{},{},{},{},{},{},{}",
                path,
                text.len(),
                f.words,
                f.word_chars,
                f.punct,
                f.space_runs,
                f.newlines,
                f.non_ascii,
                ctrim::token::bpe::count(&text),
                ctrim::token::heuristic::count(&text),
            );
        }
    }
}

#[cfg(feature = "exact-tokens")]
#[derive(Default)]
struct Features {
    words: usize,
    word_chars: usize,
    punct: usize,
    space_runs: usize,
    newlines: usize,
    non_ascii: usize,
}

#[cfg(feature = "exact-tokens")]
fn features(text: &str) -> Features {
    let mut f = Features::default();
    let mut in_word = false;
    let mut in_space = false;
    for ch in text.chars() {
        let word_char = ch.is_ascii_alphanumeric() || ch == '_';
        if word_char {
            if !in_word {
                f.words += 1;
            }
            f.word_chars += 1;
        }
        in_word = word_char;
        if !word_char {
            match ch {
                '\n' => {
                    f.newlines += 1;
                    in_space = false;
                }
                ' ' | '\t' | '\r' => {
                    if !in_space {
                        f.space_runs += 1;
                    }
                    in_space = true;
                }
                c if c.is_ascii() => {
                    f.punct += 1;
                    in_space = false;
                }
                _ => {
                    f.non_ascii += 1;
                    in_space = false;
                }
            }
        } else {
            in_space = false;
        }
    }
    f
}
