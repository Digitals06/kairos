<script lang="ts">
  /**
   * Grind session coach (v0.3.0): while the player grinds, the coach strip
   * surfaces the next 3 cheapest scenario targets for an explicitly chosen
   * family. Read-only, app polling only — no game overlay, no auto-refresh
   * race: picks a family inline (favorites surface first), then polls
   * grind-next every 30s; the strip hides itself while unselected.
   */
  import { onMount } from 'svelte'
  import { getOverview, grindNext, type BenchmarkCard, type GrindNext } from './api'

  let cards = $state<BenchmarkCard[]>([])
  let chosen = $state<BenchmarkCard | null>(null)
  let grade = $state<GrindNext | null>(null)
  let error = $state<string | null>(null)
  let open = $state(false)

  const POLL_MS = 30_000

  $effect(() => {
    if (!chosen) return
    const family = chosen
    let alive = true
    const load = async () => {
      try {
        const g = await grindNext(family.benchmark_id)
        if (!alive) return
        grade = g
        error = null
      } catch (e) {
        if (alive) error = String(e)
      }
    }
    load()
    const t = setInterval(load, POLL_MS)
    return () => {
      alive = false
      clearInterval(t)
    }
  })

  onMount(async () => {
    try {
      const all = await getOverview()
      cards = [...all].sort(
        (a, b) => (b.is_favorite ? 1 : 0) - (a.is_favorite ? 1 : 0) || a.benchmark_name.localeCompare(b.benchmark_name),
      )
    } catch {
      /* coach is decorative; overview load failures surface elsewhere */
    }
  })

  function pick(id: string) {
    const family = cards.find((c) => String(c.benchmark_id) === id) ?? null
    chosen = family
    grade = null
  }

  const pickers = $derived(cards.slice(0, 40))
</script>

<div class="coach">
  {#if open}
    <div class="coach-body">
      <div class="coach-row">
        <label class="coach-label" for="coach-family">Grind</label>
        <select id="coach-family" class="coach-select" value={chosen ? String(chosen.benchmark_id) : ''} onchange={(e) => pick(e.currentTarget.value)}>
          <option value="" disabled>choose a family…</option>
          {#each pickers as c (c.benchmark_id)}
            <option value={String(c.benchmark_id)}>
              {c.is_favorite ? '★ ' : ''}{c.benchmark_name}
            </option>
          {/each}
        </select>
        {#if chosen}
          <span class="coach-rank display">{chosen.difficulty_name} · {grade?.currentRank ?? chosen.rank?.name ?? '—'}</span>
        {/if}
      </div>
      {#if chosen}
        {#if error}
          <p class="coach-err">{error}</p>
        {:else if grade?.complete}
          <p class="coach-note">Complete — every scenario at the ladder top. Nothing left to grind.</p>
        {:else if grade && grade.targets.length > 0}
          <ol class="targets">
            {#each grade.targets.slice(0, 3) as target, i (target.scenario)}
              <li>
                <span class="order display">{i + 1}</span>
                <span class="scenario">{target.scenario}</span>
                <span class="delta">{target.delta} pts</span>
                {#if target.cv !== null}<span class="cv" title="How steady your recent runs are — lower = more consistent">spread {target.cv.toFixed(1)}%</span>{/if}
                {#if target.plateaued}<span class="plateau">plateaued</span>{/if}
              </li>
            {/each}
          </ol>
          <p class="coach-note">→ {grade.nextRank}. Refreshes as your CSVs change.</p>
        {:else if grade && grade.plan.length > 0}
          <ol class="targets">
            {#each grade.plan.slice(0, 3) as target, i (target.scenario)}
              <li>
                <span class="order display">{i + 1}</span>
                <span class="scenario">{target.scenario}</span>
                <span class="delta">{target.delta} pts</span>
                {#if target.rungsCrossed > 1}<span class="cv">×{target.rungsCrossed} rungs</span>{/if}
              </li>
            {/each}
          </ol>
          <p class="coach-note">Combined plan → {grade.nextRank} (no single scenario flips it together).</p>
        {:else if grade}
          <p class="coach-note">No grind targets for this family yet — sync once it has data.</p>
        {:else}
          <p class="coach-note">Reading latest scores…</p>
        {/if}
      {/if}
    </div>
  {/if}
  <button class="coach-toggle" aria-expanded={open} onclick={() => (open = !open)}>
    {open ? 'collapse coach' : 'grind coach ⚑'}
  </button>
</div>

<style>
  .coach {
    margin: 10px auto 0;
    max-width: 820px;
  }

  .coach-body {
    background: color-mix(in srgb, var(--card) 88%, transparent);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    box-shadow: 0 12px 26px -20px color-mix(in srgb, var(--text) 45%, transparent);
    padding: 10px 14px 12px;
    animation: coach-in var(--dur-medium, 0.3s) var(--ease-out-soft, ease-out) both;
  }

  @keyframes coach-in {
    from {
      opacity: 0;
      transform: translateY(6px);
    }
  }

  .coach-row {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .coach-label {
    font-family: var(--font-display, inherit);
    letter-spacing: 0.08em;
    font-size: 0.78rem;
    color: var(--faint);
  }

  .coach-select {
    background: var(--card);
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: var(--radius, 0.25rem);
    padding: 5px 8px;
    max-width: 210px;
  }

  .coach-select:focus-visible,
  .coach-toggle:focus-visible {
    outline: none;
    box-shadow: inset 0 0 0 2px color-mix(in srgb, var(--accent) 60%, transparent);
  }

  .coach-rank {
    margin-left: auto;
    font-size: 0.85rem;
    color: var(--accent-2);
  }

  .targets {
    list-style: none;
    margin: 8px 0 0;
    padding: 0;
    display: grid;
    gap: 6px;
  }

  .targets li {
    display: flex;
    align-items: baseline;
    gap: 10px;
    flex-wrap: wrap;
  }

  .order {
    width: 22px;
    height: 22px;
    align-self: center;
    display: grid;
    place-items: center;
    border-radius: 50%;
    border: 1px solid var(--accent);
    color: var(--accent);
    font-size: 0.78rem;
  }

  .scenario {
    font-weight: 600;
  }

  .delta {
    font-family: var(--font-mono, monospace);
    font-size: 0.8rem;
    color: var(--text);
  }

  .cv,
  .plateau {
    font-size: 0.72rem;
    letter-spacing: 0.05em;
    color: var(--faint);
    border: 1px solid var(--border);
    border-radius: var(--radius-full, 999px);
    padding: 1px 8px;
  }

  .plateau {
    color: var(--warn, #b0563a);
    border-color: color-mix(in srgb, var(--warn, #b0563a) 45%, transparent);
  }

  .coach-note,
  .coach-err {
    margin: 8px 0 0;
    font-size: 0.82rem;
    color: var(--faint);
  }

  .coach-err {
    color: var(--danger, #b0563a);
  }

  .coach-toggle {
    margin-top: 8px;
    background: none;
    border: none;
    color: var(--faint);
    font-size: 0.78rem;
    letter-spacing: 0.08em;
    cursor: pointer;
    padding: 4px 2px;
  }

  .coach-toggle:hover {
    color: var(--accent);
  }
</style>
