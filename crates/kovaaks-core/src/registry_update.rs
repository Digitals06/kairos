//! Registry updater: fetch evxl's live benchmark registry at runtime so new
//! benchmark seasons appear without a rebuild.
//!
//! evxl is a SvelteKit app; the registry ships as a JSON array inside a
//! `JSON.parse(\`[…]\`)` template literal in one of the `_app/immutable/chunks/`
//! bundles. Discovery: fetch the homepage, walk the referenced chunks (bounded),
//! and extract the array. The result is validated (≥100 benchmarks with the
//! expected keys) and persisted next to the app DB; on the next launch
//! [`crate::registry::Registry::all`] prefers a valid override over the
//! embedded snapshot (compiled `fetched` date stays as fallback provenance).

use crate::http::USER_AGENT;

/// Evxl homepage the chunk walk starts from.
const EVXL_BASE: &str = "https://evxl.app";
/// Cap on chunk fetches per update check (the walk found ~44 in recon).
const MAX_CHUNKS: usize = 60;
/// A real registry has 100+ benchmarks; anything less is a parse accident.
const MIN_BENCHMARKS: usize = 100;

/// Fetch the live registry from evxl and return it as pretty JSON text.
///
/// Fails soft (Err) on any network/parse hiccup — the caller keeps the
/// previously persisted override (or the embedded registry).
pub async fn fetch_live_registry() -> Result<String, crate::Error> {
    let client = crate::http::shared_client();
    let html = client
        .get(EVXL_BASE)
        .header("User-Agent", USER_AGENT)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;

    // Collect chunk paths referenced by the page.
    let mut chunks: Vec<String> = html
        .split("_app/immutable/")
        .skip(1)
        .filter_map(|seg| {
            let end = seg.find(".js")?;
            let rel = &seg[..end + 3];
            let rel = rel.rsplit('/').next().unwrap_or(rel);
            Some(format!("chunks/{rel}"))
        })
        .collect();
    chunks.sort();
    chunks.dedup();
    chunks.truncate(MAX_CHUNKS);

    // Walk the chunks (plus their relative imports) until the registry shows up.
    let base = format!("{EVXL_BASE}/_app/immutable/");
    let mut queue: Vec<String> = chunks;
    let mut seen = std::collections::HashSet::new();
    while let Some(rel) = queue.pop() {
        if !seen.insert(rel.clone()) || seen.len() > MAX_CHUNKS {
            continue;
        }
        let body = match client
            .get(format!("{base}{rel}"))
            .header("User-Agent", USER_AGENT)
            .send()
            .await
        {
            Ok(r) => match r.error_for_status() {
                Ok(r) => r.text().await.unwrap_or_default(),
                Err(_) => continue,
            },
            Err(_) => continue,
        };
        if let Some(json) = extract_registry_json(&body) {
            return json;
        }
        // Follow relative imports one hop (the registry may sit behind the
        // entry chunk that references it).
        for dep in relative_imports(&body) {
            if !seen.contains(&format!("chunks/{dep}")) {
                queue.push(format!("chunks/{dep}"));
            }
        }
    }
    Err(crate::Error::Decode("evxl registry chunk not found".into()))
}

/// Extract the benchmark registry from a JS chunk: the first
/// `JSON.parse(\`[{…benchmarkName…}]\`)` template literal that parses and
/// validates as a benchmark array.
fn extract_registry_json(js: &str) -> Option<Result<String, crate::Error>> {
    let needle = "JSON.parse(`";
    let mut from = 0usize;
    while let Some(rel) = js[from..].find(needle) {
        let start = from + rel + needle.len();
        let end_rel = js[start..].find("`)")?;
        let raw = &js[start..start + end_rel];
        from = start + end_rel + 2;
        // Only consider literals that look like the benchmark registry.
        if !raw.contains("benchmarkName") {
            continue;
        }
        let raw = raw.trim_start();
        if !raw.starts_with('[') {
            continue;
        }
        // The template literal is raw JSON (SvelteKit escapes only for its own
        // serialization); unescape backtick/${} escapes defensively.
        let unescaped = raw.replace("\\`", "`").replace("\\${", "${");
        return Some(
            match serde_json::from_str::<serde_json::Value>(&unescaped) {
                // Validate structurally, then persist the RAW text: re-serializing
                // through serde_json::Value would sort object keys alphabetically
                // and DESTROY the rankColors ladder order (worst→best insertion
                // order is the only ordering the wire carries).
                Ok(v) => validate_registry(&v).map(|_| unescaped),
                Err(_) => continue,
            },
        );
    }
    None
}

/// Validate a parsed registry value (structure + size only; ordering lives in
/// the raw text we persist).
fn validate_registry(v: &serde_json::Value) -> Result<(), crate::Error> {
    let arr = v
        .as_array()
        .ok_or_else(|| crate::Error::Decode("registry override is not an array".into()))?;
    if arr.len() < MIN_BENCHMARKS {
        return Err(crate::Error::Decode(format!(
            "registry override too small: {} benchmarks",
            arr.len()
        )));
    }
    let ok = arr.iter().all(|b| {
        b.get("benchmarkName").is_some()
            && b.get("difficulties").and_then(|d| d.as_array()).is_some()
    });
    if !ok {
        return Err(crate::Error::Decode(
            "registry override entries missing benchmarkName/difficulties".into(),
        ));
    }
    Ok(())
}

/// Relative `./x.js` imports inside a chunk body.
fn relative_imports(js: &str) -> Vec<String> {
    js.split("from\"./")
        .skip(1)
        .filter_map(|seg| {
            let end = seg.find('"')?;
            Some(seg[..end].to_string())
        })
        .collect()
}

