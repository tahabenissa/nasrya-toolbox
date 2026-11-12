//! Scope matcher (#64): a pure `target x scope -> verdict` function with no
//! UI and no policy. It is the first building block of engagement scope
//! enforcement (#56, #66); nothing calls it yet.
//!
//! ```text
//! Scope::parse("10.0.0.0/8\n*.example.com", "10.1.0.0/16") -> Scope
//! scope.check(&Target::parse("10.2.3.4")?)                 -> Verdict::InScope
//! ```
//!
//! # Rules (one per line, or separated by commas / spaces; `# comment`)
//!
//! | rule                         | covers                                        |
//! |------------------------------|-----------------------------------------------|
//! | `10.0.0.5`, `2001:db8::1`    | that address                                  |
//! | `10.0.0.0/8`                 | the range; a CIDR *target* needs to be fully inside |
//! | `example.com`                | that exact host (not its subdomains)          |
//! | `*.example.com`              | any subdomain, at any depth                   |
//! | `https://app.example.com/api/` | scheme + host + port + path prefix, all of them |
//!
//! # Decisions
//!
//! - **`*.example.com` does not include `example.com` itself.** The apex has
//!   to be listed on its own line. Being too narrow fails safe (OutOfScope).
//! - **Exclusions always win**, and an exclusion also fires on a CIDR target
//!   that only *overlaps* it: `10.0.0.0/15` is out when `10.1.0.0/16` is
//!   excluded, even though most of it is allowed.
//! - **Include = fully contained, exclude = touched.** A host rule never
//!   covers a URL-prefix rule's narrower paths and vice versa: a bare host
//!   target is broader than `https://host/api/`, so it is not inside it.
//! - **`Unknown`** means this scope cannot judge the target without DNS: a
//!   host name against a scope made only of IP ranges (or an address against
//!   only names), or a scope with no include rules at all. The caller should
//!   ask the user instead of guessing. Names are never resolved here.
//! - **URL targets are the exact starting URL**, not a crawl: the query and
//!   fragment are ignored, the path is normalised (`.`/`..`, `//`, `\`, and
//!   the encodings `%2e` `%2f` `%5c`) so `/api/../admin` is `/admin`, the
//!   scheme's default port is filled in, and `user@` credentials are skipped
//!   the way a client skips them (`https://good.com@evil.com/` is evil.com).
//! - **Host names** are lowercased, lose one trailing dot, and become
//!   punycode per label. Full UTS-46 mapping is not done: only fullwidth
//!   ASCII, ideographic full stops and the invisible characters IDNA
//!   ignores are folded.
//! - **Ambiguous numbers are errors**, not host names: `2130706433`,
//!   `127.1`, `0x7f.0.0.1` and `010.0.0.1` are all "127.0.0.1" or something
//!   else depending on the tool, so write the dotted quad.
//! - **CIDR with host bits set** (`10.0.0.5/24`) is an error rather than
//!   being silently widened to `10.0.0.0/24`.
//! - IPv4-mapped IPv6 addresses (`::ffff:10.0.0.1`) count as the IPv4 one.
//!
//! Limits: rule items can't contain spaces or commas (they separate items).

#![allow(dead_code)] // until #56 / #66 wire it in

use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

// ---------------------------------------------------------------- CIDR ---

/// An address range. `addr` is always the network address (host bits are 0).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cidr {
    addr: IpAddr,
    prefix: u8,
}

/// (address as an integer, width in bits)
fn bits(addr: IpAddr) -> (u128, u32) {
    match addr {
        IpAddr::V4(a) => (u128::from(u32::from(a)), 32),
        IpAddr::V6(a) => (u128::from(a), 128),
    }
}

/// The top `prefix` bits of a `width`-bit address.
fn mask(width: u32, prefix: u32) -> u128 {
    let full = if width == 128 {
        !0u128
    } else {
        (1u128 << width) - 1
    };
    if prefix == 0 {
        0
    } else {
        full & !((1u128 << (width - prefix)) - 1)
    }
}

