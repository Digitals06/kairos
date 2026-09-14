<script lang="ts">
  import { weeklyReport, type WeeklyReport } from './api'

  // Session cache: the report is expensive (~many merged series). Fetch once
  // per app run and reuse it whenever the overview remounts; the user can
  // force a refresh explicitly. (Req: no reload churn when navigating
  // in/out of benchmarks.)
  let cached: WeeklyReport | null = null
  let inflight: Promise<WeeklyReport> | null = null

  let report = $state<WeeklyReport | null>(cached)
  let error = $state<string | null>(null)
  let refreshing = $state(false)

  async function load(force = false) {
    if (cached && !force) {
      report = cached
      return
    }
    if (!inflight) {
      inflight = weeklyReport()
    }
    const p = inflight
    try {
      const r = await p
      // Only apply if this is still the newest request.
      if (inflight === p) cached = r
      report = r
      error = null
    } catch (e) {
      if (cached) report = cached
      else error = String(e)
    } finally {
      if (inflight === p) inflight = null
    }
  }

  $effect(() => {
    load()
  })

  async function refresh() {
    refreshing = true
    await load(true)
    refreshing = false
  }
</script>

<section class="weekly">
  {#if error}
    <div class="empty">Weekly report unavailable: {error}</div>
  {:else if !report}
    <div class="empty">Loading this week…</div>
  {:else}
    <div class="row">
      <span class="stat"><b>{report.plays}</b> plays · <b>{report.days_played}/7</b> days ·
        <b>{report.current_streak}</b> streak · <b>{report.xp.toLocaleString()}</b> XP
      </span>
      <span class="ranks">
        {#each report.rank_changes as rc (rc.benchmark_id + rc.from)}
          <span class="rank-chip" title="{rc.benchmark}: {rc.from} → {rc.to}">
            {rc.benchmark} <s>{rc.from}</s> → <b>{rc.to}</b>
          </span>
        {/each}
      </span>
      <button class="mini" onclick={refresh} disabled={refreshing}>
        {refreshing ? '…' : '↻'}
      </button>
    </div>
  {/if}
</section>

<style>
  .weekly {
    border: 1px solid var(--border, #1d2733);
    border-radius: 8px;
    padding: 8px 12px;
    background: var(--card, #0d1420);
    margin-bottom: 10px;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
  }
  .stat {
    color: var(--muted-foreground, #6b7b8d);
    font-size: 13px;
  }
  .stat b {
    color: var(--accent, #00e5ff);
    font-weight: 600;
  }
  .ranks {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
    flex: 1;
  }
  .rank-chip {
    font-size: 12px;
    color: var(--foreground, #dbe4ee);
    border: 1px solid var(--border, #1d2733);
    border-radius: 5px;
    padding: 2px 8px;
    white-space: nowrap;
  }
  .rank-chip s {
    color: var(--muted-foreground, #6b7b8d);
  }
  .rank-chip b {
    color: var(--accent, #00e5ff);
  }
  .mini {
    border: 1px solid var(--border, #1d2733);
    background: transparent;
    color: var(--muted-foreground, #6b7b8d);
    border-radius: 5px;
    cursor: pointer;
    padding: 2px 8px;
    font-size: 12px;
  }
  .empty {
    color: var(--muted-foreground, #6b7b8d);
    font-size: 13px;
  }
</style>
