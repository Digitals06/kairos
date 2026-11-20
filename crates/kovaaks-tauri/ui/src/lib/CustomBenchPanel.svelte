<script lang="ts">
  /**
   * Custom bench builder (v0.5.0).
   *
   * A bench is authored entirely here: scenarios grouped into subcategories,
   * author-supplied thresholds per scenario, tier names, and one of three
   * aggregation rules (all scenarios / top N per subcategory / global energy).
   * The rank preview calls a Rust command that runs the real engine
   * (kovaaks_core::custom::rank_custom), so what the author sees before
   * saving is what they get after — no mirrored rule to drift.
   */
  import {
    customBenchPreview,
    customBenches,
    deleteCustomBench,
    exportCustomBench,
    getOverview,
    importCustomBench,
    knownScenarios,
    registryBenchPreset,
    saveCustomBench,
    type CustomBench,
    type CustomBenchPreview,
    type CustomScenario,
    type ScenarioChoice,
  } from './api'

  let { onclose, onsaved }: { onclose: () => void; onsaved: () => void } = $props()

  const DEFAULT_COLOR = '#b08d57'
  const DEFAULT_TIERS = 'Bronze, Silver, Gold, Platinum'
  const DEFAULT_SUBCATEGORY = 'Main'

  type Rule = 'all' | 'top_n' | 'energy'
  type Row = { scenario: string; category: string; subcategory: string; score: number; text: string }

  let saved = $state<CustomBench[]>([])
  let choices = $state<ScenarioChoice[]>([])
  let presets = $state<{ id: number; label: string }[]>([])
  let presetId = $state(0)
  let presetNote = $state<string | null>(null)
  let loading = $state(true)
  let busy = $state(false)
  let error = $state<string | null>(null)
  let search = $state('')

  // form state
  let editingId = $state(0)
  let name = $state('')
  let abbreviation = $state('CS')
  let color = $state(DEFAULT_COLOR)
  let tierColors = $state<string[]>(
    DEFAULT_TIERS.split(',').map(() => DEFAULT_COLOR),
  )
  let rule = $state<Rule>('all')
  let topN = $state(2)
  let tiersText = $state(DEFAULT_TIERS)
  let rows = $state<Row[]>([])
  let activeSubcat = $state(DEFAULT_SUBCATEGORY)
  let newSubcat = $state('')
  // Import/export box: holds a bench file as text so it can be copied out or
  // pasted in without a file dialog.
  let transfer = $state('')
  let transferNote = $state<string | null>(null)

  function parseTierNames(text: string): string[] {
    return text
      .split(',')
      .map((t) => t.trim())
      .filter((t) => t.length > 0)
  }

  function isHexColor(value: string): boolean {
    return /^#[0-9a-fA-F]{6}$/.test(value)
  }

  const tierNames = $derived(parseTierNames(tiersText))

  /** Author-defined subcategories, in first-appearance order. */
  const subcategories = $derived.by(() => {
    const out: string[] = []
    for (const row of rows) {
      const name = row.subcategory.trim() || DEFAULT_SUBCATEGORY
      if (!out.includes(name)) out.push(name)
    }
    return out
  })

  /** Parse the comma-separated threshold text of one row. */
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
  const tierColorErrors = $derived(
    tierNames.map((_, i) => {
      const value = (tierColors[i] ?? '').trim()
      return value.length === 0 || isHexColor(value) ? null : 'use #RRGGBB'
    }),
  )
  const formValid = $derived(
    name.trim().length > 0 &&
      tierNames.length > 0 &&
      rows.length > 0 &&
      rowErrors.every((e) => e === null) &&
      tierColorErrors.every((e) => e === null) &&
      (rule !== 'top_n' || (topN >= 1 && topN <= rows.length)),
  )

  /** Rank preview, fed by the Rust command (see the $effect below). */
  let preview = $state<CustomBenchPreview>({ rank: 0, energy: 0, subcategories: 0 })

  /** Wire shape of the current draft — mirrors `save()`'s payload exactly. */
  function previewPayload(): CustomBench {
    return {
      benchmark_id: editingId,
      name: name.trim(),
      abbreviation: abbreviation.trim() || 'CS',
      color,
      tier_colors: tierNames.map((_, i) => (tierColors[i] ?? '').trim()),
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
    }
  }

  /**
   * Debounced preview against the real Rust rank rule. A generation counter
   * guards assignment: a stale in-flight response can never overwrite a newer
   * one. The previous preview stays visible until the new one lands (no
   * flicker), and failures are silent — the chip is supplementary.
   */
  let previewGeneration = 0
  $effect(() => {
    // Tracked inputs: every draft edit that can move the rank.
    for (const row of rows) {
      void row.subcategory
      void row.text
    }
    void rule
    void topN
    void tierNames

    const generation = ++previewGeneration
    const timer = setTimeout(() => {
      customBenchPreview(previewPayload())
        .then((next) => {
          if (generation === previewGeneration) preview = next
        })
        .catch(() => {
          // Keep the last preview.
        })
    }, 400)

    return () => {
      clearTimeout(timer)
      previewGeneration += 1
    }
  })

  const previewName = $derived(
    preview.rank > 0 ? (tierNames[preview.rank - 1] ?? '—') : 'Unranked',
  )

  const previewColor = $derived(
    preview.rank > 0 ? (tierColors[preview.rank - 1] || color) : color,
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
      const [benches, scenarios, overview] = await Promise.all([
        customBenches(),
        knownScenarios(),
        getOverview().catch(() => []),
      ])
      saved = benches
      choices = scenarios
      presets = overview
        .filter((card) => card.benchmark_id > 0)
        .map((card) => ({
          id: card.benchmark_id,
          label: `${card.benchmark_name} — ${card.difficulty_name}`,
        }))
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
    color = DEFAULT_COLOR
    tierColors = parseTierNames(DEFAULT_TIERS).map(() => DEFAULT_COLOR)
    rule = 'all'
    topN = 2
    tiersText = DEFAULT_TIERS
    rows = []
    activeSubcat = DEFAULT_SUBCATEGORY
    newSubcat = ''
    search = ''
    presetId = 0
    presetNote = null
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
    tierColors = bench.tier_names.map((_, i) => bench.tier_colors?.[i] || bench.color)
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
    presetId = 0
    presetNote = null
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
    // Tier count changed: every row and its color swatch move together.
    const names = parseTierNames(tiersText)
    tierColors = Array.from({ length: names.length }, (_, i) => tierColors[i] ?? color)
    rows = rows.map((r) => {
      const values = parseThresholds(r.text)
      const next = values.slice(0, names.length)
      while (next.length < names.length) {
        const last = next[next.length - 1] ?? (r.score > 0 ? r.score : 100)
        next.push(Math.round(last * 1.2))
      }
      return { ...r, text: next.join(', ') }
    })
  }

  /** Load a saved bench's data into the form as a new, unsaved bench. */
  async function applyPreset(id: number) {
    if (id === 0 || busy) return
    busy = true
    error = null
    presetNote = null
    try {
      const preset = await registryBenchPreset(id)
      editingId = 0
      name = preset.name
      abbreviation = preset.abbreviation
      color = preset.color
      rule = 'all'
      topN = 2
      tiersText = preset.tier_names.join(', ')
      tierColors = preset.tier_names.map((_, i) => preset.tier_colors[i] || preset.color)
      rows = preset.scenarios.map((s) => ({
        scenario: s.scenario,
        category: s.category,
        subcategory: s.subcategory || DEFAULT_SUBCATEGORY,
        score: s.score,
        text: s.thresholds.join(', '),
      }))
      activeSubcat = rows[0]?.subcategory ?? DEFAULT_SUBCATEGORY
      newSubcat = ''
      search = ''
      if (preset.warnings.length > 0) presetNote = preset.warnings.join('; ')
    } catch (e) {
      error = String(e)
      presetId = 0
    } finally {
      busy = false
    }
  }

  /** "<name> copy", then "<name> copy 2", … against the loaded list. */
  function copyName(base: string): string {
    const used = new Set(saved.map((b) => b.name.trim().toLowerCase()))
    let candidate = `${base} copy`
    if (!used.has(candidate.toLowerCase())) return candidate
    let n = 2
    while (used.has(`${base} copy ${n}`.toLowerCase())) n += 1
    return `${base} copy ${n}`
  }

  /** Save a second bench: id 0 forces the insert path, everything else copies. */
  async function duplicate(bench: CustomBench) {
    if (busy) return
    busy = true
    error = null
    try {
      await saveCustomBench({
        ...bench,
        benchmark_id: 0,
        name: copyName(bench.name),
      })
      await load()
      onsaved()
    } catch (e) {
      error = String(e)
    } finally {
      busy = false
    }
  }

  async function exportBench(bench: CustomBench) {
    try {
      const out = await exportCustomBench(bench.benchmark_id)
      transfer = out.json
      transferNote = `Saved to ${out.path}`
    } catch (e) {
      error = String(e)
    }
  }

  async function importBench() {
    if (busy || transfer.trim().length === 0) return
    busy = true
    error = null
    transferNote = null
    try {
      const result = await importCustomBench(transfer)
      transferNote =
        result.warnings.length > 0
          ? `Imported "${result.bench.name}" — dropped ${result.warnings.length}: ${result.warnings.join('; ')}`
          : `Imported "${result.bench.name}".`
      await load()
      onsaved()
    } catch (e) {
      error = String(e)
    } finally {
      busy = false
    }
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
        tier_colors: tierNames.map((_, i) => (tierColors[i] ?? '').trim()),
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
        <div class="preset-picker">
          <select
            aria-label="Start from a benchmark"
            bind:value={presetId}
            onchange={(e) => applyPreset(Number((e.currentTarget as HTMLSelectElement).value))}
          >
            <option value={0}>Start from a benchmark…</option>
            {#each presets as preset (preset.id)}
              <option value={preset.id}>{preset.label}</option>
            {/each}
          </select>
          {#if presetNote}
            <span class="muted small">{presetNote}</span>
          {/if}
        </div>
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
                <button class="btn btn-small" disabled={busy} onclick={() => duplicate(bench)}
                  >Duplicate</button
                >
                <button class="btn btn-small" onclick={() => exportBench(bench)}>Export</button>
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
          <label class="field narrow">
            <span>Bench color</span>
            <input class="color-input" type="color" bind:value={color} />
          </label>
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

        <div class="tier-colors">
          <span class="field-label">Tier colors (worst → best)</span>
          <div class="tier-color-grid">
            {#each tierNames as tier, i (i)}
              <label class="tier-color">
                <input
                  type="color"
                  value={tierColors[i] ?? color}
                  aria-label="{tier} color"
                  oninput={(e) => {
                    const next = tierColors.slice()
                    next[i] = (e.currentTarget as HTMLInputElement).value
                    tierColors = next
                  }}
                />
                <span>{tier}</span>
                {#if tierColorErrors[i]}
                  <span class="row-hint">{tierColorErrors[i]}</span>
                {/if}
              </label>
            {/each}
          </div>
        </div>

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

        <details class="transfer" open={transfer.length > 0}>
          <summary>Import / export a bench file</summary>
          <textarea
            class="transfer-box"
            bind:value={transfer}
            placeholder={'Paste an exported bench JSON here…\n\nExport any bench on the left to fill this box.'}
            rows="5"
          ></textarea>
          <div class="transfer-actions">
            <button
              class="btn btn-small"
              disabled={busy || transfer.trim().length === 0}
              onclick={importBench}>Import from this text</button
            >
            <button
              class="btn btn-small"
              disabled={transfer.length === 0}
              onclick={() => navigator.clipboard.writeText(transfer)}
              >Copy JSON</button
            >
            {#if transferNote}
              <span class="muted small">{transferNote}</span>
            {/if}
          </div>
        </details>
      </section>
    </div>

    <footer class="bench-foot">
      <span class="preview">
        Rank now: <b style={`color: ${previewColor}`}>{previewName}</b>
        {#if rule === 'energy' && preview.energy > 0}
          <span class="muted small">energy {preview.energy.toFixed(2)}</span>
        {/if}
        {#if preview.rank > 0 && preview.rank < tierNames.length}
          <span class="muted small">→ next: {tierNames[preview.rank]}</span>
        {/if}
        <span class="muted small">· {preview.subcategories} subcategories</span>
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
  .color-input {
    width: 44px;
    height: 26px;
    padding: 0;
    border: 1px solid var(--border);
    border-radius: 4px;
    background: var(--surface);
    cursor: pointer;
  }
  .preset-picker {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .preset-picker select {
    width: 100%;
    padding: 5px 6px;
    border: 1px solid var(--border);
    border-radius: 4px;
    background: var(--surface);
    color: var(--text);
  }
  .tier-colors {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .tier-color-grid {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
  }
  .tier-color {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    font-size: 11px;
  }
  .tier-color input[type='color'] {
    width: 30px;
    height: 24px;
    padding: 0;
    border: 1px solid var(--border);
    border-radius: 4px;
    background: var(--surface);
    cursor: pointer;
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
  .transfer summary {
    cursor: pointer;
    font-family: var(--font-display);
    font-size: 10px;
    letter-spacing: 0.12em;
    text-transform: uppercase;
    color: var(--muted);
  }
  .transfer-box {
    width: 100%;
    margin-top: 6px;
    padding: 6px 8px;
    border: 1px solid var(--border);
    border-radius: 4px;
    background: var(--surface);
    color: var(--text);
    font-family: ui-monospace, monospace;
    font-size: 11px;
    resize: vertical;
  }
  .transfer-actions {
    display: flex;
    gap: 8px;
    align-items: center;
    flex-wrap: wrap;
    margin-top: 6px;
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
