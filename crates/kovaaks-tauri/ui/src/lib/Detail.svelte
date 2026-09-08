<script lang="ts">
  import Chart from 'chart.js/auto'
  import { getBenchmarkDetail, grindNext, type BenchmarkDetail, type GrindNext } from '../lib/api'
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

  // --- scenario table (API document order — same as evxl) ---------------------
  const scenarios = $derived(detail?.scenario_ranks ?? [])
  // One scenario is ALWAYS selected (defaults to the first with data); the
  // chart and the stat cards both reflect it. When the payload changes
  // (different benchmark), reset to that benchmark's first scenario.
  let chartScope = $state('')

  $effect(() => {
    // reset when the detail payload identity changes
    const d = detail
    if (d && !d.scenario_history.some((s) => s.scenario === chartScope)) {
      chartScope = d.scenario_history.find((s) => s.points.length > 0)?.scenario
        ?? d.scenario_history[0]?.scenario
        ?? ''
    }
  })

  const historyOptions = $derived.by(() => {
    if (!detail) return []
    return detail.scenario_history
      .filter((s) => s.points.length > 0)
      .map((s) => ({ scenario: s.scenario, category: s.category, n: s.points.length }))
  })

  function scopeTitle(): string {
    return chartScope ? `Score history — ${chartScope}` : 'Score history'
  }

  // --- stat cards follow the selected scenario --------------------------------
  const activeMetrics = $derived.by(() => {
    if (!detail || !chartScope) return null
    return detail.scenario_metrics?.[chartScope] ?? null
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
  let grind = $state<GrindNext | null>(null)

  // "What to grind next": engine-computed per-scenario targets for this benchmark.
  $effect(() => {
    const id = detail?.card.benchmark_id
    if (!id) {
      grind = null
      return
    }
    grindNext(id)
      .then((g) => (grind = g))
      .catch(() => (grind = null))
  })
  let lineChart: Chart | undefined

  $effect(() => {
    const d = detail
    if (!d || !lineCanvas) return

    // Series: the selected scenario only (chart is always scenario-scoped).
    if (!chartScope) return
    const series = d.scenario_history.find((s) => s.scenario === chartScope)
    if (!series || series.points.length === 0) return
    // The backend owns the merged run history (local plays + non-echo
    // snapshot new-highs, same-run rule included) — points are plotted
    // verbatim. No client-side dedupe or merging happens here.
    const trend = series.points.map((p) => ({
      x: new Date(p.captured_at).getTime(),
      y: p.score,
      fromPlay: p.from_play,
    }))
    const seriesSource = series.source
    // Magenta dots mark play-sourced runs; hidden when every point is a
    // play (local-backed series — the cyan line already is those plays).
    const playPts = trend.filter((p) => p.fromPlay)
    const snapPts = trend.filter((p) => !p.fromPlay)
    const showPlayDots = snapPts.length > 0 && playPts.length > 0

    let high = -Infinity
    const highPts = trend.map((p) => ({ x: p.x, y: (high = Math.max(high, p.y)) }))
    const rolling = trend.map((p) => {
      const win = trend.filter((q) => q.x > p.x - DAY_MS && q.x <= p.x)
      return { x: p.x, y: win.reduce((s, q) => s + q.y, 0) / win.length }
    })
    // Loop-invariant: sorted trend ⇒ first/last points bound the span.
    const span = trend.length > 1 ? trend[trend.length - 1].x - trend[0].x : 0

    lineChart = new Chart(lineCanvas, {
      type: 'line',
      data: {
        datasets: [
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
            stepped: 'before',
          },
          {
            // Merged run line. Play-sourced points get a bigger magenta
            // marker (scriptable per-point color) — they sit exactly on the
            // line now that the backend owns the merge, so a separate
            // scatter dataset would paint under/over ambiguously.
            label: 'runs',
            data: trend,
            borderColor: CYAN,
            backgroundColor: CYAN,
            borderWidth: 2,
            pointRadius: (ctx) =>
              ctx.dataset.data[ctx.dataIndex]?.fromPlay ? 4 : 2.5,
            pointHoverRadius: 5,
            pointBackgroundColor: (ctx) =>
              ctx.dataset.data[ctx.dataIndex]?.fromPlay ? MAGENTA : CYAN,
            pointBorderColor: (ctx) =>
              ctx.dataset.data[ctx.dataIndex]?.fromPlay ? MAGENTA : CYAN,
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
            ticks: {
              maxTicksLimit: 8,
              // Sub-day spans need clock labels — repeating "Sep 3" eight
              // times makes a 5-minute chart look like a full day.
              callback: (v) => {
                if (span < 20 * 3600 * 1000) {
                  return new Date(Number(v)).toLocaleTimeString([], {
                    hour: '2-digit',
                    minute: '2-digit',
                  })
                }
                return fmtDay(Number(v))
              },
            },
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
      <RankBadge tier={detail.card.rank} />
    </header>

    <div class="stat-row">
      {#if activeMetrics}
        <div class="stat-card">
          <span class="stat-label">Avg Score</span>
          <span class="stat-value num">{fmtScore(activeMetrics.avg_score)}</span>
        </div>
        <div class="stat-card">
          <span class="stat-label">High Score</span>
          <span class="stat-value num">{fmtScore(activeMetrics.high_score)}</span>
        </div>
        <div class="stat-card">
          <span class="stat-label">Avg Improvement % (30d)</span>
          <span class="stat-value num" class:up={(activeMetrics.avg_improvement_pct ?? 0) > 0}>
            {fmtPct(activeMetrics.avg_improvement_pct)}
          </span>
        </div>
        <div class="stat-card">
          <span class="stat-label">High Improvement % (30d)</span>
          <span class="stat-value num" class:up={(activeMetrics.high_improvement_pct ?? 0) > 0}>
            {fmtPct(activeMetrics.high_improvement_pct)}
          </span>
        </div>
      {:else}
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
      {/if}
    </div>

    {#if grind && !grind.complete}
      <section class="panel grind-panel">
        <h3>What to grind next</h3>
        <p class="grind-head">
          <span class="grind-current">{grind.currentRank}</span>
          <span class="grind-arrow">→</span>
          <span class="grind-next">{grind.nextRank}</span>
        </p>
        {#if grind.targets.length === 0 && grind.plan.length === 0}
          <p class="grind-note">
            No single-scenario path to the next rank — raise several scenarios
            together.
          </p>
        {:else if grind.targets.length === 0}
          <p class="grind-note">
            No single scenario gets you there — work down this step-by-step
            plan (all scores within each scenario's ladder):
          </p>
          <ul class="grind-list">
            {#each grind.plan.slice(0, 8) as t, i (i)}
              <li>
                <span class="grind-scenario">
                  {i + 1}. {t.scenario}
                </span>
                <span class="num grind-scores">
                  {t.currentScore.toLocaleString()} →
                  {t.targetScore.toLocaleString()}
                  <span class="grind-delta">(+{t.delta.toLocaleString()})</span>
                </span>
              </li>
            {/each}
            {#if grind.plan.length > 8}
              <li class="grind-more">+{grind.plan.length - 8} more…</li>
            {/if}
          </ul>
        {:else}
          <ul class="grind-list">
            {#each grind.targets.slice(0, 5) as t (t.scenario)}
              <li>
                <span class="grind-scenario">{t.scenario}</span>
                <span class="num grind-scores">
                  {t.currentScore.toLocaleString()} →
                  {t.targetScore.toLocaleString()}
                  <span class="grind-delta">(+{t.delta.toLocaleString()})</span>
                </span>
              </li>
            {/each}
            {#if grind.targets.length > 5}
              <li class="grind-more">+{grind.targets.length - 5} more…</li>
            {/if}
          </ul>
          <p class="grind-note">
            Minimal score per scenario to reach {grind.nextRank} (others held).
          </p>
        {/if}
      </section>
    {/if}

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
              aria-label="Select scenario"
            >
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

    <section class="panel table-panel">
      <div class="table-head">
        <h3>Scenarios</h3>
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
