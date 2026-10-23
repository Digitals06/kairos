<script lang="ts">
  /**
   * Custom bench builder (v0.5.0).
   *
   * A bench is authored entirely here: scenarios grouped into subcategories,
   * author-supplied thresholds per scenario, tier names, and one of three
   * aggregation rules (all scenarios / top N per subcategory / global energy).
   * The rank preview mirrors the Rust engine exactly (rankcalc::rank_of plus
   * the rule), so what the author sees before saving is what they get after.
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
  const DEFAULT_SUBCATEGORY = 'Main'

  type Rule = 'all' | 'top_n' | 'energy'
  type Row = { scenario: string; category: string; subcategory: string; score: number; text: string }

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
  let rule = $state<Rule>('all')
  let topN = $state(2)
  let tiersText = $state(DEFAULT_TIERS)
  let rows = $state<Row[]>([])
  let activeSubcat = $state(DEFAULT_SUBCATEGORY)
  let newSubcat = $state('')

  const tierNames = $derived(
    tiersText
      .split(',')
      .map((t) => t.trim())
      .filter((t) => t.length > 0),
  )

  /** Author-defined subcategories, in first-appearance order. */
  const subcategories = $derived.by(() => {
    const out: string[] = []
    for (const row of rows) {
      const name = row.subcategory.trim() || DEFAULT_SUBCATEGORY
      if (!out.includes(name)) out.push(name)
    }
    return out
  })

  /** Tier index for a score (0 = below the first threshold). Mirrors rankcalc::rank_of. */
  function tierOf(score: number, thresholds: number[]): number {
    if (score <= 0 || thresholds.length === 0) return 0
    for (let i = thresholds.length - 1; i >= 0; i--) {
      if (score >= thresholds[i]) return i + 1
    }
    return 0
  }

  /** Precise tier (tier + band fraction, clamped at the top). Mirrors PreciseRank. */
  function preciseOf(score: number, thresholds: number[]): number {
    const tier = tierOf(score, thresholds)
    if (tier === 0 || tier >= thresholds.length) return tier
    const low = thresholds[tier - 1]
    const high = thresholds[tier]
    const band = high - low
    return band > 0 ? tier + (score - low) / band : tier
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
      (rule !== 'top_n' || (topN >= 1 && topN <= rows.length)),
  )

  /** Per-subcategory preview, mirroring the Rust rule. */
  const preview = $derived.by(() => {
    const groups = new Map<string, number[]>()
    const energies = new Map<string, number>()
    for (const row of rows) {
      const thresholds = parseThresholds(row.text)
      const sub = row.subcategory.trim() || DEFAULT_SUBCATEGORY
      const tier = tierOf(row.score, thresholds)
      const precise = preciseOf(row.score, thresholds)
      if (!groups.has(sub)) groups.set(sub, [])
      groups.get(sub)!.push(tier)
      energies.set(sub, Math.max(energies.get(sub) ?? 0, precise))
    }
    let rank = 0
    let benchEnergy = 0
    if (groups.size > 0) {
      if (rule === 'energy') {
        const values = [...energies.values()]
        benchEnergy = values.some((v) => v <= 0)
          ? 0
          : values.length / values.reduce((sum, v) => sum + 1 / v, 0)
        rank = Math.floor(Math.min(benchEnergy, tierNames.length))
      } else if (rule === 'top_n') {
        rank = Math.min(
          ...[...groups.values()].map((tiers) => {
            const sorted = [...tiers].sort((a, b) => b - a)
            const take = Math.min(topN, sorted.length)
            return sorted[take - 1] ?? 0
          }),
        )
      } else {
        rank = Math.min(...[...groups.values()].map((tiers) => Math.min(...tiers)))
      }
    }
    return { rank: Number.isFinite(rank) ? rank : 0, energy: benchEnergy, count: groups.size }
  })

  const previewName = $derived(
    preview.rank > 0 ? (tierNames[preview.rank - 1] ?? '—') : 'Unranked',
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
    rule = 'all'
    topN = 2
    tiersText = DEFAULT_TIERS
    rows = []
    activeSubcat = DEFAULT_SUBCATEGORY
    newSubcat = ''
    search = ''
    error = null
  }

  function edit(bench: CustomBench) {
    editingId = bench.benchmark_id
    name = bench.name
    abbreviation = bench.abbreviation
    color = bench.color
    rule = bench.rule === 'top_n' ? 'top_n' : bench.rule === 'energy' ? 'energy' : 'all'
    topN = bench.top_n || 2
    tiersText = bench.tier_names.join(', ')
    rows = bench.scenarios.map((s: CustomScenario) => ({
      scenario: s.scenario,
      category: s.category,
      subcategory: s.subcategory || DEFAULT_SUBCATEGORY,
      score: s.score,
      text: s.thresholds.join(', '),
    }))
    activeSubcat = rows[0]?.subcategory ?? DEFAULT_SUBCATEGORY
    newSubcat = ''
    search = ''
    error = null
  }

  function add(choice: ScenarioChoice) {
    const score = choice.score ?? 0
    // Seed a plausible ladder from the player's current score so the row is
    // never empty; the author edits it immediately.
    const base = score > 0 ? score : 100
    const factors = [0.7, 0.85, 1.0, 1.15]
    const values = Array.from({ length: Math.max(1, tierNames.length) }, (_, i) => {
      const f = factors[i] ?? 1.15 + 0.15 * (i - 3)
      return Math.round(base * f)
    })
    rows = [
      ...rows,
      {
        scenario: choice.scenario,
        category: choice.category,
        subcategory: activeSubcat || DEFAULT_SUBCATEGORY,
        score,
        text: values.join(', '),
      },
    ]
    search = ''
  }

  function addSubcategory() {
    const clean = newSubcat.trim()
    if (clean.length === 0 || subcategories.includes(clean)) return
    activeSubcat = clean
    newSubcat = ''
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
        rule,
        top_n: rule === 'top_n' ? topN : 0,
        tier_names: tierNames,
        scenarios: rows.map((r) => ({
          scenario: r.scenario,
          category: r.category,
          subcategory: r.subcategory.trim() || DEFAULT_SUBCATEGORY,
          thresholds: parseThresholds(r.text),
          score: r.score,
          tier: 0,
          tier_name: null,
          next_threshold: null,
          delta: null,
        })),
        rank_index: 0,
        complete: false,
        energy: 0,
        rank: null,
        next_rank_name: null,
        subcategories: [],
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
                <span class="muted small">
                  {bench.rule === 'energy'
                    ? 'global energy'
                    : bench.rule === 'top_n'
                      ? `top ${bench.top_n} per subcategory`
                      : 'all scenarios'} · {bench.subcategories.length} subcat
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
          <span class="field-label">Rank rule</span>
          <div class="rules">
            <label class="rule">
              <input type="radio" bind:group={rule} value="all" />
              full subcategory <span class="muted small">every scenario counts</span>
            </label>
            <label class="rule">
              <input type="radio" bind:group={rule} value="top_n" />
              top
              <input
                class="topn"
                type="number"
                min="1"
                max="24"
                bind:value={topN}
                disabled={rule !== 'top_n'}
              />
              per subcategory
            </label>
            <label class="rule">
              <input type="radio" bind:group={rule} value="energy" />
              global energy <span class="muted small">harmonic across subcategories</span>
            </label>
          </div>
        </div>
        <p class="muted small">
          {#if rule === 'all'}
            Each subcategory sits at its weakest scenario, and the bench takes its weakest
            subcategory.
          {:else if rule === 'top_n'}
            Only the best {topN} scenario{topN === 1 ? '' : 's'} of each subcategory count; the
            weakest subcategory decides.
          {:else}
            Each subcategory is carried by its best scenario; the bench is the harmonic mean across
            subcategories, so one weak subcategory drags the whole rank.
          {/if}
        </p>

        <label class="field">
          <span>Tier names (worst → best, comma separated)</span>
          <input type="text" bind:value={tiersText} onchange={resyncThresholds} />
        </label>
        <p class="muted small">
          {tierNames.length} tier{tierNames.length === 1 ? '' : 's'} → every scenario needs
          {tierNames.length} thresholds.
        </p>

        <div class="subcats">
          <span class="field-label">Subcategories</span>
          <div class="subcat-row">
            {#if subcategories.length === 0}
              <span class="muted small">No subcategories yet — add one, then pick scenarios for it.</span>
            {:else}
              {#each subcategories as sub (sub)}
                <button
                  class="subcat-chip"
                  class:active={activeSubcat === sub}
                  onclick={() => (activeSubcat = sub)}
                >
                  {sub}
                  <span class="muted small">
                    {rows.filter((r) => (r.subcategory.trim() || DEFAULT_SUBCATEGORY) === sub).length}
                  </span>
                </button>
              {/each}
            {/if}
            <input
              class="subcat-new"
              type="text"
              bind:value={newSubcat}
              placeholder="New subcategory…"
              onkeydown={(e) => {
                if (e.key === 'Enter') {
                  e.preventDefault()
                  addSubcategory()
                }
              }}
            />
            <button class="btn btn-small" onclick={addSubcategory}>Add</button>
          </div>
        </div>

        <div class="scenario-picker">
          <label class="field">
            <span>
              Add a scenario{activeSubcat ? ` to ${activeSubcat}` : ''}
            </span>
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
                <th>Subcategory</th>
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
                      class="subcat-input"
                      type="text"
                      list="subcat-options"
                      bind:value={row.subcategory}
                    />
                  </td>
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
          <datalist id="subcat-options">
            {#each subcategories as sub (sub)}
              <option value={sub}></option>
            {/each}
          </datalist>
        {/if}

      </section>
    </div>

    <footer class="bench-foot">
      <span class="preview">
        Rank now: <b>{previewName}</b>
        {#if rule === 'energy' && preview.energy > 0}
          <span class="muted small">energy {preview.energy.toFixed(2)}</span>
        {/if}
        {#if preview.rank > 0 && preview.rank < tierNames.length}
          <span class="muted small">→ next: {tierNames[preview.rank]}</span>
        {/if}
        <span class="muted small">· {preview.count} subcategories</span>
      </span>
      <button class="btn" disabled={!formValid || busy} onclick={save}>
        {editingId === 0 ? 'Create bench' : 'Save changes'}
      </button>
    </footer>
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
    width: min(1180px, 100%);
    max-height: min(88vh, 920px);
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
  .rules {
    display: flex;
    gap: 14px;
    flex-wrap: wrap;
    align-items: center;
  }
  .rule {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .rule .topn {
    width: 56px;
    padding: 2px 4px;
    border: 1px solid var(--border);
    border-radius: 4px;
    background: var(--surface);
    color: var(--text);
  }
  .subcats {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .subcat-row {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
    align-items: center;
  }
  .subcat-chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 4px 10px;
    border: 1px solid var(--border);
    border-radius: 999px;
    background: none;
    color: inherit;
    cursor: pointer;
  }
  .subcat-chip.active {
    border-color: var(--accent);
    color: var(--accent);
  }
  .subcat-new,
  .subcat-input {
    padding: 4px 8px;
    border: 1px solid var(--border);
    border-radius: 4px;
    background: var(--surface);
    color: var(--text);
  }
  .subcat-new {
    flex: 0 0 180px;
  }
  .subcat-input {
    width: 100%;
    min-width: 110px;
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
    max-width: 220px;
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
    margin-top: 10px;
    flex-wrap: wrap;
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
