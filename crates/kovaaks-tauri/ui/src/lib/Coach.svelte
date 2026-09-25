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

  // Every played family is coachable — the native select's type-ahead and
  // alphabetical order make hundreds of options navigable, so no cap.
  const pickers = $derived(cards)
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
    <!-- Chiseled torch glyph: engraved shaft + carved flame; hover/open lights it -->
    <span class="torch" aria-hidden="true">
      <svg viewBox="0 0 24 24" width="20" height="20" fill="none" aria-hidden="true">
        <path class="shaft" d="M9.8 11.4 L9.2 20.6 L10.1 21.7 L10.9 20.9 L11.2 11.5 Z" />
        <rect class="collar" x="8.9" y="9.7" width="6.2" height="1.7" rx="0.85" />
        <path class="flame" d="M12 2.1 C10.4 3.9 8.9 5.5 9.5 7.8 C9.9 9.2 11.1 10.1 11.5 9.9 C11.25 8.7 12.0 7.5 12.5 6.6 C13.1 5.5 13.9 4.5 13.3 2.8 C12.95 1.9 12.4 1.7 12 2.1 Z" />
        <path class="flame-inner" d="M12.2 4.8 C11.5 5.8 11.0 6.8 11.3 7.9 C11.5 8.9 12.2 9.4 12.6 9.3 C12.3 8.2 12.7 7.2 13.05 6.3 C13.5 5.4 13.15 4.9 12.2 4.8 Z" />
      </svg>
    </span>
    <span class="label display">{open ? 'Stow the coach' : 'Grind coach'}</span>
    <span class="chevron" class:flip={open} aria-hidden="true">
      <svg viewBox="0 0 12 12" width="10" height="10" fill="none" aria-hidden="true">
        <path d="M2.5 4.2 L6 7.8 L9.5 4.2" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" />
      </svg>
    </span>
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

  /* ==== Grind-coach button: engraved tablet-end cap =========================
     Same engraved language as the pediment glyphs: inset stone bar with a
     chiseled bevel (top-light, bottom-underside). Resting = humble stone;
     hover hints; open lights the torch flame (accent + ember breathing).
     Focus uses the app-wide inset-selected ring. ============================ */
  .coach-toggle {
    display: inline-flex;
    align-items: center;
    gap: 9px;
    margin-top: 10px;
    padding: 7px 13px 7px 10px;
    /* carved gold lip: engraved bevel, vertical material gradient */
    background: linear-gradient(
      180deg,
      color-mix(in srgb, var(--text) 7%, var(--card)) 0%,
      color-mix(in srgb, var(--card) 92%, transparent) 38%,
      color-mix(in srgb, var(--text) 4%, var(--card)) 100%
    );
    border: 1px solid color-mix(in srgb, var(--border) 85%, transparent);
    box-shadow:
      inset 0 1.5px 0 color-mix(in srgb, var(--text) 26%, transparent),
      inset 0 -1.5px 0 color-mix(in srgb, var(--text) 30%, transparent),
      inset 1.5px 0 0 color-mix(in srgb, var(--text) 12%, transparent),
      inset -1.5px 0 0 color-mix(in srgb, var(--text) 12%, transparent),
      0 1px 2px color-mix(in srgb, var(--text) 18%, transparent),
      0 6px 16px -8px color-mix(in srgb, var(--text) 38%, transparent);
    border-radius: var(--radius-lg);
    color: var(--faint);
    cursor: pointer;
    transition:
      border-color var(--dur-fast, 0.12s) var(--ease-out, ease-out),
      color var(--dur-fast, 0.12s) var(--ease-out, ease-out),
      box-shadow var(--dur-fast, 0.12s) var(--ease-out, ease-out),
      transform var(--dur-fast, 0.12s) var(--ease-out, ease-out);
  }

  .coach-toggle:hover {
    color: var(--text);
    border-color: color-mix(in srgb, var(--accent) 60%, var(--border));
    transform: translateY(-1px);
    box-shadow:
      inset 0 1.5px 0 color-mix(in srgb, var(--text) 30%, transparent),
      inset 0 -1.5px 0 color-mix(in srgb, var(--text) 32%, transparent),
      0 2px 4px color-mix(in srgb, var(--text) 16%, transparent),
      0 10px 22px -10px color-mix(in srgb, var(--accent) 38%, transparent);
  }

  .coach-toggle:active {
    transform: translateY(0);
    box-shadow:
      inset 0 2px 4px color-mix(in srgb, var(--text) 20%, transparent),
      inset 0 -1px 0 color-mix(in srgb, var(--text) 6%, transparent);
  }

  /* open state: the cap keeps the flame lit */
  .coach-toggle[aria-expanded='true'] {
    color: var(--text);
    border-color: color-mix(in srgb, var(--accent) 55%, var(--border));
    box-shadow:
      inset 0 1px 0 color-mix(in srgb, var(--text) 9%, transparent),
      inset 0 0 0 1px color-mix(in srgb, var(--accent) 22%, transparent),
      0 3px 10px -6px color-mix(in srgb, var(--accent) 38%, transparent);
  }

  .shaft,
  .collar {
    fill: currentColor;
  }

  .flame,
  .flame-inner {
    transition: fill var(--dur-fast, 0.12s) var(--ease-out, ease-out);
  }

  .flame {
    fill: color-mix(in srgb, var(--faint) 80%, var(--accent));
  }

  .flame-inner {
    fill: color-mix(in srgb, var(--faint) 45%, transparent);
  }

  .coach-toggle:hover .flame,
  .coach-toggle[aria-expanded='true'] .torch .flame {
    fill: var(--accent);
    filter: drop-shadow(0 0 2px color-mix(in srgb, var(--accent) 70%, transparent));
  }

  .coach-toggle:hover .flame-inner,
  .coach-toggle[aria-expanded='true'] .torch .flame-inner {
    fill: color-mix(in srgb, var(--accent-2) 68%, transparent);
  }

  /* ember breathing while open */
  .coach-toggle[aria-expanded='true'] .torch .flame {
    animation: ember var(--dur-slow, 0.7s) var(--ease-in-out, ease-in-out) infinite alternate;
  }

  @keyframes ember {
    from {
      opacity: 0.8;
    }
    to {
      fill: color-mix(in srgb, var(--accent) 62%, var(--accent-2));
      opacity: 1;
    }
  }

  .label {
    font-size: 0.8rem;
    letter-spacing: 0.14em;
    text-transform: uppercase;
  }

  .chevron {
    display: grid;
    place-items: center;
    transition: transform var(--dur-medium, 0.3s) var(--ease-in-out, ease-in-out);
  }

  .chevron path {
    stroke: currentColor;
  }

  .chevron.flip {
    transform: rotate(180deg);
  }

  @media (prefers-reduced-motion: reduce) {
    .chevron,
    .coach-toggle,
    .flame,
    .flame-inner {
      transition: none;
      animation: none !important;
    }
  }
</style>
