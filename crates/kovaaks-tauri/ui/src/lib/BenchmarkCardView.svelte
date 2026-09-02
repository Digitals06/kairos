<script lang="ts">
  import type { BenchmarkCard, SnapshotPoint } from '../api'
  import RankBadge from './RankBadge.svelte'

  let {
    card,
    history = [],
    onclick,
  }: {
    card: BenchmarkCard
    history?: SnapshotPoint[]
    onclick?: () => void
  } = $props()

  // Progress-to-next-rank as a bar fraction: the previous rung is derived from
  // the delta (delta = threshold - progress); progress/(progress+delta) is the
  // fraction of the current rung climbed.
  const barPct = $derived.by(() => {
    const delta = card.next_rank_delta
    if (delta === null || delta < 0) return 100
    const denom = card.benchmark_progress + delta
    return denom > 0 ? Math.min(100, (card.benchmark_progress / denom) * 100) : 0
  })

  function fmtScore(n: number): string {
    return n.toLocaleString(undefined, { maximumFractionDigits: 0 })
  }

  function fmtPct(n: number | null): string {
    return n === null ? '—' : `${n > 0 ? '+' : ''}${n.toFixed(1)}%`
  }

  // --- sparkline geometry (inline SVG) -------------------------------------
  const SPARK_W = 180
  const SPARK_H = 40
  const PAD = 3

  const spark = $derived.by(() => {
    const pts = history
    if (pts.length < 2) return null
    const xs = pts.map((p) => new Date(p.captured_at).getTime())
    const ys = pts.map((p) => p.benchmark_progress)
    const xMin = Math.min(...xs)
    const xMax = Math.max(...xs)
    const yMin = Math.min(...ys)
    const yMax = Math.max(...ys)
    const dx = xMax - xMin || 1
    const dy = yMax - yMin || 1
    const at = (i: number): [number, number] => [
      PAD + ((xs[i] - xMin) / dx) * (SPARK_W - 2 * PAD),
      SPARK_H - PAD - ((ys[i] - yMin) / dy) * (SPARK_H - 2 * PAD),
    ]
    const line = pts.map((_, i) => (i === 0 ? 'M' : 'L') + at(i).join(',')).join(' ')
    const [x0, y0] = at(0)
    const [xN, yN] = at(pts.length - 1)
    return {
      line,
      area: `${line} L${xN},${SPARK_H - 1} L${x0},${SPARK_H - 1} Z`,
      dot: [xN, yN] as [number, number],
    }
  })
</script>

<article class="panel card" role="button" tabindex="0" onclick={onclick} onkeydown={(e) => {
  if (e.key === 'Enter' || e.key === ' ') {
    e.preventDefault()
    onclick?.()
  }
}}>
  <header>
    <div class="names">
      <h3 title={card.benchmark_name}>{card.benchmark_name}</h3>
      <span class="abbr num">{card.abbreviation}</span>
    </div>
    <RankBadge tier={card.rank} progress={card.benchmark_progress} />
  </header>

  <div class="progress-row">
    <div class="bar" role="progressbar" aria-valuenow={card.benchmark_progress}>
      <div class="fill" style={`width: ${barPct}%`}></div>
    </div>
    <span class="next num">
      {#if card.next_rank_name !== null && card.next_rank_delta !== null}
        +{card.next_rank_delta.toLocaleString()} to {card.next_rank_name}
      {:else if card.rank}
        top tier
      {:else}
        —
      {/if}
    </span>
  </div>

  <div class="scores num">
    <span class="chip" title="Average score">avg {fmtScore(card.avg_score)}</span>
    <span class="chip" title="High score">high {fmtScore(card.high_score)}</span>
    <span
      class="chip"
      class:up={(card.avg_improvement_pct ?? 0) > 0}
      title="Average improvement"
    >
      Δ {fmtPct(card.avg_improvement_pct)}
    </span>
    <span class="chip" title="Samples">{card.samples}</span>
  </div>

  {#if spark}
    <svg
      class="spark"
      viewBox="0 0 {SPARK_W} {SPARK_H}"
      preserveAspectRatio="none"
      aria-hidden="true"
    >
      <path d={spark.area} class="area" />
      <path d={spark.line} class="line" />
      <circle cx={spark.dot[0]} cy={spark.dot[1]} r="2.5" class="dot" />
    </svg>
  {/if}

  <footer class="meta num">
    <span>{card.difficulty_name}</span>
    <span>progress {card.benchmark_progress.toLocaleString()}</span>
  </footer>
</article>

<style>
  .card {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 16px;
    cursor: pointer;
    transition: border-color 0.15s, box-shadow 0.15s;
  }

  .card:hover {
    border-color: var(--accent-2);
    box-shadow: var(--glow-cyan);
  }

  header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 10px;
  }

  .names {
    min-width: 0;
  }

  h3 {
    font-size: 15px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .abbr {
    color: var(--accent-2);
    font-size: 11px;
    letter-spacing: 0.1em;
    text-transform: uppercase;
  }

  .progress-row {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .bar {
    flex: 1;
    height: 6px;
    border-radius: 3px;
    background: var(--bg);
    border: 1px solid var(--border);
    overflow: hidden;
  }

  .fill {
    height: 100%;
    background: linear-gradient(90deg, var(--accent), var(--accent-2));
    box-shadow: 0 0 8px rgba(0, 229, 255, 0.5);
  }

  .next {
    font-size: 11px;
    color: var(--muted);
    white-space: nowrap;
  }

  .scores {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .chip {
    padding: 2px 8px;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 6px;
    font-size: 11px;
    color: var(--text);
  }

  .chip.up {
    color: var(--success);
    border-color: rgba(16, 185, 129, 0.4);
  }

  .spark {
    width: 100%;
    height: 40px;
  }

  .spark .line {
    fill: none;
    stroke: var(--accent-2);
    stroke-width: 1.5;
    filter: drop-shadow(0 0 3px rgba(0, 229, 255, 0.6));
  }

  .spark .area {
    fill: rgba(0, 229, 255, 0.08);
    stroke: none;
  }

  .spark .dot {
    fill: var(--accent);
    filter: drop-shadow(0 0 4px rgba(255, 46, 136, 0.8));
  }

  .meta {
    display: flex;
    justify-content: space-between;
    font-size: 11px;
    color: var(--muted);
  }
</style>
