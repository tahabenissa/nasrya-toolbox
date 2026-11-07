//! Built-in hash identifier (no Python, no install, works offline).
//!
//! Like hashid / Name-That-Hash: look at a hash's prefix, separators, length
//! and alphabet and list the likely types, most likely first, each with its
//! hashcat `-m` mode and John the Ripper `--format=` where one exists.
//! Plain hex digests are inherently ambiguous (MD5, NTLM and MD4 all look
//! the same), so those come back as several candidates.

/// One possible identification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub name: &'static str,
    pub hashcat: Option<u32>,
    pub john: Option<&'static str>,
}

const fn c(name: &'static str, hashcat: Option<u32>, john: Option<&'static str>) -> Candidate {
    Candidate {
        name,
        hashcat,
        john,
    }
}

fn is_hex(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_hexdigit())
}

fn is_b64(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'='))
}

/// The crypt(3) alphabet: `./0-9A-Za-z`.
fn is_crypt64(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'/'))
}

/// Identify `hash`. An empty result means "no idea".
pub fn identify(hash: &str) -> Vec<Candidate> {
    let h = hash.trim();
    if h.is_empty() {
        return vec![];
    }
    if let Some(found) = by_structure(h) {
        return found;
    }
    by_hex_length(h)
}

/// Formats recognisable by a prefix or by their separators.
fn by_structure(h: &str) -> Option<Vec<Candidate>> {
    let starts = |p: &str| h.len() > p.len() && h[..p.len()].eq_ignore_ascii_case(p);

    // --- modular crypt format ($id$...) ---
    for p in ["$2a$", "$2b$", "$2y$", "$2x$"] {
        // $2b$10$ + 22 salt + 31 hash = 60 chars
        if h.starts_with(p) && h.len() == 60 {
            return Some(vec![c("bcrypt", Some(3200), Some("bcrypt"))]);
        }
    }
    if h.starts_with("$1$") {
        return Some(vec![c("md5crypt (Unix MD5)", Some(500), Some("md5crypt"))]);
    }
    if h.starts_with("$apr1$") {
        return Some(vec![c("Apache apr1 MD5", Some(1600), Some("md5crypt"))]);
    }
    if h.starts_with("$5$") {
        return Some(vec![c("sha256crypt", Some(7400), Some("sha256crypt"))]);
    }
    if h.starts_with("$6$") {
        return Some(vec![c("sha512crypt", Some(1800), Some("sha512crypt"))]);
    }
    if h.starts_with("$y$") {
        return Some(vec![c("yescrypt", None, Some("crypt"))]);
    }
    if h.starts_with("$argon2id$") || h.starts_with("$argon2i$") || h.starts_with("$argon2d$") {
        return Some(vec![c("Argon2", None, Some("argon2"))]);
    }
    if (h.starts_with("$P$") || h.starts_with("$H$")) && h.len() == 34 {
        return Some(vec![c(
            "phpass (WordPress / phpBB3)",
            Some(400),
            Some("phpass"),
        )]);
    }
    if h.starts_with("$krb5tgs$23$") {
        return Some(vec![c(
            "Kerberos 5 TGS-REP etype 23 (Kerberoast)",
            Some(13100),
            Some("krb5tgs"),
        )]);
    }
    if h.starts_with("$krb5asrep$23$") {
        return Some(vec![c(
            "Kerberos 5 AS-REP etype 23 (AS-REP roast)",
            Some(18200),
            Some("krb5asrep"),
        )]);
    }
    if starts("$DCC2$") {
        return Some(vec![c(
            "Domain Cached Credentials 2 (DCC2 / MS Cache v2)",
            Some(2100),
            Some("mscash2"),
        )]);
    }

    // --- LDAP ---
    if starts("{SSHA}") && is_b64(&h[6..]) {
        return Some(vec![c(
            "SSHA-1 (LDAP salted SHA-1)",
            Some(111),
            Some("salted-sha1"),
        )]);
    }
    if starts("{SHA}") && is_b64(&h[5..]) {
        return Some(vec![c("SHA-1 (LDAP, base64)", Some(101), Some("nsldap"))]);
    }

    // --- Django ---
    if let Some(rest) = h.strip_prefix("pbkdf2_sha256$") {
        if rest.split('$').count() == 3 {
            return Some(vec![c("Django PBKDF2-SHA256", Some(10000), Some("django"))]);
        }
    }
    if let Some(rest) = h.strip_prefix("sha1$") {
        let parts: Vec<&str> = rest.split('$').collect();
        if parts.len() == 2 && parts[1].len() == 40 && is_hex(parts[1]) {
            return Some(vec![c("Django SHA-1", Some(124), None)]);
        }
    }

    // --- MySQL 4.1+ ---
    if let Some(rest) = h.strip_prefix('*') {
        if rest.len() == 40 && is_hex(rest) {
            return Some(vec![c("MySQL 4.1 / 5.x", Some(300), Some("mysql-sha1"))]);
        }
    }

    // --- NetNTLM (Responder / captured challenge-response) ---
    // user::domain:challenge:ntproofstr:blob   (v2)
    // user::domain:lm:nt:challenge             (v1)
    let parts: Vec<&str> = h.split(':').collect();
    if parts.len() == 6 && parts[1].is_empty() {
        let (a, b, d) = (parts[3], parts[4], parts[5]);
        if a.len() == 16 && is_hex(a) && b.len() == 32 && is_hex(b) && is_hex(d) {
            return Some(vec![c("NetNTLMv2", Some(5600), Some("netntlmv2"))]);
        }
        if a.len() == 48 && is_hex(a) && b.len() == 48 && is_hex(b) && d.len() == 16 && is_hex(d) {
            return Some(vec![c("NetNTLMv1", Some(5500), Some("netntlm"))]);
        }
    }

    // --- hash:salt ---
    if parts.len() == 2 && is_hex(parts[0]) && !parts[1].is_empty() {
        let found = match parts[0].len() {
            32 => vec![
                c("md5($pass.$salt)", Some(10), Some("dynamic_1")),
                c("md5($salt.$pass)", Some(20), Some("dynamic_4")),
                c(
                    "DCC / MS Cache v1 (hash:username)",
                    Some(1100),
                    Some("mscash"),
                ),
            ],
            40 => vec![
                c("sha1($pass.$salt)", Some(110), None),
                c("sha1($salt.$pass)", Some(120), None),
            ],
            64 => vec![
                c("sha256($pass.$salt)", Some(1410), None),
                c("sha256($salt.$pass)", Some(1420), None),
            ],
            _ => vec![],
        };
        if !found.is_empty() {
            return Some(found);
        }
    }

    // --- traditional DES crypt: 13 chars of ./0-9A-Za-z ---
    if h.len() == 13 && is_crypt64(h) && !is_hex(h) {
        return Some(vec![c(
            "DES crypt (traditional Unix)",
            Some(1500),
            Some("descrypt"),
        )]);
    }
    None
}

