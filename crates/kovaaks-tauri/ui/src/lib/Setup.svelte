<script lang="ts">
  import { getProfile, resolveProfile } from './api'
  import type { PlayerProfile } from './api'

  import { humanError } from './errors'

  let { onconnected }: { onconnected: (p: PlayerProfile) => void } = $props()

  let identifier = $state('')
  let connecting = $state(false)
  let error = $state<string | null>(null)

  async function connect(e: SubmitEvent) {
    e.preventDefault()
    const id = identifier.trim()
    if (!id || connecting) return
    connecting = true
    error = null
    try {
      const profile = await resolveProfile(id)
      onconnected(profile)
    } catch (err) {
      // Stay on Setup — the message renders inline below the field.
      error = humanError(err)
    } finally {
      connecting = false
    }
  }

  // Re-check on mount in case a profile got connected in another window.
  $effect(() => {
    getProfile().catch(() => {})
  })
</script>

<div class="setup">
  <div class="brand">
    <h1>KAIROS</h1>
    <p class="tagline">KovaaK's benchmark companion</p>
  </div>

  <form class="panel connect" onsubmit={connect}>
    <label for="identifier">Connect your Steam profile</label>
    <input
      id="identifier"
      class="field"
      bind:value={identifier}
      placeholder="SteamID64 · vanity URL · profile URL"
      autocomplete="off"
      spellcheck="false"
      disabled={connecting}
    />
    <button class="btn btn-accent" type="submit" disabled={connecting || !identifier.trim()}>
      {connecting ? 'Connecting…' : 'Connect'}
    </button>
    {#if error}
      <p class="error">{error}</p>
    {/if}
    <p class="hint">
      Accepts a 17-digit SteamID64, a
      <span class="num">steamcommunity.com/id/&lt;vanity&gt;</span> URL, or a full profile URL.
    </p>
  </form>
</div>

<style>
  .setup {
    min-height: 100vh;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 36px;
    padding: 24px;
  }

  .brand h1 {
    font-size: 42px;
    letter-spacing: 0.32em;
    color: var(--accent);
    text-shadow: var(--glow-magenta);
    text-align: center;
  }

  .tagline {
    margin: 8px 0 0;
    text-align: center;
    color: var(--muted);
    letter-spacing: 0.12em;
    text-transform: uppercase;
    font-size: 12px;
  }

  .connect {
    width: min(480px, 100%);
    padding: 28px;
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  label {
    font-weight: 600;
    letter-spacing: 0.04em;
  }

  .hint {
    margin: 0;
    color: var(--muted);
    font-size: 12px;
  }

  .error {
    margin: 0;
    color: var(--danger);
    font-size: 13px;
  }
</style>
