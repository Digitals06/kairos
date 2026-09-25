# Kairos — project context (kovaaks-companion)

## Domain glossary

- **Benchmark family** — one KovaaK's benchmark (e.g. "Avasive S2"). A family has difficulties.
- **Difficulty** — one playable variant of a family, its own KovaaK's map + ladder.
- **Score series** — one scenario's chronological run history: CSV plays merged with new-high
  snapshot points, sync echoes of the same run deduped by rounded score. Owned by
  `kovaaks-core::series`.
- **Grind engine** — computes, per difficulty, what to play next to reach the next rank tier.
- **Weekly recap** — trailing-7-day aggregation over score series. `weekly.rs`.
- **Coin seal** — the UI's circular rank badge (tier name + official tier color).

## ADRs (docs/adr/)

- ADR-0001 **The merged-score-series rule lives only in `kovaaks-core::series`** (2026-09-26).
  All callers — charts, weekly recap, XP streak model, sync staleness, plateau detection —
  must consume `series::series_map` / `series::merge_parts`; hand-rolled merges elsewhere are
  regressions. Motivation: perf fixes (weekly 84s→sub-second) kept re-happening because the
  dedup semantics existed in three shapes across metrics.rs/weekly.rs.
- ADR-0002 **The view layer owns DTO mapping, the fold owns `kovaaks-core::overview`** — the
  family-fold rule (hardest ranked difficulty wins, tier-depth tie-break, unranked never beats
  ranked) is engine-owned; the tauri crate derives per-row registry metadata and copies output
  onto wire DTOs.
- ADR-0003 **DTO bindings are generated, never hand-mirrored** — ts-rs emits the TS interfaces
  into `ui/src/lib/bindings/`; `api.ts` re-exports them and holds only invoke wrappers.
  Editing a binding file by hand is a bug; change the Rust struct and rerun
  `cargo test -p kovaaks-tauri export`.
