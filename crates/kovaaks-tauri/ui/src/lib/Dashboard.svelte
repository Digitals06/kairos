<script lang="ts">
  import { dashboardRollup, type DashboardRollup, type DashboardRow } from './api'

  let { onback, onselect }: { onback: () => void; onselect: (id: number) => void } = $props()

  let data = $state<DashboardRollup | null>(null)
  let error = $state<string | null>(null)

  $effect(() => {
    dashboardRollup()
      .then((r) => (data = r))
      .catch((e) => (error = String(e)))
  })

  // Deterministic tier colors from the benchmark's ladder order (fallback hues
  // when the ladder names miss the seeded palette).
  const TIER_COLORS: Record<string, string> = {
    Recruit: '#8d99a6', Iron: '#999999', Bronze: '#ff9900', Silver: '#cbd9e6',
    Gold: '#cab148', Platinum: '#4fd1c5', Diamond: '#48bbf7', Master: '#b560f0',
    Grandmaster: '#ff2e88', Nova: '#7ce4a8', Astra: '#7ce4a8',
    Berry: '#9070d8', Pear: '#8ad05f', Cherry: '#e14b65', ETH: '#6c8bd8', BTC: '#f2a63e',
  }
  const tierColor = (row: DashboardRow, tier: string): string =>
    TIER_COLORS[tier] ?? '#566b85'
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
    {#each data.sections as section}
      <div class="cat-section">
        <div class="cat-head">
          <h3>{section.category}</h3>
          <span class="count">{section.benchmarks.length} benchmarks</span>
        </div>
        <div class="banking">
          {#each section.benchmarks as row (row.benchmark_id)}
            <button class="bank-card" onclick={() => onselect(row.benchmark_id)}>
              <span class="name">{row.benchmark_name}</span>
              <span class="tier" style={`color:${tierColor(row, row.current_tier)}`}>
                {row.current_tier}
              </span>
              {#if row.tier_names.length > 0}
                <div class="rung-bar">
                  {#each row.tier_names as t, i}
                    <span
                      class="rung"
                      class:lit={i <= row.current_rank}
                      style={`--c:${tierColor(row, t)}`}
                      title={t}
                    ></span>
                  {/each}
                </div>
              {/if}
            </button>
          {/each}
        </div>
      </div>
    {/each}
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
  .cat-section {
    margin-bottom: 22px;
  }
  .cat-head {
    display: flex;
    align-items: baseline;
    gap: 10px;
    margin-bottom: 8px;
  }
  .cat-head h3 {
    margin: 0;
    font-size: 14px;
    text-transform: uppercase;
    letter-spacing: 0.1em;
    color: var(--accent);
  }
  .count {
    color: var(--muted-foreground);
    font-size: 12px;
  }
  .banking {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
  .bank-card {
    display: grid;
    grid-template-columns: 1fr auto;
    align-items: center;
    gap: 4px 10px;
    text-align: left;
    border: 1px solid var(--border);
    background: var(--card);
    border-radius: 7px;
    padding: 8px 12px;
    cursor: pointer;
    min-width: 210px;
    color: var(--foreground);
  }
  .bank-card:hover {
    border-color: var(--accent);
  }
  .name {
    font-size: 13px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .tier {
    grid-row: 1;
    font-weight: 700;
    font-size: 12px;
  }
  .rung-bar {
    grid-column: 1 / -1;
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
</style>