impl Cidr {
    pub fn parse(s: &str) -> Result<Cidr, String> {
        let (ip, p) = s
            .split_once('/')
            .ok_or_else(|| format!("`{s}` is not a CIDR (expected address/prefix)"))?;
        let addr: IpAddr = ip
            .parse()
            .map_err(|_| format!("`{ip}` is not an IPv4 or IPv6 address"))?;
        let max = if addr.is_ipv4() { 32 } else { 128 };
        if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) {
            return Err(format!("`{p}` is not a prefix length"));
        }
        let prefix: u8 = p
            .parse()
            .ok()
            .filter(|n| *n <= max)
            .ok_or_else(|| format!("prefix /{p} is out of range (0-{max})"))?;
        let net = Cidr { addr, prefix }.network();
        if net != addr {
            return Err(format!(
                "{s} has host bits set; did you mean {net}/{prefix}?"
            ));
        }
        Ok(Cidr { addr, prefix })
    }

    /// A single address as a /32 or /128.
    pub fn host(ip: IpAddr) -> Cidr {
        let addr = ip.to_canonical();
        Cidr {
            addr,
            prefix: if addr.is_ipv4() { 32 } else { 128 },
        }
    }

    fn network(&self) -> IpAddr {
        let (b, w) = bits(self.addr);
        let net = b & mask(w, u32::from(self.prefix));
        if w == 32 {
            IpAddr::V4(Ipv4Addr::from(net as u32))
        } else {
            IpAddr::V6(Ipv6Addr::from(net))
        }
    }

    /// Every address of `other` is inside `self`.
    pub fn contains(&self, other: &Cidr) -> bool {
        let (a, w) = bits(self.addr);
        let (b, w2) = bits(other.addr);
        w == w2 && self.prefix <= other.prefix && (a ^ b) & mask(w, u32::from(self.prefix)) == 0
    }

    /// The two ranges share at least one address.
    pub fn overlaps(&self, other: &Cidr) -> bool {
        let (a, w) = bits(self.addr);
        let (b, w2) = bits(other.addr);
        let p = u32::from(self.prefix.min(other.prefix));
        w == w2 && (a ^ b) & mask(w, p) == 0
    }
}

impl fmt::Display for Cidr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.addr, self.prefix)
    }
}

// ----------------------------------------------------------- host names ---

/// Characters IDNA drops: soft hyphen, zero-width space / (non-)joiner,
/// word joiner, BOM. Dropping them stops `adm\u{200b}in` slipping past an
/// exclusion for `admin`.
fn is_ignored(c: char) -> bool {
    matches!(
        c,
        '\u{00ad}' | '\u{200b}' | '\u{200c}' | '\u{200d}' | '\u{2060}' | '\u{feff}'
    )
}

/// RFC 3492 punycode of one label (without the `xn--` prefix).
fn punycode(input: &str) -> Option<String> {
    const BASE: u32 = 36;
    const TMIN: u32 = 1;
    const TMAX: u32 = 26;
    const SKEW: u32 = 38;
    const DAMP: u32 = 700;

    fn digit(d: u32) -> char {
        if d < 26 {
            (b'a' + d as u8) as char
        } else {
            (b'0' + (d - 26) as u8) as char
        }
    }
    fn adapt(mut delta: u32, points: u32, first: bool) -> u32 {
        delta = if first { delta / DAMP } else { delta / 2 };
        delta += delta / points;
        let mut k = 0;
        while delta > ((BASE - TMIN) * TMAX) / 2 {
            delta /= BASE - TMIN;
            k += BASE;
        }
        k + (BASE - TMIN + 1) * delta / (delta + SKEW)
    }

    let chars: Vec<u32> = input.chars().map(|c| c as u32).collect();
    let mut out: String = input.chars().filter(char::is_ascii).collect();
    let basic = out.len() as u32;
    let mut handled = basic;
    if basic > 0 {
        out.push('-');
    }
    let (mut n, mut delta, mut bias) = (128u32, 0u32, 72u32);
    while (handled as usize) < chars.len() {
        let m = chars.iter().copied().filter(|&c| c >= n).min()?;
        delta = delta.checked_add((m - n).checked_mul(handled + 1)?)?;
        n = m;
        for &c in &chars {
            if c < n {
                delta = delta.checked_add(1)?;
            }
            if c == n {
                let mut q = delta;
                let mut k = BASE;
                loop {
                    let t = if k <= bias {
                        TMIN
                    } else if k >= bias + TMAX {
                        TMAX
                    } else {
                        k - bias
                    };
                    if q < t {
                        break;
                    }
                    out.push(digit(t + (q - t) % (BASE - t)));
                    q = (q - t) / (BASE - t);
                    k += BASE;
                }
                out.push(digit(q));
                bias = adapt(delta, handled + 1, handled == basic);
                delta = 0;
                handled += 1;
            }
        }
        delta += 1;
        n += 1;
    }
    Some(out)
}

/// A host name in its comparable form: lowercase ASCII, punycode labels, no
/// trailing dot. Errors say what is wrong.
fn normalize_host(raw: &str) -> Result<String, String> {
    let folded: String = raw
        .trim()
        .chars()
        .filter(|c| !is_ignored(*c))
        .map(|c| match c {
            '\u{3002}' | '\u{FF0E}' | '\u{FF61}' => '.',
            // fullwidth ASCII (U+FF01..U+FF5E) -> the ASCII it stands for
            '\u{FF01}'..='\u{FF5E}' => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
            c => c,
        })
        .collect::<String>()
        .to_lowercase();
    let name = folded.strip_suffix('.').unwrap_or(&folded);
    if name.is_empty() {
        return Err("empty host name".into());
    }
    let mut labels = Vec::new();
    for label in name.split('.') {
        if label.is_empty() {
            return Err("empty label (a leading dot or `..`)".into());
        }
        for c in label.chars() {
            let ok = if c.is_ascii() {
                c.is_ascii_alphanumeric() || c == '-' || c == '_'
            } else {
                !c.is_whitespace() && !c.is_control()
            };
            if !ok {
                return Err(format!("{c:?} is not allowed in a host name"));
            }
        }
        let label = if label.is_ascii() {
            label.to_string()
        } else {
            format!(
                "xn--{}",
                punycode(label).ok_or_else(|| format!("can't encode `{label}`"))?
            )
        };
        if label.len() > 63 {
            return Err(format!("label `{label}` is longer than 63 characters"));
        }
        labels.push(label);
    }
    let last = labels.last().map(String::as_str).unwrap_or("");
    let hex = last
        .strip_prefix("0x")
        .is_some_and(|h| !h.is_empty() && h.bytes().all(|b| b.is_ascii_hexdigit()));
    if last.bytes().all(|b| b.is_ascii_digit()) || hex {
        return Err(
            "looks like a number, not a host name; write IP addresses as a dotted quad (a.b.c.d)"
                .into(),
        );
    }
    let joined = labels.join(".");
    if joined.len() > 253 {
        return Err("host name is longer than 253 characters".into());
    }
    Ok(joined)
}

