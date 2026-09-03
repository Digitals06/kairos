<script lang="ts">
  import Chart from 'chart.js/auto'
  import { getBenchmarkDetail, type BenchmarkDetail } from '../lib/api'
  import RankBadge from './RankBadge.svelte'

  let { benchmarkId, onback }: { benchmarkId: number; onback: () => void } = $props()

  // --- detail payload --------------------------------------------------------
  let detail = $state<BenchmarkDetail | null>(null)
  let loadError = $state<string | null>(null)

  // Fetch keyed on the id only; detail/loadError writes stay untracked so the
  // effect never re-triggers itself.
  $effect(() => {
    const id = benchmarkId
    detail = null
    loadError = null
    let alive = true
    getBenchmarkDetail(id)
      .then((d) => {
        if (alive) detail = d
      })
      .catch((err) => {
        if (alive) loadError = String(err)
      })
    return () => {
      alive = false
    }
  })

  // --- formatting (stat values come straight from the card DTO) --------------
  function fmtScore(n: number): string {
    return n.toLocaleString(undefined, { maximumFractionDigits: 0 })
  }

  function fmtPct(n: number | null): string {
    return n === null ? '—' : `${n > 0 ? '+' : ''}${n.toFixed(1)}%`
  }

  function fmtDay(ts: number): string {
    return new Date(ts).toLocaleDateString(undefined, { month: 'short', day: 'numeric' })
  }

  // --- rank-up bar (same derivation as BenchmarkCardView) --------------------
  const hasRankup = $derived(
    detail !== null &&
      detail.card.next_rank_name !== null &&
      detail.card.next_rank_delta !== null &&
      detail.card.next_rank_delta >= 0,
  )

  const rankupPct = $derived.by(() => {
    if (!detail) return 0
    const delta = detail.card.next_rank_delta
    if (delta === null || delta < 0) return 0
    const denom = detail.card.benchmark_progress + delta
    return denom > 0 ? Math.min(100, (detail.card.benchmark_progress / denom) * 100) : 0
  })

  // --- scenario table: score sort toggle -------------------------------------
  let scoreDesc = $state(true)

  // --- per-scenario history picker -------------------------------------------
  // 'ALL' = benchmark aggregate (snapshot_history); otherwise a scenario name
  // scopes the chart to that scenario's per-snapshot scores (issue #3).
  const ALL = 'ALL'
  let chartScope = $state(ALL)

  const historyOptions = $derived.by(() => {
    if (!detail) return []
    return detail.scenario_history
      .filter((s) => s.points.length > 0)
      .map((s) => ({ scenario: s.scenario, category: s.category, n: s.points.length }))
  })

  function scopeTitle(): string {
    return chartScope === ALL
      ? 'Progress history'
      : `Score history — ${chartScope}`
  }

  const scenarios = $derived.by(() => {
    const rows = detail?.scenario_ranks ?? []
    return [...rows].sort((a, b) => (scoreDesc ? b.score - a.score : a.score - b.score))
  })

  // --- evxl-style rank threshold matrix ---------------------------------------
  // Each scenario row shows the score thresholds for every tier (from the
  // scenario's rank_maxes), colored by tier, with the achieved tiers shaded;
  // the score cell renders "score / top-threshold" like evxl's table.
  function wordWrap(name: string): string[] {
    const i = name.lastIndexOf(' ')
    return i > 0 ? [name.slice(0, i), name.slice(i + 1)] : [name]
  }

  function thresholdFor(rankMaxes: number[] | undefined, tierIdx: number): number | null {
    if (!rankMaxes || rankMaxes.length === 0) return null
    return tierIdx < rankMaxes.length ? rankMaxes[tierIdx] : null
  }

  function achievedIdx(row: { scenario_rank: number }): number {
    return row.scenario_rank - 1
  }

  async function copyName(name: string): Promise<void> {
    try {
      await navigator.clipboard.writeText(name)
    } catch {
      /* clipboard unavailable in some webviews; non-fatal */
    }
  }

  // --- main progress chart ----------------------------------------------------
  // Datasets: local CSV plays as an underlay scatter (magenta), then the
  // running-high step line, the 7-day rolling average, and the raw snapshot
  // line. Running high + rolling average are presentation-only series derived
  // from snapshot_history in TS; every plotted number originates from the DTO.
  const CYAN = '#00e5ff'
  const MAGENTA = '#ff2e88'
  const GREEN = '#10b981'
  const GREY = '#9ca3af'
  const GRID = '#1f2937'
  const DAY_MS = 7 * 24 * 60 * 60 * 1000

  let lineCanvas: HTMLCanvasElement | undefined = $state()
  let lineChart: Chart | undefined

  $effect(() => {
    const d = detail
    if (!d || !lineCanvas) return

    // Scoped series: ALL = benchmark aggregate; otherwise one scenario.
    let raw: { x: number; y: number }[]
    let playPts: { x: number; y: number }[]
    if (chartScope === ALL) {
      raw = d.snapshot_history.map((p) => ({
        x: new Date(p.captured_at).getTime(),
        y: p.benchmark_progress,
      }))
      playPts = d.plays.map((p) => ({ x: new Date(p.played_at).getTime(), y: p.score }))
    } else {
      const series = d.scenario_history.find((s) => s.scenario === chartScope)
      raw = (series?.points ?? []).map((p) => ({
        x: new Date(p.captured_at).getTime(),
        y: p.score,
      }))
      // Local plays for exactly this scenario.
      playPts = d.plays
        .filter((p) => p.scenario === chartScope)
        .map((p) => ({ x: new Date(p.played_at).getTime(), y: p.score }))
    }
    if (raw.length === 0) return

    let high = -Infinity
    const highPts = raw.map((p) => ({ x: p.x, y: (high = Math.max(high, p.y)) }))
    const rolling = raw.map((p) => {
      const win = raw.filter((q) => q.x > p.x - DAY_MS && q.x <= p.x)
      return { x: p.x, y: win.reduce((s, q) => s + q.y, 0) / win.length }
    })

    lineChart = new Chart(lineCanvas, {
      type: 'line',
      data: {
        datasets: [
          {
            label: 'local play',
            type: 'scatter',
            data: playPts,
            showLine: false,
            pointRadius: 3.5,
            pointHoverRadius: 5,
            pointBackgroundColor: MAGENTA,
            pointBorderColor: MAGENTA,
          },
          {
            label: 'running high',
            data: highPts,
            stepped: 'before',
            borderColor: GREEN,
            backgroundColor: 'rgba(16, 185, 129, 0.06)',
            borderWidth: 1.5,
            pointRadius: 0,
            fill: true,
          },
          {
            label: '7-day avg',
            data: rolling,
            borderColor: GREY,
            borderWidth: 1.5,
            borderDash: [4, 4],
            pointRadius: 0,
            tension: 0.3,
          },
          {
            label: 'sync snapshot',
            data: snaps,
            borderColor: CYAN,
            backgroundColor: CYAN,
            borderWidth: 2,
            pointRadius: 2.5,
            pointHoverRadius: 4,
            tension: 0.25,
          },
        ],
      },
      options: {
        responsive: true,
        maintainAspectRatio: false,
        color: GREY,
        interaction: { mode: 'nearest', intersect: false },
        scales: {
          x: {
            type: 'linear',
            grid: { color: GRID },
            ticks: { maxTicksLimit: 8, callback: (v) => fmtDay(Number(v)) },
          },
          y: {
            grid: { color: GRID },
            ticks: { callback: (v) => Number(v).toLocaleString() },
          },
        },
        plugins: {
          legend: { labels: { color: '#e5e7eb', boxWidth: 12 } },
          tooltip: {
            callbacks: {
              title: (items) =>
                items.length ? new Date(items[0].parsed.x).toLocaleString() : '',
              label: (item) =>
                `${item.dataset.label}: ${Math.round(item.parsed.y).toLocaleString()}`,
            },
          },
        },
      },
    })
    return () => {
      lineChart?.destroy()
      lineChart = undefined
    }
  })
  // --- weakness coach: radar + weakest-category banner (Task 2.4) -------------
  // The wire DTO (`CategoryCard`) ships only { name, progress, rank_tier } —
  // no per-category tier ladder or thresholds — so absolute normalization
  // (progress / max tier threshold) is not derivable client-side and the
  // backend does not pre-normalize. Normalize relatively instead: each value
  // is progress / max(progress across categories), a presentation-only series
  // exactly like the chart's running-high / rolling-average lines.
  const radarData = $derived.by(() => {
    const cats = detail?.categories ?? []
    const max = Math.max(0, ...cats.map((c) => c.progress))
    return cats.map((c) => (max > 0 ? c.progress / max : 0))
  })

  // Weakest = lowest tier. The payload has no numeric tier index, so progress
  // stands in as the tier proxy (spec: rank_tier *or* progress).
  const weakest = $derived.by(() => {
    const cats = detail?.categories ?? []
    if (cats.length < 2) return null
    return cats.reduce((min, c) => (c.progress < min.progress ? c : min))
  })

  let radarCanvas: HTMLCanvasElement | undefined = $state()
  let radarChart: Chart | undefined

  $effect(() => {
    const d = detail
    // A radar polygon needs >= 3 axes; 1-2 categories fall back to a list.
    if (!d || !radarCanvas || d.categories.length < 3) return

    radarChart = new Chart(radarCanvas, {
      type: 'radar',
      data: {
        labels: d.categories.map((c) => c.name),
        datasets: [
          {
            label: 'category progress (normalized)',
            data: radarData,
            borderColor: CYAN,
            backgroundColor: 'rgba(0, 229, 255, 0.12)',
            borderWidth: 2,
            pointBackgroundColor: CYAN,
            pointRadius: 3,
          },
        ],
      },
      options: {
        responsive: true,
        maintainAspectRatio: false,
        color: GREY,
        scales: {
          r: {
            min: 0,
            max: 1,
            grid: { color: GRID },
            angleLines: { color: GRID },
            pointLabels: { color: '#e5e7eb', font: { size: 11 } },
            ticks: {
              color: GREY,
              backdropColor: 'transparent',
              callback: (v) => `${Math.round(Number(v) * 100)}%`,
            },
          },
        },
        plugins: {
          legend: { display: false },
          tooltip: {
            callbacks: {
              label: (item) => {
                const cat = d.categories[item.dataIndex]
                return `${Math.round(item.parsed.r * 100)}% · ${cat.progress.toLocaleString()} pts`
              },
            },
          },
        },
      },
    })
    return () => {
      radarChart?.destroy()
      radarChart = undefined
    }
  })
