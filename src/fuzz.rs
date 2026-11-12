//! Test-only helpers for property/fuzz tests (#119): a tiny deterministic
//! generator (no extra dependency, and a failing case reproduces from its
//! seed) and a corpus of strings that tend to break quoting code.
//!
//! Not compiled outside `cargo test`.

/// xorshift64*: small, fast, and good enough to wander through odd inputs.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        // xorshift must not start at 0
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// 0..n
    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    pub fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[self.below(items.len())]
    }
}

/// Characters that matter to the args splitter, to cmd.exe and to humans
/// pasting things, over-represented so short strings hit the edge cases.
const NASTY_CHARS: &[char] = &[
    ' ', ' ', '\t', '"', '"', '\'', '\'', '\\', '\\', '\\', '{', '}', '&', '|', '%', '^', '<', '>',
    ';', '`', '$', '(', ')', '=', '-', '/', ':', '.', '*', '\n', '\r', 'a', 'b', 'x', '0', 'é',
    '日', '\u{200b}',
];

/// A random string of 0..=`max_len` characters, mostly nasty ones.
pub fn nasty_string(rng: &mut Rng, max_len: usize) -> String {
    let len = rng.below(max_len + 1);
    (0..len).map(|_| rng.pick(NASTY_CHARS)).collect()
}

/// Strings that have broken (or nearly broken) argument quoting before.
pub const CORPUS: &[&str] = &[
    "",
    " ",
    "\"",
    "'",
    "\\",
    "\\\\",
    "\\\"",
    "\\'",
    "a b\\",
    "a b\\\\",
    "C:\\Program Files\\x\\",
    "C:\\dir with space\\\"",
    "x\" --script \"evil",
    "x' --script 'evil",
    "10.10.10.3 -oN C:\\Windows\\evil.txt",
    "a & calc.exe",
    "a | calc.exe",
    "a && calc.exe",
    "%PATH%",
    "%COMSPEC% /c calc",
    "^&",
    "\"; calc; \"",
    "$(calc)",
    "`calc`",
    "line1\nline2",
    "line1\r\nline2",
    "{target}",
    "{file} {out}",
    "{",
    "}",
    "{}",
    "\u{200b}",
    "日本語 パス",
    "-- --help",
    "--opt=\"quoted value\"",
    "tab\tseparated",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generator_is_deterministic_and_varied() {
        let a: Vec<u64> = {
            let mut r = Rng::new(7);
            (0..8).map(|_| r.next()).collect()
        };
        let b: Vec<u64> = {
            let mut r = Rng::new(7);
            (0..8).map(|_| r.next()).collect()
        };
        assert_eq!(a, b);
        let mut r = Rng::new(8);
        assert_ne!(a[0], r.next());
        let mut r = Rng::new(0);
        assert_ne!(r.next(), r.next(), "seed 0 must not get stuck");
    }

    #[test]
    fn nasty_strings_cover_the_awkward_characters() {
        let mut r = Rng::new(1);
        let all: String = (0..400).map(|_| nasty_string(&mut r, 12)).collect();
        for c in ['"', '\'', '\\', ' ', '&', '%'] {
            assert!(all.contains(c), "never generated {c:?}");
        }
    }
}
