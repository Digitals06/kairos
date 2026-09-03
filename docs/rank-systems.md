# Rank Systems — reverse-engineering evxl's `rankCalculation` engine

Status: **specification for v0.2** (not yet implemented).

## Problem

KovaaK's webapp-backend returns `overall_rank` / `category_rank` per benchmark, but
those values do **not** match evxl's displayed ranks for most benchmarks. evxl
re-computes the rank client-side from the per-scenario data using a per-benchmark
`rankCalculation` method (44 distinct methods across the 125-benchmark registry).
We currently trust the API index, so e.g. VT S5 Novice can show Silver where evxl
shows Gold.

Examples from the registry:

| Benchmark | `rankCalculation` |
|---|---|
| Sparky (Voltaic) S1, MattyOW OW, hahAim | `basic` (36 benchmarks) |
| Voltaic S5 / S5.5 | `vt-energy` |
| Jade Palace Ground/Air/Dynamic | `jade-palace` |
| 143 Skand / Beginner Rank Up / FINALGEAR | `generic-energy-alt` (18× `generic-energy*` family) |
| Aimbeast V1/V2, CIS Aimers, VRTCL, OOT | `aimbeast` |
| RXZU, MIYU, hewchy, aoi, e1se, dojo* | count/points-based |

## Data we already have (no new sources needed)

Every method is derivable from what sync already stores:

- per-scenario `score` (API centi-scale → divide by 100), `scenario_rank`,
  `rank_maxes` (per-scenario score thresholds, **display scale**, one per rank)
- the embedded registry: `rankColors` (ordered rank names per difficulty),
  `categories` → `subcategories` → `scenarioCount` (the canonical scenario order)
- difficulty position within the benchmark (for cross-difficulty threshold math)

## Common primitives (from evxl's engine)

```
norm(score)             = score / 100                      // API centi-scale → display
rankOf(score, maxes)    = { base: 1-based index of highest maxes[i] <= score (0 = below first),
                            precise: base + fractional progress into current band,
                            progress: fraction (0..1) within current band,
                            isMaxed: base == maxes.len() && score > maxes[last] }
energyW(state, thresholds, fakeLower, fakeUpper)
                        = piecewise-linear mapping of a scenario's precise rank
                          onto an "energy" axis: thresholds are 100 apart starting
                          at fakeLower below the first real threshold, extrapolate
                          up to fakeUpper fake ranks above the last
harm(values)            = n / Σ(1/v), 0 if any v == 0 or count mismatch   // strict
harmSoft(values)        = same but zeros are replaced by 0.1              // jade-palace
totalRanks(difficulties)= Σ len(rankColors) over ALL difficulties
thresholdsFor(difficulty, difficulties)
                        = 100-based slice belonging to this difficulty:
                          offsets = Σ len(rankColors) of *earlier* difficulties
scenarioOrder(progress, difficulty)
                        = scenario names flattened in registry category →
                          subcategory → scenarioCount order (we already store this order)
```

