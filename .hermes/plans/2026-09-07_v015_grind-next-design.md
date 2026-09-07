# Design pass: "What to grind next" (v0.1.5 roadmap item)

Goal: for every played benchmark, tell the player WHAT to do to rank up —
which scenario, and what score — without running the rank engine backwards
per family (that inversion is the hard, benchmark-specific part).

## What the engine already gives us (facts, verified in code)

- `compute_rank(progress, bench, difficulty) -> RankResult { rank, name, complete }`
- Every method family internally computes a scalar the ladder thresholds compare
  against: `total` (basic = min scenario rank · VT-family = harmonic energy ·
  me = all-scenario harmonic · se = top-N subcat average · ne = count of
  scenarios at/above a bar).
- `difficulty_thresholds` / `vt_energy_thresholds` expose the ladder cut points.
- Subcategory spans (`subcategory_spans`) give scenario→subcategory mapping.

## Core idea: probe-based inversion (no per-family reverse math)

The engine is cheap (<1 ms per benchmark, measured in the rank-fix era) and
deterministic. Instead of inverting it, **bisect the input**:

```
next_target(bench, difficulty, current_progress):
    need = current_rank + 1            # target ladder position
    for each scenario s of the target difficulty's subcategories:
        binary-search the minimal score y on s such that
        compute_rank(progress with s=y) reaches `need`
        (hold everything else at current scores)
    emit candidates where the search converges (score is finite)
```

- Monotonicity: every family's scalar is monotone non-decreasing in each
  scenario score (min-of-ranks, harmonic means, averages, counts) — verified
  across all ported families during the batch port. Binary search is valid.
- Cost: scenarios per benchmark ≤ 40; log2(4000) ≈ 12 probes per scenario →
  ≤ ~500 engine calls per benchmark worst case, still well under 50 ms.
- Accuracy is EXACT (same code path ranks are displayed from), no analytic
  approximation to maintain.

## Output shape (UI)

`GrindTarget { benchmark_id, benchmark_name, difficulty, current_rank_name,
next_rank_name, candidates: [{ scenario, current_score, target_score,
delta, is_bottleneck }] }`

- `is_bottleneck`: candidate whose current score is furthest below its
  category's requirement — the evxl-style "this one is holding you back".
- Sort candidates by `target_score - current_score` ascending (cheapest win first).
- "Complete" benchmarks (rank = ladder_len+1) emit no target.

## Integration points

1. Backend: `kovaaks-core::rankcalc::next_targets(progress, bench, difficulty)`
   — pure, unit-testable offline (synthetic progress fixtures per family:
   basic, vt-energy, me, se, ne — assert the found target actually flips the
   rank when fed back through `compute_rank`).
2. Tauri command `grind_next(benchmark_id?)`: all played benchmarks when no id,
   single benchmark otherwise; recomputed on demand (cheap), never stored.
3. UI: a "Grind next" panel on the overview (top 3 cheapest wins across all
   benchmarks) + per-benchmark block on the detail page.

## Edge cases to handle in implementation

- Scenario already at max rank (can't raise further) → excluded from search.
- Unplayed scenario (score 0): searching from 0 works — the found target is
  "play scenario X to Y".
- `progress` shape mismatch (scenario missing from latest snapshot): seed
  those scenarios at 0, documented behavior.
- Rank already maxed: return `complete: true`, no candidates.
- families with external caps (Avasive-S2 energy cap): the probe naturally
  converges at the cap; no special code.

## Non-goals (explicitly out)

- Historical "what would have ranked me up sooner" (needs replay of old data).
- Percentile/ETA predictions (no data model for hours-per-point).
- Any server call — fully local computation.

## Estimated implementation (next session)

1. `next_targets` + family unit tests: ~3–4 h
2. Tauri command + UI panel: ~2 h
3. Live verification (CDP screenshot vs evxl rank-boundary sanity): ~1 h
