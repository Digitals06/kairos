# v0.1.7 — Grind-Next Reachability Fix

**Goal:** Fix "what to grind next" showing unreachable targets (up to 98k on
scenarios whose ladder tops out ~100), then ship as v0.1.7 with roadmap/readme
updates.

**Root cause (reproduced):** `next_targets` bisects each scenario's minimal
rank-flipping score against a fixed `CEILING = 100_000`. For family-interaction
rank methods, one giant outlier score can flip the benchmark rank mathematically
while being game-impossible (e.g. id 584 `patCircleSwitch NR`: target 98,438 on a
scenario whose top ladder rung is 108). Live sweep: 61/384 targets unreachable.

**Fix:** every scenario's own `rank_maxes` ladder is the reachable ceiling.
Cap the probe at the scenario's top rung; scenarios already at/above their top
rung have no headroom (skip); scenarios whose flip score exceeds the cap get no
target (several-scenarios-together case, honestly reported). Scenarios with
empty `rank_maxes` keep the old fixed ceiling (no better data available).

---

### Task 1: Cap probe at scenario ladder top (grind.rs)

**Files:** `crates/kovaaks-core/src/grind.rs`

- Collect per-scenario cap alongside (name, cur): `cap = entry.rank_maxes.last()`
  (max across duplicate name entries), ceiling as integer.
- `minimal_score_for_rank(..., hi = cap)` instead of CEILING; fall back to
  CEILING only when maxes empty.
- Skip scenarios with `cur >= cap` (no headroom).
- Unit test (TDD): scenario with top rung 108 whose rank only flips at 20k must
  yield NO target (was: 19,9xx); a scenario whose flip lands under its top rung
  still yields a target <= top rung; a scenario already at its top rung yields
  none.

### Task 2: Full gate + live sweep re-verification

- `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace --offline` (expect 131+ passed)
- Rebuild release, relaunch app, re-run the 194-benchmark IPC sweep:
  assert 0 targets above their scenario's top rung (rungs from store.db).

### Task 3: Docs — ROADMAP.md + README.md

- ROADMAP: mark v0.1.6 released (evxl taxonomy, grind-next shipped); new
  **v0.1.7** section: grind-next reachability cap (targets bounded by each
  scenario's ladder; unreachable combinations reported honestly).
- README: update the features list — evxl 12-tab filter + grind-next panel
  with ladder-capped targets.

### Task 4: Release v0.1.7

- Bump `Cargo.toml` + `tauri.conf.json` to 0.1.7, commit, tag `v0.1.7`, push.
- Watch Release workflow; patch curated notes via REST (git credential fill);
  refresh `Desktop/Kairos/kairos.exe` (sha-verified).
