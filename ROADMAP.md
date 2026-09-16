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

## v0.1.9 — released

- [x] Consistency & plateau detection: grind targets carry CV (spread over the
  recent merged series) and a plateaued tag (no new PB in 3+ days)
- [x] Grind targets lead with the cheapest single-tier wins and label how many
  ladder tiers a step spans

## v0.2.0 — released

- [x] Benchmark dashboard: banking page with one section per evxl tab
  (12 categories), rank chips tier-ladder bars, click-through to detail
- [x] Weekly report: plays, days trained, PBs, streak + XP, improvements with
  trend arrows and PB tags, benchmark rank changes
- [x] History export (CSV series alongside the JSON backup)
- [x] Streaks / XP meta-progression from consistency and milestones
- [x] Rank coverage gap audit — last drifted method (`Good-Energy`) ported; no
  API-fallback families remain
- [x] Scenario deep pages with PB progression and plateau detection

## v0.2.1 — polish

- [x] Grind chips on overview family cards — per-difficulty "▶ N runs to next
  tier", "plateaued", or "complete" badges inline in the unrolled rows
- [x] Time-trained estimate in the weekly strip — scored-min chip (sum of
  hit_count / avg_fps per local play; never guessed)
- [x] Scenario sort control on benchmark deep pages: weakest → strongest
  ordering (by rank tier, then score gap to next tier)

## v0.3.0 — workflows while playing

- [ ] Tray mode: background watcher keeps the app fresh while KovaaK's runs;
  auto-launch optional
- [ ] Reconcile view: API snapshot vs local CSV discrepancy report, per-card
  mismatch chip (hidden when in sync)
- [ ] Grind session coach: while KovaaK's runs, the app surfaces the next 3
  scenarios to grind for the selected family (no game overlay, app polling
  only)
- [ ] Playlists & launch: launch a scenario in KovaaK's from the app (same
  deep-link style evxl uses), plus benchmark playlist sharecodes / playlist
  launching like scenarios

## v0.4.0 — identity & analytics

- [ ] Full UI revamp: a true unique identity for Kairos (not a generic dark
  theme) — signature typography, Kairos-branded rank/tier color system, motion
  language, and a legacy-free component sheet used across every screen
- [ ] Scenario analytics view: ALL scenarios of the current benchmark on one
  screen at once (grid of per-scenario mini-charts with PB progression, CV,
  plateau, improvement %) — built for screenshot-ability
- [ ] Capture button: composes that all-scenarios report into one image and
  copies it to the clipboard (same render path as the Discord share card)

## v0.5.0 — custom benches & community

- [ ] Local bench support: user-defined benchmarks from .json or built in-app
  (typable scenario picker backed by the KovaaK's public API), custom rank
  names + thresholds; ranks computed with the same engine
- [ ] Discord share card: composed PNG export (benchmark table, weekly strip +
  level bar)
- [ ] Opt-in global leaderboards: per-scenario global ranks/percentiles, cached
  hard, off by default — local-only rank engine untouched

## Later (unpromised)
## Later (unpromised)

- Windows installer (NSIS via Tauri bundler) — dropped from v0.2.1, revisit later
