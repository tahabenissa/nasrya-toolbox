//! Turning the args line the user typed into an argv.
//!
//! The rules are deliberately small and Windows-friendly:
//! - whitespace separates arguments;
//! - `"double"` or `'single'` quotes group text, including spaces, and can
//!   join with unquoted text (`--x="a b"` is the single arg `--x=a b`);
//! - `\"` (and `\'`) is a literal quote, inside or outside quotes;
//! - every other backslash is literal, so `C:\Program Files\x` needs no
//!   doubling (POSIX shell rules would eat those backslashes);
//! - `""` / `''` is an explicit empty argument.
//!
//! One consequence: a quoted path can't end in a backslash (`"C:\dir\"`
//! reads as an escaped quote and is reported as unterminated). Drop the
//! trailing backslash: `"C:\dir"`.
//!
//! Quoting is only for the splitter: each resulting argument goes to
//! `Command::arg`, which re-escapes it correctly for the child process.

/// Split `line` into arguments. Errors on an unterminated quote.
pub fn split(line: &str) -> Result<Vec<String>, String> {
    let mut args = Vec::new();
    let mut cur = String::new();
    // true once the current token has started (so `""` yields an empty arg)
    let mut in_token = false;
    let mut quote: Option<char> = None;
    let mut chars = line.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '\\' if matches!(chars.peek(), Some('"') | Some('\'')) => {
                cur.push(chars.next().unwrap());
                in_token = true;
            }
            '"' | '\'' if quote.is_none() => {
                quote = Some(c);
                in_token = true;
            }
            q if Some(q) == quote => quote = None,
            c if c.is_whitespace() && quote.is_none() => {
                if in_token {
                    args.push(std::mem::take(&mut cur));
                    in_token = false;
                }
            }
            c => {
                cur.push(c);
                in_token = true;
            }
        }
    }
    if let Some(q) = quote {
        return Err(format!("unterminated {q} quote"));
    }
    if in_token {
        args.push(cur);
    }
    Ok(args)
}

/// Render an argv back into a line for display, quoting only where needed.
pub fn join(args: &[String]) -> String {
    args.iter()
        .map(|a| {
            if a.is_empty() {
                "\"\"".to_string()
            } else if a
                .chars()
                .any(|c| c.is_whitespace() || c == '"' || c == '\'')
            {
                format!("\"{}\"", a.replace('"', "\\\""))
            } else {
                a.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(line: &str) -> Vec<String> {
        split(line).unwrap()
    }

    #[test]
    fn plain_whitespace() {
        assert_eq!(
            s("  -sV   -T4\tscanme.nmap.org "),
            ["-sV", "-T4", "scanme.nmap.org"]
        );
        assert!(s("").is_empty());
        assert!(s("   ").is_empty());
    }

    #[test]
    fn double_quotes_group() {
        assert_eq!(
            s(r#"-u https://t/FUZZ -H "User-Agent: winsploit""#),
            ["-u", "https://t/FUZZ", "-H", "User-Agent: winsploit"]
        );
    }

    #[test]
    fn single_quotes_group() {
        assert_eq!(s("-H 'X-A: 1'"), ["-H", "X-A: 1"]);
        // double quotes are literal inside single quotes and vice versa
        assert_eq!(s(r#"'say "hi"' "it's""#), [r#"say "hi""#, "it's"]);
    }

    #[test]
    fn windows_paths_keep_backslashes() {
        assert_eq!(
            s(r#"-w "C:\My Lists\big.txt" C:\tools\x.exe"#),
            ["-w", r"C:\My Lists\big.txt", r"C:\tools\x.exe"]
        );
        // a directory with no trailing backslash is the safe spelling
        assert_eq!(s(r#"-o "C:\My Out" -v"#), ["-o", r"C:\My Out", "-v"]);
    }

    #[test]
    fn trailing_backslash_before_closing_quote_is_reported() {
        // `\"` is an escaped quote, so this quote never closes. Better a clear
        // error than silently passing `C:\dir" next` to the tool.
        assert!(split(r#""C:\dir\" next"#).is_err());
    }

    #[test]
    fn escaped_quotes() {
        assert_eq!(s(r#"--data "{\"a\": 1}""#), ["--data", r#"{"a": 1}"#]);
        assert_eq!(s(r#"\"bare\""#), [r#""bare""#]);
    }

    #[test]
    fn quotes_join_adjacent_text() {
        assert_eq!(s(r#"--header="A: b" x"#), ["--header=A: b", "x"]);
        assert_eq!(s(r#"pre"mid dle"post"#), ["premid dlepost"]);
    }

    #[test]
    fn empty_quoted_arg() {
        assert_eq!(s(r#"-p "" -q ''"#), ["-p", "", "-q", ""]);
    }

    #[test]
    fn unterminated_quote_is_an_error() {
        assert!(split(r#"-H "oops"#).is_err());
        assert!(split("-H 'oops").is_err());
    }

    #[test]
    fn join_round_trips() {
        for line in [
            r#"-H "User-Agent: x" -u https://t"#,
            r#"-w "C:\My Lists\a.txt""#,
            r#"-p "" plain"#,
            r#"--data "{\"a\": 1}""#,
        ] {
            let argv = s(line);
            assert_eq!(s(&join(&argv)), argv, "round-trip of {line}");
        }
    }
}
