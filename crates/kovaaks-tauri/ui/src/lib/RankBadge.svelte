<script lang="ts">
  import type { RankTier } from '../lib/api'

  let { tier, progress = null }: { tier: RankTier | null; progress?: number | null } = $props()

  // Decode a tier hex color and return readable text (black/white) + a dim
  // variant for the badge background wash.
  function readable(color: string): { fg: string; bg: string } {
    const m = /^#?([0-9a-f]{6})$/i.exec(color.trim())
    if (!m) return { fg: '#0a0e14', bg: color || '#9ca3af' }
    const n = parseInt(m[1], 16)
    const r = (n >> 16) & 0xff
    const g = (n >> 8) & 0xff
    const b = n & 0xff
    const luminance = 0.2126 * r + 0.7152 * g + 0.0722 * b
    return {
      fg: luminance > 140 ? '#0a0e14' : '#ffffff',
      bg: `rgba(${r}, ${g}, ${b}, 0.22)`,
    }
  }

  const style = $derived(
    tier
      ? `color: ${readable(tier.color).fg}; background: ${readable(tier.color).bg};` +
          ` border-color: ${tier.color}; text-shadow: 0 0 8px ${tier.color}66;`
      : ''
  )
</script>

{#if tier}
  <span
    class="badge num"
    style={style}
    title={progress !== null ? `Progress ${progress.toLocaleString()}` : tier.name}
  >
    {tier.name}
  </span>
{:else}
  <span class="badge num unranked" title="No rank data yet">UNRANKED</span>
{/if}

<style>
  .badge {
    display: inline-block;
    padding: 3px 12px;
    border: 1px solid var(--border);
    border-radius: 8px;
    font-size: 12px;
    font-weight: 700;
    letter-spacing: 0.14em;
    text-transform: uppercase;
    white-space: nowrap;
  }

  .unranked {
    color: var(--muted);
    background: rgba(156, 163, 175, 0.08);
    border-color: var(--border);
    text-shadow: none;
  }
</style>
