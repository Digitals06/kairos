# Kairos — ΚΑΙΡΟΣ

[![CI](https://github.com/Digitals06/kairos/actions/workflows/ci.yml/badge.svg)](https://github.com/Digitals06/kairos/actions/workflows/ci.yml)

A desktop companion for [KovaaK's FPS Aim Trainer](https://store.steampowered.com/app/824270/KovaKs/): track every benchmark, watch ranks climb, and see per-scenario improvement over time — carved into marble and basalt. No login, no accounts, your data stays in one local file on your PC.

*"καιρός — the opportune moment: the exact instant in which you act."*

If Kairos helps your grind, consider supporting development:

[![ko-fi](https://ko-fi.com/img/githubbutton_sm.svg)](https://ko-fi.com/Q2S1237A82)

![Kairos overview — benchmark grid in the basalt theme with the weekly plaque and grind coach](docs/screenshots/app-grid-v030.png)

## What's new in v0.5.0 — your own benches

- **Custom benchmarks** — build a bench from scenarios you already play, group them into subcategories, and write the thresholds yourself. Three rank rules: **every scenario counts**, **top N per subcategory**, or **global energy** (the harmonic mean Voltaic uses, where one weak subcategory drags the whole rank).
- **Share a bench** — export it to a small JSON file (or copy it as text) and import someone else's. The file holds the structure only: scenarios, subcategories, thresholds, tier names, rule. Never your scores.
- **Discord share card** — a 1200×630 card with your week, your level and your benchmarks, ready to copy straight into a chat or save as a PNG.
- **Global percentiles (opt-in)** — the scenario you're looking at can show its public board position, e.g. `#258 · top 0.7%`. Off by default; the only calls it makes are board sizes, cached for a day.
- Everything from v0.4.0 stays: tray mode, post-session recaps, GO buttons, the weak-spot strip, keyboard navigation.



## Features

### The grind

- **Grind coach** — pick a benchmark family and the coach works out what to play: **▶ N runs to next tier** is the count of scenarios that must improve to flip the rank (0 means maxed), **plateaued** means mid-progress but no new PB in 3+ days, **complete** when every scenario sits at its top rung. Choose a difficulty and the coach plans the grind for it.
- **What to grind next** — on each benchmark's page: the cheapest per-scenario scores that push you to the next rank tier, probed against the local rank engine so they're exact for every method family. Targets are capped at each scenario's ladder top, so they're always actually achievable, and every row carries a GO button that opens the scenario in KovaaK's; when no single scenario can carry you up, the panel produces a numbered step-by-step plan (each step within its scenario's ladder) that reaches the next tier together.
- **Consistency & plateau tags** — grind targets show **spread** (how steady your recent runs are, 0% = identical scores) and flag scenarios stalled without a new personal best, so you can swap a stalled grind for something still climbing.

![Kairos family card expanded showing difficulty rung bars, grind chips and rank seals](docs/screenshots/app-family-v030.png)

- **evxl-accurate ranks, computed locally** — the rank engine reverse-engineered from evxl.app recomputes your rank from your scores (all major benchmark families: Voltaic energy, harmonic/top-N/count-based methods, …). The server's own rank is only a fallback, because it's wrong for most benchmarks.
- **Rank-change toasts** — every sync diffs the two newest snapshots per benchmark; when your overall rank moves, you get a "RANK UP" toast with the old and new tier.
- **Live local plays** — a background watcher reads KovaaK's `Stats` folder while the app is open: new sessions appear in charts on their own, no refresh click. (Ranks still update on Sync Now.)
- **Full benchmark coverage** — every benchmark tracked on [evxl.app](https://evxl.app) (Voltaic, Revosect, PureG, Aimerz+, …), with official rank names and colors.
- **One card per family** — the overview groups benchmarks by family: best rank badge, difficulty count, and an unroll that shows every difficulty with its tier-ladder rungs lit up to your rank.

### Custom benches

![Kairos custom bench builder: rank rules, subcategory chips and a per-scenario threshold table](docs/screenshots/app-custombench-v050.png)

- **Author your own** — pick scenarios from your own history, split them into subcategories (Clicking / Tracking / whatever you actually practise), and give each one its own ascending threshold ladder. Tier names are yours too.
- **Three rules, honestly different** — on the same scores, *every scenario counts* can read Unranked while *top 1 per subcategory* reads Silver; global energy lands between them, because it harmonic-means the subcategories instead of letting the weakest one veto.
- **It is a real bench** — custom benches appear in the stoa, take favorites, open a detail page with per-scenario gaps and GO buttons, and are scored by the same engine as the registry ones.
- **Import / export** — the JSON is a template, not a backup: no ids, no scores, just the bench.

### The numbers

![Kairos weekly plaque with torch day strip, Twelve Steps frieze and rank inscription](docs/screenshots/app-weekly-v030.png)

- **Weekly plaque** — plays-per-day torch strip, current level along the Twelve Steps, and rank inscriptions — in a single element.
- **Per-scenario detail** — score history chart (your local plays + sync snapshots, deduplicated so the same run never counts twice), running high, 7-day average, avg/high scores, and 30-day improvement %. Snapshots that don't set a new high are ignored, so stale syncs never pollute charts or averages. Scenario tables cycle **weakest → strongest → order** so the closest-to-flip grind heads the list.

![Kairos score history chart with a global percentile chip next to the title](docs/screenshots/app-global-percentile-v050.png)

- **Global percentiles (opt-in)** — the scenario in focus can show where it sits on the public board (`#53 · top 10.1%`), computed from your stored rank and the board's size. Enable it in settings; board sizes are cached for a day and nothing else about you leaves the machine.

### Around the app

- **Benchmark type filter** — 12 tabs mirroring evxl's own grouping (Mixed, Tracking, Static, Clicking, Ground, Precise, Smooth, Micro, Dynamic, Reactive, Evasive, Switching), multi-select, combined with the name search — plus a **Custom** tab that shows only your own benches. Tag assignments match evxl's live filter, so the Static tab shows only genuinely static benchmarks, Ground catches ground-tracking specialists, and everything unspecialized sits under Mixed.
- **Search + favorites** — filter the grid by name, pin benchmarks to the top. Pins survive restarts.
- **Tray mode** — closing the window keeps Kairos in the tray so the stats watcher stays live; restore it from the tray menu, or launch KovaaK's straight from there.
- **Keyboard-first** — arrows walk the benchmark cards, Enter opens, Esc backs out, f toggles favorites.
- **Theme picker** — Marble (light), Basalt (dark), or follow the OS. Charts re-palette on the flip.
- **Backup export** — one click in the settings menu writes your entire history as a JSON file in Documents.
- **One-click sync** — a quick pass for your benchmarks; "Check everything at launch" refreshes every benchmark from the leaderboard when you want a deep pass. If the server throttles, Kairos waits and retries politely on its own. **Refresh Local** re-scans the stats folder without touching the network.
- **Share card** — one button composes a 1200×630 card (week, level, benchmarks, top movers) you can copy into Discord or save to Pictures/Kairos.
- **Private by design** — no accounts, no telemetry; only public APIs, and live API tests skip unless you opt in. The global-percentile lookup is opt-in and returns only a board size.

![Kairos Discord share card: wordmark, weekly stats, level bar, plays-per-day strip and benchmark table](docs/screenshots/share-card-v050.png)

![Kairos what-to-grind-next panel: a step-by-step plan with GO buttons per scenario](docs/screenshots/app-grind-v040.png)

![Kairos weak-spot strip: the next scenario, its gap and a GO button, pinned at the bottom of the page](docs/screenshots/app-weakspot-v040.png)

## How it works

| Source | Used for |
|---|---|
| KovaaK's public score server (no login needed) | Scores, ranks, leaderboard positions; board sizes only when you opt into global percentiles |
| evxl.app public lookup + built-in registry | Finding your profile, benchmark and rank definitions |
| Your KovaaK's `Stats` folder (`*.csv` files) | Local play history |

Everything is stored in one file: `%LOCALAPPDATA%\kairos\store.db`. Nothing is uploaded anywhere.

## Run it (no building needed)

1. Download `kairos.exe` from the [latest release](https://github.com/Digitals06/kairos/releases).
2. Double-click it, enter your Steam ID (or vanity name / profile link), hit **Sync Now**.

## Build from source (Windows, step by step)

You only need this if you want to change the code. Takes ~10 minutes the first time.

**Step 0 — install the tools** (skip any you already have):

1. **Git**: download from [git-scm.com](https://git-scm.com/download/win), run the installer, keep all defaults.
2. **Rust**: download from [rustup.rs](https://rustup.rs/) and run `rustup-init.exe`. When it asks, choose **Desktop development with C++** if prompted about Visual Studio (Rust needs Microsoft's C++ build tools to link apps on Windows — the installer will point you to them).
3. **Node.js 24**: download the LTS installer from [nodejs.org](https://nodejs.org/), run it, keep defaults. This gives you both `node` and `npm`.

Check they work (open a fresh terminal — search "PowerShell" in Start — and run):

```powershell
git --version
rustc --version
node --version
npm --version
```

Each should print a version number. If a command is "not recognized", close and reopen the terminal (installers update the `PATH` only for new windows).

**Step 1 — download the code:**

```powershell
cd $HOME\Desktop
git clone https://github.com/Digitals06/kairos.git
cd kairos
```

This creates a `kairos` folder on your Desktop with the full source.

**Step 2 — build the frontend** (the visual part; plain Rust builds skip it, so this step is mandatory):

```powershell
cd crates\kovaaks-tauri\ui
npm install
npm run build
```

`npm install` downloads the UI libraries (once), `npm run build` compiles them into the `dist` folder.

**Step 3 — build the app:**

```powershell
cd ..\src-tauri
cargo build --release --features custom-protocol
```

The `--features custom-protocol` part is required: it bakes the built frontend from step 2 into the `.exe`. Without it the app opens to a connection error. The finished app lands at `..\..\..\target\release\kairos.exe` (i.e. `kairos\target\release\kairos.exe` from the repo root).

**Step 4 — run it:**

```powershell
..\..\..\target\release\kairos.exe
```

**Running the tests** (from the repo root folder):

```powershell
cd ..\..\..\   # back to the kairos folder
cargo test --workspace --offline        # fast, no network
cargo clippy --workspace --all-targets -- -D warnings   # linter, CI gate
```

Live API tests exist but are skipped by default (`#[ignore]`) so the suite never hammers the public servers; a few of them read environment variables (`KAIROS_TEST_STEAM_ID`, `KAIROS_LIVE_DB`, `KAIROS_LIVE_STEAM_ID`) and skip cleanly when unset.

## Development workflow

CI runs on every push (rustfmt, clippy with warnings denied, the offline test suite, and a frontend build) and a Windows release build runs automatically on `v*` tags — the exe is attached to the GitHub release by the workflow itself.

The architecture is documented for contributors: domain glossary in [CONTEXT.md](CONTEXT.md), decision records in [docs/adr/](docs/adr/). Debug/probe scripts are intentionally **not** part of the repo; keep local tooling outside git (`.gitignore` covers `/scripts/`).

## Roadmap

Planned features live in [ROADMAP.md](ROADMAP.md) — v0.5.0 (custom benches, share card, opt-in percentiles) is done; the v0.6.0 candidate list is the next conversation.

## Layout

```
crates/kovaaks-core/    registry, API clients, SQLite store, sync engine, metrics,
                        rank engine (rankcalc), rank-diff (rankdiff), score series,
                        weekly recap, grind engine
crates/kovaaks-tauri/   Tauri v2 shell (src-tauri, incl. the CSV watcher) + Svelte 5 UI (ui)
                        TS bindings are generated by ts-rs into ui/src/lib/bindings/
docs/                   rank-systems spec, ADRs + README screenshots
```

Built with Rust, Tauri v2, Svelte 5, Chart.js, and rusqlite.
