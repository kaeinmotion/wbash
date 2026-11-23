/// Fast deterministic token estimator. Character-class based with no network
/// calls or tokenizer dependency.
///
/// Coefficients are calibrated against a pinned `cl100k_base` tokenizer over
/// `corpus/scrubbed` (see `crates/wxec-core/examples/calibrate.rs` and
/// `crates/wxec-core/tests/calibration.rs`). The spec gate is MAPE <= 10%.
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
        let bytes = text.as_bytes();
        let mut i = 0;
        let len = bytes.len();
        while i < len {
            let b = bytes[i];
            if b < 128 {
                let word = b.is_ascii_alphanumeric() || b == b'_';
                if word && !in_word {
                    f.words += 1;
                }
                in_word = word;
                if !word && !b.is_ascii_whitespace() {
                    f.punct += 1;
                }
                if b.is_ascii_uppercase() {
                    f.upper += 1;
                }
                i += 1;
            } else {
                let ch = text[i..].chars().next().unwrap_or('\0');
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
                f.non_ascii += 1;
                i += ch.len_utf8();
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
        let bytes = line.as_bytes();
        let mut i = 0;
        let len = bytes.len();
        while i < len {
            let b = bytes[i];
            if b < 128 {
                let word = b.is_ascii_alphanumeric() || b == b'_';
                if word && !in_word {
                    f.words += 1;
                }
                in_word = word;
                if !word && !b.is_ascii_whitespace() {
                    f.punct += 1;
                }
                if b.is_ascii_uppercase() {
                    f.upper += 1;
                }
                i += 1;
            } else {
                let ch = line[i..].chars().next().unwrap_or('\0');
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
                f.non_ascii += 1;
                i += ch.len_utf8();
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
