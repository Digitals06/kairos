<script lang="ts">
  import type { RankTier } from '../lib/api'

  let { tier, progress = null }: { tier: RankTier | null; progress?: number | null } = $props()

  const color = $derived(tier ? tier.color : 'var(--faint)')
  const label = $derived(tier ? tier.name : 'UNRANKED')
</script>

{#if tier}
  <span
    class="seal num"
    style={`--ink:${color}`}
    title={progress !== null ? `Progress ${progress.toLocaleString()}` : tier.name}
  >
    <b>{label}</b>
  </span>
{:else}
  <span class="seal num none" title="No rank data yet">
    <b>UNRANKED</b>
  </span>
{/if}

<style>
  /* ancient coin seal — official tier ink; sized so every rank fits one line */
  .seal {
    position: relative;
    flex: none;
    width: 62px;
    height: 62px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    text-align: center;
    font-family: var(--font-display);
    font-size: 8.5px;
    font-weight: 700;
    line-height: 1.05;
    padding: 6px;
    border: 2px solid var(--ink);
    color: var(--ink);
    background:
      radial-gradient(circle at 32% 28%, color-mix(in srgb, var(--text) 6%, transparent), transparent 60%),
      var(--panel-raised);
    overflow-wrap: anywhere;
    hyphens: auto;
    transition: transform 0.18s ease, box-shadow 0.18s ease;
  }

  .seal::after {
    content: '';
    position: absolute;
    inset: 3px;
    border-radius: 50%;
    border: 1px dotted currentColor;
    opacity: 0.45;
  }

  /* coin catches the light on hover */
  button:hover > .seal,
  .variant-row:hover .seal {
    transform: translateY(-2px) rotateX(12deg) rotateZ(-2deg);
    box-shadow: 0 4px 10px color-mix(in srgb, var(--text) 22%, transparent);
  }

  .seal b { display: block; }
  .seal.none { color: var(--faint); font-size: 7px; }
</style>
