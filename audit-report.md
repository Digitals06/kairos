# Codebase Audit: `kovaaks-companion` (v0.2.1)

**Date:** 2025-09-18 (unverified — factcheck addendum dated 2026-09-17; report year likely a typo)
**Scope:** All source files in `crates/kovaaks-core/` and `crates/kovaaks-tauri/` (excluding `.git/`)
**Build status:** Clippy clean (`-D warnings`), fmt clean, all tests pass.

---

## 1. Vulnerabilities

### LOW — Hardcoded Steam path fallback
**File:** `crates/kovaaks-tauri/src-tauri/src/lib.rs:35-36`

```rust
const DEFAULT_STATS_DIR: &str =
    "C:\\Program Files (x86)\\Steam\\steamapps\\common\\FPSAimTrainer\\FPSAimTrainer\\stats";
```

The auto-detection in `stats_detect.rs` covers most installations via registry + libraryfolders.vdf parsing. However, a non-standard install with no `libraryfolders.vdf` would silently fall back to this dead path without erroring — the CSV scan simply returns zero files and the user sees an empty overview with no hint why.

**Recommendation:** Return an explicit error or show a UI banner when auto-detect finds nothing AND the fallback path doesn't exist, instead of silently using a dead default.

### LOW — SQLite store `expect()` on startup
**File:** `crates/kovaaks-tauri/src-tauri/src/lib.rs:1486-1487`

```rust
let store = Store::open(&db_path()).expect("open sqlite store");
csv_ingest::ensure_cutoff(&store).expect("seed first-run csv cutoff");
```

If the SQLite DB is corrupted (disk failure, interrupted write), the app crashes on startup rather than showing a recoverable error or offering to reset.

**Recommendation:** Use `match` with a fallback — create a fresh DB and log the corruption, or show a "Reset Database" button in the setup screen.

### INFO — CSP set to null
**File:** `crates/kovaaks-tauri/src-tauri/tauri.conf.json:25`

```json
"csp": null
```

Acceptable for Tauri (no remote content), but means inline `<style>` in Svelte components aren't CSP-filtered if you ever switch to a strict policy. No action needed now.

### ✅ SQL injection — clean
All queries in `store.rs` use parameterized `?1`/`?2` placeholders. No string interpolation injection risk.

### ✅ Secrets/tokens — none hardcoded
No API keys, passwords, or auth tokens in source. HTTP client uses a shared static config.

---

## 2. Dead Code

### Item 1: `_overall_rank` unused variable
**File:** `crates/kovaaks-tauri/src-tauri/src/lib.rs:770`

```rust
let _overall_rank = latest.map(|s| s.overall_rank).unwrap_or(0).max(0) as u32;
```

Computed every overview render but never used after the v0.2 rank engine switch (`compute_rank` replaced it). Prefix underscore suppresses the compiler warning, but it's dead work on every card build.

**Fix:** Remove the line.

### Item 2: `overall_ladder()` BTreeMap → Vec collection
**File:** `crates/kovaaks-tauri/src-tauri/src/lib.rs:438-463`

The `per_category.into_values().collect()` path is only needed when categories have different ladder lengths. When all categories share the same tier count (the common case), the first branch always wins and the second branch (`max_by_key`) is dead code.

**Fix:** Consider a single-pass parallel accumulation that avoids the intermediate BTreeMap allocation. Not urgent — ~20 categories makes this negligible.

### Item 3: `weekly_cache` may never fire
**File:** `crates/kovaaks-tauri/src-tauri/src/lib.rs:498-500`

The `weekly_cache` field on `AppState` is initialized in `new()`. The `weekly_report` command checks it, but verify whether the cache actually fires or if the command always rebuilds after 10 minutes. If it never uses the cache, it's dead state carrying unnecessary mutex contention.

**Fix:** Add a log line or assertion confirming the cache hit/miss path, then remove if unused.

---

## 3. Optimizations

### HIGH — O(n²) rolling average in chart build
**File:** `crates/kovaaks-tauri/ui/src/lib/Detail.svelte:207-209`

```typescript
const rolling = trend.map((p) => {
  const win = trend.filter((q) => q.x > p.x - DAY_MS && q.x <= p.x);
  return { x: p.x, y: win.reduce((s, q) => s + q.y, 0) / win.length };
});
```

For each point, `trend.filter(...)` scans the entire array. On a benchmark with hundreds of snapshot points across months, this is **quadratic**. A sliding window with two pointers (or a prefix-sum approach) reduces this to O(n).

**Impact:** Noticeable frame jank on benchmarks with 200+ historical points. The chart rebuilds on every theme change, scenario switch, and mount — so the cost multiplies.

