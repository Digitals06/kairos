<script lang="ts">
  import { weeklyReport, type WeeklyReport } from './api'

  let report = $state<WeeklyReport | null>(null)
  let error = $state<string | null>(null)

  $effect(() => {
    weeklyReport()
      .then((r) => (report = r))
      .catch((e) => (error = String(e)))
  })

  const arrow = (trend: number) => (trend > 0 ? '▲' : trend < 0 ? '▼' : '–')
  const deltaStr = (d: number) => (d > 0 ? `+${d.toFixed(1)}` : d.toFixed(1))
</script>

<section class="weekly">
  {#if error}
    <div class="empty">Weekly report unavailable: {error}</div>
  {:else if !report}
    <div class="empty">Loading this week…</div>
  {:else}
    <div class="stats">
      <div class="cell">
        <span class="num">{report.plays}</span>
        <span class="label">plays</span>
      </div>
      <div class="cell">
        <span class="num">{report.days_played}/7</span>
        <span class="label">days trained</span>
      </div>
      <div class="cell">
        <span class="num">{report.pb_events}</span>
        <span class="label">personal bests</span>
      </div>
      <div class="cell">
        <span class="num">{report.current_streak}</span>
        <span class="label">day streak</span>
      </div>
      <div class="cell">
        <span class="num">{report.xp.toLocaleString()}</span>
        <span class="label">XP</span>
      </div>
    </div>

    {#if report.rank_changes.length}
      <div class="ranks">
        {#each report.rank_changes as rc}
          <div class="rank-row">
            <span class="benchmark">{rc.benchmark}</span>
            <span class="from">{rc.from}</span>
            <span class="to">{rc.to}</span>
          </div>
        {/each}
      </div>
    {/if}

    {#if report.improvements.length}
      <div class="improvements">
        {#each report.improvements as imp}
          <div class="improve-row">
            <span class="scenario">{imp.scenario}</span>
            {#if imp.pb_this_week}<span class="pb-tag">PB</span>{/if}
            <span class="delta" class:down={imp.trend < 0} class:flat={imp.trend === 0}>
              {arrow(imp.trend)} {deltaStr(imp.delta)}
            </span>
          </div>
        {/each}
      </div>
    {/if}
  {/if}
</section>

<style>
  .weekly {
    border: 1px solid var(--border, #1d2733);
    border-radius: 8px;
    padding: 12px 14px;
    background: var(--card, #0d1420);
  }
  .empty {
    color: var(--muted-foreground, #6b7b8d);
    font-size: 13px;
  }
  .stats {
    display: flex;
    gap: 22px;
    flex-wrap: wrap;
  }
  .cell {
    display: flex;
    flex-direction: column;
    min-width: 64px;
  }
  .num {
    font-size: 20px;
    font-weight: 600;
    color: var(--accent, #00e5ff);
  }
  .label {
    font-size: 11px;
    color: var(--muted-foreground, #6b7b8d);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .ranks,
  .improvements {
    margin-top: 10px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .rank-row,
  .improve-row {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
  }
  .benchmark {
    color: var(--foreground, #dbe4ee);
    min-width: 140px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .from {
    color: var(--muted-foreground, #6b7b8d);
  }
  .to {
    color: var(--accent, #00e5ff);
    font-weight: 600;
  }
  .scenario {
    color: var(--foreground, #dbe4ee);
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .pb-tag {
    font-size: 10px;
    font-weight: 700;
    color: #0a0e14;
    background: var(--accent, #00e5ff);
    border-radius: 3px;
    padding: 1px 5px;
  }
  .delta {
    color: #38d67c;
    font-variant-numeric: tabular-nums;
  }
  .delta.down {
    color: #ff5470;
  }
  .delta.flat {
    color: var(--muted-foreground, #6b7b8d);
  }
</style>
