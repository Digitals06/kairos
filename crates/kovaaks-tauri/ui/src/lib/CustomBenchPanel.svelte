<script lang="ts">
  /**
   * Custom bench builder (v0.5.0).
   *
   * A bench is authored entirely here: a scenario list, author-supplied
   * thresholds per scenario, tier names, and one of two scoring modes. The
   * rank preview mirrors the Rust rule exactly so what the user sees before
   * saving is what they get after (validated by the backend on save too).
   */
  import {
    customBenches,
    deleteCustomBench,
    knownScenarios,
    saveCustomBench,
    type CustomBench,
    type CustomScenario,
    type ScenarioChoice,
  } from './api'

  let { onclose, onsaved }: { onclose: () => void; onsaved: () => void } = $props()

  const PRESET_COLORS = ['#b08d57', '#8fa8a0', '#a86f6f', '#7f8fb0', '#b09a6a', '#8a7fa8']
  const DEFAULT_TIERS = 'Bronze, Silver, Gold, Platinum'

  type Row = { scenario: string; category: string; score: number; text: string }

  let saved = $state<CustomBench[]>([])
  let choices = $state<ScenarioChoice[]>([])
  let loading = $state(true)
  let busy = $state(false)
  let error = $state<string | null>(null)
  let search = $state('')

  // form state
  let editingId = $state(0)
  let name = $state('')
  let abbreviation = $state('CS')
  let color = $state(PRESET_COLORS[0])
  let mode = $state<'all' | 'top_n'>('all')
  let topN = $state(3)
  let tiersText = $state(DEFAULT_TIERS)
  let rows = $state<Row[]>([])

  const tierNames = $derived(
    tiersText
      .split(',')
      .map((t) => t.trim())
      .filter((t) => t.length > 0),
  )

  /** Tier index for a score (0 = below the first threshold). Mirrors rankcalc::rank_of. */
  function tierOf(score: number, thresholds: number[]): number {
    if (score <= 0 || thresholds.length === 0) return 0
    for (let i = thresholds.length - 1; i >= 0; i--) {
      if (score >= thresholds[i]) return i + 1
    }
    return 0
  }

  function parseThresholds(text: string): number[] {
    return text
      .split(',')
      .map((t) => t.trim())
      .filter((t) => t.length > 0)
      .map((t) => Number(t))
  }

  function rowError(row: Row): string | null {
    const values = parseThresholds(row.text)
    if (values.length !== tierNames.length) {
      return `${values.length}/${tierNames.length} thresholds`
    }
    if (values.some((v) => !Number.isFinite(v) || v < 0)) return 'numbers only'
    if (values.some((v, i) => i > 0 && v <= values[i - 1])) return 'must ascend'
    return null
  }

  const rowErrors = $derived(rows.map((r) => rowError(r)))
  const formValid = $derived(
    name.trim().length > 0 &&
      tierNames.length > 0 &&
      rows.length > 0 &&
      rowErrors.every((e) => e === null) &&
      (mode === 'all' || (topN >= 1 && topN <= rows.length)),
  )

  const previewRank = $derived.by(() => {
    const tiers = rows.map((r) => tierOf(r.score, parseThresholds(r.text)))
    if (tiers.length === 0) return 0
    if (mode === 'all') return Math.min(...tiers)
    let best = 0
    for (let candidate = tierNames.length; candidate >= 1; candidate--) {
      if (tiers.filter((t) => t >= candidate).length >= topN) {
        best = candidate
        break
      }
    }
    return best
  })

  const previewName = $derived(
    previewRank > 0 ? (tierNames[previewRank - 1] ?? '—') : 'Unranked',
  )

  const matches = $derived(
    search.trim().length === 0
      ? []
      : choices
          .filter((c) => c.scenario.toLowerCase().includes(search.trim().toLowerCase()))
          .filter((c) => !rows.some((r) => r.scenario === c.scenario))
          .slice(0, 12),
  )

  async function load() {
    loading = true
    error = null
    try {
      ;[saved, choices] = await Promise.all([customBenches(), knownScenarios()])
    } catch (e) {
      error = String(e)
    } finally {
      loading = false
    }
  }

  $effect(() => {
    void load()
  })

  function resetForm() {
    editingId = 0
    name = ''
    abbreviation = 'CS'
    color = PRESET_COLORS[0]
    mode = 'all'
    topN = 3
    tiersText = DEFAULT_TIERS
    rows = []
    search = ''
    error = null
  }

  function edit(bench: CustomBench) {
    editingId = bench.benchmark_id
    name = bench.name
    abbreviation = bench.abbreviation
    color = bench.color
    mode = bench.mode === 'top_n' ? 'top_n' : 'all'
    topN = bench.top_n || 3
    tiersText = bench.tier_names.join(', ')
    rows = bench.scenarios.map((s: CustomScenario) => ({
      scenario: s.scenario,
      category: s.category,
      score: s.score,
      text: s.thresholds.join(', '),
    }))
    search = ''
    error = null
  }

  function add(choice: ScenarioChoice) {
    const score = choice.score ?? 0
    // Seed a plausible shape from the player's current score so the row is
    // never empty; the author edits it immediately.
    const base = score > 0 ? score : 100
    const seeded = [0.7, 0.85, 1.0, 1.15].slice(0, Math.max(1, tierNames.length || 3))
    while (seeded.length < tierNames.length) seeded.push(seeded[seeded.length - 1] + 0.15)
    rows = [
      ...rows,
      {
        scenario: choice.scenario,
        category: choice.category,
        score,
        text: seeded.map((f) => Math.round(base * f)).join(', '),
      },
    ]
    search = ''
  }

  function resyncThresholds() {
    // Tier count changed: every row now needs the same number of values.
    rows = rows.map((r) => {
      const values = parseThresholds(r.text)
      const next = values.slice(0, tierNames.length)
      while (next.length < tierNames.length) {
        const last = next[next.length - 1] ?? (r.score > 0 ? r.score : 100)
        next.push(Math.round(last * 1.2))
      }
      return { ...r, text: next.join(', ') }
    })
  }

  async function save() {
    if (!formValid || busy) return
    busy = true
    error = null
    try {
      await saveCustomBench({
        benchmark_id: editingId,
        name: name.trim(),
        abbreviation: abbreviation.trim() || 'CS',
        color,
        mode,
        top_n: mode === 'top_n' ? topN : 0,
        tier_names: tierNames,
        scenarios: rows.map((r) => ({
          scenario: r.scenario,
          category: r.category,
          thresholds: parseThresholds(r.text),
          score: r.score,
          tier: 0,
          tier_name: null,
          next_threshold: null,
          delta: null,
        })),
        rank_index: 0,
        complete: false,
        rank: null,
        next_rank_name: null,
      })
      await load()
      onsaved()
      resetForm()
    } catch (e) {
      error = String(e)
    } finally {
      busy = false
    }
  }

  async function remove(bench: CustomBench) {
    if (busy) return
    busy = true
    error = null
    try {
      await deleteCustomBench(bench.benchmark_id)
      if (editingId === bench.benchmark_id) resetForm()
      await load()
      onsaved()
    } catch (e) {
      error = String(e)
    } finally {
      busy = false
    }
  }