/// Read the persisted override (raw JSON text) if present and non-empty.
pub fn load_override() -> Option<String> {
    let path = crate::registry::Registry::override_path()?.clone();
    let text = std::fs::read_to_string(path).ok()?;
    if text.len() < 10_000 {
        return None; // truncated/garbage — fall back to embedded
    }
    // Cheap sanity: it must still be the benchmark array.
    if !text.contains("benchmarkName") {
        return None;
    }
    // Reject artifacts of the v0.1.5 re-serialization bug (serde_json::Value
    // sorts object keys alphabetically, destroying the rankColors ladder
    // order). Those files were pretty-printed; the wire text is minified.
    // A rejected override falls back to the embedded registry and the next
    // launch re-fetches with the fixed extractor.
    if text.contains("\n  ") || text.contains("\n    ") {
        return None;
    }
    Some(text)
}

/// Persist a fetched override (pretty JSON) to `path` atomically.
pub fn persist_override(path: &std::path::Path, json: &str) -> Result<(), crate::Error> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| crate::Error::Decode(format!("mkdir failed: {e}")))?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(|e| crate::Error::Decode(format!("write failed: {e}")))?;
    std::fs::rename(&tmp, path).map_err(|e| crate::Error::Decode(format!("rename failed: {e}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_parses_realistic_chunk() {
        let js = r#"const o=JSON.parse(`[{"benchmarkName":"A","rankCalculation":"basic","difficulties":[]},{"benchmarkName":"B","difficulties":[]}]`);export{o};"#;
        // Too small for the validator, but extraction must produce an Err (not None)
        // — proves the literal was found and parsed.
        let r = extract_registry_json(js).expect("literal found");
        assert!(r.is_err(), "below MIN_BENCHMARKS must Err");
    }

    #[test]
    fn extract_skips_non_registry_literals() {
        let js = r#"const o=JSON.parse(`[{"other":1}]`);const p=JSON.parse(`[`)"#;
        assert!(extract_registry_json(js).is_none());
    }

    #[test]
    fn validate_rejects_small_and_malformed() {
        let small = serde_json::json!([]);
        assert!(validate_registry(&small).is_err());
        let bad = serde_json::Value::Array(vec![serde_json::json!({"nope": 1}); 150]);
        assert!(validate_registry(&bad).is_err());
    }

    #[test]
    fn validate_accepts_realistic_registry() {
        let mut arr = Vec::new();
        for i in 0..MIN_BENCHMARKS + 1 {
            arr.push(serde_json::json!({
                "benchmarkName": format!("B{i}"),
                "difficulties": [{"difficultyName": "All", "kovaaksBenchmarkId": i}]
            }));
        }
        let v = serde_json::json!(arr);
        validate_registry(&v).expect("valid");
    }

    #[test]
    fn extraction_preserves_wire_key_order() {
        // rankColors carries the ladder order in insertion order (worst→best).
        // The persisted text must NOT be re-serialized through serde_json::Value
        // (BTreeMap) — that would alphabetize the ladder and destroy ranks.
        // Build a registry big enough to pass validation (150 entries with the
        // ordered ladder on the first).
        let mut items: Vec<String> = Vec::new();
        items.push("{\"benchmarkName\":\"X\",\"rankColors\":{\"Bronze\":\"#1\",\"Silver\":\"#2\",\"Gold\":\"#3\"},\"difficulties\":[]}".to_string());
        for i in 1..151 {
            items.push(format!(
                "{{\"benchmarkName\":\"B{i}\",\"difficulties\":[]}}"
            ));
        }
        let big = format!("const o=JSON.parse(`[{}]`);", items.join(","));
        let out = extract_registry_json(&big).expect("found").expect("valid");
        // Wire order preserved: Bronze appears BEFORE Silver BEFORE Gold, and
        // NOT alphabetical (Bronze, Gold, Silver).
        let bronze = out.find("\"Bronze\"").expect("bronze");
        let silver = out.find("\"Silver\"").expect("silver");
        let gold = out.find("\"Gold\"").expect("gold");
        assert!(
            bronze < silver && silver < gold,
            "ladder order preserved, got: {out}"
        );
    }

    #[test]
    fn relative_imports_extracts_deps() {
        let js = r#"import{a}from"./X.js";import{b}from"./Y.js""#;
        assert_eq!(relative_imports(js), vec!["X.js", "Y.js"]);
    }

    #[test]
    fn persist_then_load_roundtrip() {
        let dir = std::env::temp_dir().join(format!(
            "kairos-regupd-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        let path = dir.join("registry-override.json");
        let body = "x".repeat(12_000) + "benchmarkName";
        persist_override(&path, &body).expect("persist");
        // The persisted file is readable and passes the loader's sanity gate.
        // (load_override() reads through the process-global path, which
        // start-up installs once — not re-configurable per test.)
        let text = std::fs::read_to_string(&path).expect("read back");
        assert_eq!(text, body);
        assert!(text.len() >= 10_000 && text.contains("benchmarkName"));
        std::fs::remove_dir_all(&dir).ok();
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;

    /// Live probe: fetch evxl's real registry through the chunk walk.
    /// Skipped by default (network); run with --ignored.
    #[tokio::test]
    #[ignore]
    async fn live_fetch_parses_evxl_registry() {
        let json = fetch_live_registry().await.expect("live fetch");
        let v: serde_json::Value = serde_json::from_str(&json).expect("parse");
        let n = v.as_array().expect("array").len();
        println!("live registry: {n} benchmarks");
        assert!(n >= MIN_BENCHMARKS);
    }
}