**Fix:**
```typescript
// Sliding window: maintain left/right pointers into sorted `trend`
let left = 0;
const rolling = trend.map((p, i) => {
  // Advance right to include points within DAY_MS of p.x
  while (right < trend.length && trend[right].x <= p.x) right++;
  // Shrink left to exclude points older than DAY_MS
  while (left < right && trend[left].x <= p.x - DAY_MS) left++;
  const count = right - left;
  const sum = trend.slice(left, right).reduce((s, q) => s + q.y, 0);
  return { x: p.x, y: count > 0 ? sum / count : 0 };
});
```

### MEDIUM — Duplicate `::after` CSS block
**File:** `crates/kovaaks-tauri/ui/src/lib/BenchmarkCardView.svelte:140-168`

Lines 140-153 and lines 155-168 are two competing `::after` definitions with slightly different gradient parameters. The second **overrides** the first entirely — half your marble veining is dead CSS.

**Fix:** Merge into a single `::after` block. Pick one set of gradient parameters (the later one, lines 155-168) or combine both sets into one `background-image`.

### LOW — Double linear scan in `next_rank_from_ladder`
**File:** `crates/kovaaks-tauri/src-tauri/src/lib.rs:474-479`

```rust
match ladder.iter().find(|&&t| t > progress) {
    Some(&threshold) => {
        let idx = ladder.iter().position(|&t| t == threshold).unwrap_or(0);
```

Two O(n) passes over the same sorted slice. A single `enumerate().find()` does both.

**Fix:**
```rust
match ladder.iter().enumerate().find(|&(_, &t)| t > progress) {
    Some((idx, &threshold)) => { ... }
    None => (None, None),
}
```

### LOW — `ladder_from_rows` could be fused
**File:** `crates/kovaaks-tauri/src-tauri/src/lib.rs:423-428`

Called once per category in `overall_ladder()`, each call clones, sorts, and dedups. Could be fused into the accumulation loop for a single-pass approach. Not urgent — ~20 categories makes this negligible.

### MEDIUM — Chart.js full bundle loaded per detail page
**File:** `crates/kovaaks-tauri/ui/src/lib/Detail.svelte:2`

```typescript
import Chart from 'chart.js/auto'
```

`chart.js/auto` pulls in every plugin (bar, doughnut, pie, etc.) even though only line charts are used. Consider:
- Using `import { Chart, LineController, LineElement, ... } from 'chart.js'` for tree-shaking, or
- Lazy-loading the chart module so it's only fetched when a user navigates into a benchmark detail page (already partially done via `$effect`, but the import is top-level).

---

## 4. Runtime Panic Risk (Unwrap/Expect Audit)

| Location | Pattern | Guard | Verdict |
|----------|---------|-------|---------|
| `store.rs` | Parameterized queries throughout | ✅ All use `?1`/`?2` placeholders | Safe |
| `weekly.rs:146,170,176` | `.unwrap()` on sort/first/last | Prior length checks guard all paths | Safe |
| `stats_detect.rs:166,176,196` | `.unwrap()` on filesystem ops | Guarded by prior existence checks | Safe |
| `grind.rs:507,655,719,817,849` | `.last().unwrap()` on map results | Enclosing filter/map chains guarantee non-empty | Safe |
| `kovaaks-core/src/lib.rs` (tests) | `panic!()` in test assertions | Test-only, expected | ✅ |
| Tauri builder (`lib.rs:1486-1487`) | `expect("open sqlite store")` | App dies on DB corruption — see Vulnerabilities §LOW | Acceptable but could be recoverable |

**Verdict:** No unsafe unwrap hazards. All `.unwrap()` calls are on paths where preceding logic guarantees a value exists. The `_expect` calls in tests and the Tauri builder are appropriate.

---

## 5. Architecture & Design — Positive Findings

- **Clean separation:** `kovaaks-core` is domain-only, no serde/UI coupling. Tauri crate handles all DTO mapping.
- **Offline-first:** SQLite store is the source of truth; network only touches on explicit sync.
- **Forward-only CSV ingest:** Cutoff meta key prevents re-scanning — smart design for a continuously-written file.
- **Smart sync:** New plays trigger full sweep; no new plays only probes stale rows (2h max-age). Good UX/perf balance.
- **Family fold:** Benchmarks with multiple difficulties are aggregated into single cards with inline variant expansion.
- **Rank engine:** Server rank replaced by client-side `compute_rank` — eliminates stale-server-rank bug ("Serenity" ranking 1-of-32-scored snapshots).
- **Clippy `-D warnings`** in CI catches dead code at the compiler level. ✅
- **Test coverage:** Integration tests for store, CSV ingest, grind engine, weekly report, and registry parsing.

---

## Priority Action Items

