<script lang="ts">
  import { listen } from '@tauri-apps/api/event'
  import { playsCalendar, weeklyReport, type PlaysCalendar, type WeeklyReport } from './api'

  // Shared report from the overview (App prefetches it for the level frieze).
  let { shared = null }: { shared?: WeeklyReport | null } = $props()

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
    if (shared && !cached && !inflight) {
      cached = shared
      report = shared
      return
    }
    load()
  })

  async function refresh() {
    refreshing = true
    await load(true)
    await loadCalendar()
    refreshing = false
  }

  const DAY_LETTERS = ['Δ', 'Δ', 'E', 'T', 'Π', 'Π', 'Σ'] // Mon..Sun greek caps
  const ROMAN = ['0', 'I', 'II', 'III', 'IV', 'V', 'VI', 'VII', 'VIII', 'IX', 'X', 'XI', 'XII']

  // Bars for the torch strip: 7 slots (plays_per_day), lit ones get flame height.
  const torches = $derived.by(() => {
    if (!report) return []
    const days = report.plays_per_day.length ? [...report.plays_per_day] : [0, 0, 0, 0, 0, 0, 0]
    const max = Math.max(1, ...days)
    return days.map((n, i) => ({
      n,
      h: n > 0 ? 30 + (22 * n) / max : 10,
      lit: n > 0,
    }))
  })

  // --- streak calendar ------------------------------------------------------
  // Its own cheap command (one grouped query), NOT the weekly report: a fresh
  // play should light today's cell without rebuilding the merged-series view.
  let calendar = $state<PlaysCalendar | null>(null)
  let calendarError = $state<string | null>(null)
  let calendarReq = 0

  async function loadCalendar() {
    const req = ++calendarReq
    try {
      const r = await playsCalendar()
      if (req === calendarReq) {
        calendar = r
        calendarError = null
      }
    } catch (e) {
      if (req === calendarReq) calendarError = String(e)
    }
  }

  $effect(() => {
    void loadCalendar()
  })

  // New local plays -> refresh only the calendar, never the whole overview.
  $effect(() => {
    let unlisten: (() => void) | undefined
    listen<{ inserted: number }>('local-plays-updated', (e) => {
      if (e.payload.inserted > 0) void loadCalendar()
    }).then((fn) => (unlisten = fn))
    return () => unlisten?.()
  })

  type HeatCell = { date: string; n: number; level: number } | null

  // 7 weekday columns (Monday first); leading blanks keep every date under
  // its real weekday. Intensity is relative to the busiest day in the window.
  const heatCells = $derived.by(() => {
    const days = calendar?.days ?? []
    if (!days.length) return [] as HeatCell[]
    const first = new Date(`${days[0].date}T00:00:00Z`)
    const lead = (first.getUTCDay() + 6) % 7
    const max = Math.max(1, ...days.map((d) => d.plays))
    const cells: HeatCell[] = days.map((d) => {
      let level = 0
      if (d.plays > 0) {
        const ratio = d.plays / max
        level = ratio <= 0.25 ? 1 : ratio <= 0.5 ? 2 : ratio <= 0.75 ? 3 : 4
      }
      return { date: d.date, n: d.plays, level }
    })
    return [...Array<HeatCell>(lead).fill(null), ...cells]
  })

  const lastDay = $derived(calendar?.days.length ? calendar.days[calendar.days.length - 1].date : '')

</script>