// ----------------------------------------------------------------- URLs ---

/// The host part of a URL, or of a host rule.
#[derive(Clone, Debug, PartialEq, Eq)]
enum HostPat {
    Addr(IpAddr),
    Name(String),
    /// `*.example.com`, stored as `example.com`
    Suffix(String),
}

/// Does rule host `rule` cover `target` (a target never is a `Suffix`)?
fn host_covers(rule: &HostPat, target: &HostPat) -> bool {
    match (rule, target) {
        (HostPat::Addr(a), HostPat::Addr(b)) => a == b,
        (HostPat::Name(a), HostPat::Name(b)) => a == b,
        (HostPat::Suffix(s), HostPat::Name(n)) => {
            n.len() > s.len() + 1
                && n.ends_with(s.as_str())
                && n[..n.len() - s.len()].ends_with('.')
        }
        _ => false,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Url {
    scheme: String,
    host: HostPat,
    port: Option<u16>,
    /// normalised, always starts with `/`, no trailing slash except the root
    path: String,
}

fn default_port(scheme: &str) -> Option<u16> {
    match scheme {
        "http" | "ws" => Some(80),
        "https" | "wss" => Some(443),
        "ftp" => Some(21),
        _ => None,
    }
}

fn hex_val(b: u8) -> Option<u8> {
    (b as char).to_digit(16).map(|d| d as u8)
}

/// `%2e` `%2f` `%5c` decoded and `\` turned into `/` (so they can't hide a
/// `..` or a segment boundary), then `.`/`..`/empty segments resolved.
fn normalize_path(path: &str) -> String {
    let b = path.as_bytes();
    let mut decoded = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hex_val(b[i + 1]), hex_val(b[i + 2])) {
                let v = h * 16 + l;
                if matches!(v, 0x2e | 0x2f | 0x5c) {
                    decoded.push(v);
                    i += 3;
                    continue;
                }
            }
        }
        decoded.push(b[i]);
        i += 1;
    }
    let s = String::from_utf8_lossy(&decoded).replace('\\', "/");
    let mut segs: Vec<&str> = Vec::new();
    for seg in s.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                segs.pop();
            }
            x => segs.push(x),
        }
    }
    format!("/{}", segs.join("/"))
}

impl Url {
    /// `rule`: parsing a scope rule (no credentials, `*.` hosts allowed).
    fn parse(s: &str, rule: bool) -> Result<Url, String> {
        let (scheme, rest) = s.split_once("://").ok_or("missing `://`")?;
        let scheme_ok = scheme.starts_with(|c: char| c.is_ascii_alphabetic())
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
        if !scheme_ok {
            return Err(format!("`{scheme}` is not a URL scheme"));
        }
        let scheme = scheme.to_ascii_lowercase();
        // clients treat `\` like `/` in the authority, so it ends it too
        let end = rest.find(['/', '\\', '?', '#']).unwrap_or(rest.len());
        let (authority, tail) = rest.split_at(end);
        let path = tail.split(['?', '#']).next().unwrap_or("");
        let hostport = match authority.rsplit_once('@') {
            Some(_) if rule => {
                return Err("credentials (`user@`) don't belong in a scope rule".into())
            }
            Some((_, h)) => h,
            None => authority,
        };

        let (host_s, port_s, bracketed) = if let Some(inner) = hostport.strip_prefix('[') {
            let (ip, after) = inner.split_once(']').ok_or("missing `]` in IPv6 address")?;
            let port = match after {
                "" => None,
                a => Some(a.strip_prefix(':').ok_or("junk after the IPv6 address")?),
            };
            (ip, port, true)
        } else {
            match hostport.matches(':').count() {
                0 => (hostport, None, false),
                1 => {
                    let (h, p) = hostport.split_once(':').unwrap_or((hostport, ""));
                    (h, Some(p), false)
                }
                _ => return Err("IPv6 addresses in a URL need [brackets]".into()),
            }
        };
        let port = match port_s {
            None | Some("") => None,
            Some(p) if p.bytes().all(|b| b.is_ascii_digit()) => Some(
                p.parse::<u16>()
                    .map_err(|_| format!("port `{p}` is out of range"))?,
            ),
            Some(p) => return Err(format!("`{p}` is not a port")),
        };

        let host = if bracketed {
            let ip: Ipv6Addr = host_s
                .parse()
                .map_err(|_| format!("`{host_s}` is not an IPv6 address"))?;
            HostPat::Addr(IpAddr::V6(ip).to_canonical())
        } else if let Ok(ip) = host_s.parse::<IpAddr>() {
            HostPat::Addr(ip.to_canonical())
        } else if let Some(sfx) = host_s.strip_prefix("*.").filter(|_| rule) {
            HostPat::Suffix(normalize_host(sfx)?)
        } else {
            HostPat::Name(normalize_host(host_s)?)
        };
        let port = port.or_else(|| default_port(&scheme));
        Ok(Url {
            scheme,
            host,
            port,
            path: normalize_path(path),
        })
    }