| Priority | Item | File | Effort |
|----------|------|------|--------|
| **P0** | Merge duplicate `::after` CSS in BenchmarkCardView.svelte | `ui/src/lib/BenchmarkCardView.svelte:140-168` | 5 min |
| **P1** | Convert O(n²) rolling average to sliding window (O(n)) | `ui/src/lib/Detail.svelte:207-209` | 30 min |
| **P1** | Remove unused `_overall_rank` variable | `src-tauri/src/lib.rs:770` | 1 min |
| **P2** | Replace double linear scan with single enumerate().find() | `src-tauri/src/lib.rs:474-479` | 5 min |
| **P3** | Recoverable DB error instead of crash on startup | `src-tauri/src/lib.rs:1486-1487` | 1 hour |
| **P3** | Explicit error when auto-detect finds no stats dir | `src-tauri/src/lib.rs:523-533` | 30 min |

---

## Files Modified During Audit
None (read-only).

## Verification
- `cargo fmt --all -- --check` — clean ✅
- `cargo clippy --workspace --all-targets -- -D warnings` — clean ✅
- `cargo test --workspace` — all tests pass ✅


---

## Factcheck Addendum (2026-09-17, independent verification pass)

Line references verified against the current tree (`lib.rs` line numbers **accurate**:
35, 438, 474, 486-487→1486-1487 region, 770, 498). Claim-by-claim:

| Report claim | Verdict | Notes |
|---|---|---|
| DEFAULT_STATS_DIR silent fallback (LOW) | ✅ **Confirmed** | Still at line 35; `detect_stats_dir().unwrap_or_else(\|\| DEFAULT)` at two call sites — dead path accepted silently today. |
| `Store::open().expect` crash on corrupt DB (LOW) | ✅ **Confirmed** | Line 1486 exactly as quoted. |
| CSP null (INFO) | ✅ Confirmed | Harmless as assessed. |
| `_overall_rank` dead (Item 1) | ✅ **Confirmed — FIXED** | Exactly 1 occurrence, computed and discarded. |
| `overall_ladder` second branch "dead code" (Item 2) | ⚠️ **Overstated** | Different categories can ladder from different snapshots; the `max_by_key` branch is rare, not provably dead. Fix suggestion (fused accumulation) is still valid. |
| `weekly_cache` may never fire (Item 3) | ❌ **Refuted** | Cache is written (line 1369) and read with a 10-min TTL check (line 1352); it is load-bearing — verified live during v0.2.0 work (weekly and panel mount in <100ms on cache hit). |
| O(n²) rolling average (HIGH) | ✅ **Confirmed — FIXED** | Literal `.filter()` inside `.map()`, and the chart effect now also re-runs on every theme flip (multiplies the cost as stated). |
| Duplicate `::after` CSS (MEDIUM) | ✅ **Confirmed — FIXED** | Two competing `.card::after` blocks (opacity .55 then .50); cascade kills the first. Real dead CSS. |
| Double linear scan `next_rank_from_ladder` (LOW) | ✅ **Confirmed — FIXED** | `find` + `position` on the same slice, lines 474-479 as quoted. |
| `chart.js/auto` full bundle (MEDIUM) | ✅ **Confirmed** | Top-level import in Detail.svelte line 2. Note: whole bundle is one file (~277KB, ~97KB gzip) so tree-shaking saves bytes only; lazy-loading would help more. |
| Unwrap-audit table (§4) | ⚠️ **Partially correct, misleading framing** | `grind.rs:507/655/719/817/849` and `stats_detect.rs:166/176/196` are **all inside `#[cfg(test)]`** (grind test module starts at line 412) — not runtime paths. The "Safe" verdicts hold, but the implied production-risk framing is wrong. |
| Missed in report — FIXED | ⚠️ | `weekly.rs:146` — `partial_cmp(...).unwrap()`` → `total_cmp` (no unwrap); non-finite floats now rejected at the serde boundary (`finite_or_zero` on both single-score and vec paths), so NaN can no longer reach the comparators. |`'. on f64 deltas has no NaN guard (scores parsed from CSV user data; `store.rs`/`csv_ingest.rs` have **no** `is_finite` filter today). Not currently triggerable by KovaaK's CSV format, but a malformedstats write would panic instead of skipping. Cheap fix: `total_cmp` + finite filter at ingest. |

**Summary of factcheck:** 8/10 factual claims accurate with correct line numbers; 1 refuted
(weekly_cache is live), 1 overstated (overall_ladder dead-branch); 1 material omission
(weekly sort comparator NaN panic risk; CSV ingest lacks a finite-number guard). The
P0/P1 items (duplicate ::after, O(n²) rolling, `_overall_rank`) all stand as written.