/// Bare hex digests: identified by length only, so usually ambiguous.
fn by_hex_length(h: &str) -> Vec<Candidate> {
    if !is_hex(h) {
        return vec![];
    }
    match h.len() {
        8 => vec![c("CRC32", Some(11500), Some("crc32"))],
        16 => vec![c("MySQL 3.23 (OLD_PASSWORD)", Some(200), Some("mysql"))],
        32 => vec![
            c("MD5", Some(0), Some("raw-md5")),
            c("NTLM", Some(1000), Some("nt")),
            c("MD4", Some(900), Some("raw-md4")),
            c("LM", Some(3000), Some("lm")),
        ],
        40 => vec![
            c("SHA-1", Some(100), Some("raw-sha1")),
            c("RIPEMD-160", Some(6000), Some("ripemd-160")),
            c(
                "MySQL 4.1 / 5.x (without leading *)",
                Some(300),
                Some("mysql-sha1"),
            ),
        ],
        56 => vec![
            c("SHA-224", Some(1300), Some("raw-sha224")),
            c("SHA3-224", Some(17300), None),
        ],
        64 => vec![
            c("SHA-256", Some(1400), Some("raw-sha256")),
            c("SHA3-256", Some(17400), None),
            c("Keccak-256", Some(17800), None),
        ],
        96 => vec![
            c("SHA-384", Some(10800), Some("raw-sha384")),
            c("SHA3-384", Some(17500), None),
        ],
        128 => vec![
            c("SHA-512", Some(1700), Some("raw-sha512")),
            c("SHA3-512", Some(17600), None),
            c("Whirlpool", Some(6100), Some("whirlpool")),
        ],
        _ => vec![],
    }
}

