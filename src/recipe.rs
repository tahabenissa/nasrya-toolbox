//! Recipes: named argument presets per tool (`recipes:` in tools.yaml) with
//! placeholders the active challenge fills in:
//!
//! | placeholder  | value                                                    |
//! |--------------|----------------------------------------------------------|
//! | `{target}`   | the challenge's target (IP, host, URL, `nc host port`)   |
//! | `{file}`     | a challenge file (from its `files\`)                     |
//! | `{out}`      | the challenge's `out\` folder, for files tools write     |
//! | `{wordlist}` | the default wordlist (SecLists-lite common.txt)          |
//!
//! Placeholders are written *unquoted* in recipes: each value is inserted as
//! one quoted argument, so a target or path with spaces or quotes can never
//! split into extra arguments. Quoted text joins the text around it, so
//! `-oN {out}\nmap.txt` still makes a single `-oN` value.

/// The placeholders recipes may use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    Target,
    File,
    Out,
    Wordlist,
}

impl Slot {
    pub const ALL: [Slot; 4] = [Slot::Target, Slot::File, Slot::Out, Slot::Wordlist];

    pub fn name(self) -> &'static str {
        match self {
            Slot::Target => "target",
            Slot::File => "file",
            Slot::Out => "out",
            Slot::Wordlist => "wordlist",
        }
    }

    fn from_name(n: &str) -> Option<Slot> {
        Slot::ALL.into_iter().find(|s| s.name() == n)
    }
}

/// Values for the placeholders; None = not known yet (ask for it).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Values {
    pub target: Option<String>,
    pub file: Option<String>,
    pub out: Option<String>,
    pub wordlist: Option<String>,
}

impl Values {
    pub fn get(&self, s: Slot) -> Option<&str> {
        match s {
            Slot::Target => self.target.as_deref(),
            Slot::File => self.file.as_deref(),
            Slot::Out => self.out.as_deref(),
            Slot::Wordlist => self.wordlist.as_deref(),
        }
        .filter(|v| !v.trim().is_empty())
    }
}

/// One `{name}` in `args`: (byte range of the whole `{name}`, name, whether
/// it sits inside quotes). Quote rules match args.rs: `"` and `'` group,
/// `\"` / `\'` are literal.
fn tokens(args: &str) -> Vec<(std::ops::Range<usize>, String, bool)> {
    let mut out = Vec::new();
    let mut quote: Option<char> = None;
    let mut it = args.char_indices().peekable();
    while let Some((i, c)) = it.next() {
        match c {
            '\\' if matches!(it.peek(), Some((_, '"' | '\''))) => {
                it.next();
            }
            '"' | '\'' if quote.is_none() => quote = Some(c),
            q if Some(q) == quote => quote = None,
            '{' => {
                let rest = &args[i + 1..];
                let Some(end) = rest.find('}') else { continue };
                let name = &rest[..end];
                if !name.is_empty() && name.chars().all(|ch| ch.is_ascii_lowercase() || ch == '_') {
                    out.push((i..i + end + 2, name.to_string(), quote.is_some()));
                }
            }
            _ => {}
        }
    }
    out
}

/// The placeholders `args` uses, each once, in order.
pub fn slots(args: &str) -> Vec<Slot> {
    let mut v = Vec::new();
    for (_, name, _) in tokens(args) {
        if let Some(s) = Slot::from_name(&name) {
            if !v.contains(&s) {
                v.push(s);
            }
        }
    }
    v
}

/// What's wrong with a recipe's args, for the catalog check.
pub fn problems(args: &str) -> Vec<String> {
    let mut p = Vec::new();
    for (_, name, quoted) in tokens(args) {
        match Slot::from_name(&name) {
            Some(_) if quoted => p.push(format!(
                "{{{name}}} is inside quotes; write it unquoted (its value is quoted for you)"
            )),
            Some(_) => {}
            // the catalog's own placeholder, expanded at load time
            None if name == "scoop_app" => {}
            None => p.push(format!(
                "unknown placeholder {{{name}}} (use {{target}}, {{file}}, {{out}} or {{wordlist}})"
            )),
        }
    }
    p
}

/// `v` as exactly one argument for args::split.
pub fn quote(v: &str) -> String {
    // a trailing backslash is dropped on purpose: `{out}` is usually a folder
    // written `...\out\`, and recipes append `\name` right after it
    crate::args::quote(v.trim().trim_end_matches('\\'))
}

/// Fill every placeholder, or say which values are missing.
pub fn fill(args: &str, v: &Values) -> Result<String, Vec<Slot>> {
    let missing: Vec<Slot> = slots(args)
        .into_iter()
        .filter(|s| v.get(*s).is_none())
        .collect();
    if !missing.is_empty() {
        return Err(missing);
    }
    Ok(fill_partial(args, v))
}

