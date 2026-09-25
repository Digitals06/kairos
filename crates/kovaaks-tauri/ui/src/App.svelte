<script lang="ts">
import { listen } from '@tauri-apps/api/event'
  import { getCurrentWindow } from '@tauri-apps/api/window'
  import { onMount } from 'svelte'
  import {
    getProfile,
    getOverview,
    ingestStatus,
    refreshLocal,
    syncNow,
    rankChanges,
    getSettings,
    setSettings,
    getBenchmarkDetail,
    toggleFavorite,
    exportBackup,
    exportSeriesCsv,
    type BenchmarkCard,
    type PlayerProfile,
    type AppSettings,
    weeklyReport,
    type WeeklyReport as WeeklyRep,
  } from './lib/api'
  import Setup from './lib/Setup.svelte'
  import BenchmarkCardView from './lib/BenchmarkCardView.svelte'
  import Detail from './lib/Detail.svelte'
  import Weekly from './lib/Weekly.svelte'
    import { humanError } from './lib/errors'

  // --- app flow state --------------------------------------------------------
  type Screen = 'loading' | 'setup' | 'overview'
  let screen = $state<Screen>('loading')

  let profile = $state<PlayerProfile | null>(null)
  let cards = $state<BenchmarkCard[]>([])
  let searchQuery = $state('')
  let loadingOverview = $state(false)
  let toast = $state<string | null>(null)
  let toastTimer: ReturnType<typeof setTimeout> | undefined

  function showToast(msg: string) {
    toast = msg
    clearTimeout(toastTimer)
    toastTimer = setTimeout(() => (toast = null), 5000)
  }

  // --- theme engine (Kairos identity: marble/basalt) -------------------------
  type ThemeName = 'marble' | 'basalt' | 'system'
  let theme = $state<ThemeName>('system')
  let resolvedTheme = $state<'marble' | 'basalt'>('basalt')

  const themeMedia = typeof matchMedia === 'function' ? matchMedia('(prefers-color-scheme: light)') : null

  function currentSystemTheme(): 'marble' | 'basalt' {
    return themeMedia?.matches ? 'marble' : 'basalt'
  }

  function applyTheme() {
    resolvedTheme = theme === 'system' ? currentSystemTheme() : theme
    document.documentElement.dataset.theme = resolvedTheme
    try { localStorage.setItem('kairos-theme', theme) } catch {}
  }

  $effect(() => {
    try { theme = (localStorage.getItem('kairos-theme') as ThemeName) ?? 'system' } catch { theme = 'system' }
    applyTheme()
    themeMedia?.addEventListener('change', () => { if (theme === 'system') applyTheme() })
  })

  // --- frameless window controls (custom titlebar) ---------------------------
  const appWindow = getCurrentWindow()
  async function winMin() { await appWindow.minimize() }
  async function winToggle() {
    if (await appWindow.isMaximized()) { await appWindow.unmaximize() } else { await appWindow.maximize() }
  }
  async function winClose() { await appWindow.close() }

  // --- sync bar state --------------------------------------------------------
  let syncing = $state(false)
  let lastSyncedAt = $state<string | null>(null)

  const STALE_MS = 12 * 60 * 60 * 1000 // 12h

  // Ticks once a minute so the stale badge flips without a sync.
  let now = $state(Date.now())
  $effect(() => {
    const t = setInterval(() => (now = Date.now()), 60_000)
    return () => clearInterval(t)
  })

  // Live CSV watcher (backend): new local plays land -> refresh the current
  // view without any click. Strictly local; ranks still change on Sync Now.
  $effect(() => {
    let unlisten: (() => void) | undefined
    listen<{ seen: number; inserted: number }>('local-plays-updated', (e) => {
      if (e.payload.inserted > 0) {
        loadOverview()
        refreshLastSynced()
      }
    }).then((fn) => (unlisten = fn))
    return () => unlisten?.()
  })

  function refreshLastSynced() {
    ingestStatus()
      .then((s) => (lastSyncedAt = s.last_synced_at))
      .catch(() => {})
  }

  const isStale = $derived(
    lastSyncedAt !== null && now - new Date(lastSyncedAt).getTime() > STALE_MS,
  )

  function fmtLastSynced(): string {
    if (!lastSyncedAt) return 'never'
    const t = new Date(lastSyncedAt)
    const mins = Math.max(0, Math.floor((now - t.getTime()) / 60_000))
    if (mins < 1) return 'just now'
    if (mins < 60) return `${mins}m ago`
    const h = Math.floor(mins / 60)
    return `${h}h ${mins % 60}m ago`
  }

  async function doSync(deep: boolean) {
    if (syncing) return
    syncing = true
    try {
      const report = await syncNow(deep)
      refreshLastSynced()
      await loadOverview()
      if (report.failed > 0) {
        showToast(`Sync finished with ${report.failed} failure(s): ${report.errors[0] ?? ''}`)
      }
      // Rank-up/down surfacing: engine-computed diffs between the last two
      // snapshots per benchmark. Summary toast when several change at once.
      try {
        const changes = (await rankChanges()).filter((c) => c.curName || c.prevName)
        const ups = changes.filter((c) => c.improved)
        const downs = changes.filter((c) => !c.improved)
        if (ups.length === 1 && downs.length === 0) {
          const c = ups[0]
          showToast(`RANK UP — ${c.benchmarkName}: ${c.prevName || 'Unranked'} → ${c.curName}`)
        } else if (downs.length === 1 && ups.length === 0) {
          const c = downs[0]
          showToast(`${c.benchmarkName}: ${c.prevName} → ${c.curName}`)
        } else if (changes.length > 1) {
          const parts = changes.slice(0, 3).map((c) => `${c.benchmarkName} ${c.prevName || '—'}→${c.curName || '—'}`)
          const extra = changes.length - 3
          const dir = ups.length >= downs.length ? 'up' : 'down'
          showToast(`${changes.length} rank changes (${ups.length} ${dir}): ${parts.join(', ')}${extra > 0 ? ` +${extra} more` : ''}`)
        }
      } catch {
        /* rank diff is best-effort; never fail the sync toast path */
      }
    } catch (err) {
      showToast(humanError(err))
    } finally {
      syncing = false
    }
  }

  let refreshingLocal = $state(false)

  async function doRefreshLocal() {
    if (refreshingLocal) return
    refreshingLocal = true
    try {
      const status = await refreshLocal()
      refreshLastSynced()
      await loadOverview()
      showToast(`Local refresh: ${status.csv_inserted} new play(s) from ${status.csv_seen} CSV file(s)`)
    } catch (err) {
      showToast(humanError(err))
    } finally {
      refreshingLocal = false
    }
  }

  let exportingCsv = $state(false)
  let exporting = $state(false)
  async function doExportSeriesCsv() {
    if (exportingCsv) return
    exportingCsv = true
    try {
      const path = await exportSeriesCsv()
      showToast(`Series CSV written: ${path}`)
    } catch (err) {
      showToast(humanError(err))
    } finally {
      exportingCsv = false
    }
  }

  async function doExportBackup() {
    if (exporting) return
    exporting = true
    try {
      const path = await exportBackup()
      showToast(`Backup written: ${path}`)
    } catch (err) {
      showToast(humanError(err))
    } finally {
      exporting = false
    }
  }

  // --- settings dropdown -----------------------------------------------------
  let settingsOpen = $state(false)
  let deepScan = $state(false)
  // `deep` is a per-call flag on sync_now, not persisted server-side; map the
  // toggle onto the sync interval (0h = deep mode) so it survives restarts.
  let deepScanDirty = $state(true)
  let settingsEl: HTMLDivElement | undefined = $state()

  async function toggleSettings() {
    settingsOpen = !settingsOpen
    if (settingsOpen && deepScanDirty) {
      try {
        const s = await getSettings()
        deepScan = s.sync_interval_hours === 0
        deepScanDirty = false
      } catch {
        /* keep current toggle state */
      }
    }
  }

  async function applyDeepScan(on: boolean) {
    deepScan = on
    try {
      const s: AppSettings = await getSettings()
      await setSettings({ ...s, sync_interval_hours: on ? 0 : Math.max(1, s.sync_interval_hours || 6) })
    } catch (err) {
      showToast(`Could not save settings: ${String(err)}`)
    }
  }

  function onWindowClick(e: MouseEvent) {
    if (settingsOpen && settingsEl && !settingsEl.contains(e.target as Node)) {
      settingsOpen = false
    }
  }

  // --- data loading ----------------------------------------------------------

  // Card order: favorited benchmarks pinned on top, then alphabetical.
  // Single comparator shared by initial load and local favorite toggles
  // so a reload can never silently drop pins to the bottom of the list.
  function sortCards<T extends { is_favorite: boolean; benchmark_name: string }>(
    list: T[],
  ): T[] {
    return [...list].sort(
      (a, b) =>
        (b.is_favorite ? 1 : 0) - (a.is_favorite ? 1 : 0) ||
        a.benchmark_name.localeCompare(b.benchmark_name),
    )
  }

  let weekly = $state<WeeklyRep | null>(null)

  async function loadOverview() {
    loadingOverview = true
    try {
      cards = sortCards(await getOverview())
      try {
        weekly = await weeklyReport()
      } catch (e) {
        // Non-fatal: frieze stays hidden; daily cache may still be computing.
        setTimeout(() => weeklyReport().then((r) => (weekly = r)).catch(() => {}), 4000)
      }
    } catch (err) {
      showToast(`Failed to load overview: ${String(err)}`)
    } finally {
      loadingOverview = false
    }
  }

  onMount(async () => {
    try {
      profile = await getProfile()
    } catch (err) {
      showToast(String(err))
      screen = 'setup'
      return
    }
    if (!profile) {
      screen = 'setup'
      return
    }
    screen = 'overview'
    refreshLastSynced()
    loadOverview()
    // Auto-sync on launch: smart-sync keeps it cheap when nothing changed
    // (no new CSVs -> only stale rows probed). Fire-and-forget; failures
    // surface through the sync toast path.
    void doSync(false)
  })

  function onConnected(p: PlayerProfile) {
    profile = p
    screen = 'overview'
    refreshLastSynced()
    loadOverview()
  }

  // --- detail drill-down (client-side state, no router lib) -------------------
  let selectedBenchmarkId = $state<number | null>(null)

  // --- search filter + favorites ----------------------------------------------
  // Benchmark-type filter (evxl-style tabs): empty selection = show all.
  const ALL_TYPES = [
    'Mixed',
    'Tracking',
    'Static',
    'Clicking',
    'Ground',
    'Precise',
    'Smooth',
    'Micro',
    'Dynamic',
    'Reactive',
    'Evasive',
    'Switching',
  ] as const
  let activeTypes = $state<string[]>([])

  function toggleType(ty: string) {
    activeTypes = activeTypes.includes(ty)
      ? activeTypes.filter((x) => x !== ty)
      : [...activeTypes, ty]
  }

  const filteredCards = $derived.by(() => {
    const q = searchQuery.trim().toLowerCase()
    return cards.filter((c) => {
      if (q && !c.benchmark_name.toLowerCase().includes(q)) return false
      if (
        activeTypes.length > 0 &&
        !c.benchmark_types.some((ty) => activeTypes.includes(ty))
      )
        return false
      return true
    })
  })

  async function toggleFavoriteLocal(benchmarkId: number) {
    try {
      const nowFavorite = await toggleFavorite(benchmarkId)
      const card = cards.find((c) => c.benchmark_id === benchmarkId)
      if (card) card.is_favorite = nowFavorite
      // Re-sort locally to match the backend ordering (favorites on top).
      cards = sortCards(cards)
    } catch (err) {
      showToast(humanError(err))
    }
  }

  function openDetail(id: number) {
    selectedBenchmarkId = id
  }

  function closeDetail() {
    selectedBenchmarkId = null
  }