/// Human-readable report, one line per candidate.
pub fn report(hash: &str) -> Vec<String> {
    let found = identify(hash);
    if found.is_empty() {
        return vec![format!(
            "no match for {:?} — not a hash format winsploit knows",
            hash.trim()
        )];
    }
    let w = found.iter().map(|f| f.name.len()).max().unwrap_or(0);
    let mut out = Vec::new();
    if found.len() > 1 {
        out.push(format!("{} candidates, most likely first:", found.len()));
    }
    for f in found {
        let hc = f
            .hashcat
            .map(|m| format!("hashcat -m {m}"))
            .unwrap_or_else(|| "hashcat: n/a".into());
        let jn = f
            .john
            .map(|j| format!("john --format={j}"))
            .unwrap_or_default();
        out.push(
            format!("  {:<w$}  {hc:<16} {jn}", f.name)
                .trim_end()
                .to_string(),
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn top(h: &str) -> (&'static str, Option<u32>) {
        let f = identify(h);
        assert!(!f.is_empty(), "no match for {h}");
        (f[0].name, f[0].hashcat)
    }

    #[test]
    fn raw_digests() {
        // md5("password")
        assert_eq!(top("5f4dcc3b5aa765d61d8327deb882cf99"), ("MD5", Some(0)));
        let all = identify("5f4dcc3b5aa765d61d8327deb882cf99");
        assert!(all
            .iter()
            .any(|c| c.name == "NTLM" && c.hashcat == Some(1000)));
        // sha1("password")
        assert_eq!(
            top("5baa61e4c9b93f3f0682250b6cf8331b7ee68fd8"),
            ("SHA-1", Some(100))
        );
        // sha256("password")
        assert_eq!(
            top("5e884898da28047151d0e56f8dc6292773603d0d6aabbdd62a11ef721d1542d8"),
            ("SHA-256", Some(1400))
        );
        // sha512("password")
        assert_eq!(top("b109f3bbbc244eb82441917ed06d618b9008dd09b3befd1b5e07394c706a8bb980b1d7785e5976ec049b46df5f1326af5a2ea6d103fd07c95385ffab0cacbc86"), ("SHA-512", Some(1700)));
        assert_eq!(top("cbf43926"), ("CRC32", Some(11500)));
        assert_eq!(top(&"a".repeat(96)).0, "SHA-384");
        assert_eq!(top(&"a".repeat(56)).0, "SHA-224");
    }

    #[test]
    fn uppercase_and_whitespace() {
        assert_eq!(
            top("  5F4DCC3B5AA765D61D8327DEB882CF99\n"),
            ("MD5", Some(0))
        );
    }

    #[test]
    fn crypt_formats() {
        assert_eq!(
            top("$2b$12$GhvMmNVjRW29ulnudl.LbuAnUtN/LRfe1JsBm1Xu6LE3059z5Tr8m"),
            ("bcrypt", Some(3200))
        );
        assert_eq!(top("$1$28772684$iEwNOgGugqO9.bIz5sk8k/").1, Some(500));
        assert_eq!(top("$apr1$71850310$gh9m4xcAn3MGxogwX/ztb.").1, Some(1600));
        assert_eq!(
            top("$5$rounds=5000$GX7BopJZJxPc/KEK$le16UF8I2Anb.rOrn22AUPWvzUETDGefUmAV8AZkGcD").1,
            Some(7400)
        );
        assert_eq!(top("$6$52450745$k5ka2p8bFuSmoVT1tzOyyuaREkkKBcCNqoDKzYiJL9RaE8yMnPgh2XzzF0NDrUhgrcLwg78xs1w5pJiypEdFX/").1, Some(1800));
        assert_eq!(top("$P$984478476IagS59wHZvyQMArzfx58u.").1, Some(400));
        assert_eq!(
            top("$argon2id$v=19$m=65536,t=3,p=4$c2FsdA$aGFzaA").0,
            "Argon2"
        );
        assert_eq!(top("$y$j9T$salt$hash").0, "yescrypt");
        assert_eq!(top("rEK1ecacw.7.c").1, Some(1500));
    }

    #[test]
    fn windows_and_ad_formats() {
        assert_eq!(
            top("$krb5tgs$23$*user$realm$test/spn*$63386d22d359fe42230300d56852c9eb$891ad31d09ab89c6b3b8c5e5de6c06a7f49fd559d7a9a3c32576c8fedf705376cea582ab5938f7fc8bc741acf05c5990741b36ef4311fe3562a41b70a4ec6ecba849905f2385bb3799d92499909658c7287c49160276bca0006c350b0db4fd387adc27c01e9e9ad0c20ed53a7e6356dee2452e35eca2a6a1d1432796fc5c19d068978df74d3d0baf35c77de12456bf1144b6a750d11f55805f5a16ece2975246e2d026dce997fba34ac8757312e9e4e6272de35e20d52fb668c5ed").1,
            Some(13100)
        );
        assert_eq!(top("$krb5asrep$23$user@domain.local:3e156ada591263b8aab0965f5aebd837$007497cb51b6c8116d6407a782ea0e1c5402b17db7afa6b05a6d30ed164a9933c754d720e279c6c573679bd27128fe77e5fea1f72334c1193c8ff0b370fadc6368bf2d49bbfdba4c5dccab95e8c8ebfdc75f438a0797dbfb2f8a1a5f4c423f9bfc1fea483342a11bd56a216f4d5158ccc4b224b52894fadfba3957dfe4b6b8f5f9f9fe422811a314768673e0c924340b8ccb84775ce9defaa3baa0910b676ad0036d13032b0dd94e3b13903cc738a7b6d00b0b3c210d1f972a6c7cae9bd3c959acf7565be528fc179118f28c679f6deeee1456f0781eb8154e18e49cb27b64bf74cd7112a0ebae2102ac").1, Some(18200));
        assert_eq!(
            top("admin::N46iSNekpT:08ca45b7d7ea58ee:88dcbe4446168966a153a0064958dac6:5c7830315c7830310000000000000b45c67103d07d7b95acd12ffa11230e0000000052920b85f78d013c31cdb3b92f5d765c783030").1,
            Some(5600)
        );
        assert_eq!(
            top("u4-netntlm::kNS:338d08f8e26de93300000000000000000000000000000000:9526fb8c23a90751cdd619b6cea564742e1e4bf33006ba41:cb8086049ec4736c").1,
            Some(5500)
        );
        assert_eq!(
            top("$DCC2$10240#tom#e4e938d12fe5974dc42a90120bd9c90f").1,
            Some(2100)
        );
    }

    #[test]
    fn web_and_db_formats() {
        assert_eq!(
            top("*2470C0C06DEE42FD1618BB99005ADCA2EC9D1E19").1,
            Some(300)
        );
        assert_eq!(top("606717496665bcba").1, Some(200));
        assert_eq!(
            top("{SSHA}AZKja92fbuuB9SpRlHqaoXxbTc43Mzc2MDM1Ng==").1,
            Some(111)
        );
        assert_eq!(top("{SHA}W6ph5Mm5Pz8GgiULbPgzG37mj9g=").1, Some(101));
        assert_eq!(
            top("pbkdf2_sha256$20000$H0dPx8NeajVu$GiC4k5kqbbR9qWBlsRgDywNqC2vd9kqfk7zdorEnNas=").1,
            Some(10000)
        );
        assert_eq!(
            top("sha1$fe76b$02d5916550edf7fc8c886f044887f4b1abf9b013").1,
            Some(124)
        );
        assert_eq!(top("01dfae6e5d4d90d9892622325959afbe:7050461").1, Some(10));
    }

    #[test]
    fn junk_is_unknown() {
        assert!(identify("").is_empty());
        assert!(identify("hello world").is_empty());
        assert!(identify("zzzz").is_empty());
        assert!(identify(&"a".repeat(33)).is_empty());
    }

    #[test]
    fn report_lists_modes() {
        let r = report("5f4dcc3b5aa765d61d8327deb882cf99");
        assert!(r[0].contains("candidates"));
        assert!(r.iter().any(|l| l.contains("MD5")
            && l.contains("hashcat -m 0")
            && l.contains("--format=raw-md5")));
        assert!(report("nope").last().unwrap().contains("no match"));
    }
}
