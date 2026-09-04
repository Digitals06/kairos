<script lang="ts">
  import { onMount } from 'svelte'
  import {
    getProfile,
    getOverview,
    ingestStatus,
    refreshLocal,
    syncNow,
    getSettings,
    setSettings,
    getBenchmarkDetail,
    toggleFavorite,
    type BenchmarkCard,
    type PlayerProfile,
    type AppSettings,
  } from './lib/api'
  import Setup from './lib/Setup.svelte'
  import BenchmarkCardView from './lib/BenchmarkCardView.svelte'
  import Detail from './lib/Detail.svelte'
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

  async function loadOverview() {
    loadingOverview = true
    try {
      cards = sortCards(await getOverview())
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
  const filteredCards = $derived.by(() => {
    const q = searchQuery.trim().toLowerCase()
    if (!q) return cards
    return cards.filter((c) => c.benchmark_name.toLowerCase().includes(q))
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
    <h1 class="logo">KAIROS</h1>
  </div>
{:else if screen === 'setup'}
  <Setup onconnected={onConnected} />
{:else if profile}
  <div class="app">
    <header class="topbar">
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
            ⚙
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
            </div>
          {/if}
        </div>
      </div>
    </header>

    <main>
      {#if selectedBenchmarkId !== null}
        <Detail benchmarkId={selectedBenchmarkId} onback={closeDetail} />
      {:else if loadingOverview && cards.length === 0}
        <div class="grid">
          {#each Array(6) as _, i (i)}
            <div class="skeleton"></div>
          {/each}
        </div>
      {:else if cards.length === 0}
        <div class="empty panel">
          <p>No benchmarks yet.</p>
          <p class="muted">Hit <strong>Sync Now</strong> to pull your KovaaK's benchmark data.</p>
        </div>
      {:else}
        <div class="search-row">
          <input
            class="search-input"
            type="text"
            placeholder="Filter benchmarks…"
            bind:value={searchQuery}
          />
          {#if searchQuery}
            <button class="btn btn-small" onclick={() => (searchQuery = '')}>clear</button>
          {/if}
        </div>
        {#if filteredCards.length === 0}
          <div class="empty panel">
            <p>No benchmarks match “{searchQuery}”.</p>
          </div>
        {:else}
          <div class="grid">
            {#each filteredCards as card (card.benchmark_id)}
              <BenchmarkCardView
                {card}
                onclick={() => openDetail(card.benchmark_id)}
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

  .logo {
    font-size: 26px;
    letter-spacing: 0.3em;
    color: var(--accent);
    text-shadow: var(--glow-magenta);
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
    padding: 16px 0;
    border-bottom: 1px solid var(--border);
    margin-bottom: 20px;
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
  }

  .empty {
    padding: 40px;
    text-align: center;
  }

  .empty .muted {
    color: var(--muted);
  }
</style>