    /// Does this rule URL's path cover `other`'s (prefix on a segment boundary)?
    fn path_covers(&self, other: &Url) -> bool {
        let r = self.path.trim_end_matches('/');
        r.is_empty() || other.path == r || other.path.starts_with(&format!("{r}/"))
    }

    fn covers(&self, other: &Url) -> bool {
        self.scheme == other.scheme
            && self.port == other.port
            && host_covers(&self.host, &other.host)
            && self.path_covers(other)
    }
}

// -------------------------------------------------------------- targets ---

/// What a tool is about to be pointed at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Ip(IpAddr),
    Cidr(Cidr),
    Host(String),
    Url(Url),
}

/// For `x/y` text that isn't a CIDR, say what was probably meant.
fn cidr_or_hint(item: &str) -> Result<Cidr, String> {
    let before = item.split_once('/').map_or(item, |(a, _)| a);
    // `example.com/api` looks like a URL missing its scheme; `10.0.0/8` is
    // just a mistyped address
    let looks_like_name = before.chars().any(char::is_alphabetic) && !before.contains(':');
    if looks_like_name && before.parse::<IpAddr>().is_err() {
        return Err(format!(
            "`{item}` is not a CIDR; for a URL prefix write the scheme, e.g. https://{item}"
        ));
    }
    Cidr::parse(item)
}

impl Target {
    /// Auto-detects the kind: `://` is a URL, `a/n` a CIDR, an address, else a host.
    pub fn parse(s: &str) -> Result<Target, String> {
        let s = s.trim();
        if s.is_empty() {
            return Err("empty target".into());
        }
        if s.contains("://") {
            return Url::parse(s, false).map(Target::Url);
        }
        if s.contains('*') {
            return Err("a target can't be a wildcard".into());
        }
        if s.contains('/') {
            return cidr_or_hint(s).map(Target::Cidr);
        }
        if let Ok(ip) = s.parse::<IpAddr>() {
            return Ok(Target::Ip(ip.to_canonical()));
        }
        normalize_host(s).map(Target::Host)
    }

    fn family(&self) -> Family {
        match self {
            Target::Ip(_) | Target::Cidr(_) => Family::Addr,
            Target::Host(_) => Family::Name,
            Target::Url(u) => host_family(&u.host),
        }
    }
}

/// Addresses can only be compared with address rules, names with name rules
/// (no DNS here), which is what makes a verdict `Unknown`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Family {
    Addr,
    Name,
}

fn host_family(h: &HostPat) -> Family {
    match h {
        HostPat::Addr(_) => Family::Addr,
        _ => Family::Name,
    }
}

// ---------------------------------------------------------------- rules ---

#[derive(Clone, Debug)]
enum Kind {
    Net(Cidr),
    Host(HostPat),
    Url(Url),
}

#[derive(Clone, Debug)]
struct Rule {
    /// as the user wrote it, for messages
    text: String,
    kind: Kind,
}

impl Rule {
    fn parse(item: &str) -> Result<Rule, String> {
        let kind = if item.contains("://") {
            Kind::Url(Url::parse(item, true)?)
        } else if let Some(sfx) = item.strip_prefix("*.") {
            Kind::Host(HostPat::Suffix(normalize_host(sfx)?))
        } else if item.contains('*') {
            return Err("wildcards look like *.example.com".into());
        } else if item.contains('/') {
            Kind::Net(cidr_or_hint(item)?)
        } else if let Ok(ip) = item.parse::<IpAddr>() {
            Kind::Net(Cidr::host(ip))
        } else {
            Kind::Host(HostPat::Name(normalize_host(item)?))
        };
        Ok(Rule {
            text: item.to_string(),
            kind,
        })
    }

    fn family(&self) -> Family {
        match &self.kind {
            Kind::Net(_) => Family::Addr,
            Kind::Host(_) => Family::Name,
            Kind::Url(u) => host_family(&u.host),
        }
    }

    /// The rule fully contains the target (what an include needs).
    fn covers(&self, t: &Target) -> bool {
        self.matches(t, false)
    }