</script>

<svelte:window onclick={onWindowClick} />

{#if screen === 'loading'}
  <div class="boot">
    <h1 class="boot-logo display">ΚΑΙΡΟΣ</h1>
  </div>
{:else if screen === 'setup'}
  <Setup onconnected={onConnected} />
{:else if profile}
  <div class="app">
    <header class="topbar" data-tauri-drag-region>
      <h1 class="logo">KAIROS</h1>

      <div class="profile-chip" title={profile.steam_id}>
        {#if profile.avatar_url}
          <img src={profile.avatar_url} alt="" referrerpolicy="no-referrer" />
        {:else}
          <span class="avatar-fallback">{profile.persona.slice(0, 1).toUpperCase()}</span>
        {/if}
        <span class="persona">{profile.persona}</span>
      </div>

      <div class="sync-cluster">
        <span class="last-synced num" class:stale={isStale}>
          {fmtLastSynced()}{#if isStale}<span class="stale-badge">STALE</span>{/if}
        </span>
        <button class="btn btn-accent" onclick={() => doSync(deepScan)} disabled={syncing}>
          {#if syncing}
            <span class="spinner" aria-hidden="true"></span>
          {/if}
          {syncing ? 'Syncing…' : 'Sync Now'}
        </button>

        <button class="btn" onclick={() => doRefreshLocal()} disabled={refreshingLocal}>
          {#if refreshingLocal}
            <span class="spinner" aria-hidden="true"></span>
          {/if}
          {refreshingLocal ? 'Scanning…' : 'Refresh Local'}
        </button>

        <div class="settings-wrap" bind:this={settingsEl}>
          <button
            class="btn icon-btn"
            aria-label="Settings"
            aria-expanded={settingsOpen}
            onclick={toggleSettings}
          >
            <span class="gear-glyph" aria-hidden="true"></span>
          </button>
          {#if settingsOpen}
            <div class="dropdown panel">
              <label class="row">
                <span>Deep Scan<br /><small>re-probe every benchmark, ignore cache</small></span>
                <input
                  type="checkbox"
                  checked={deepScan}
                  onchange={(e) => applyDeepScan(e.currentTarget.checked)}
                />
              </label>
              <button class="btn" onclick={() => doExportBackup()} disabled={exporting}>
                {exporting ? 'Exporting…' : 'Export data…'}
              </button>
              <button class="btn" onclick={() => doExportSeriesCsv()} disabled={exportingCsv}>
                {exportingCsv ? 'Exporting…' : 'Export CSV…'}
              </button>
            <div class="theme-row" role="radiogroup" aria-label="Theme">
              <span class="theme-label">Theme</span>
              {#each ['marble', 'system', 'basalt'] as it (it)}
                <button
                  class="theme-opt"
                  class:active={theme === it}
                  onclick={() => { theme = it as ThemeName; applyTheme() }}
                >
                  {it === 'marble' ? 'Marble' : it === 'basalt' ? 'Basalt' : 'System'}
                </button>
              {/each}
            </div>
            </div>
          {/if}
        </div>
      </div>
    
        <div class="win-controls" role="group" aria-label="Window controls">
          <button class="win-btn" onclick={winMin} aria-label="Minimize"><span class="g g-min"></span></button>
          <button class="win-btn" onclick={winToggle} aria-label="Maximize"><span class="g g-max"></span></button>
          <button class="win-btn close" onclick={winClose} aria-label="Close"><span class="g g-close"></span></button>
        </div>
      </header>

    <main>
      {#if selectedBenchmarkId === null}
        <header class="pediment display">
          <span class="roof" aria-hidden="true"></span>
          <span class="cornice" aria-hidden="true"></span>
          <h1 class="gable">ΚΑΙΡΟΣ</h1>
          <p class="epigraph"><small>καιρός — the opportune moment</small></p>
          <span class="meander" aria-hidden="true"></span>
        </header>

      {/if}
      {#if selectedBenchmarkId !== null}
        <Detail benchmarkId={selectedBenchmarkId} onback={closeDetail} />
      {:else}
        <Weekly shared={weekly} />
        <div class="search-row">
          <input
            class="search-input"
            type="text"
            placeholder="Filter benchmarks…"
            bind:value={searchQuery}
          />
          {#if searchQuery || activeTypes.length > 0}
            <button
              class="btn btn-small"
              onclick={() => {
                searchQuery = ''
                activeTypes = []
              }}>clear</button
            >
          {/if}
        </div>
        <div class="type-tabs">
          {#each ALL_TYPES as ty (ty)}
            <button
              class="type-tab"
              class:active={activeTypes.includes(ty)}
              onclick={() => toggleType(ty)}>{ty}</button
            >
          {/each}
        </div>
        {#if filteredCards.length === 0}
          <div class="empty panel">
            <p>
              No benchmarks match “{searchQuery}”{activeTypes.length > 0
                ? ` in ${activeTypes.join(', ')}`
                : ''}.
            </p>
          </div>
        {:else}
          <h2 class="section-title display">Stoa <span>στοά — the colonnade of benchmarks</span></h2>
          <div class="grid">
            {#each filteredCards as card (card.benchmark_id)}
              <BenchmarkCardView
                {card}
                onclick={() => openDetail(card.benchmark_id)}
                onselectvariant={(id) => openDetail(id)}
                ontogglefavorite={() => toggleFavoriteLocal(card.benchmark_id)}
              />
            {/each}
          </div>
        {/if}
      {/if}
    </main>
  </div>
{/if}

{#if toast}
  <div class="toast" role="alert">{toast}</div>
{/if}

<style>
  .boot {
    min-height: 100vh;
    display: grid;
    place-items: center;
  }

  .pediment {
    position: relative;
    text-align: center;
    padding: 54px 0 10px;
    margin-bottom: 8px;
  }

  .pediment .roof {
    position: absolute;
    left: -6px;
    right: -6px;
    top: 0;
    height: clamp(44px, 6.5vw, 68px);
    background: linear-gradient(180deg, var(--panel-raised), var(--chrome));
    clip-path: polygon(50% 0, 100% 100%, 0 100%);
    filter: drop-shadow(0 2px 0 color-mix(in srgb, var(--text) 16%, transparent));
  }

  .pediment .cornice {
    position: absolute;
    left: 2px;
    right: 2px;
    top: clamp(44px, 6.5vw, 68px);
    height: 3px;
    background: linear-gradient(90deg, transparent, var(--border-strong) 12%, var(--border-strong) 88%, transparent);
  }

  .pediment .gable {
    position: relative;
    font-size: 34px;
    font-weight: 900;
    letter-spacing: 0.24em;
    color: var(--accent);
    margin: 0;
    font-family: var(--font-display);
  }

  .pediment .epigraph {
    margin: 4px 0 8px;
  }

  .pediment .epigraph small {
    font-family: var(--font-serif);
    font-size: 13px;
    font-style: italic;
    text-transform: none;
    letter-spacing: 0.06em;
    color: var(--muted);
  }

  .pediment .meander {
    height: 10px;
    width: min(560px, 92%);
    margin: 0 auto;
    background: var(--meander);
    opacity: 0.55;
  }

  .logo { display: none; }

  .boot-logo {
    font-family: var(--font-display);
    font-size: 34px;
    font-weight: 900;
    letter-spacing: 0.2em;
    color: var(--accent);
  }

    .app {
    max-width: 1280px;
    margin: 0 auto;
    padding: 0 24px 40px;
  }

  /* --- top bar -------------------------------------------------------------- */
  .topbar {
    display: flex;
    align-items: center;
    gap: 18px;
    padding: 16px 14px;
    margin: 0 -14px 20px;          /* band stretch into the chrome zone */
    background: var(--chrome);
    border-bottom: 1px solid var(--border-strong);
    box-shadow: 0 1px 0 color-mix(in srgb, var(--text) 5%, transparent);
  }

  .profile-chip {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 4px 12px 4px 4px;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 999px;
  }

  .profile-chip img,
  .avatar-fallback {
    width: 26px;
    height: 26px;
    border-radius: 50%;
    object-fit: cover;
  }

  .avatar-fallback {
    display: grid;
    place-items: center;
    background: var(--panel-raised);
    border: 1px solid var(--accent-2);
    color: var(--accent-2);
    font-weight: 700;
  }

  .persona {
    max-width: 160px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .sync-cluster {
    margin-left: auto;
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .last-synced {
    font-size: 11px;
    color: var(--muted);
  }

  .last-synced.stale {
    color: #f59e0b;
  }

  .stale-badge {
    margin-left: 6px;
    padding: 1px 6px;
    border: 1px solid #f59e0b;
    border-radius: 6px;
    font-size: 10px;
    font-weight: 700;
    letter-spacing: 0.1em;
  }

  .spinner {
    width: 12px;
    height: 12px;
    border: 2px solid rgba(255, 46, 136, 0.3);
    border-top-color: var(--accent);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  .icon-btn {
    padding: 8px 10px;
  }

  .settings-wrap {
    position: relative;
  }

  .dropdown {
    position: absolute;
    right: 0;
    top: calc(100% + 8px);
    width: 260px;
    padding: 12px;
    z-index: 100;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.5);
  }

  .dropdown .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    cursor: pointer;
  }

  .dropdown small {
    color: var(--muted);
    font-weight: 400;
  }

  .dropdown input {
    accent-color: var(--accent);
    width: 16px;
    height: 16px;
  }

  /* --- search ---------------------------------------------------------------- */
  .type-tabs {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin: 10px 0 14px;
  }
  .type-tab {
    font: inherit;
    font-size: 12px;
    padding: 3px 12px;
    border-radius: 999px;
    border: 1px solid var(--border, #1f2937);
    background: transparent;
    color: var(--muted-foreground, #9ca3af);
    cursor: pointer;
    transition:
      color 0.15s,
      border-color 0.15s,
      background 0.15s;
  }
  .type-tab:hover {
    color: var(--foreground);
  }
  .type-tab.active {
    color: #0a0e14;
    background: var(--accent, #ff2e88);
    border-color: var(--accent, #ff2e88);
    font-weight: 600;
  }
  .search-row {
    display: flex;
    gap: 8px;
    align-items: center;
    margin-bottom: 16px;
  }

  .search-input {
    flex: 1;
    max-width: 420px;
    background: var(--bg);
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: 10px;
    padding: 9px 14px;
    font-size: 13px;
  }

  .search-input::placeholder {
    color: var(--muted);
  }

  .search-input:focus {
    outline: none;
    border-color: var(--accent-2);
    box-shadow: 0 0 10px rgba(0, 229, 255, 0.25);
  }

  /* --- grid ------------------------------------------------------------------ */
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(300px, 1fr));
    gap: 14px;
    align-items: start;   /* expanding a tablet must not stretch its row-mates */
  }

  .empty {
    padding: 40px;
    text-align: center;
  }

  .empty .muted {
    color: var(--muted);
  }

  /* settings as ancient coin-boss wheel: ring + center hub dot */
  :global(.gear-glyph) {
    position: relative;
    display: block;
    width: 14px;
    height: 14px;
  }
  :global(.gear-glyph)::before {
    content: '';
    position: absolute;
    inset: 0;
    border: 1.5px solid currentColor;
    border-radius: 50%;
    box-shadow:
      0 0 0 2px transparent,
      0 -3px 0 -1px currentColor, 0 3px 0 -1px currentColor,
      -3px 0 0 -1px currentColor, 3px 0 0 -1px currentColor,
      -2px 2px 0 -1.5px currentColor, 2px -2px 0 -1.5px currentColor,
      2px 2px 0 -1.5px currentColor, -2px -2px 0 -1.5px currentColor;
  }
  :global(.gear-glyph)::after {
    content: '';
    position: absolute;
    top: 50%;
    left: 50%;
    width: 4px;
    height: 4px;
    transform: translate(-50%, -50%);
    background: currentColor;
    border-radius: 50%;
  }

  /* frameless window: titlebar merges with the app chrome */
  .win-controls {
    display: flex;
    gap: 4px;
    margin-left: auto;
  }

  .win-btn {
    width: 34px;
    height: 26px;
    display: grid;
    place-items: center;
    border: 1px solid var(--border-strong);
    outline: 1px solid var(--border);
    outline-offset: 1px;
    border-radius: var(--radius);
    background: linear-gradient(180deg, var(--panel-raised), var(--panel));
    color: var(--muted);
    box-shadow: 0 1px 0 color-mix(in srgb, var(--text) 8%, transparent) inset;
  }

  .win-btn:hover {
    color: var(--accent);
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--accent) 12%, transparent);
  }

  .win-btn.close:hover {
    color: var(--danger);
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--danger) 14%, transparent);
  }

  .g { display: block; position: relative; filter: drop-shadow(0 0.5px 0.5px color-mix(in srgb, var(--text) 25%, transparent)); }
  .g::before, .g::after {
    content: '';
    position: absolute;
    background: currentColor;
    border-radius: 0.5px;
  }
  /* minimize — a single carved underside bar (larger, bevel-tipped) */
  .g-min { width: 12px; height: 12px; }
  .g-min::before {
    bottom: 2px; left: 0.5px;
    width: 11px; height: 2.5px;
    clip-path: polygon(0 0, 100% 0, calc(100% - 1.5px) 100%, 1.5px 100%);
  }
  /* maximize — a chisel-cut open square, 2.2px stroke with corner notches */
  .g-max { width: 10px; height: 10px; }
  .g-max::before {
    inset: 0;
    background: none;
    border: 2.2px solid currentColor;
    clip-path: polygon(0 0, 100% 0, 100% calc(100% - 2px), calc(100% - 2px) 100%, 0 100%, 0 2px, 2px 0);
  }
  /* close — broad chisel x (2.6px strokes with bevel tips) */
  .g-close { width: 12px; height: 12px; }
  .g-close::before, .g-close::after {
    top: 4.7px; left: 0.5px;
    width: 11px; height: 2.6px;
    border-radius: 0;
    clip-path: polygon(0 0, calc(100% - 1px) 0, 100% 100%, 1px 100%);
  }
  .g-close::before { transform: rotate(45deg); }
  .g-close::after  { transform: rotate(-45deg); }

  .win-btn.close:hover {
    background: var(--danger);
    border-color: var(--danger);
    color: #fff;
  }
</style>