Final rank name resolution (dispatcher `qn`):
1. compute `engineRank` with the method below
2. `scenarioFloorRank` = rank name derived from the **minimum** `scenario_rank`
   across all scenarios (the API's per-scenario ranks)
3. displayed rank = `max(engineRank, scenarioFloorRank)` — except
   `selectable-top-n`, which always uses the engine rank
4. `aimbeast` / `aimbeast-partial` get a `_1`-suffix ladder when the next rank
   name starts with the same word (Iron → Iron_1 → Bronze …)
5. a special "Pinnacle" name overrides when every scenario is maxed (xyz2 family)

## Method catalog (verified against the minified engine)

### Family 1 — `basic` (36 benchmarks)
- rank = **min** `baseRank` over all subcategories (best scenario per subcategory)
- any unranked subcategory ⇒ rank 0
- if every subcategory reaches the last rank color ⇒ "Complete" (rank = max+1)
- progress = mean of per-subcategory fractional progress (0.001 for unranked)

### Family 2 — energy systems (harmonic mean of subcategory energies)
Energy per subcategory = `energyW(best scenario preciseRank)`; overall =
`harm(energies)`; rank = highest threshold ≤ overall.

- **`vt-energy`** (Voltaic S5/S5.5): thresholds per difficulty = Novice
  [100,200,300,400], Intermediate [500..800], Advanced [900..1200] (Elite
  unofficial variant slices by first non-junk rank name); **subcategories whose
  name contains "strafe" are excluded**; fakeLower = 0 for Novice else 100;
  Advanced caps at max then recomputes uncapped when at the top.
- **`generic-energy`**: thresholds = the difficulty's slice of the 100-per-rank
  ladder across all difficulties (drop last), fakeLower = 100, fakeUpper = 1.
- **`generic-energy-uncapped`**: same, fakeUpper = 9999 (no cap).
- **`generic-energy-alt`** (18 benchmarks): thresholds start at
  `(Σ rankColors of earlier difficulties − duplicated-boundary count + 1) × 100`,
  one per rank color of this difficulty; subcategory energy = **average of its
  top-2 scenario energies**; overall = **mean** of subcategory energies (not
  harmonic); unranked scenario energy = 0.
- **`jade-palace`**: thresholds = 100-ladder slice minus last; subcategory energy
  = **average of its best half** of scenario energies (Fundamentals difficulty:
  top-3; Easy: capped-energy variant with an "over-energy uncaps at 600" rule);
  harmonic mean treats zeros as 0.1 (`harmSoft`).
- **`mh-tracking` / `mh-reactive` / `mh-precise`**: same as generic-energy-alt but
  with a 12-scenario top-half rule and fixed threshold base 100.
- **`val-energy`**: thresholds [100..1500] sliced Easy [0..4] / Medium [4..8] /
  Hard [8..12]; standard Y with fakeLower 100, fakeUpper 1.
- **`ca-s1`**: Y with fixed thresholds [1500..1800], fakeLower 50, fakeUpper 2,
  strafe subcategories excluded.
- **`avasive` / `Avasive-S2` / `snakbox` / `mira-apex` / `dm` / `dm-s3` / `ra-s5`**:
  energy variants with per-difficulty fixed threshold tables or caps
  (Avasive-S2 caps: Easier 600 / Medium 1000 / Hard 1400; dm-s3 Boss tables end
  at 1510; ra-s5: subcat = avg of top-2 scenario energies, reactive subcats pair
  scores, harmonic mean; avasive: per-scenario energy, all 18 required).

### Family 3 — count / requirement systems (no energy)
- **`e1se`**: ≥ 6 scenarios at rank R ⇒ rank R.
- **`hewchy`**: ≥ 12 scenarios at rank R ⇒ rank R.
- **`aoi`**: rank R if ≥1 score at R in **4 different subcategories**, OR ≥2
  scores at R in **3 different subcategories**; take the best qualifying R.
- **`dojo` / `dojo2` / `dojo3`**: 4 / 3 / 5 scenarios at rank R ⇒ rank R.
- **`tpt`, `tsk`, `sa-s2`, `asb`, `xyz`, `xyz2`, `xyz-smoothness-v2`, `mira`,
  `dm`, `33`, `iris`**: count-threshold variants (same shape: bucket scenarios by
  achieved rank, require N at rank R or a top-k rule). xyz2: 4th-highest score
  per category, overall = min across categories; xyz-smoothness-v2: 9 scores per
  base rank, 6 Prismatic, 15 for Pinnacle.
- **`MIYU`**: points per scenario = `2 + (scenario_rank − 1)` (0 if unranked);
  total vs fixed thresholds [16, 24, 32, 40, 48, 56, 63].

### Family 4 — score/average systems
- **`cb-s1`**: scenario score → weighted 0..1800 via fixed table
  [300,600,900,1200,1500,1800]; total vs same table; requires "Complete".
- **`ra-s4`**: top-4 scenarios per category, weighted via fixed tables (Easy
  [20..170]→[240..2040], else [200..400]→[2400..4800]) with 3/2 extrapolation.
- **`RXZU`**: per-scenario points from difficulty-specific rank-point tables
  (Easy: Worm 200 … Radiant 1200; Hard: Aimstar 400 … Biomaschine 1200);
  overall = average points per scenario.
- **`aimbeast`**: category rank = **average of scenario ranks** in the category;
  overall = average of category ranks; any unranked scenario ⇒ category (and
  overall) unranked. `aimbeast-partial`: requires ≥ half of scenarios ranked,
  unranked ones excluded from averages.
- **`selectable-top-n`**: uses the benchmark's `scenarioSelection` config
  (selectCount, requiredPerCategory, requiredCategories, full-pool bonus count);
  rank from the top-N best scores.

### Unknown / fallback
Methods not in the dispatcher table fall back to `rank = scenarioFloorRank`
(today's behavior). That is already correct for `basic`-adjacent sheets.

## Implementation plan (v0.2)

1. **Core port** (`crates/kovaaks-core/src/rankcalc/`): one module per family,
   pure functions over `BenchmarkProgress + BenchmarkDef + difficulty`. Port the
   primitives first (`rankOf`, `energyW`, harmonic means, threshold slicing) with
   unit tests per primitive.
2. **Method dispatch**: `rank_calc_method(benchmark) -> Method` from
   `rankCalculation` (already in the embedded registry JSON — add it to
   `BenchmarkDef`).
3. **Wire-up**: cards and detail views use the engine rank instead of the API
   `overall_rank`; keep the API value as `api_rank` for the scenario-floor term
   and fall back to it for unknown methods.
4. **Verification harness**: `#[ignore]` live test that, for every played
   benchmark, compares our computed rank name against the name scraped from the
   user's evxl page (`/u/<steamid>` HTML contains per-benchmark rank text).
   Gate: ≥ 95% agreement before enabling.
5. **UI**: show a small "rank method" hint on detail view when engine rank
   differs from the API rank (transparency while data settles).

Priority order (by user impact): `vt-energy` (Voltaic S5/S5.5) → `basic` →
`generic-energy*` (18) → `jade-palace` → `aimbeast*` → count family → long tail.