</script>

<div class="detail">
  <div class="detail-topbar">
    <button class="btn" onclick={onback}>← Back</button>
    <span class="crumb muted">Overview / {detail?.card.benchmark_name ?? '…'}</span>
  </div>

  {#if loadError}
    <div class="empty-state panel">
      <p>Failed to load benchmark.</p>
      <p class="muted">{loadError}</p>
    </div>
  {:else if !detail}
    <div class="skeleton detail-skeleton"></div>
  {:else}
    <header class="detail-header panel">
      <div class="titles">
        <h2>{detail.card.benchmark_name}</h2>
        <span class="detail-difficulty">{detail.card.difficulty_name}</span>
      </div>
      <RankBadge tier={detail.card.rank} progress={detail.card.benchmark_progress} />
      {#if hasRankup}
        <div class="rankup">
          <div class="bar" role="progressbar" aria-valuenow={rankupPct}>
            <div class="fill" style={`width: ${rankupPct}%`}></div>
          </div>
          <span class="label num">
            +{detail.card.next_rank_delta?.toLocaleString()} to {detail.card.next_rank_name}
          </span>
        </div>
      {/if}
    </header>

    {#if weakest}
      <div class="weakness-banner" role="status">
        <span class="wk-label">Weakest category: <strong>{weakest.name}</strong></span>
        <span class="wk-suggestion">Prioritize {weakest.name} drills this week</span>
      </div>
    {/if}

    <div class="stat-row">
      <div class="stat-card">
        <span class="stat-label">Avg Score</span>
        <span class="stat-value num">{fmtScore(detail.card.avg_score)}</span>
      </div>
      <div class="stat-card">
        <span class="stat-label">High Score</span>
        <span class="stat-value num">{fmtScore(detail.card.high_score)}</span>
      </div>
      <div class="stat-card">
        <span class="stat-label">Avg Improvement % (30d)</span>
        <span class="stat-value num" class:up={(detail.card.avg_improvement_pct ?? 0) > 0}>
          {fmtPct(detail.card.avg_improvement_pct)}
        </span>
      </div>
      <div class="stat-card">
        <span class="stat-label">High Improvement % (30d)</span>
        <span class="stat-value num" class:up={(detail.card.high_improvement_pct ?? 0) > 0}>
          {fmtPct(detail.card.high_improvement_pct)}
        </span>
      </div>
    </div>

    {#if detail.snapshot_history.length === 0}
      <div class="empty-state panel">
        <p>No syncs yet — hit Sync Now.</p>
      </div>
    {:else}
      <section class="panel chart-panel">
        <div class="chart-head">
          <h3>{scopeTitle()}</h3>
          {#if historyOptions.length > 0}
            <select
              class="scope-select"
              bind:value={chartScope}
              aria-label="Chart scope: all scenarios or a single scenario"
            >
              <option value={ALL}>All scenarios (aggregate)</option>
              {#each historyOptions as opt (opt.scenario)}
                <option value={opt.scenario}>{opt.scenario} ({opt.n})</option>
              {/each}
            </select>
          {/if}
        </div>
        <div class="chart-box">
          <canvas bind:this={lineCanvas}></canvas>
        </div>
      </section>
    {/if}

    {#if detail.categories.length >= 3}
      <section class="panel chart-panel">
        <h3>Category radar</h3>
        <div class="chart-box radar-box">
          <canvas bind:this={radarCanvas}></canvas>
        </div>
      </section>
    {:else if detail.categories.length > 0}
      <section class="panel chart-panel">
        <h3>Categories</h3>
        <ul class="cat-list">
          {#each detail.categories as c (c.name)}
            <li>
              <span>{c.name}</span>
              <span class="num">{c.progress.toLocaleString()} pts</span>
            </li>
          {/each}
        </ul>
      </section>
    {/if}

    <section class="panel table-panel">
      <div class="table-head">
        <h3>Scenarios</h3>
        <button class="btn btn-small num" onclick={() => (scoreDesc = !scoreDesc)}>
          score {scoreDesc ? '↓' : '↑'}
        </button>
      </div>
      {#if scenarios.length === 0}
        <p class="muted">No scenarios in the latest snapshot.</p>
      {:else}
        <div class="table-scroll">
          <table class="detail-table bench-table">
            <thead>
              <tr>
                <th class="scenario-col">Scenario</th>
                <th class="score-col">Score</th>
                <th class="lb-col r num">#</th>
                {#each detail.rank_tiers as tier, i (tier.name)}
                  <th
                    class="rank-col num"
                    style={`color: ${tier.color};`}
                    title={`${tier.name} — score needed`}
                  >
                    {#each wordWrap(tier.name) as line, li (li)}
                      <span class="rank-header-line">{line}</span>
                    {/each}
                  </th>
                {/each}
              </tr>
            </thead>
            <tbody>
              {#each scenarios as s (s.scenario)}
                {@const achieved = achievedIdx(s)}
                {@const top = thresholdFor(s.rank_maxes, s.rank_maxes.length - 1)}
                <tr>
                  <td class="scenario-col">
                    <button
                      class="scenario-name"
                      title="Click to copy scenario name"
                      onclick={() => copyName(s.scenario)}
                    >{s.scenario}</button>
                  </td>
                  <td class="score-col num">
                    {#if s.score > 0}
                      <span class="score-pair">
                        <span class="score-value">{s.score.toLocaleString(undefined, { maximumFractionDigits: 0 })}</span>
                        {#if top !== null}
                          <span class="slash">&nbsp;/&nbsp;</span>
                          <span class="num muted">{top.toLocaleString(undefined, { maximumFractionDigits: 0 })}</span>
                        {/if}
                      </span>
                    {:else}
                      <span class="muted">—</span>
                    {/if}
                  </td>
                  <td class="lb-col r num">
                    {#if s.leaderboard_rank > 0}
                      #{s.leaderboard_rank.toLocaleString()}
                    {:else}
                      <span class="muted">—</span>
                    {/if}
                  </td>
                  {#each detail.rank_tiers as tier, i (tier.name)}
                    {@const thr = thresholdFor(s.rank_maxes, i)}
                    {@const isAchieved = achieved >= i}
                    <td
                      class="rank-col num rank-cell"
                      class:achieved={isAchieved}
                      style={`color: ${tier.color}; ${isAchieved && thr !== null ? `background: ${tier.color}22;` : ''}`}
                      title={thr !== null ? `${tier.name}: ${thr.toLocaleString()}+ points` : `${tier.name}: no threshold`}
                    >
                      {#if thr !== null}
                        {thr.toLocaleString(undefined, { maximumFractionDigits: 0 })}
                      {:else}
                        <span class="muted">·</span>
                      {/if}
                    </td>
                  {/each}
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {/if}
    </section>
  {/if}
</div>
