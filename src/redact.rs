//! Secret redaction (#70): keep passwords, tokens and keys out of the run
//! logs, the pane's `$` line and anything shared with a LAN guest.
//!
//! Pure string functions; nothing here touches the disk or the UI.
//!
//! # What is masked
//!
//! - **Catalog flags.** A tool's `secret_args:` in tools.yaml lists the
//!   flags whose value is a secret: `-p`, `--password`, or `"-H Authorization"`
//!   for a header flag whose value starts with that header. The value is
//!   masked in `-p VALUE`, `--password=VALUE` and (single-letter flags)
//!   `-pVALUE`. See [`argv`].
//! - **Patterns, in any text** ([`text`]): `user:pass@` in URLs,
//!   `Authorization:` / `Cookie:` headers, `Bearer` tokens, `password=...` /
//!   `"token": "..."` pairs, JWTs, AWS access key ids and well-known token
//!   prefixes (GitHub, GitLab, Slack, OpenAI-style `sk-`, Google `AIza`).
//! - **Typed input** at a password prompt ([`is_secret_prompt`]): the pane
//!   and the log show `> ********`.
//!
//! # Levels (`redact:` in settings.yaml, default `commands`)
//!
//! | level      | command line (pane + log) | typed at a prompt | tool output in the log |
//! |------------|---------------------------|-------------------|------------------------|
//! | `off`      | as typed                  | as typed          | as written             |
//! | `commands` | masked                    | masked            | as written             |
//! | `all`      | masked                    | masked            | patterns masked        |
//!
//! Tool output is evidence (a cracked credential is a finding), so the
//! default keeps it intact. Reports (#57) should call [`text`] on what they
//! quote, so they redact by default while the log keeps the evidence.
//!
//! Limits: pattern matching can't know a secret it has no clue about, so a
//! password that is just printed on a line by itself is not found; and a
//! prompt is only noticed when the tool ends its line (the output pump reads
//! whole lines).

use serde::{Deserialize, Serialize};

pub const MASK: &str = "********";

/// How much of a run is redacted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Level {
    /// nothing
    Off,
    /// command lines and secrets typed at a prompt (the default)
    #[default]
    Commands,
    /// also secret patterns in the tool's output, in the log
    All,
}

// ------------------------------------------------------------- patterns ---

/// How much of the text after a name `name=` / `name:` is the secret.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Span {
    /// up to whitespace, `&`, `;` or a quote
    Word,
    /// to the end of the line or the closing quote (a header value)
    Rest,
    /// like `Rest`, but a leading auth scheme (`Bearer `, `Basic `) stays
    RestScheme,
}

/// (name, only after `=`, span). Longest name wins when two start together.
const NAMES: &[(&str, bool, Span)] = &[
    ("password", false, Span::Word),
    ("passwd", false, Span::Word),
    ("passphrase", false, Span::Word),
    ("pass", true, Span::Word),
    ("pwd", true, Span::Word),
    ("secret", false, Span::Word),
    ("token", false, Span::Word),
    ("api_key", false, Span::Word),
    ("apikey", false, Span::Word),
    ("api-key", false, Span::Word),
    ("access_key", false, Span::Word),
    ("secret_key", false, Span::Word),
    ("private_key", false, Span::Word),
    ("authorization", false, Span::RestScheme),
    ("proxy-authorization", false, Span::RestScheme),
    ("cookie", false, Span::Rest),
    ("set-cookie", false, Span::Rest),
];

const SCHEMES: &[&str] = &[
    "bearer",
    "basic",
    "digest",
    "token",
    "negotiate",
    "ntlm",
    "apikey",
    "aws4-hmac-sha256",
];

/// `s[..i]` doesn't end in a letter or digit (so `compass=` isn't `pass=`,
/// but `db_pass=` and `x-api-key:` are).
fn starts_word(s: &str, i: usize) -> bool {
    s[..i]
        .chars()
        .next_back()
        .is_none_or(|c| !c.is_ascii_alphanumeric())
}

