/// Fast deterministic token estimator. Character-class based with no network
/// calls or tokenizer dependency.
///
/// Coefficients are calibrated against a pinned `cl100k_base` tokenizer over
/// `corpus/scrubbed` (see `crates/wbash-core/examples/calibrate.rs` and
/// `crates/wbash-core/tests/calibration.rs`). The spec gate is MAPE <= 10%.
struct Features {
    words: u64,
    punct: u64,
    upper: u64,
    non_ascii: u64,
}

impl Features {
    fn of(text: &str) -> Self {
        let mut f = Features {
            words: 0,
            punct: 0,
            upper: 0,
            non_ascii: 0,
        };
        let mut in_word = false;
        for ch in text.chars() {
            let word = ch.is_alphanumeric() || ch == '_';
            if word && !in_word {
                f.words += 1;
            }
            in_word = word;
            if !word && !ch.is_whitespace() {
                f.punct += 1;
            }
            if ch.is_uppercase() {
                f.upper += 1;
            }
            if !ch.is_ascii() {
                f.non_ascii += 1;
            }
        }
        f
    }

    fn tokens(&self) -> u32 {
        let est = 1.518 * self.words as f64
            + 0.338 * self.punct as f64
            + 0.218 * self.upper as f64
            + 1.117 * self.non_ascii as f64;
        (est.round() as u64).max(1) as u32
    }
}

pub fn estimate_text(text: &str) -> u32 {
    if text.is_empty() {
        return 1;
    }
    Features::of(text).tokens()
}

pub fn estimate_lines(lines: &[String]) -> u32 {
    if lines.is_empty() {
        return 0;
    }
    let mut f = Features {
        words: 0,
        punct: 0,
        upper: 0,
        non_ascii: 0,
    };
    for line in lines {
        let mut in_word = false;
        for ch in line.chars() {
            let word = ch.is_alphanumeric() || ch == '_';
            if word && !in_word {
                f.words += 1;
            }
            in_word = word;
            if !word && !ch.is_whitespace() {
                f.punct += 1;
            }
            if ch.is_uppercase() {
                f.upper += 1;
            }
            if !ch.is_ascii() {
                f.non_ascii += 1;
            }
        }
    }
    f.tokens()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monotonic_and_nonzero() {
        assert_eq!(estimate_text(""), 1);
        assert_eq!(estimate_lines(&[]), 0);
        assert!(estimate_text("a") <= estimate_text("a longer sentence with words"));
        assert!(estimate_text("aaaa") <= estimate_text("aaaa aaaa aaaa"));
    }
}
