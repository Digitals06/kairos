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

  const scenarios = $derived.by(() => {
    const rows = detail?.scenario_ranks ?? []
    return [...rows].sort((a, b) => (scoreDesc ? b.score - a.score : a.score - b.score))
  })

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
    if (!d || !lineCanvas || d.snapshot_history.length === 0) return

    const snaps = d.snapshot_history.map((p) => ({
      x: new Date(p.captured_at).getTime(),
      y: p.benchmark_progress,
    }))
    let high = -Infinity
    const highPts = snaps.map((p) => ({ x: p.x, y: (high = Math.max(high, p.y)) }))
    const rolling = snaps.map((p) => {
      const win = snaps.filter((q) => q.x > p.x - DAY_MS && q.x <= p.x)
      return { x: p.x, y: win.reduce((s, q) => s + q.y, 0) / win.length }
    })
    const playPts = d.plays.map((p) => ({ x: new Date(p.played_at).getTime(), y: p.score }))

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
        <h3>Progress history</h3>
        <div class="chart-box">
          <canvas bind:this={lineCanvas}></canvas>
        </div>
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
        <table class="detail-table">
          <thead>
            <tr>
              <th>Scenario</th>
              <th class="r">Score</th>
              <th class="r">Leaderboard</th>
              <th>Tier</th>
            </tr>
          </thead>
          <tbody>
            {#each scenarios as s (s.scenario)}
              <tr>
                <td>{s.scenario}</td>
                <td class="r num">{s.score.toLocaleString()}</td>
                <td class="r num">#{s.leaderboard_rank.toLocaleString()}</td>
                <td>
                  {#if s.tier}
                    <RankBadge tier={s.tier} progress={s.score} />
                  {:else}
                    <span class="muted">—</span>
                  {/if}
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}
    </section>
  {/if}
</div>