/// Where the secret after the name that ends at `end` starts and ends, and
/// what to put there. None when there's nothing (or nothing new) to mask.
fn secret_after(
    s: &str,
    start: usize,
    end: usize,
    eq_only: bool,
    span: Span,
) -> Option<(usize, usize, String)> {
    let b = s.as_bytes();
    let blank = |j: usize| j < b.len() && (b[j] == b' ' || b[j] == b'\t');
    let mut j = end;
    if j < b.len() && (b[j] == b'"' || b[j] == b'\'') {
        j += 1; // the closing quote of a JSON key
    }
    while blank(j) {
        j += 1;
    }
    match b.get(j) {
        Some(b'=') => {}
        Some(b':') if !eq_only => {}
        _ => return None,
    }
    j += 1;
    while blank(j) {
        j += 1;
    }
    let first = *b.get(j)?;
    let quoted = first == b'"' || first == b'\'';
    let (vs, ve) = if quoted {
        let vs = j + 1;
        (vs, s[vs..].find(first as char).map_or(s.len(), |x| vs + x))
    } else if span == Span::Word {
        let ve = s[j..]
            .find(|c: char| c.is_whitespace() || matches!(c, '&' | ';' | '"' | '\''))
            .map_or(s.len(), |x| j + x);
        (j, ve)
    } else {
        // a header value runs to the end of the line, unless the header sits
        // inside a quoted argument (`-H "Authorization: ..."`): then to the
        // closing quote. (A digest header has quotes of its own.)
        let before = &s[..start];
        let enclosing = ['"', '\'']
            .into_iter()
            .find(|q| before.matches(*q).count() % 2 == 1);
        let ve = enclosing
            .and_then(|q| s[j..].find(q))
            .map_or(s.len(), |x| j + x);
        (j, ve)
    };
    let value = s[vs..ve].trim_end();
    let ve = vs + value.len();
    if value.is_empty() || value == MASK {
        return None;
    }
    if span == Span::RestScheme {
        if let Some((scheme, rest)) = value.split_once(' ') {
            let rest = rest.trim_start();
            if SCHEMES.contains(&scheme.to_ascii_lowercase().as_str()) && !rest.is_empty() {
                if rest == MASK {
                    return None;
                }
                return Some((ve - rest.len(), ve, MASK.into()));
            }
        }
    }
    Some((vs, ve, MASK.into()))
}

fn named_secrets(s: &str) -> String {
    let low = s.to_ascii_lowercase();
    let mut out = String::with_capacity(s.len());
    let (mut copied, mut i) = (0, 0);
    while i < low.len() {
        let best = NAMES
            .iter()
            .filter_map(|n| low[i..].find(n.0).map(|p| (p + i, n)))
            .min_by_key(|(p, n)| (*p, std::cmp::Reverse(n.0.len())));
        let Some((p, &(name, eq_only, span))) = best else {
            break;
        };
        i = p + 1; // an ASCII match: p + 1 is a char boundary
        if !starts_word(s, p) {
            continue;
        }
        if let Some((vs, ve, repl)) = secret_after(s, p, p + name.len(), eq_only, span) {
            out.push_str(&s[copied..vs]);
            out.push_str(&repl);
            copied = ve;
            i = ve.max(i);
        }
    }
    out.push_str(&s[copied..]);
    out
}

/// `scheme://user:password@host` -> `scheme://user:********@host`.
fn url_credentials(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(p) = rest.find("://") {
        let (head, tail) = rest.split_at(p + 3);
        out.push_str(head);
        let end = tail
            .find(|c: char| matches!(c, '/' | '?' | '#' | '"' | '\'') || c.is_whitespace())
            .unwrap_or(tail.len());
        let (authority, after) = tail.split_at(end);
        match authority
            .rsplit_once('@')
            .map(|(u, h)| (u.split_once(':'), h))
        {
            Some((Some((user, pw)), host)) if !pw.is_empty() && pw != MASK => {
                out.push_str(&format!("{user}:{MASK}@{host}"));
            }
            _ => out.push_str(authority),
        }
        rest = after;
    }
    out.push_str(rest);
    out
}

/// `Bearer <token>` anywhere (the token is 8+ characters, so prose about a
/// "bearer of news" is left alone).
fn bearer_tokens(s: &str) -> String {
    let low = s.to_ascii_lowercase();
    let mut out = String::with_capacity(s.len());
    let (mut copied, mut i) = (0, 0);
    while let Some(p) = low[i..].find("bearer ").map(|x| x + i) {
        i = p + 7;
        if !starts_word(s, p) {
            continue;
        }
        let tok_len = s[i..]
            .find(|c: char| !(c.is_ascii_alphanumeric() || "._~+/=-".contains(c)))
            .unwrap_or(s.len() - i);
        if tok_len >= 8 {
            out.push_str(&s[copied..i]);
            out.push_str(MASK);
            copied = i + tok_len;
            i = copied;
        }
    }
    out.push_str(&s[copied..]);
    out
}

