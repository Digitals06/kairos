# Roadmap

Kairos is developed in the open. This roadmap lists what we actually intend
to build, grouped by milestone. No dates — features ship when they're done
and verified. Items may be re-ordered within a milestone.

Audience stance: public but small. Docs, CI, and releases are maintained as
if strangers will arrive; features stay focused on a single connected
player. No onboarding flows, no multi-user support, no telemetry — ever.

## v0.1.4

- [x] CI: fmt + clippy (`-D warnings`) + test gate on every push; Windows
      release build attached to GitHub releases on tags
- [x] Rank-up/down toasts after a sync, computed by the local rank engine
      (never the API's stored rank), with a summary toast when several
      benchmarks change at once
- [x] Live CSV watcher: local plays appear in charts while the app is open,
      no refresh click. Strictly local — ranks still change on Sync Now
- [x] Typed `Source` enums replacing stringly-typed provenance fields

## v0.1.5 — released

- [x] Consolidate the same-run rule: `build_scenario_history` is the single
  owner of the merged-series logic; the frontend's client-side dedupe is
  removed (backend series is authoritative, covered by tests + screenshot
  verification)
- [x] Smart sync: only re-probe benchmarks whose local CSVs changed since the
  last sync (mtime-based) — fewer rate-limited sweeps
- [x] Registry updater: fetch evxl's public registry in-app with a
  version-stamped cache, so new benchmark seasons don't wait on a rebuild
- [x] Auto-sync on launch + a "last synced X ago" indicator
- [x] "What to grind next": design pass complete (probe-based inversion,
  see `.hermes/plans/2026-09-07_v015_grind-next-design.md`); implementation
  follows in v0.1.6

## v0.1.6 — released

- [x] Benchmark-type filter on the overview using evxl's own 12-tab taxonomy
  (Mixed, Tracking, Static, Clicking, Ground, Precise, Smooth, Micro, Dynamic,
  Reactive, Evasive, Switching) — curated tag assignments match evxl's live
  filter exactly; multi-select, combined with the name search
- [x] "What to grind next" panel per the v0.1.5 design pass

## v0.1.7

- [x] Grind-next reachability: targets are capped at each scenario's ladder
  top (the best score a real player can set), so the panel never suggests a
  game-impossible number; scenarios already at their ladder top are skipped,
  and no-single-scenario-path states are reported honestly
- [x] Combined step-by-step plans for floor/harmonic benchmarks: when no
  single scenario can flip the tier, the panel produces numbered rung-by-rung
  steps (each within its ladder) that reach the next rank together

## v0.1.8 — released

- [x] 100% client-side rank coverage: the last API-rank fallback is closed;
      every benchmark computes its rank from local data
- [x] One-click full backup export (JSON, Documents/kairos-export-<ts>.json)

## v0.2

- Benchmark dashboard: all-benchmarks grid with rank chips and
  progress-to-next-rank bars; scenario deep pages with PB progression and
  plateau detection
- Weekly report: time trained, improvements, rank changes, trend arrows
- Windows installer (NSIS via Tauri bundler)
- Close the last benchmark families that still fall back to the API rank
  (100% client-side rank coverage)
- History export (JSON/CSV backup)
- Streaks / XP meta-progression from consistency and milestones

## Later (unpromised)

- Tray mode; auto-launch when KovaaK's starts
- Reconcile view: API vs local CSV discrepancy report
