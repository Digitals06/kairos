# Plan: v0.1.5 — consolidate the same-run rule (roadmap item 1)

## Current state (verified by reading the code)

The same-run rule (round-to-integer match) exists in **4 places**:

| # | Location | Role |
|---|----------|------|
| A | `metrics.rs::merge_plays_snapshots_dedup` | stat-card metrics (`metrics_for_scenario_combined`) |
| B | `metrics.rs::series_for_scenario` | chart series built backend-side (echo-drop of snapshots) |
| C | `Detail.svelte` (snapPts filter) | client-side re-dedupe of the chart series |
| D | `local_points` (round) / `snapshot_points` (as i64) | storage format of the two point kinds |

A and B use `is_same_run` (the single primitive — good). C re-implements the
rule in TS (`Math.round(p.y) === Math.round(s.y)`). The chart's `trend` is
assembled client-side from `raw` (series points) + `playPts` (d.plays), i.e.
the backend does NOT emit the final merged series — the frontend recomputes
the merge, with its own copy of the rule.

## Target design

**`build_scenario_history` becomes the single owner.** It emits the FINAL
merged run history per scenario:

- `ScenarioSeries.points` = local plays (verbatim float scores, exact times) +
  snapshot new-highs that are not same-run echoes of any play — merged
  chronologically. This is exactly `merge_plays_snapshots_dedup`'s output, so
  **A and B collapse into one call path**.
- Per-point provenance so the frontend can draw magenta play-dots without
  re-deriving: `ScenarioSeries` gains `play_count: usize` — the number of
  leading points that came from plays (plays sort before their echo? no—) …

**Revised (simpler, wire-compatible):** keep `points: Vec<(t, i64-rounded)>`
BUT round plays to i64 inside the merge (KovaaK's echoes ARE rounded ints; the
chart already plots ints). Add `contains_plays: bool` to `ScenarioSeries` +
the wire DTO. Magenta-dot rule becomes: dots = series points at timestamps
matching a play (exact timestamp match from `d.plays`, which the DTO already
carries verbatim float scores + times) — pure presentational join by
timestamp, no score rule involved.

**Frontend (Detail.svelte):** delete the snapPts dedupe + trend merge.
`trend` = `series.points` verbatim. Dots = `d.plays` filtered by exact
timestamp ∈ trend. Stat cards stay on `metrics_for_scenario_combined`
(backend), which now uses the same single merge fn.

## Rule instances after consolidation

1. `is_same_run` — the primitive (only place the comparison exists)
2. `merge_plays_snapshots_dedup` — the only merge (used by BOTH chart series
   and stat-card metrics)
3. `local_points`/`snapshot_points` — collapse into the merge call
4. Frontend — zero copies of the rule (timestamp-join only)

## Verification (never assume)

1. Unit: existing `merge_dedup.rs` (6 tests) + `scenario_history_local.rs`
   (8 tests) must pass — update expectations where the series semantics
   changed (points now include plays in snapshot-backed series).
2. New unit: snapshot-backed series WITH non-echo plays contains both kinds;
   `contains_plays` flag correct in all three cases.
3. CDP live: Avasive S2 Easier → JennClick chart unchanged (3 plays visible
   as dots + cyan line), Pasu (snapshot+play mixed) unchanged, BounceClick /
   AbyssClick (local-only) unchanged — pixel-compare against pre-change
   screenshots.
4. Full gate: fmt, clippy -D warnings, all tests, npm build.

## Out of scope

- `metrics_for_benchmark` (card-level avg/high) — different data path,
  not part of the same-run rule.
- Historical backfill — forward-only data rule untouched.
