<script lang="ts">
  /**
   * Scenario analytics (v0.3.0): ALL scenarios of the current benchmark on one
   * screen — a tablet grid of per-scenario mini-charts (PB progression, spread,
   * plateau, improvement %). Data comes from getBenchmarkDetail's
   * scenario_history (backend owns the merged history). Compact SVG sparklines
   * keep the grid render cheap for 60+ scenarios; no per-chart Chart.js
   * instance → no 277KB duplicate cost per card.
   */
  import {
    getBenchmarkDetail,
    type BenchmarkDetail,
    type ScenarioHistorySeries,
  } from '../lib/api'

  let { benchmarkId, onback }: { benchmarkId: number; onback: () => void } = $props()

  let detail = $state<BenchmarkDetail | null>(null)
  let query = $state('')
  let sortKey = $state<'weakest' | 'strongest' | 'name'>('weakest')
  let error = $state<string | null>(null)
  let capturing = $state(false)

  /** Compose the analytics report into one image and copy it (v0.3.0 capture
   * button). The report is generated as inline SVG (text + spark paths + tier
   * bars), rasterized on an offscreen canvas at 2×, then handed to the Rust
   * clipboard command. Zero extra frontend deps. */
  async function captureReport() {
    if (!detail) return
    capturing = true
    try {
      const list = ranked
      const COLS = 4
      const CW = 320
      const CH = 126
      const cols = Math.min(COLS, list.length)
      const margin = 28
      const headH = 86
      const width = cols * CW + margin * 2
      const rows = Math.ceil(list.length / cols)
      const height = headH + rows * CH + margin * 2
      const esc = (x: string) =>
        x.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
      // Theme-aware palette: renders the report in the app's CURRENT theme
      // (marble light / basalt dark), read from documentElement.dataset.theme.
      const C =
        document.documentElement.dataset.theme === 'marble'
          ? {
              bg: '#f6f2e9',
              card: '#ffffff',
              border: '#d8cfbe',
              text: '#2c2620',
              faint: '#6f6350',
              line: '#a06b2e',
              accent: '#b08d3e',
              fill: '#b08d3e22',
              up: '#4c7a34',
              warn: '#b0563a',
              empty: '#e2d9c8',
            }
          : {
              bg: '#141210',
              card: '#1e1a16',
              border: '#3a3227',
              text: '#efe6d4',
              faint: '#967f63',
              line: '#d9b066',
              accent: '#c9a55c',
              fill: '#c9a55c33',
              up: '#8fb25e',
              warn: '#d08a6e',
              empty: '#3a3227',
            }
      const parts: string[] = []
      parts.push(
        `<rect width="${width}" height="${height}" fill="${C.bg}"/>`,
        `<text x="${margin}" y="${40}" font-family="Georgia, serif" font-size="26" font-weight="bold" fill="${C.text}">${esc(detail.card.benchmark_name)} — all scenarios</text>`,
        `<text x="${margin}" y="${64}" font-family="Georgia, serif" font-size="14" fill="${C.faint}">${esc(String(list.length))} scenarios · Kairos</text>`,
      )
      list.forEach((s, idx) => {
        const r = detail!.scenario_ranks.find((x) => x.scenario === s.scenario)
        const x = margin + (idx % cols) * CW
        const y = headH + Math.floor(idx / cols) * CH
        parts.push(
          `<rect x="${x}" y="${y}" width="${CW - 12}" height="${CH - 14}" rx="6" fill="${C.card}" stroke="${C.border}"/>`,
          `<text x="${x + 10}" y="${y + 22}" font-family="Georgia, serif" font-size="12.5" font-weight="bold" fill="${C.text}">${esc(s.scenario).slice(0, 38)}</text>`,
        )
        if (r?.tier) {
          parts.push(
            `<text x="${x + 10}" y="${y + 40}" font-family="Georgia, serif" font-size="11" font-weight="bold" fill="${esc(r.tier.color)}">${esc(r.tier.name)}</text>`,
          )
          const maxes = r.rank_maxes.length
          maxes > 0 &&
            maxes <= 8 &&
            [...Array(maxes)].forEach((_, m) => {
              parts.push(
                `<rect x="${x + 10 + m * 13}" y="${y + 46}" width="10" height="4" rx="1" fill="${m < r.scenario_rank ? C.accent : C.empty}"/>`,
              )
            })
        }
        const pts = s.pb_points
        if (pts.length >= 2) {
          const sw = CW - 36
          const sh = 34
          const sx = x + 14
          const sy = y + 56
          const ys0 = pts.map((p) => p.score)
          const lo = Math.min(...ys0)
          const hi = Math.max(...ys0)
          const span = hi - lo || 1
          let d = ''
          pts.forEach((p, i) => {
            const px = sx + (i / (pts.length - 1)) * sw
            const py = sy + sh - ((p.score - lo) / span) * (sh - 4) - 2
            d += (i ? ' L ' : 'M ') + px.toFixed(1) + ' ' + py.toFixed(1)
          })
          parts.push(
            `<path d="${d} L ${sx + sw} ${sy + sh} L ${sx} ${sy + sh} Z" fill="${C.fill}"/>`,
            `<path d="${d}" fill="none" stroke="${C.line}" stroke-width="1.6"/>`,
          )
        }
        const gap = gapPct(s)
        let numY = y + CH - 26
        if (gap !== null)
          parts.push(
            `<text x="${x + 10}" y="${numY}" font-family="Georgia, serif" font-size="11" fill="${C.up}">+${gap.toFixed(1)}%</text>`,
          )
        if (s.plateau.cv)
          parts.push(
            `<text x="${x + 72}" y="${numY}" font-family="Georgia, serif" font-size="11" fill="${C.faint}">spread ${s.plateau.cv.toFixed(1)}%</text>`,
          )
        if (s.plateau.plateaued)
          parts.push(
            `<text x="${x + 150}" y="${numY}" font-family="Georgia, serif" font-size="11" fill="${C.warn}">plateaued ${s.plateau.days_since_pb}d</text>`,
          )
      })
      const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}">${parts.join('')}</svg>`
      const img = new Image()
      const blob = new Blob([svg], { type: 'image/svg+xml' })
      const url = URL.createObjectURL(blob)
      await new Promise((res, rej) => {
        img.onload = res
        img.onerror = () => rej(new Error('svg rasterize failed'))
        img.src = url
      })
      const scale = 2
      const canvas = document.createElement('canvas')
      canvas.width = width * scale
      canvas.height = height * scale
      const ctx = canvas.getContext('2d')!
      ctx.scale(scale, scale)
      ctx.drawImage(img, 0, 0)
      URL.revokeObjectURL(url)
      const data = ctx.getImageData(0, 0, canvas.width, canvas.height)
      const { invoke } = await import('@tauri-apps/api/core')
      // Pass the RGBA bytes as a binary ArrayBuffer (Tauri 2 native invoke
      // serializes typed arrays without a JS-array marshal pass).
      await invoke('copy_report_image', {
        width: canvas.width,
        height: canvas.height,
        rgba: new Uint8Array(data.data.buffer),
      })
      return true
    } catch (e) {
      error = `Capture failed: ${String(e)}`
      return false
    } finally {
      capturing = false
    }
  }

  $effect(() => {
    const id = benchmarkId
    let alive = true
    getBenchmarkDetail(id)
      .then((d) => {
        if (alive) {
          detail = d
          error = null
        }
      })
      .catch((e) => {
        if (alive) error = String(e)
      })
    return () => {
      alive = false
    }
  })

  function sortedTierComplete(d: BenchmarkDetail): number {
    // How many scenarios sit at the ladder top (max tier achieved).
    return d.scenario_ranks.filter(
      (r) => r.rank_maxes.length > 0 && r.scenario_rank >= r.rank_maxes.length,
    ).length
  }

  const series = $derived(detail?.scenario_history ?? [])

  function gapPct(s: ScenarioHistorySeries): number | null {
    // Improvement from earliest PB to the current best (matplotlib-free).
    const pb = s.pb_points
    if (pb.length < 2) return null
    const first = pb[0].score
    const best = pb[pb.length - 1].score
    if (!first) return null
    return ((best - first) / first) * 100
  }

  function tierScore(s: ScenarioHistorySeries): number {
    // Weakest = lowest achieved tier index (from scenario_ranks via rank name?).
    // We rank by current best score percentile against rank_maxes — but
    // scenario_ranks lives on detail.scenario_ranks keyed by scenario.
    return s.pb_points.length ? s.pb_points[s.pb_points.length - 1].score : 0
  }

  const ranked = $derived.by(() => {
    const q = query.trim().toLowerCase()
    let list = series.filter((s) => !q || s.scenario.toLowerCase().includes(q))
    const rankByIdx = new Map(detail?.scenario_ranks.map((r) => [r.scenario, r]) ?? [])
    if (sortKey === 'name') {
      list = [...list].sort((a, b) => a.scenario.localeCompare(b.scenario))
    } else {
      // Weakest ordering: biggest gap between current score and ladder top.
      const room = (s: ScenarioHistorySeries): number => {
        const r = rankByIdx.get(s.scenario)
        const top = r?.rank_maxes?.length ? Math.max(...r.rank_maxes) : Infinity
        return top - tierScore(s)
      }
      list = [...list].sort((a, b) =>
        sortKey === 'weakest' ? room(b) - room(a) : room(a) - room(b),
      )
    }
    return list
  })

  type Spark = { path: string; area: string; w: number; h: number }
  function spark(s: ScenarioHistorySeries): Spark | null {
    const pts = s.pb_points
    if (pts.length < 2) return null
    const w = 220
    const h = 40
    const xs = pts.map((_, i) => (i / (pts.length - 1)) * w)
    const ys0 = pts.map((p) => p.score)
    const lo = Math.min(...ys0)
    const hi = Math.max(...ys0)
    const span = hi - lo || 1
    const ys = ys0.map((v) => h - ((v - lo) / span) * (h - 6) - 3)
    let d = `M ${xs[0].toFixed(1)} ${ys[0].toFixed(1)}`
    for (let i = 1; i < xs.length; i++) d += ` L ${xs[i].toFixed(1)} ${ys[i].toFixed(1)}`
    const area = `${d} L ${w} ${h} L 0 ${h} Z`
    return { path: d, area, w, h }
  }