<section class="weekly">
  {#if error}
    <div class="empty">Weekly report unavailable: {error}</div>
  {:else if !report}
    <div class="empty">Loading this week…</div>
  {:else}
    <h2 class="display">This week <em>— ἑβδομάς</em></h2>

    <div class="days">
      <span class="big num">{report.days_played}<span>/7 days</span></span>
      <span class="torches" title="Plays per day, trailing 7 days">
        {#each torches as day, i (i)}
          <span class="torch" class:lit={day.lit} style={`--h:${day.h}px`} title={`${day.n} played`}>
            <i class="flame"></i>
          </span>
        {/each}
      </span>
    </div>

    <div class="calendar">
      <span class="calendar-title">Plays per day — last 6 weeks</span>
      {#if calendar}
        <div class="heatmap" role="img" aria-label="Plays per day over the last six weeks">
          {#each heatCells as cell, i (i)}
            {#if cell}
              <span
                class="cell l{cell.level}"
                class:today={cell.date === lastDay}
                title={`${cell.n} played`}
              ></span>
            {:else}
              <span class="cell blank"></span>
            {/if}
          {/each}
        </div>
      {:else if calendarError}
        <span class="calendar-empty">Plays calendar unavailable</span>
      {:else}
        <span class="calendar-empty">Loading plays…</span>
      {/if}
    </div>

    <p class="sub">
      <b class="num">{report.plays}</b> scored runs ·
      streak <b class="num">{report.current_streak}</b> ·
      presently at <b>{report.level_name}</b> ({report.level_progress_pct}%)
      <button class="mini" onclick={refresh} disabled={refreshing} aria-label="refresh week">
        {refreshing ? '…' : '↻'}
      </button>
    </p>

    {#if report.rank_changes.length}
      <div class="ranks">
        {#each report.rank_changes as rc (rc.benchmark_id + rc.from)}
          <span class="rank-chip" title="{rc.benchmark}: {rc.from} → {rc.to}">
            {rc.benchmark} <s>{rc.from}</s> <i>→</i> <b>{rc.to}</b>
          </span>
        {/each}
      </div>
    {/if}

    {#if report.level_steps?.length}
      <div class="frieze" role="img" aria-label="Level frieze — the Twelve Steps">
        {#each report.level_steps as step, i (step.name)}
          <span
            class="step"
            class:done={step.threshold <= report!.xp}
            class:here={i + 1 === report!.level}
            title={`${step.name} — ${step.threshold.toLocaleString()} XP`}
          >
            <i>{step.name}</i>
            {#if i + 1 === report!.level}<em class="bar"><span style={`width:${report!.level_progress_pct}%`}></span></em>{/if}
          </span>
        {/each}
      </div>
    {/if}
  {/if}
</section>

<style>
  .weekly {
    position: relative;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 12px 16px 14px;
    background: linear-gradient(180deg, var(--panel-raised), var(--panel));
    box-shadow:
      var(--hair-shadow),
      0 2px 6px color-mix(in srgb, var(--text) 10%, transparent);
    margin-bottom: 12px;
  }
  /* fluted right edge mirrors the tablet cards */
  .weekly::after {
    content: '';
    position: absolute;
    right: 0;
    top: 0;
    bottom: 0;
    width: 6px;
    background: repeating-linear-gradient(
      180deg,
      color-mix(in srgb, var(--text) 8%, transparent) 0 2px,
      transparent 2px 7px
    );
    pointer-events: none;
  }

  h2 {
    font-family: var(--font-display);
    font-size: 12px;
    letter-spacing: 0.2em;
    text-transform: uppercase;
    color: var(--muted);
    margin: 0 0 8px;
  }

  h2 em {
    font-family: var(--font-serif);
    font-style: italic;
    text-transform: none;
    letter-spacing: 0.02em;
    color: var(--faint);
  }

  .days {
    display: flex;
    align-items: flex-end;
    gap: 16px;
  }

  .big {
    font-family: var(--font-display);
    font-size: 30px;
    font-weight: 900;
    color: var(--text);
    line-height: 1;
    letter-spacing: 0.02em;
  }

  .big span {
    font-family: var(--font-serif);
    font-size: 12px;
    font-weight: 400;
    letter-spacing: 0.14em;
    text-transform: uppercase;
    color: var(--muted);
  }

  .torches {
    display: flex;
    align-items: flex-end;
    gap: 7px;
    margin-left: auto;
    padding-bottom: 2px;
  }

  .torch {
    width: 9px;
    height: var(--h, 10px);
    background: var(--panel-sunken);
    border: 1px solid var(--border);
    border-radius: 2px 2px 0 0;
    position: relative;
    display: flex;
    align-items: flex-start;
    justify-content: center;
  }

  .torch .flame {
    width: 5px;
    height: 0;
    border-radius: 2px;
    background: var(--border);
    transition: height 0.3s ease, background 0.3s ease;
  }

  .torch.lit .flame {
    height: 7px;
    background: var(--accent);
    border-radius: 50% 50% 40% 40%;
    box-shadow: 0 0 6px color-mix(in srgb, var(--accent) 45%, transparent);
  }

  .calendar {
    display: flex;
    flex-direction: column;
    gap: 5px;
    margin-top: 10px;
  }

  .calendar-title {
    font-family: var(--font-serif);
    font-size: 11px;
    letter-spacing: 0.12em;
    text-transform: uppercase;
    color: var(--faint);
  }

  .heatmap {
    display: grid;
    grid-template-columns: repeat(7, 7px);
    gap: 3px;
    width: max-content;
  }

  .cell {
    width: 7px;
    height: 7px;
    border-radius: 1px;
    background: var(--panel-sunken);
    border: 1px solid color-mix(in srgb, var(--border) 70%, transparent);
  }

  .cell.blank {
    border-color: transparent;
    background: transparent;
  }

  .cell.l1 {
    background: color-mix(in srgb, var(--accent) 25%, var(--panel-sunken));
    border-color: transparent;
  }

  .cell.l2 {
    background: color-mix(in srgb, var(--accent) 50%, var(--panel-sunken));
    border-color: transparent;
  }

  .cell.l3 {
    background: color-mix(in srgb, var(--accent) 75%, var(--panel-sunken));
    border-color: transparent;
  }

  .cell.l4 {
    background: var(--accent);
    border-color: transparent;
  }

  .cell.today {
    box-shadow: 0 0 0 1px var(--accent-2);
  }

  .calendar-empty {
    font-family: var(--font-serif);
    font-size: 12px;
    color: var(--faint);
    font-style: italic;
  }

  .sub {
    margin: 10px 0 8px;
    font-size: 14px;
    color: var(--muted);
  }

  .sub b {
    color: var(--text);
    font-weight: 600;
  }

  .ranks {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
    margin-top: 10px;
  }

  .rank-chip {
    font-family: var(--font-serif);
    font-size: 13px;
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 3px 9px;
    white-space: nowrap;
  }

  .rank-chip s {
    color: var(--faint);
  }

  .rank-chip i {
    color: var(--faint);
    font-style: normal;
  }

  .rank-chip b {
    color: var(--accent-2);
  }

  
  
  
  
  
  
  
  .mini {
    border: 1px solid var(--border-strong);
    background: transparent;
    color: var(--muted);
    border-radius: var(--radius);
    cursor: pointer;
    padding: 1px 7px;
    font-size: 12px;
  }

  .mini:hover:not(:disabled) {
    border-color: var(--accent);
    color: var(--accent);
  }

  .empty {
    color: var(--muted);
    font-size: 14px;
    font-style: italic;
  }
</style>
