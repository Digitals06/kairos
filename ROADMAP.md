# Roadmap

Kairos is developed in the open. This roadmap lists what we actually intend
to build, grouped by milestone. No dates — features ship when they're done
and verified. Items may be re-ordered within a milestone.

Audience stance: public but small. Docs, CI, and releases are maintained as
if strangers will arrive; features stay focused on a single connected
player. No onboarding flows, no multi-user support, no telemetry — ever.

## v0.1.4 — current release

- [x] CI: fmt + clippy (`-D warnings`) + test gate on every push; Windows
      release build attached to GitHub releases on tags
- [x] Rank-up/down toasts after a sync, computed by the local rank engine
      (never the API's stored rank), with a summary toast when several
      benchmarks change at once
- [x] Live CSV watcher: local plays appear in charts while the app is open,
      no refresh click. Strictly local — ranks still change on Sync Now
- [x] Typed `Source` enums replacing stringly-typed provenance fields

## v0.1.5

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
