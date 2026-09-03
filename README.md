# Kairos

A desktop companion for [KovaaK's FPS Aim Trainer](https://store.steampowered.com/app/824270/KovaKs/): track every benchmark, watch ranks climb, and see per-scenario improvement over time — all in a neon-soaked native app. No login, no accounts, your data stays in a local SQLite file.

![Kairos card list](docs/screenshots/card-list.png)

## Features

- **Full benchmark coverage** — every benchmark tracked on [evxl.app](https://evxl.app) (Voltaic, Revosect, PureG, Aimerz+, …), with official rank names and colors resolved per benchmark rules.
- **One-click sync** — shallow pass for your main benchmarks, Deep Scan for everything. Throttled requests get a cooled-down sequential retry pass that honors the server's `Retry-After`.
- **Per-scenario detail** — score history chart (local plays + sync snapshots), 7-day average, running high, avg/high scores, and 30-day improvement % for the selected scenario. Snapshots that don't set a new high are ignored, so stale syncs never pollute charts or averages.
- **Local CSV ingest** — reads KovaaK's own `Stats/` CSVs forward-only, so sessions played offline still count.
- **Search + favorites** — filter the grid by name, pin benchmarks to the top. Pins survive restarts.

![Kairos benchmark detail](docs/screenshots/benchmark-detail.png)

## How it works

| Source | Used for |
|---|---|
| KovaaK's public `webapp-backend` (no auth) | Scores, ranks, leaderboard positions |
| evxl.app public resolver + embedded registry | Steam ID → profile, benchmark/difficulty/rank definitions |
| `%USERPROFILE%\…\FPSAimTrainer\FPSAimTrainer\stats\*.csv` | Local play history |

All state lives in `%LOCALAPPDATA%\kairos\store.db`. Nothing is uploaded anywhere.

## Run it

Windows: grab `kairos.exe` from `target/release/` (or build it below), launch, enter a Steam ID / vanity / profile URL, hit **Sync Now**.

## Build from source

Prereqs: Rust stable (MSVC), Node 24 + npm.

```bash
# 1. Frontend first — plain cargo does NOT rebuild it
cd crates/kovaaks-tauri/ui && npm install && npm run build

# 2. Backend (custom-protocol embeds the built frontend into the binary)
cd ../src-tauri && cargo build --release --features custom-protocol
# → ../../../target/release/kairos.exe
```

Tests (offline by default; live API tests are `#[ignore]`d):

```bash
cargo test --workspace --offline
cargo clippy --workspace --offline
```

## Layout

```
crates/kovaaks-core/    registry, API clients, SQLite store, sync engine, metrics
crates/kovaaks-tauri/   Tauri v2 shell (src-tauri) + Svelte 5 UI (ui)
docs/screenshots/       README images
```

Built with Rust, Tauri v2, Svelte 5, Chart.js, and rusqlite.
