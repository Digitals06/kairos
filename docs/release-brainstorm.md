# Drafts for v0.2.1 & v0.3.0 brainstorm

## Method used

"Jobs-to-be-Done + pre-mortem" — I'm framing each release by the job the app does
for a KovaaK's player (turn KovaaK's + evxl + community benchmarks into ONE
deciding surface) and pre-mortems the obvious ideas to refuse (more filters, more
numbers, global "leaderboard" features — every rank estimate drifts because we
shift data sources; refused).

---

## v0.2.1 — small polish release (1–2 evenings)

**Grounded pick** of these 3:

1. **Windows installer (NSIS via Tauri bundler)** — already promised, plumbing
   done for CI on `v*` tags; carries `kairos.exe` + shortcuts.
   ⏱ week  •  🔧 Tauri bundler, CI addition.
   Failure mode: NSIS avatars false-positive in Windows Defender — mitigate via
   `release.yml` jobs using `tauri build --bundles nsis` and verifying
   `codesign-free` notarization-free is noisy but testable in CI before shipping.
2. **Grind target chips on the overview** — a small corner badge on each family
   card showing `▲ 2 runs` or `plateaued` so the grind surface doesn't need the
   deep-page click. Reuses existing `grind_next` payloads already loaded in
   family cards.
   ⏱ evening  •  frontend-only.
   Failure mode: chip noise on 117 cards; snap to only multi-diff + ranked.
3. **Play-time estimates in the weekly strip** — the plays table tracks scored
   runs; a "≈ X min in KovaaK's" chip beside plays uses
   `hit_count`/`avg_fps`-derived duration or play count. Cheap and meaningful.
   ⏱ evening.
   Failure mode: only-scored-plays makes "time" wrong if CSV ingest ever gets
   hit_count gaps — the number must be labeled "scored time" and never guessed.

---

## v0.3.0 — bigger build release (week or two)

**Grounded flagship:** "Session replay timeline." The deep page chart already
draws per-scenario merged series; extend it into **PB trajectories on one canvas**
— across the whole family, every difficulty's PB history drawn side by side with
the rank-ladder lines as horizontal guides. Mechanism: reuse the
snapshot-history + pb_points already in the wire; purely a UI comp.
Failure mode: chart noise when ladders differ — ladders aren't a global scale, so
normalize to *progress % into each difficulty's ladder* instead of scores.

Other candidates:

- **Tray mode** — silently watch KovaaK's CSV dir, refresh the app when plays
  land while KovaaK's runs. Already broached as "Later".
  Failure mode: Windows tray costs a hang risk around the WebView2 process model;
  the watcher is already a background thread so tray is mostly plumbing.
- **Reconcile view** — show exactly where the API snapshot and the local CSV
  disagree (last-sync drift, missing scenarios). Mechanism: diff snapshots
  table vs plays table rows with the same scenario; surface a small mismatch
  chip 🟡 at the card level.
  Failure mode: noise — needs an "only show mismatches" toggle (default on).
- **Grind session coach** — when KovaaK's is running, the tray/overlay suggests
  the top 3 grind scenarios for a family; finishing a run surfaces "run again in
  X scenarios to reach next tier".
  Failure mode: game overlay permission complexity on Windows 11; scope down to
  the app window polling instead (no injected overlay).
- **Export study-report** — a "share to Discord" button that exports the weekly
  panel + tier-ladder bar as a small PNG (not screenshots, a composed canvas).
  Failure mode: share-viols people's privacy; default off.
- **Per-scenario server-side leaderboards** — fetch global ranks for chosen
  scenarios (public KovaaK's API) to see how far you are from the 99th-99.99th
  percentile. Failure mode: this reintroduces the network dependency the whole
  rank engine exists to avoid; make it opt-in per scenario and cache hard.
