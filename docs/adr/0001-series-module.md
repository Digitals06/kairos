# ADR-0001: The merged-score-series rule lives only in `kovaaks-core::series`

Date: 2026-09-26 · Status: accepted

## Context
Three divergent copies of the plays/snapshot dedup rule existed (metrics.rs::merge_plays_snapshots_dedup, a dead series_from_parts, weekly.rs inline). Every perf fix (weekly recap -24x, launch two-phase) had to reroute around the shallow pass-through, then reintroduce complexity inside callers.

## Decision
`series::series_map(store, steam_id, bids)` and `series::merge_parts()` are the single owners. Callers must not hand-roll merges.

## Consequences
New consumers compose at the seam in one call; the next series-shaped feature cannot re-open the wound. Per-site perf instrumentation (``[perf]`` eprintlns) stays local to lib.rs.