/// Fill what's known and leave `{name}` where a value is missing (the web
/// view shows those gaps for the user to replace).
pub fn fill_partial(args: &str, v: &Values) -> String {
    let mut out = String::new();
    let mut last = 0;
    for (range, name, _) in tokens(args) {
        let Some(slot) = Slot::from_name(&name) else {
            continue;
        };
        if let Some(val) = v.get(slot) {
            out.push_str(&args[last..range.start]);
            out.push_str(&quote(val));
            last = range.end;
        }
    }
    out.push_str(&args[last..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::args;

    fn vals() -> Values {
        Values {
            target: Some("10.10.10.3".into()),
            file: Some(r"C:\Users\me\ctf\pico\rev 1\files\crack me.exe".into()),
            out: Some(r"C:\Users\me\ctf\pico\rev 1\out\".into()),
            wordlist: None,
        }
    }

    #[test]
    fn slots_in_order_and_once() {
        assert_eq!(
            slots("-sV {target} -oN {out}\\a.txt {target}"),
            [Slot::Target, Slot::Out]
        );
        assert_eq!(slots(r#"-w "{app:seclists-lite}\x.txt""#), []);
    }

    #[test]
    fn filled_values_stay_one_argument() {
        let filled = fill(r"-oN {out}\nmap.txt -d {file} {target}", &vals()).unwrap();
        let argv = args::split(&filled).unwrap();
        assert_eq!(
            argv,
            [
                "-oN",
                r"C:\Users\me\ctf\pico\rev 1\out\nmap.txt",
                "-d",
                r"C:\Users\me\ctf\pico\rev 1\files\crack me.exe",
                "10.10.10.3",
            ]
        );
        // a hostile value can't add arguments
        let evil = Values {
            target: Some(r#"x" --script "evil"#.into()),
            ..Default::default()
        };
        let argv = args::split(&fill("-sV {target}", &evil).unwrap()).unwrap();
        assert_eq!(argv, ["-sV", r#"x" --script "evil"#]);
    }

    #[test]
    fn missing_values_are_reported_and_partial_fill_leaves_gaps() {
        assert_eq!(
            fill("-w {wordlist} -u {target}", &vals()),
            Err(vec![Slot::Wordlist])
        );
        assert_eq!(
            fill_partial("-w {wordlist} -u {target}", &vals()),
            "-w {wordlist} -u \"10.10.10.3\""
        );
    }

    #[test]
    fn catalog_problems() {
        assert!(problems("-sV {target} -oN {out}\\x").is_empty());
        assert!(problems(r#"-w "{app:seclists-lite}\x" '{scoop_app}'"#).is_empty());
        assert_eq!(problems(r#"-u "{target}""#).len(), 1);
        assert!(problems("-u {targt}")[0].contains("unknown placeholder {targt}"));
    }

    // ---- property / fuzz tests (#119) -----------------------------------

    use crate::fuzz::{nasty_string, Rng, CORPUS};

    /// What `quote` promises: the value, trimmed and without trailing
    /// backslashes (they would eat the closing quote), as ONE argument.
    fn expected(v: &str) -> String {
        v.trim().trim_end_matches('\\').to_string()
    }

    #[test]
    fn quote_is_always_exactly_one_argument_with_the_value_intact() {
        for v in CORPUS {
            let q = quote(v);
            assert_eq!(
                args::split(&q),
                Ok(vec![expected(v)]),
                "{v:?} quoted as {q:?}"
            );
        }
        let mut rng = Rng::new(0x119);
        for case in 0..5000 {
            let v = nasty_string(&mut rng, 14);
            let q = quote(&v);
            assert_eq!(
                args::split(&q),
                Ok(vec![expected(&v)]),
                "case {case}: {v:?} quoted as {q:?}"
            );
        }
    }

    /// The headline property: however hostile the values, a filled recipe
    /// has exactly the arguments the recipe wrote, and each value is one of
    /// them. Nothing a value contains can add, drop or move a flag.
    #[test]
    fn filled_recipe_never_gains_or_loses_arguments() {
        let template = r"-sV --max-retries 2 {target} -oN {out}\nmap.txt -d {file} -w {wordlist}";
        let mut rng = Rng::new(0xF111);
        let hostile = |rng: &mut Rng| loop {
            let v = if rng.below(3) == 0 {
                rng.pick(CORPUS).to_string()
            } else {
                nasty_string(rng, 14)
            };
            // blank values count as "not set" and are rejected by fill()
            if !v.trim().is_empty() {
                break v;
            }
        };
        for case in 0..4000 {
            let vals = Values {
                target: Some(hostile(&mut rng)),
                file: Some(hostile(&mut rng)),
                out: Some(hostile(&mut rng)),
                wordlist: Some(hostile(&mut rng)),
            };
            let filled = fill(template, &vals).unwrap();
            let argv = args::split(&filled)
                .unwrap_or_else(|e| panic!("case {case}: {vals:?} gave {filled:?}: {e}"));
            let want = [
                "-sV".to_string(),
                "--max-retries".to_string(),
                "2".to_string(),
                expected(vals.target.as_deref().unwrap()),
                "-oN".to_string(),
                format!("{}\\nmap.txt", expected(vals.out.as_deref().unwrap())),
                "-d".to_string(),
                expected(vals.file.as_deref().unwrap()),
                "-w".to_string(),
                expected(vals.wordlist.as_deref().unwrap()),
            ];
            assert_eq!(argv, want, "case {case}: {vals:?} gave {filled:?}");
        }
    }

    /// A value that looks like a placeholder is data, not a second round of
    /// expansion.
    #[test]
    fn values_that_look_like_placeholders_stay_literal() {
        let vals = Values {
            target: Some("{file}".into()),
            file: Some("{target}".into()),
            out: Some("{out}".into()),
            wordlist: Some("{wordlist}".into()),
        };
        let argv = args::split(&fill("{target} {file} {out} {wordlist}", &vals).unwrap()).unwrap();
        assert_eq!(argv, ["{file}", "{target}", "{out}", "{wordlist}"]);
    }

    /// Slots written into a recipe that are NOT a known placeholder are left
    /// alone, and a value is never searched for placeholders afterwards.
    #[test]
    fn unknown_braces_and_random_templates_never_panic() {
        let mut rng = Rng::new(0xB4AC);
        let vals = vals();
        for _ in 0..3000 {
            let template = nasty_string(&mut rng, 24);
            let _ = slots(&template);
            let _ = problems(&template);
            let _ = fill_partial(&template, &vals);
            let _ = fill(&template, &vals);
        }
    }
}
