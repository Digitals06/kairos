# ADR-0002: Family fold owned by `kovaaks-core::overview`, tauri crate stays thin

Date: 2026-09-26 · Status: accepted

## Decision
The fold rule (hardest ranked difficulty wins, tier-depth tie-break, unranked never beats ranked) is core::overview::fold_families — pure, unit-tested. lib.rs derives registry metadata and maps the output onto wire DTOs.

## Consequences
Any future consumer (CLI, web, mobile) reuses the rule without spawning Tauri state. New fold behavior changes need a core test alongside the DTO tweak.