fn is_aws_key_id(w: &str) -> bool {
    w.len() == 20
        && [
            "AKIA", "ASIA", "AGPA", "AIDA", "AROA", "ANPA", "ANVA", "AIPA",
        ]
        .iter()
        .any(|p| w.starts_with(p))
        && w.bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
}

fn has_token_prefix(w: &str) -> bool {
    const PREFIXES: &[(&str, usize)] = &[
        ("ghp_", 30),
        ("gho_", 30),
        ("ghu_", 30),
        ("ghs_", 30),
        ("ghr_", 30),
        ("github_pat_", 30),
        ("glpat-", 20),
        ("xoxb-", 20),
        ("xoxp-", 20),
        ("xoxa-", 20),
        ("xoxr-", 20),
        ("xoxs-", 20),
        ("sk-", 24),
        ("AIza", 35),
        ("npm_", 30),
    ];
    PREFIXES
        .iter()
        .any(|(p, min)| w.len() >= *min && w.starts_with(p))
}

/// A JWT: `eyJ...` with exactly three non-empty dot-separated parts.
fn is_jwt(w: &str) -> bool {
    let parts: Vec<&str> = w.split('.').collect();
    w.starts_with("eyJ") && parts.len() == 3 && parts.iter().all(|p| !p.is_empty())
}

/// JWTs, AWS key ids and known token prefixes, found as whole words.
fn token_shapes(s: &str) -> String {
    let in_word = |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.');
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while !rest.is_empty() {
        let start = rest.find(in_word).unwrap_or(rest.len());
        out.push_str(&rest[..start]);
        rest = &rest[start..];
        let len = rest.find(|c| !in_word(c)).unwrap_or(rest.len());
        let (word, after) = rest.split_at(len);
        let core = word.trim_end_matches('.');
        let tail = &word[core.len()..];
        if is_jwt(core) {
            out.push_str(MASK);
            out.push_str(tail);
        } else {
            let (first, more) = core.split_once('.').unwrap_or((core, ""));
            if is_aws_key_id(first) || has_token_prefix(first) {
                out.push_str(MASK);
                if !more.is_empty() {
                    out.push('.');
                    out.push_str(more);
                }
                out.push_str(tail);
            } else {
                out.push_str(word);
            }
        }
        rest = after;
    }
    out
}

/// Mask the secrets `s` visibly contains (see the module docs). Idempotent:
/// `text(&text(x)) == text(x)`.
pub fn text(s: &str) -> String {
    let s = url_credentials(s);
    let s = named_secrets(&s);
    let s = bearer_tokens(&s);
    token_shapes(&s)
}

// ------------------------------------------------------------ catalog ---

/// One `secret_args:` entry: the flag, and the header-name prefix for
/// `"-H Authorization"`-style entries.
fn parse_rule(entry: &str) -> Option<(&str, Option<&str>)> {
    let entry = entry.trim();
    if entry.is_empty() {
        return None;
    }
    Some(match entry.split_once(char::is_whitespace) {
        Some((flag, prefix)) => (flag, Some(prefix.trim()).filter(|p| !p.is_empty())),
        None => (entry, None),
    })
}

/// What a `secret_args:` entry must look like, for the catalog check.
pub fn entry_problem(entry: &str) -> Option<String> {
    let Some((flag, prefix)) = parse_rule(entry) else {
        return Some("secret_args: an entry is empty".into());
    };
    if !flag.starts_with('-') || flag.trim_start_matches('-').is_empty() {
        return Some(format!(
            "secret_args: \"{entry}\" must start with a flag (-p, --password, or \"-H Authorization\")"
        ));
    }
    if prefix.is_some_and(|p| p.contains(char::is_whitespace)) {
        return Some(format!(
            "secret_args: \"{entry}\" takes at most a flag and one header name"
        ));
    }
    None
}

