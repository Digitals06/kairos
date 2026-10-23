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

- [x] Consistency & plateau detection: grind targets carry spread (coefficient of variation over the
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

## v0.2.1 — released

- [x] Grind chips on overview family cards — per-difficulty "▶ N runs to next
  tier", "plateaued", or "complete" badges inline in the unrolled rows
- [x] Time-trained estimate in the weekly strip — scored-min chip (sum of
  hit_count / avg_fps per local play; never guessed)
- [x] Scenario sort control on benchmark deep pages: weakest → strongest
  ordering (by rank tier, then score gap to next tier)

## v0.3.0 — the Kairos identity

- [x] Design pass first: signature typography, Kairos-branded rank/tier color
  system, motion language, and a legacy-free component sheet applied across
  every screen (overview, deep pages, weekly strip, setup, menus) — features
  after this build ON the final identity instead of being rewired to it later
- [x] Scenario analytics view on the new components: ALL scenarios of the
  current benchmark on one screen at once (grid of per-scenario mini-charts
  with PB progression, spread, plateau, improvement %) — built for
  screenshot-ability
- [x] Capture button: composes that all-scenarios report into one image and
  copies it to the clipboard (render path shared with the later Discord
  share card)
- [x] Grind session coach: while KovaaK's runs, the app surfaces the next 3
  scenarios to grind for the selected family (no game overlay, app polling
  only) — small, lands so the identity is proven on a newer screen too

## v0.3.1 — bug & UI fixes

Triage pass over v0.3.0: user-reported bugs and UI polish from real
usage. Scope is open — fix what surfaces rather than planning ahead.

## v0.3.2 — per-method grind plans

- [x] Per-method grind planner: count-qualified engines (dojo, one-per-subcat
  REVENGE) plan only the scenarios that still need to qualify — and repair the
  tier floor first; harmonic/energy, avg, and floor families all keep the
  spread plan (cheapest rung across scenarios, maxing deferred)
- [x] Grind "GO" button: launch the suggested scenario in KovaaK's straight
  from the plan row (steam deep-link, official scheme)
- [x] Frameless window: titlebar dragging permission restored (was blocked by
  a missing window capability — resizing worked, dragging didn't)
- [x] At-ladder-top benchmarks no longer advertise "Tier -> Tier" as next
- [x] New app icon: chiseled temple reticle in marble and torchlight gold,
  retired the old neon one
- [x] Grind panel spacing matches the other panels; chart legend readable on
  dark themes; select popups follow the marble/basalt theme

## v0.4.0 — play-session workflows

- [x] Tray mode: closing the window keeps Kairos running in the tray (the
  CSV watcher stays live); tray menu with Show / Launch KovaaK's / Quit and
  settings toggles for both
- [x] Post-session summary toast: a >20 min play gap closes the session; the
  toast reports plays and PBs (with the best gain and scenario) from local
  CSV data only
- [x] Reconcile chip: the detail header surfaces local plays newer than the
  last leaderboard snapshot ("N unranked plays"), hidden when current
- [x] Playlists & launch: scenario GO buttons (grind plan rows) + a
  "Launch in KovaaK's" button per benchmark detail — both use the official
  steam deep links (jump-to-scenario / jump-to-playlist, app id 824270)
- [x] Weak-spot footer strip in detail: sticky bar with the next scenario,
  its gap and a GO button
- [x] Keyboard navigation in the stoa: ↓/↑ walk the card order, Enter
  expands/opens (same semantics as clicking), Esc backs out of detail/analytics
- [x] Favorites inline tick: the star flips in place; the reorder settles
  after a short delay or on clicking the "Order updated" chip

Complex widgets (v0.4.0+): prefer zero-dep Svelte 5 libs for stateful widgets;
mukade-ui is token-driven and Tailwind-free, remap its tokens to marble/basalt.
Avoid Tailwind-coupled kits — they re-import the grey-box problem. open-props
easing/shadow tokens stay cherry-picked by hand.

## v0.5.0 — custom benches & community

- [x] Custom benches: authored in-app (scenario picker over your own history),
  grouped into subcategories, author-supplied thresholds and tier names, three
  rank rules (full subcategory, top N per subcategory, global energy) scored by
  the shared engine; JSON import/export for sharing a bench as a file
- [x] Discord share card: composed 1200×630 card (weekly plaque, level bar,
  plays-per-day strip, benchmark table, top movers) — copy to clipboard or save
  as PNG
- [x] Opt-in global percentiles: the scenario's public leaderboard position
  ("#258 · top 0.7%") in the detail chart header, off by default, board sizes
  cached 24 h — the local-only rank engine is untouched

## v0.6.0 — candidates (unscoped)

Pick and order these when the next cycle starts. Nothing here is promised.

- **Scenario goals** — a personal target per scenario ("720 on Pasu by December")
  with the countdown on the weak-spot strip
- **Streak calendar** — the weekly torch strip expanded to a month heatmap
- **Auto-sync on exit** — when KovaaK's closes, run a quiet sync so ranks are
  current the next time the app opens (pairs with the session marker)
- **In-app updater** — a "new version available" chip via the GitHub releases
  API, so users stop re-downloading installers by hand
- **Taskbar jump list** — right-click the icon for Launch KovaaK's and your
  three favourite benchmarks
- **Localization skeleton** — all UI strings in one module (English + French
  first), so translations don't mean a refactor later
- **Custom-bench polish** — per-tier colour picker, bench duplication, and a
  "start from a registry benchmark" preset

## Housekeeping

- Windows installers (NSIS + MSI) ship in every release since v0.3.2 — done,
  not a roadmap item anymore.