</script>

<svelte:head>
  <title>ΚΑΙΡΟΣ — Analytics</title>
</svelte:head>

{#if error}
  <div class="panel empty">
    <p>Failed to load analytics: {error}</p>
  </div>
{:else if !detail}
  <div class="panel empty"><p>Consulting the ledgers…</p></div>
{:else}
  <header class="an-head">
    <button class="back engraved" onclick={onback}>◀ {detail.card.benchmark_name}</button>
    <span class="an-sub">
      {series.length} scenarios · {sortedTierComplete(detail)} at max tier
    </span>
  </header>
  <div class="an-controls">
    <input
      class="search-input"
      type="text"
      placeholder="Filter scenarios…"
      bind:value={query}
    />
    <button
      class="btn btn-small"
      class:active={sortKey === 'weakest'}
      onclick={() => (sortKey = 'weakest')}>weakest</button
    >
    <button
      class="btn btn-small"
      class:active={sortKey === 'strongest'}
      onclick={() => (sortKey = 'strongest')}>strongest</button
    >
    <button
      class="btn btn-small"
      class:active={sortKey === 'name'}
      onclick={() => (sortKey = 'name')}>A→Z</button
    >
    <button class="btn btn-small capture" disabled={capturing} onclick={() => void captureReport()}>
      {capturing ? 'composing…' : 'copy report ⧉'}
    </button>
  </div>

  <section class="focus panel">
    <p>
      Every scenario of {detail.card.benchmark_name} on one tablet — PB
      progression, spread, and stagnation at a glance.
    </p>
  </section>

  <div class="an-grid">
    {#each ranked as s (s.scenario)}
      {@const r = detail.scenario_ranks.find((x) => x.scenario === s.scenario)}
      {@const sp = spark(s)}
      <article class="an-card">
        <h3 class="an-name display">{s.scenario}</h3>
        <div class="an-tierline">
          {#if r?.tier}<span class="tier" style={`color:${r.tier.color}`}>{r.tier.name}</span>{:else}<span class="tier faint">unplayed</span>{/if}
          {#if r && r.rank_maxes.length > 0}
            <div class="tierbar" aria-hidden="true">
              {#each r.rank_maxes as _t, i (i)}
                <span class="run {i < r.scenario_rank ? 'filled' : ''}"></span>
              {/each}
            </div>
          {/if}
        </div>
        {#if sp}
          <svg viewBox={`0 0 ${sp.w} ${sp.h}`} class="spark" preserveAspectRatio="none" aria-hidden="true">
            <path d={sp.area} class="spark-area" />
            <path d={sp.path} class="spark-line" />
          </svg>
        {:else}
          <p class="faint an-none">fewer than 2 PBs</p>
        {/if}
        <p class="an-nums">
          {#if gapPct(s) !== null}
            <span class="up">+{gapPct(s)!.toFixed(1)}%</span>
          {/if}
          {#if s.plateau.cv}
            <span class="cv" title="How steady your recent runs are — 0% is rock-steady, higher = up-and-down">spread {s.plateau.cv.toFixed(1)}%</span>
          {/if}
          {#if s.plateau.plateaued}
            <span class="plat">plateaued {s.plateau.days_since_pb}d</span>
          {/if}
        </p>
      </article>
    {/each}
  </div>
{/if}

<style>
  .an-head {
    display: flex;
    align-items: center;
    gap: 12px;
    margin: 14px 0 6px;
  }

  .back {
    background: none;
    border: 1px solid var(--border);
    border-radius: var(--radius, 0.25rem);
    color: var(--text);
    cursor: pointer;
    padding: 6px 12px;
  }

  .back:hover {
    border-color: var(--accent);
    color: var(--accent);
  }

  .back:focus-visible,
  .an-controls .btn:focus-visible {
    outline: none;
    box-shadow: inset 0 0 0 2px color-mix(in srgb, var(--accent) 60%, transparent);
  }

  .an-sub {
    color: var(--faint);
    font-size: 0.82rem;
    letter-spacing: 0.04em;
  }

  .an-controls {
    display: flex;
    gap: 8px;
    align-items: center;
    margin: 8px 0 12px;
  }

  .an-controls .btn.active {
    border-color: var(--accent);
    color: var(--accent);
  }

  .focus {
    margin-bottom: 14px;
    padding: 10px 14px;
  }

  .focus p {
    margin: 0;
    color: var(--faint);
    font-size: 0.85rem;
  }

  .an-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(240px, 1fr));
    gap: 12px;
  }

  .an-card {
    background: var(--card);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    padding: 10px 12px 12px;
    position: relative;
    overflow: hidden;
  }

  .an-card::after {
    content: '';
    position: absolute;
    inset: 0;
    pointer-events: none;
    border-radius: inherit;
    opacity: 0.55;
    mix-blend-mode: multiply;
    background-image:
      radial-gradient(140% 60% at 60% 30%, transparent 52%, color-mix(in srgb, #7d715c 26%, transparent) 78%, transparent 96%),
      repeating-linear-gradient(15deg, transparent 0 10px, color-mix(in srgb, #8a7f6d 10%, transparent) 10px 11.5px, transparent 11.5px 24px);
  }

  .an-name {
    margin: 0 0 4px;
    font-size: 0.92rem;
    letter-spacing: 0.02em;
  }

  .an-tierline {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 6px;
  }

  .tier {
    font-size: 0.78rem;
    font-weight: 700;
  }

  .tier.faint {
    font-weight: 400;
    color: var(--faint);
  }

  .tierbar {
    display: flex;
    gap: 2px;
  }

  .run {
    width: 10px;
    height: 5px;
    border: 1px solid var(--border);
    border-radius: 1px;
  }

  .run.filled {
    background: var(--accent);
    border-color: var(--accent);
  }

  .spark {
    display: block;
    width: 100%;
    height: 44px;
  }

  .spark-line {
    fill: none;
    stroke: var(--accent-2);
    stroke-width: 1.6;
  }

  .spark-area {
    fill: color-mix(in srgb, var(--accent-2) 18%, transparent);
    stroke: none;
  }

  .an-nums {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
    margin: 6px 0 0;
    font-size: 0.76rem;
  }

  .up {
    color: var(--laurel, #5e8a3c);
  }

  .cv,
  .plat {
    color: var(--faint);
  }

  .plat {
    color: var(--warn, #b0563a);
  }
</style>