/// The value of a secret flag. With a header-name `prefix`, only values that
/// start with `Name:` are secret (and the name is kept).
fn flag_value(value: &str, prefix: Option<&str>) -> String {
    match prefix {
        None if value.is_empty() => String::new(), // `-p ""` hides nothing
        None => MASK.into(),
        Some(name) => {
            let n = name.len();
            if value.is_char_boundary(n)
                && value[..n].eq_ignore_ascii_case(name)
                && value[n..].trim_start().starts_with(':')
            {
                format!("{}: {MASK}", &value[..n])
            } else {
                text(value)
            }
        }
    }
}

/// `args` with the values of the tool's `secret_args` masked, and the
/// patterns of [`text`] masked in every other argument.
pub fn argv(args: &[String], secret_args: &[String]) -> Vec<String> {
    let rules: Vec<(&str, Option<&str>)> =
        secret_args.iter().filter_map(|e| parse_rule(e)).collect();
    let mut out = Vec::with_capacity(args.len());
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        let rule_for = |flag: &str| rules.iter().find(|(f, _)| *f == flag);
        if let Some((_, prefix)) = rule_for(a) {
            // `-p VALUE`
            out.push(a.clone());
            if let Some(v) = args.get(i + 1) {
                out.push(flag_value(v, *prefix));
            }
            i += 2;
            continue;
        }
        if let Some((flag, v)) = a.split_once('=') {
            if let Some((_, prefix)) = rule_for(flag) {
                // `--password=VALUE`
                out.push(format!("{flag}={}", flag_value(v, *prefix)));
                i += 1;
                continue;
            }
        }
        // `-pVALUE`, only for single-letter flags without a header name
        let attached = rules.iter().find(|(f, prefix)| {
            prefix.is_none()
                && f.len() == 2
                && !f.starts_with("--")
                && a.len() > 2
                && a.starts_with(f)
        });
        if let Some((flag, _)) = attached {
            out.push(format!("{flag}{MASK}"));
        } else {
            out.push(text(a));
        }
        i += 1;
    }
    out
}

/// The command line to show and log: `run_cmd` + the joined `args`, masked
/// according to `level`.
pub fn command(run_cmd: &str, args: &[String], secret_args: &[String], level: Level) -> String {
    let shown = if level == Level::Off {
        args.to_vec()
    } else {
        argv(args, secret_args)
    };
    format!("{} {}", run_cmd, crate::args::join(&shown))
}

// -------------------------------------------------------------- prompts ---

