//! Sanity checks for the Scoop manifests winsploit ships in `bucket/`.
//!
//! The bucket is added by Scoop straight from this repo, so a broken
//! manifest would only surface on a user's machine. These tests catch the
//! usual mistakes (bad JSON, missing/short hash, URL without a hash, no
//! bin), and that every catalog entry using the winsploit bucket points at a
//! manifest that exists.

use std::collections::BTreeMap;
use std::path::Path;

use serde_yaml::Value;

fn manifests() -> BTreeMap<String, Value> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("bucket");
    let mut out = BTreeMap::new();
    for entry in std::fs::read_dir(&dir).expect("bucket/ exists") {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        // JSON is valid YAML, so serde_yaml parses it without a new dependency.
        let v: Value = serde_yaml::from_str(&text)
            .unwrap_or_else(|e| panic!("{}: invalid JSON: {e}", path.display()));
        let name = path.file_stem().unwrap().to_string_lossy().to_string();
        out.insert(name, v);
    }
    out
}

fn as_list(v: &Value) -> Vec<String> {
    match v {
        Value::String(s) => vec![s.clone()],
        Value::Sequence(items) => items
            .iter()
            .filter_map(|i| i.as_str().map(str::to_string))
            .collect(),
        _ => vec![],
    }
}

fn is_sha256(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// (url, hash) pairs at the top level and per architecture.
fn downloads(m: &Value) -> Vec<(Vec<String>, Vec<String>)> {
    let mut out = Vec::new();
    let mut take = |node: &Value| {
        if let Some(url) = node.get("url") {
            let hash = node.get("hash").map(as_list).unwrap_or_default();
            out.push((as_list(url), hash));
        }
    };
    take(m);
    if let Some(Value::Mapping(arch)) = m.get("architecture") {
        for (_, node) in arch {
            take(node);
        }
    }
    out
}

#[test]
fn manifests_are_well_formed() {
    let all = manifests();
    assert!(!all.is_empty(), "no manifests in bucket/");
    for (name, m) in &all {
        assert!(
            m.get("version").and_then(Value::as_str).is_some(),
            "{name}: no version"
        );
        assert!(m.get("homepage").is_some(), "{name}: no homepage");
        assert!(m.get("license").is_some(), "{name}: no license");
        let dataset = m
            .get("##")
            .and_then(Value::as_str)
            .is_some_and(|c| c.starts_with("dataset"));
        assert!(
            dataset || m.get("bin").is_some(),
            "{name}: no bin (mark data-only manifests with \"##\": \"dataset: ...\")"
        );
        let dl = downloads(m);
        assert!(!dl.is_empty(), "{name}: no url");
        for (urls, hashes) in dl {
            assert_eq!(urls.len(), hashes.len(), "{name}: every url needs a hash");
            for u in &urls {
                assert!(u.starts_with("https://"), "{name}: non-https url {u}");
            }
            for h in &hashes {
                assert!(is_sha256(h), "{name}: bad sha256 {h:?}");
            }
        }
    }
}

#[test]
fn catalog_entries_in_the_winsploit_bucket_have_manifests() {
    let cat = crate::registry::parse(include_str!("../tools.yaml")).unwrap();
    let all = manifests();
    for t in cat
        .tools
        .iter()
        .filter(|t| t.bucket.as_deref() == Some("winsploit"))
    {
        let pkg = t.scoop.as_deref().unwrap();
        assert!(all.contains_key(pkg), "{}: no bucket/{pkg}.json", t.name);
    }
    assert!(
        cat.buckets.contains_key("winsploit"),
        "tools.yaml `buckets:` needs a URL for the winsploit bucket"
    );
}