    /// The rule touches the target (what an exclude needs): like `covers`,
    /// but a CIDR target that merely overlaps counts.
    fn hits(&self, t: &Target) -> bool {
        self.matches(t, true)
    }

    fn matches(&self, t: &Target, overlap: bool) -> bool {
        match (&self.kind, t) {
            (Kind::Net(c), Target::Ip(ip)) => c.contains(&Cidr::host(*ip)),
            (Kind::Net(c), Target::Cidr(x)) => {
                if overlap {
                    c.overlaps(x)
                } else {
                    c.contains(x)
                }
            }
            (Kind::Net(c), Target::Url(u)) => match u.host {
                HostPat::Addr(ip) => c.contains(&Cidr::host(ip)),
                _ => false,
            },
            (Kind::Host(p), Target::Host(h)) => host_covers(p, &HostPat::Name(h.clone())),
            (Kind::Host(p), Target::Url(u)) => host_covers(p, &u.host),
            (Kind::Url(r), Target::Url(u)) => r.covers(u),
            _ => false,
        }
    }
}

// ---------------------------------------------------------------- scope ---

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    InScope,
    /// `rule` is the exclusion that fired; `None` = nothing includes it
    OutOfScope {
        rule: Option<String>,
    },
    /// Can't be judged without resolving names (or no include rules at all)
    Unknown,
}

/// One problem in the scope text, with where it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    pub part: &'static str,
    /// 1-based, within that part's text
    pub line: usize,
    pub text: String,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} line {}: `{}`: {}",
            self.part, self.line, self.text, self.message
        )
    }
}

#[derive(Clone, Debug, Default)]
pub struct Scope {
    include: Vec<Rule>,
    exclude: Vec<Rule>,
}

/// Text before an inline `# comment` (a `#` at the start or after whitespace).
fn strip_comment(line: &str) -> &str {
    let mut prev_ws = true;
    for (i, c) in line.char_indices() {
        if c == '#' && prev_ws {
            return &line[..i];
        }
        prev_ws = c.is_whitespace();
    }
    line
}

fn parse_rules(part: &'static str, text: &str, errors: &mut Vec<ParseError>) -> Vec<Rule> {
    let mut rules = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let items = strip_comment(line)
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter(|s| !s.is_empty());
        for item in items {
            match Rule::parse(item) {
                Ok(r) => rules.push(r),
                Err(message) => errors.push(ParseError {
                    part,
                    line: i + 1,
                    text: item.to_string(),
                    message,
                }),
            }
        }
    }
    rules
}

impl Scope {
    /// Parse the include and exclude texts; every problem is reported, with
    /// its line.
    pub fn parse(include: &str, exclude: &str) -> Result<Scope, Vec<ParseError>> {
        let mut errors = Vec::new();
        let include = parse_rules("include", include, &mut errors);
        let exclude = parse_rules("exclude", exclude, &mut errors);
        if errors.is_empty() {
            Ok(Scope { include, exclude })
        } else {
            Err(errors)
        }
    }

    pub fn is_empty(&self) -> bool {
        self.include.is_empty()
    }