/// Is `line` a tool asking for a secret ("Password:", "Enter passphrase for
/// key:", "[sudo] password for x:")? The next line typed is then masked.
pub fn is_secret_prompt(line: &str) -> bool {
    let t = line.trim();
    if t.is_empty() || t.len() > 100 || !(t.ends_with(':') || t.ends_with('?')) {
        return false;
    }
    t.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .any(|w| {
            matches!(
                w,
                "password" | "passphrase" | "passcode" | "pin" | "secret" | "token"
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fuzz::{nasty_string, Rng};

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn url_credentials_are_masked() {
        for (input, want) in [
            (
                "smb://admin:Passw0rd@10.0.0.5/share",
                "smb://admin:********@10.0.0.5/share",
            ),
            (
                "https://u:p@a.com:8443/x?y=1",
                "https://u:********@a.com:8443/x?y=1",
            ),
            (
                "git clone https://bob:p@ss@host/r.git",
                "git clone https://bob:********@host/r.git",
            ),
            ("ftp://anonymous:@host/", "ftp://anonymous:@host/"), // no password
            ("https://host/a@b:c", "https://host/a@b:c"),         // the @ is in the path
            (
                "https://a.com https://x:y@b.com",
                "https://a.com https://x:********@b.com",
            ),
        ] {
            assert_eq!(text(input), want, "{input}");
        }
    }

    #[test]
    fn headers_and_bearer_tokens_are_masked() {
        for (input, want) in [
            (
                "Authorization: Bearer abc123.def456",
                "Authorization: Bearer ********",
            ),
            (
                "authorization: Basic dXNlcjpwYXNz",
                "authorization: Basic ********",
            ),
            (
                "Authorization: Digest username=\"a\", realm=\"b\"",
                "Authorization: Digest ********",
            ),
            ("Authorization: abcdef", "Authorization: ********"),
            (
                "Proxy-Authorization: Basic Zm9vOmJhcg==",
                "Proxy-Authorization: Basic ********",
            ),
            ("Cookie: session=abc; theme=dark", "Cookie: ********"),
            ("Set-Cookie: sid=xyz; HttpOnly", "Set-Cookie: ********"),
            (
                r#"-H "Authorization: Bearer abc123def456""#,
                r#"-H "Authorization: Bearer ********""#,
            ),
            (
                r#"{"Authorization": "Bearer abc123def456"}"#,
                r#"{"Authorization": "Bearer ********"}"#,
            ),
            ("sent Bearer eyabcdefgh1234 ok", "sent Bearer ******** ok"),
            ("the bearer of bad news", "the bearer of bad news"),
            ("Bearer short", "Bearer short"), // under 8 characters: not a token
            (
                "X-Forwarded-Authorization: Bearer abcdefgh12",
                "X-Forwarded-Authorization: Bearer ********",
            ),
        ] {
            assert_eq!(text(input), want, "{input}");
        }
    }

    #[test]
    fn named_secrets_are_masked() {
        for (input, want) in [
            ("password=hunter2", "password=********"),
            ("Password: hunter2", "Password: ********"),
            (
                "PASSWORD = \"hunter 2\" next",
                "PASSWORD = \"********\" next",
            ),
            (
                "login?user=a&password=p%40ss&x=1",
                "login?user=a&password=********&x=1",
            ),
            ("db_password=abc; port=1", "db_password=********; port=1"),
            ("--password=abc", "--password=********"),
            (
                r#"{"password":"abc","user":"x"}"#,
                r#"{"password":"********","user":"x"}"#,
            ),
            ("api_key=AbC123", "api_key=********"),
            ("x-api-key: AbC123", "x-api-key: ********"),
            (
                "AWS_SECRET_ACCESS_KEY=wJalrXUtnFEMI",
                "AWS_SECRET_ACCESS_KEY=********",
            ),
            ("client_secret=abc", "client_secret=********"),
            ("access_token=abc.def&x=1", "access_token=********&x=1"),
            (
                "Server=a;Uid=sa;Pwd=Sup3r;",
                "Server=a;Uid=sa;Pwd=********;",
            ),
            ("pass=abc", "pass=********"),
            // not secrets, or nothing to hide
            ("Enter password:", "Enter password:"),
            ("Password: ", "Password: "),
            ("password", "password"),
            ("passwordless login", "passwordless login"),
            ("compass=north", "compass=north"),
            ("bypass: yes", "bypass: yes"),
            ("pass: 22/tcp open", "pass: 22/tcp open"), // `pass` only counts after `=`
            ("tokenizer: fast", "tokenizer: fast"),
            ("password=", "password="),
        ] {
            assert_eq!(text(input), want, "{input}");
        }
    }

    #[test]
    fn token_shapes_are_masked() {
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0In0.c2lnbmF0dXJl";
        for (input, want) in [
            (format!("token {jwt} end"), "token ******** end".to_string()),
            (format!("{jwt}."), "********.".to_string()),
            (
                "key AKIAIOSFODNN7EXAMPLE used".into(),
                "key ******** used".into(),
            ),
            ("AKIAIOSFODNN7EXAMPL".into(), "AKIAIOSFODNN7EXAMPL".into()), // 19 chars
            (
                "ghp_abcdefghijklmnopqrstuvwxyz0123456789".into(),
                "********".into(),
            ),
            (
                "export X=xoxb-1234-5678-abcdefghijkl".into(),
                "export X=********".into(),
            ),
            (
                "sk-ant-api03-abcdefghijklmnopqrstuvwxyz".into(),
                "********".into(),
            ),
            ("sk-learn is a tool".into(), "sk-learn is a tool".into()),
            ("see ghp_short".into(), "see ghp_short".into()),
            ("eyJonly.twoparts".into(), "eyJonly.twoparts".into()),
        ] {
            assert_eq!(text(&input), want, "{input}");
        }
    }

    /// Ordinary tool output must come through untouched.
    #[test]
    fn benign_lines_are_left_alone() {
        for line in [
            "Starting Nmap 7.95 ( https://nmap.org ) at 2026-11-07 03:15 UTC",
            "Nmap scan report for scanme.nmap.org (45.33.32.156)",
            "22/tcp open  ssh     OpenSSH 8.9p1 Ubuntu",
            "80/tcp open  http    nginx 1.18.0",
            "[+] 10.0.0.5:445 SMB  DC01  [*] Windows Server 2019 (name:DC01)",
            "GET /index.html HTTP/1.1 200 1024",
            "C:\\Users\\me\\ctf\\pico\\rev 1\\files\\crack me.exe",
            "Status: 200, Size: 1337, Words: 12, Lines: 4",
            "https://example.com/a/b?x=1&y=2#frag",
            "contact: admin@example.com",
            "-----BEGIN CERTIFICATE-----",
            "2026-11-07T03:15:02Z INFO started worker 3",
            "",
            "   ",
        ] {
            assert_eq!(text(line), line);
        }
    }

    #[test]
    fn text_is_idempotent_and_never_panics() {
        let mut rng = Rng::new(0x70);
        let pieces = [
            "password=",
            "token: ",
            "Bearer ",
            "Authorization: ",
            "Cookie: ",
            "https://u:p@h/",
            "AKIAIOSFODNN7EXAMPLE",
            "ghp_abcdefghijklmnopqrstuvwxyz0123456789",
            "\"",
            "'",
            "eyJa.b.c",
        ];
        for _ in 0..4000 {
            let mut s = String::new();
            for _ in 0..rng.below(5) {
                if rng.below(2) == 0 {
                    s.push_str(rng.pick(&pieces));
                } else {
                    s.push_str(&nasty_string(&mut rng, 8));
                }
            }
            let once = text(&s);
            assert_eq!(text(&once), once, "not idempotent for {s:?}");
            assert!(!once.contains('\n') || s.contains('\n'), "{s:?}");
        }
    }

    #[test]
    fn secret_flags_hide_their_values() {
        let secret = v(&["-p", "--password", "-H", "-H Authorization", "--cookie"]);
        let cases: &[(&[&str], &[&str])] = &[
            (
                &["smb", "10.0.0.5", "-u", "admin", "-p", "Passw0rd"],
                &["smb", "10.0.0.5", "-u", "admin", "-p", "********"],
            ),
            (&["--password=Passw0rd", "x"], &["--password=********", "x"]),
            (&["--password", "Passw0rd"], &["--password", "********"]),
            (&["-pPassw0rd", "-v"], &["-p********", "-v"]),
            (&["-p", ""], &["-p", ""]),
            (&["-p"], &["-p"]), // flag without a value
            (&["--cookie", "sid=abc"], &["--cookie", "********"]),
            // a header flag with a header name only hides that header
            (
                &["-H", "Authorization: Bearer abc123def456"],
                &["-H", "Authorization: ********"],
            ),
            (
                &["-H", "authorization:token"],
                &["-H", "authorization: ********"],
            ),
            (
                &["-H", "User-Agent: winsploit"],
                &["-H", "User-Agent: winsploit"],
            ),
            // other arguments still get the pattern pass
            (
                &["-u", "https://a:b@host/"],
                &["-u", "https://a:********@host/"],
            ),
            (
                &["-u", "https://host/?token=abc"],
                &["-u", "https://host/?token=********"],
            ),
        ];
        // -H appears twice in `secret` (with and without a header name); the
        // first rule wins, so test the header form on its own list
        let hdr = v(&["-H Authorization"]);
        for (args, want) in cases {
            let args = v(args);
            let list = if args.iter().any(|a| a == "-H") {
                &hdr
            } else {
                &secret
            };
            assert_eq!(argv(&args, list), v(want), "{args:?}");
        }
        // no catalog entry: only the patterns apply
        assert_eq!(argv(&v(&["-p", "Passw0rd"]), &[]), v(&["-p", "Passw0rd"]));
        // a flag only matches whole: `-pt` is not `-p` plus a value `t` for a long rule
        assert_eq!(
            argv(&v(&["--passwordless"]), &v(&["--password"])),
            v(&["--passwordless"])
        );
    }

    #[test]
    fn commands_are_shown_joined_and_masked_by_level() {
        let args = v(&[
            "smb",
            "10.0.0.5",
            "-u",
            "a b",
            "-p",
            "Pass w0rd",
            "-H",
            "Authorization: Bearer abcdefgh12",
        ]);
        let secret = v(&["-p", "-H Authorization"]);
        let masked = command("nxc", &args, &secret, Level::Commands);
        assert_eq!(
            masked,
            r#"nxc smb 10.0.0.5 -u "a b" -p ******** -H "Authorization: ********""#
        );
        assert_eq!(command("nxc", &args, &secret, Level::All), masked);
        let raw = command("nxc", &args, &secret, Level::Off);
        assert!(
            raw.contains("Pass w0rd") && raw.contains("abcdefgh12"),
            "{raw}"
        );
        // what is shown still splits back into the same number of arguments
        assert_eq!(
            crate::args::split(masked.strip_prefix("nxc ").unwrap())
                .unwrap()
                .len(),
            args.len()
        );
        // no args: same text as the old `format!("{} {}", ..)`
        assert_eq!(command("nmap", &[], &[], Level::Commands), "nmap ");
    }

    /// The entries in the real tools.yaml, not just the fixtures above.
    #[test]
    fn the_shipped_catalog_masks_credentials() {
        let cat = crate::registry::parse(include_str!("../tools.yaml")).unwrap();
        let show = |tool: &str, args: &[&str]| {
            let t = cat.tools.iter().find(|t| t.name == tool).unwrap();
            command(&t.run_cmd, &v(args), &t.secret_args, Level::Commands)
        };
        assert_eq!(
            show(
                "netexec",
                &["smb", "10.0.0.5", "-u", "admin", "-p", "Passw0rd"]
            ),
            "nxc smb 10.0.0.5 -u admin -p ********"
        );
        assert_eq!(
            show(
                "netexec",
                &[
                    "smb",
                    "10.0.0.5",
                    "-u",
                    "admin",
                    "-H",
                    "aad3b435b51404eeaad3b435b51404ee"
                ]
            ),
            "nxc smb 10.0.0.5 -u admin -H ********"
        );
        assert_eq!(
            show(
                "ffuf",
                &[
                    "-u",
                    "https://x/FUZZ",
                    "-H",
                    "Authorization: Bearer abc.def",
                    "-H",
                    "User-Agent: winsploit"
                ]
            ),
            r#"ffuf -u https://x/FUZZ -H "Authorization: ********" -H "User-Agent: winsploit""#
        );
        assert_eq!(
            show(
                "sqlmap",
                &["-u", "https://x/?id=1", "--cookie=PHPSESSID=abc"]
            ),
            "sqlmap -u https://x/?id=1 --cookie=********"
        );
        assert_eq!(
            show("steghide", &["extract", "-sf", "a.jpg", "-p", "s3cret"]),
            "steghide extract -sf a.jpg -p ********"
        );
        // a tool without secret_args still gets the pattern pass
        assert_eq!(
            show("nmap", &["-sV", "https://u:p@host/"]),
            "nmap -sV https://u:********@host/"
        );
    }

    #[test]
    fn catalog_entries_are_checked() {
        for ok in ["-p", "--password", "-H Authorization", "  --cookie  "] {
            assert_eq!(entry_problem(ok), None, "{ok}");
        }
        for bad in ["", "  ", "password", "-", "--", "-H Authorization Bearer"] {
            assert!(entry_problem(bad).is_some(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn password_prompts_are_recognised() {
        for yes in [
            "Password:",
            "password: ",
            "Enter passphrase for key '/home/x/id_rsa':",
            "[sudo] password for kali:",
            "Enter PIN:",
            "Please enter the token?",
            "SMB password for admin@10.0.0.5:",
        ] {
            assert!(is_secret_prompt(yes), "{yes}");
        }
        for no in [
            "",
            "Password: hunter2", // a value follows: that's a result, not a prompt
            "password",
            "Starting login...",
            "Mapping: pin 4 to gpio",
            "Enter your name:",
            "spinning:",
            &format!("password {}:", "x".repeat(120)),
        ] {
            assert!(!is_secret_prompt(no), "{no}");
        }
    }

    #[test]
    fn levels_use_kebab_case_in_settings() {
        for (text, want) in [
            ("off", Level::Off),
            ("commands", Level::Commands),
            ("all", Level::All),
        ] {
            let got: Level = serde_yaml::from_str(text).unwrap();
            assert_eq!(got, want);
            assert_eq!(serde_yaml::to_string(&want).unwrap().trim(), text);
        }
        assert_eq!(Level::default(), Level::Commands);
        assert!(serde_yaml::from_str::<Level>("sometimes").is_err());
    }
}