</script>

<div class="modal-backdrop" role="presentation">
  <div class="panel bench-modal" role="dialog" aria-label="Custom benchmarks">
    <header class="modal-head">
      <h2 class="display">Custom benches</h2>
      <button class="btn btn-small" onclick={onclose}>Close</button>
    </header>

    {#if error}
      <p class="bench-error">{error}</p>
    {/if}

    <div class="bench-body">
      <aside class="bench-list">
        <button class="btn btn-small" onclick={resetForm}>+ New bench</button>
        {#if loading}
          <p class="muted">Loading…</p>
        {:else if saved.length === 0}
          <p class="muted">No custom benches yet.</p>
        {:else}
          {#each saved as bench (bench.benchmark_id)}
            <div class="bench-item" class:editing={editingId === bench.benchmark_id}>
              <div class="bench-item-main">
                <span class="bench-name">{bench.name}</span>
                <span class="chip">
                  {bench.rank?.name ?? 'Unranked'}{bench.complete ? ' · complete' : ''}
                </span>
              </div>
              <div class="bench-item-actions">
                <button class="btn btn-small" onclick={() => edit(bench)}>Edit</button>
                <button class="btn btn-small danger" onclick={() => remove(bench)}>Delete</button>
              </div>
            </div>
          {/each}
        {/if}
      </aside>

      <section class="bench-form">
        <div class="field-row">
          <label class="field">
            <span>Name</span>
            <input type="text" bind:value={name} placeholder="My daily five" />
          </label>
          <label class="field narrow">
            <span>Tag</span>
            <input type="text" bind:value={abbreviation} maxlength="4" />
          </label>
        </div>

        <div class="field-row">
          <span class="field-label">Color</span>
          <div class="swatches">
            {#each PRESET_COLORS as preset (preset)}
              <button
                class="swatch"
                class:active={color === preset}
                style="background: {preset}"
                aria-label="Use color {preset}"
                onclick={() => (color = preset)}
              ></button>
            {/each}
          </div>
        </div>

        <div class="field-row">
          <span class="field-label">Scoring</span>
          <div class="modes">
            <label class="mode">
              <input type="radio" bind:group={mode} value="all" />
              every scenario counts
            </label>
            <label class="mode">
              <input type="radio" bind:group={mode} value="top_n" />
              top
              <input
                class="topn"
                type="number"
                min="1"
                max="24"
                bind:value={topN}
                disabled={mode !== 'top_n'}
              />
              count
            </label>
          </div>
        </div>

        <label class="field">
          <span>Tier names (worst → best, comma separated)</span>
          <input type="text" bind:value={tiersText} onchange={resyncThresholds} />
        </label>
        <p class="muted small">
          {tierNames.length} tier{tierNames.length === 1 ? '' : 's'} → every scenario needs
          {tierNames.length} thresholds.
        </p>

        <div class="scenario-picker">
          <label class="field">
            <span>Add a scenario</span>
            <input
              type="text"
              bind:value={search}
              placeholder="Search your scenarios…"
              autocomplete="off"
            />
          </label>
          {#if matches.length > 0}
            <ul class="matches">
              {#each matches as match (match.scenario)}
                <li>
                  <button class="match" onclick={() => add(match)}>
                    <span class="match-name">{match.scenario.trim()}</span>
                    <span class="muted small">
                      {match.category || '—'} · {match.score != null
                        ? match.score.toLocaleString()
                        : 'never played'}
                    </span>
                  </button>
                </li>
              {/each}
            </ul>
          {:else if search.trim().length > 0}
            <p class="muted small">No match.</p>
          {/if}
        </div>

        {#if rows.length > 0}
          <table class="bench-rows">
            <thead>
              <tr>
                <th>Scenario</th>
                <th>Score</th>
                <th>Thresholds</th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              {#each rows as row, i (row.scenario)}
                <tr>
                  <td class="row-name">{row.scenario.trim()}</td>
                  <td class="num">{row.score.toLocaleString()}</td>
                  <td>
                    <input
                      class="threshold-input"
                      class:invalid={rowErrors[i] !== null}
                      type="text"
                      bind:value={row.text}
                      placeholder="100, 200, 300"
                    />
                    {#if rowErrors[i]}
                      <span class="row-hint">{rowErrors[i]}</span>
                    {/if}
                  </td>
                  <td>
                    <button
                      class="btn btn-small danger"
                      onclick={() => (rows = rows.filter((r) => r.scenario !== row.scenario))}
                      >×</button
                    >
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        {/if}

        <footer class="bench-foot">
          <span class="preview">
            Rank now: <b>{previewName}</b>
            {#if previewRank > 0 && previewRank < tierNames.length}
              <span class="muted small">→ next: {tierNames[previewRank]}</span>
            {/if}
          </span>
          <button class="btn" disabled={!formValid || busy} onclick={save}>
            {editingId === 0 ? 'Create bench' : 'Save changes'}
          </button>
        </footer>
      </section>
    </div>
  </div>
</div>

<style>
  .modal-backdrop {
    position: fixed;
    inset: 0;
    background: rgb(0 0 0 / 55%);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 40;
    padding: 24px;
  }
  .bench-modal {
    width: min(1100px, 100%);
    max-height: min(86vh, 900px);
    display: flex;
    flex-direction: column;
    padding: 16px 18px;
    overflow: hidden;
  }
  .modal-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    margin-bottom: 10px;
  }
  .modal-head h2 {
    margin: 0;
    font-size: 15px;
  }
  .bench-body {
    display: grid;
    grid-template-columns: 260px 1fr;
    gap: 16px;
    min-height: 0;
    overflow: hidden;
  }
  .bench-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
    overflow-y: auto;
    padding-right: 4px;
  }
  .bench-item {
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 8px;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .bench-item.editing {
    border-color: var(--accent);
  }
  .bench-item-main {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .bench-name {
    font-weight: 600;
  }
  .bench-item-actions {
    display: flex;
    gap: 6px;
  }
  .bench-form {
    display: flex;
    flex-direction: column;
    gap: 10px;
    overflow-y: auto;
    padding-right: 4px;
  }
  .field-row {
    display: flex;
    gap: 12px;
    align-items: center;
    flex-wrap: wrap;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 4px;
    flex: 1 1 200px;
  }
  .field.narrow {
    flex: 0 0 90px;
  }
  .field span,
  .field-label {
    font-family: var(--font-display);
    font-size: 10px;
    letter-spacing: 0.12em;
    text-transform: uppercase;
    color: var(--muted);
  }
  .field input {
    padding: 6px 8px;
    border: 1px solid var(--border);
    border-radius: 4px;
    background: var(--surface);
    color: var(--text);
  }
  .swatches {
    display: flex;
    gap: 6px;
  }
  .swatch {
    width: 22px;
    height: 22px;
    border-radius: 50%;
    border: 2px solid transparent;
    cursor: pointer;
  }
  .swatch.active {
    border-color: var(--accent-2);
  }
  .modes {
    display: flex;
    gap: 14px;
    flex-wrap: wrap;
    align-items: center;
  }
  .mode {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .mode .topn {
    width: 56px;
    padding: 2px 4px;
    border: 1px solid var(--border);
    border-radius: 4px;
    background: var(--surface);
    color: var(--text);
  }
  .matches {
    list-style: none;
    margin: 4px 0 0;
    padding: 0;
    border: 1px solid var(--border);
    border-radius: 6px;
    max-height: 200px;
    overflow-y: auto;
  }
  .match {
    width: 100%;
    text-align: left;
    background: none;
    border: none;
    border-bottom: 1px solid var(--border);
    padding: 6px 8px;
    cursor: pointer;
    color: inherit;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .match:hover {
    background: var(--surface-2, rgb(255 255 255 / 4%));
  }
  .bench-rows {
    width: 100%;
    border-collapse: collapse;
  }
  .bench-rows th {
    text-align: left;
    font-family: var(--font-display);
    font-size: 10px;
    letter-spacing: 0.12em;
    text-transform: uppercase;
    color: var(--muted);
    padding: 4px 6px;
  }
  .bench-rows td {
    padding: 4px 6px;
    border-top: 1px solid var(--border);
    vertical-align: top;
  }
  .row-name {
    max-width: 260px;
  }
  .threshold-input {
    width: 100%;
    padding: 4px 6px;
    border: 1px solid var(--border);
    border-radius: 4px;
    background: var(--surface);
    color: var(--text);
    font-variant-numeric: tabular-nums;
  }
  .threshold-input.invalid {
    border-color: var(--accent-2);
  }
  .row-hint {
    display: block;
    font-size: 11px;
    color: var(--accent-2);
  }
  .bench-foot {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    border-top: 1px solid var(--border);
    padding-top: 10px;
    margin-top: auto;
  }
  .preview b {
    color: var(--accent);
  }
  .bench-error {
    margin: 0 0 8px;
    padding: 6px 8px;
    border: 1px solid var(--accent-2);
    border-radius: 4px;
    color: var(--accent-2);
    font-size: 13px;
  }
  .muted {
    color: var(--muted);
    margin: 0;
  }
  .small {
    font-size: 11px;
  }
  .danger {
    color: var(--accent-2);
  }
</style>
