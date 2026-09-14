<script lang="ts">
  import {
    dashboardRollup,
    type DashboardFamilies,
    type FamilyRow,
    type FamilyDiff,
  } from './api'

  let { onback, onselect }: { onback: () => void; onselect: (id: number) => void } = $props()

  let data = $state<DashboardFamilies | null>(null)
  let error = $state<string | null>(null)
  let expanded = $state<Set<string>>(new Set())

  $effect(() => {
    dashboardRollup()
      .then((r) => (data = r))
      .catch((e) => (error = String(e)))
  })

  const TIER_COLORS: Record<string, string> = {
    Recruit: '#8d99a6', Iron: '#999999', Bronze: '#ff9900', Silver: '#cbd9e6',
    Gold: '#cab148', Platinum: '#4fd1c5', Diamond: '#48bbf7', Master: '#b560f0',
    Grandmaster: '#ff2e88', Nova: '#7ce4a8', Astra: '#7ce4a8',
    Berry: '#9070d8', Pear: '#8ad05f', Cherry: '#e14b65', ETH: '#6c8bd8', BTC: '#f2a63e',
  }
  const tierColor = (tier: string): string => TIER_COLORS[tier] ?? '#566b85'

  function peak(row: FamilyRow): { rank: number; tier: string } {
    let best: { idx: number; rank: number; tier: string } | null = null
    for (const d of row.difficulties) {
      const idx = d.tier_names.indexOf(d.current_tier)
      if (idx >= 0 && (!best || idx > best.idx)) {
        best = { idx, rank: d.current_rank, tier: d.current_tier }
      }
    }
    return best ? { rank: best.rank, tier: best.tier } : { rank: -1, tier: '—' }
  }

  function toggle(name: string) {
    const next = new Set(expanded)
    if (next.has(name)) next.delete(name)
    else next.add(name)
    expanded = next
  }

  function openFirst(diffs: FamilyDiff[]): number | null {
    return diffs[0]?.kovaaks_id ?? null
  }
</script>

<section class="dashboard-page">
  <header class="dash-head">
    <button class="btn" onclick={onback}>← Back</button>
    <h2 class="dash-title">DASHBOARD</h2>
  </header>

  {#if error}
    <div class="empty">Dashboard unavailable: {error}</div>
  {:else if !data}
    <div class="empty">Loading dashboard…</div>
  {:else}
    <div class="banking">
      {#each data.families as fam (fam.benchmark_name)}
        {@const peakTier = peak(fam)}
        <div class="fam">
          <button
            class="fam-row"
            onclick={() => toggle(fam.benchmark_name)}
            aria-expanded={expanded.has(fam.benchmark_name)}
          >
            <span class="chev">{expanded.has(fam.benchmark_name) ? '▾' : '▸'}</span>
            <span class="dot" style={`background:${fam.color}`}></span>
            <span class="name">{fam.benchmark_name}</span>
            <span class="tier" style={`color:${tierColor(peakTier.tier)}`}>
              {peakTier.tier}
            </span>
            <span class="diff-count">{fam.difficulties.length}</span>
          </button>
          {#if expanded.has(fam.benchmark_name)}
            <div class="diffs">
              {#each fam.difficulties as d (d.kovaaks_id)}
                <button class="diff-row" onclick={() => onselect(d.kovaaks_id)}>
                  <span class="diff-name">{d.difficulty_name}</span>
                  {#if d.tier_names.length > 0}
                    <span class="rung-bar">
                      {#each d.tier_names as t, i}
                        <span
                          class="rung"
                          class:lit={i <= d.current_rank}
                          style={`--c:${tierColor(t)}`}
                          title={t}
                        ></span>
                      {/each}
                    </span>
                  {/if}
                  <span class="tier" style={`color:${tierColor(d.current_tier)}`}>
                    {d.current_tier}
                  </span>
                </button>
              {/each}
            </div>
          {/if}
        </div>
      {/each}
    </div>
  {/if}
</section>

<style>
  .dashboard-page {
    padding: 0 18px 28px;
  }
  .dash-head {
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 10px 0 14px;
  }
  .dash-title {
    font-size: 17px;
    letter-spacing: 0.14em;
    color: var(--foreground);
    margin: 0;
  }
  .empty {
    color: var(--muted-foreground);
    padding: 24px 0;
  }
  .banking {
    display: flex;
    flex-direction: column;
    gap: 4px;
    max-width: 720px;
  }
  .fam-row,
  .diff-row {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    text-align: left;
    border: 1px solid var(--border);
    background: var(--card);
    border-radius: 6px;
    padding: 7px 12px;
    cursor: pointer;
    color: var(--foreground);
  }
  .fam-row:hover {
    border-color: var(--accent);
  }
  .dot {
    width: 10px;
    height: 10px;
    border-radius: 3px;
    flex: none;
  }
  .name {
    flex: 1;
    font-size: 13px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .diff-count {
    font-size: 11px;
    color: var(--muted-foreground);
  }
  .diffs {
    display: flex;
    flex-direction: column;
    gap: 3px;
    padding: 3px 0 6px 26px;
  }
  .diff-name {
    min-width: 170px;
    font-size: 12.5px;
    color: var(--muted-foreground);
  }
  .rung-bar {
    flex: 1;
    display: flex;
    gap: 3px;
  }
  .rung {
    flex: 1;
    height: 4px;
    border-radius: 2px;
    background: color-mix(in srgb, var(--c) 18%, transparent);
  }
  .rung.lit {
    background: var(--c);
  }
  .tier {
    font-weight: 700;
    font-size: 12px;
    min-width: 92px;
    text-align: right;
  }
</style>