    pub fn check(&self, t: &Target) -> Verdict {
        if let Some(r) = self.exclude.iter().find(|r| r.hits(t)) {
            return Verdict::OutOfScope {
                rule: Some(r.text.clone()),
            };
        }
        if self.include.iter().any(|r| r.covers(t)) {
            return Verdict::InScope;
        }
        let family = t.family();
        if self.include.iter().any(|r| r.family() == family) {
            Verdict::OutOfScope { rule: None }
        } else {
            Verdict::Unknown
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fuzz::{nasty_string, Rng};

    const IN: Verdict = Verdict::InScope;
    const OUT: Verdict = Verdict::OutOfScope { rule: None };
    const UNK: Verdict = Verdict::Unknown;

    fn out_by(rule: &str) -> Verdict {
        Verdict::OutOfScope {
            rule: Some(rule.into()),
        }
    }

    fn scope(include: &str, exclude: &str) -> Scope {
        Scope::parse(include, exclude).unwrap_or_else(|e| panic!("scope should parse: {e:?}"))
    }

    /// Table runner: every (target, verdict) pair against one scope.
    fn table(s: &Scope, rows: &[(&str, Verdict)]) {
        for (target, want) in rows {
            let t = Target::parse(target).unwrap_or_else(|e| panic!("{target}: {e}"));
            assert_eq!(&s.check(&t), want, "target {target}");
        }
    }

    #[test]
    fn addresses_and_cidrs() {
        let s = scope(
            "10.0.0.0/8\n192.168.1.5\n2001:db8::/32",
            "10.1.0.0/16, 10.0.0.13",
        );
        table(
            &s,
            &[
                ("10.2.3.4", IN),
                ("10.1.2.3", out_by("10.1.0.0/16")),
                ("10.0.0.13", out_by("10.0.0.13")),
                ("11.0.0.1", OUT),
                ("192.168.1.5", IN),
                ("192.168.1.6", OUT),
                ("2001:db8::1", IN),
                ("2001:db9::1", OUT),
                ("::ffff:10.2.3.4", IN), // mapped IPv4 is the IPv4
                ("::ffff:10.1.2.3", out_by("10.1.0.0/16")),
                // CIDR targets: fully inside, or not
                ("10.2.0.0/16", IN),
                ("10.0.0.0/8", out_by("10.1.0.0/16")), // swallows an excluded range
                ("10.2.0.0/15", IN),
                ("10.0.0.0/7", out_by("10.1.0.0/16")), // overlaps the exclusion too
                ("10.0.0.0/15", out_by("10.1.0.0/16")),
                ("192.168.1.5/32", IN),
                ("192.168.1.0/24", OUT),
                // names against a scope of addresses: can't tell
                ("example.com", UNK),
                ("https://example.com/", UNK),
            ],
        );
    }

    #[test]
    fn cidr_edges() {
        let all = scope("0.0.0.0/0", "");
        table(&all, &[("8.8.8.8", IN), ("1.0.0.0/8", IN), ("::1", OUT)]);
        let v6 = scope("::/0", "fe80::/10");
        table(
            &v6,
            &[
                ("2001:db8::1", IN),
                ("fe80::1", out_by("fe80::/10")),
                ("1.2.3.4", OUT),
            ],
        );
        let one = scope("203.0.113.7/32", "");
        table(
            &one,
            &[
                ("203.0.113.7", IN),
                ("203.0.113.8", OUT),
                ("203.0.113.0/24", OUT), // bigger than the rule: not fully inside
                ("203.0.113.7/32", IN),
            ],
        );
        let net = scope("10.0.0.0/8", "");
        table(&net, &[("10.0.0.0/7", OUT), ("10.0.0.0/8", IN)]);
    }

    #[test]
    fn host_names_and_wildcards() {
        let s = scope(
            "*.example.com, www.other.org",
            "admin.example.com\n*.internal.example.com",
        );
        table(
            &s,
            &[
                ("a.example.com", IN),
                ("a.b.example.com", IN), // any depth
                ("example.com", OUT),    // the apex is NOT covered by the wildcard
                ("notexample.com", OUT),
                ("evil-example.com", OUT),
                ("example.com.evil.org", OUT),
                ("WWW.Other.ORG.", IN), // case and trailing dot
                ("www.other.org", IN),
                ("other.org", OUT),
                ("sub.www.other.org", OUT), // a plain host rule is exact
                ("admin.example.com", out_by("admin.example.com")),
                ("x.admin.example.com", IN), // exact exclusion, not its subdomains
                ("db.internal.example.com", out_by("*.internal.example.com")),
                ("10.0.0.1", UNK), // an address against names only
            ],
        );
        // the apex listed explicitly
        let both = scope("example.com\n*.example.com", "");
        table(&both, &[("example.com", IN), ("a.example.com", IN)]);
    }

    #[test]
    fn idn_and_punycode_compare_equal() {
        let s = scope("bücher.de\n*.münchen.de", "");
        table(
            &s,
            &[
                ("BÜCHER.de", IN),
                ("xn--bcher-kva.de", IN),
                ("bu\u{0308}cher.de", OUT), // decomposed ü is not folded (no NFC)
                ("shop.xn--mnchen-3ya.de", IN),
                ("shop.münchen.de.", IN),
                ("buecher.de", OUT),
                ("ｂücher。de", IN),       // fullwidth b, ideographic full stop
                ("bü\u{200b}cher.de", IN), // zero-width space is ignored
            ],
        );
        // an exclusion can't be dodged with invisible characters
        let s = scope("*.example.com", "admin.example.com");
        table(
            &s,
            &[
                ("adm\u{200b}in.example.com", out_by("admin.example.com")),
                ("adm\u{00ad}in.example.com", out_by("admin.example.com")),
                ("ａｄｍｉｎ.example.com", out_by("admin.example.com")),
            ],
        );
    }

    #[test]
    fn punycode_matches_the_reference_vectors() {
        for (word, want) in [
            ("bücher", "bcher-kva"),
            ("münchen", "mnchen-3ya"),
            ("日本語", "wgv71a119e"),
            ("президент", "d1abbgf6aiiy"),
            ("рф", "p1ai"),
            ("правда", "80aafi6cg"),
            ("ドメイン", "eckwd4c7c"),
            ("mañana", "maana-pta"),
            ("café-bar", "caf-bar-dya"),
            ("ü", "tda"),
            ("例え", "r8jz45g"),
            ("ليهمابتكلموشعربي؟", "egbpdaj6bu4bxfgehfvwxn"),
        ] {
            assert_eq!(punycode(word).as_deref(), Some(want), "{word}");
        }
        assert_eq!(
            normalize_host("президент.РФ").unwrap(),
            "xn--d1abbgf6aiiy.xn--p1ai"
        );
    }

    #[test]
    fn url_prefix_rules() {
        let s = scope(
            "https://app.example.com/api/",
            "https://app.example.com/api/internal/",
        );
        table(
            &s,
            &[
                ("https://app.example.com/api/v1/users?x=1#frag", IN),
                ("https://app.example.com/api", IN),
                ("https://app.example.com/api/", IN),
                ("https://APP.Example.COM:443/api/x", IN), // case + default port
                ("https://app.example.com./api/x", IN),
                ("http://app.example.com/api/", OUT), // scheme
                ("https://app.example.com:8443/api/", OUT), // port
                ("https://app.example.com/apix", OUT), // segment boundary
                ("https://app.example.com/", OUT),    // broader than the rule
                ("https://app.example.com/api/../admin", OUT),
                ("https://app.example.com/api/%2e%2e/admin", OUT),
                ("https://app.example.com/api/%2E%2e/admin", OUT),
                ("https://app.example.com/api/..%2fadmin", OUT),
                ("https://app.example.com/api/..%5cadmin", OUT),
                ("https://app.example.com/api\\..\\admin", OUT),
                ("https://app.example.com//api//v1", IN),
                ("https://app.example.com/other/../api/v1", IN),
                ("https://other.example.com/api/", OUT),
                ("https://app.example.com@evil.com/api/", OUT), // the host is evil.com
                ("https://evil.com\\@app.example.com/api/", OUT),
                ("https://user:pw@app.example.com/api/x", IN),
                (
                    "https://app.example.com/api/internal/x",
                    out_by("https://app.example.com/api/internal/"),
                ),
                (
                    "https://app.example.com/api/internal",
                    out_by("https://app.example.com/api/internal/"),
                ),
                ("https://app.example.com/api/internals", IN),
                // a bare host is broader than a path prefix
                ("app.example.com", OUT),
                ("10.0.0.1", UNK),
            ],
        );
    }

    #[test]
    fn urls_against_host_and_address_rules() {
        let s = scope("*.example.com\n10.0.0.0/8", "10.9.0.0/16");
        table(
            &s,
            &[
                ("https://a.example.com:8443/x/y", IN), // host rules ignore port and path
                ("ftp://a.example.com/", IN),
                ("https://example.com/", OUT),
                ("http://10.1.2.3:8080/x", IN),
                ("http://10.9.2.3/x", out_by("10.9.0.0/16")),
                ("http://[::1]/", OUT),
                ("http://[::ffff:10.1.2.3]/", IN),
            ],
        );
        assert!(Target::parse("http://10.1.2.3:99999/").is_err());
        // a URL rule with a wildcard host
        let w = scope("https://*.example.com/app", "");
        table(
            &w,
            &[
                ("https://x.example.com/app/y", IN),
                ("https://x.y.example.com/app", IN),
                ("https://example.com/app", OUT),
                ("https://x.example.com/", OUT),
            ],
        );
    }

    #[test]
    fn ipv6_in_urls() {
        let s = scope("https://[2001:db8::1]:8443/", "");
        table(
            &s,
            &[
                ("https://[2001:db8::1]:8443/a", IN),
                ("https://[2001:DB8:0:0:0:0:0:1]:8443/a", IN),
                ("https://[2001:db8::2]:8443/a", OUT),
                ("https://[2001:db8::1]/a", OUT), // default port 443
            ],
        );
        assert!(Target::parse("https://2001:db8::1/").is_err()); // needs brackets
        assert!(Target::parse("https://[2001:db8::1/").is_err());
    }

    #[test]
    fn empty_and_unjudgeable_scopes_are_unknown() {
        let none = scope("", "");
        assert!(none.is_empty());
        table(&none, &[("10.0.0.1", UNK), ("example.com", UNK)]);
        // exclusions alone still exclude, but never include
        let only_excl = scope("# nothing yet", "10.0.0.0/8");
        table(
            &only_excl,
            &[("10.1.1.1", out_by("10.0.0.0/8")), ("8.8.8.8", UNK)],
        );
    }

    #[test]
    fn ambiguous_numeric_hosts_are_errors() {
        for t in [
            "2130706433",
            "127.1",
            "0x7f.0.0.1",
            "0x7f000001",
            "010.0.0.1",
            "1.2.3",
            "1.2.3.4.5",
            "http://2130706433/",
            "http://0x7f.1/",
        ] {
            assert!(Target::parse(t).is_err(), "{t} should be refused");
        }
        for r in ["2130706433", "*.127.1", "http://127.1/"] {
            assert!(Scope::parse(r, "").is_err(), "{r} should be refused");
        }
        // real names with digits are fine
        for t in [
            "a1.example.com",
            "1password.com",
            "x.co2",
            "web-0x.example.com",
        ] {
            assert!(Target::parse(t).is_ok(), "{t}");
        }
    }

    #[test]
    fn parse_errors_carry_part_and_line() {
        let errs = Scope::parse(
            "10.0.0/8\n\n# a comment\n*.ok.example\nfoo bar/baz\nhttps://\n10.0.0.5/24\nhttps://u@h.com/",
            "a*.example.com\n300.1.1.1/8, fine.example.com\n::1/129",
        )
        .unwrap_err();
        let got: Vec<(&str, usize, &str)> = errs
            .iter()
            .map(|e| (e.part, e.line, e.text.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                ("include", 1, "10.0.0/8"),
                ("include", 5, "bar/baz"),
                ("include", 6, "https://"),
                ("include", 7, "10.0.0.5/24"),
                ("include", 8, "https://u@h.com/"),
                ("exclude", 1, "a*.example.com"),
                ("exclude", 2, "300.1.1.1/8"),
                ("exclude", 3, "::1/129"),
            ]
        );
        let msg = |i: usize| errs[i].message.as_str();
        assert!(msg(1).contains("https://bar/baz"), "{}", msg(1)); // the URL hint
        assert!(msg(3).contains("did you mean 10.0.0.0/24"), "{}", msg(3));
        assert!(msg(4).contains("credentials"), "{}", msg(4));
        assert!(msg(7).contains("out of range"), "{}", msg(7));
        assert_eq!(
            errs[0].to_string(),
            "include line 1: `10.0.0/8`: `10.0.0` is not an IPv4 or IPv6 address"
        );
    }

    #[test]
    fn rule_text_syntax() {
        // commas, spaces, comments, CRLF
        let s = scope(
            "10.0.0.0/8, 172.16.0.0/12 192.168.0.0/16 # private ranges\r\nexample.com\r\n",
            "  \n#\n",
        );
        table(
            &s,
            &[("172.20.1.1", IN), ("192.168.9.9", IN), ("example.com", IN)],
        );
        // # inside a URL is not a comment
        let s = scope("https://a.example.com/x#y", "");
        table(&s, &[("https://a.example.com/x", IN)]);
    }

    #[test]
    fn bad_targets() {
        for t in [
            "",
            "  ",
            "*.example.com",
            "a b",
            "host:8080",
            "exa mple.com",
            "-.-/8",
            "..",
            ".example.com",
            "a..b",
            "example.com/path",
            "10.0.0.5/24",
            "10.0.0.0/33",
            "http://",
            "://x",
            "1http://x",
            "http://a:b/",
            "http://a:70000/",
            "http://[::1/",
        ] {
            assert!(Target::parse(t).is_err(), "{t:?} should be refused");
        }
        let long = format!("{}.com", "a".repeat(64));
        assert!(Target::parse(&long).is_err());
    }

    /// CIDR maths checked against plain integer ranges.
    #[test]
    fn cidr_contains_and_overlaps_match_a_range_oracle() {
        let mut rng = Rng::new(64);
        let mk = |rng: &mut Rng| {
            let prefix = rng.below(33) as u32;
            let m = if prefix == 0 {
                0
            } else {
                u32::MAX << (32 - prefix)
            };
            // few distinct top bits, so ranges often nest and overlap
            let raw = ((rng.below(2) as u32) << 31)
                | ((rng.below(4) as u32) << 24)
                | (rng.next() as u32 & 0x00FF_FFFF);
            let addr = raw & m;
            (Ipv4Addr::from(addr), prefix, addr, addr | !m)
        };
        let (mut nested, mut overlapping, mut disjoint) = (0, 0, 0);
        for _ in 0..4000 {
            let (ia, pa, alo, ahi) = mk(&mut rng);
            let (ib, pb, blo, bhi) = mk(&mut rng);
            let a = Cidr::parse(&format!("{ia}/{pa}")).unwrap();
            let b = Cidr::parse(&format!("{ib}/{pb}")).unwrap();
            let (contains, overlaps) = (alo <= blo && bhi <= ahi, alo <= bhi && blo <= ahi);
            assert_eq!(a.contains(&b), contains, "{a} contains {b}");
            assert_eq!(a.overlaps(&b), overlaps, "{a} overlaps {b}");
            assert_eq!(a.overlaps(&b), b.overlaps(&a));
            nested += usize::from(contains);
            overlapping += usize::from(overlaps && !contains);
            disjoint += usize::from(!overlaps);
        }
        // the generator really exercises all three relations
        assert!(
            nested > 100 && overlapping > 100 && disjoint > 100,
            "{nested} {overlapping} {disjoint}"
        );
    }

    #[test]
    fn nothing_panics_on_random_input_and_included_hosts_stay_included() {
        let mut rng = Rng::new(0x5C0);
        for _ in 0..4000 {
            let text = nasty_string(&mut rng, 20);
            let _ = Target::parse(&text);
            let _ = Scope::parse(&text, &text);
            let _ = normalize_path(&text);
        }
        // appending a path, port or query to an included host never escapes it
        let s = scope("*.example.com", "");
        for _ in 0..500 {
            let junk: String = nasty_string(&mut rng, 8)
                .chars()
                .filter(|c| !matches!(c, '/' | '\\' | '?' | '#' | '@' | ':' | '%'))
                .collect();
            let url = format!("https://a.example.com/{junk}?q={junk}");
            if let Ok(t) = Target::parse(&url) {
                assert_eq!(s.check(&t), IN, "{url}");
            }
        }
    }
}
