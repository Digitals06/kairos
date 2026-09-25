<script lang="ts">
  import { slide } from 'svelte/transition'
  import type { BenchmarkCard } from '../api'
  import RankBadge from './RankBadge.svelte'

  const TIER_COLORS: Record<string, string> = {
    Recruit: '#8d99a6', Iron: '#999999', Bronze: '#ff9900', Silver: '#cbd9e6',
    Gold: '#cab148', Platinum: '#4fd1c5', Diamond: '#48bbf7', Master: '#b560f0',
    Grandmaster: '#ff2e88', Nova: '#7ce4a8', Astra: '#7ce4a8', Berry: '#9070d8',
    Pear: '#8ad05f', Cherry: '#e14b65',
  }

  let {
    card,
    onclick,
    onselectvariant,
    ontogglefavorite,
  }: {
    card: BenchmarkCard
    onclick?: () => void
    onselectvariant?: (id: number) => void
    ontogglefavorite?: () => void
  } = $props()

  let expanded = $state(false)

  function onFavClick(e: MouseEvent) {
    e.stopPropagation()
    ontogglefavorite?.()
  }

  function toggleHeader(e: MouseEvent) {
    if ((card.variants?.length ?? 0) > 0) {
      expanded = !expanded
    } else {
      onclick?.()
    }
  }

  function variantClick(e: MouseEvent, id: number) {
    e.stopPropagation()
    onselectvariant?.(id)
  }
</script>

<article
  class="panel card"
  class:favorited={card.is_favorite}
  class:expanded
  style={`--vx:${10 + (card.benchmark_id * 37) % 80}%; --vy:${10 + (card.benchmark_id * 53) % 70}%; --va:${(card.benchmark_id * 71) % 180}deg;`}
>
  <button
    class="fav-btn"
    class:active={card.is_favorite}
    title={card.is_favorite ? 'Remove from favorites' : 'Pin to top'}
    aria-label={card.is_favorite ? 'Remove from favorites' : 'Pin to top'}
    onclick={onFavClick}
  >
    ★
  </button>
  <header
    role="button"
    tabindex="0"
    onclick={toggleHeader}
    onkeydown={(e) => {
      if (e.key === 'Enter' || e.key === ' ') {
        e.preventDefault()
        toggleHeader(e)
      }
    }}
  >
    <div class="names">
      <h3 title={card.benchmark_name}>{card.benchmark_name}</h3>
      {#if (card.difficulty_count ?? 1) === 1}
        <span class="diff">{card.difficulty_name}</span>
      {:else}
        <span class="diff muted-label">best of {card.difficulty_count}</span>
      {/if}
      {#if (card.difficulty_count ?? 1) > 1}
        <span class="diff-count">{expanded ? '▾' : '▸'} {card.difficulty_count}</span>
      {/if}
    </div>
    <RankBadge tier={card.rank} />
  </header>
  {#if expanded}
    <div class="variants" transition:slide={{ duration: 180 }}>
      {#each card.variants ?? [] as v (v.benchmark_id)}
        <button class="variant-row" onclick={(e) => variantClick(e, v.benchmark_id)}>
          <span class="variant-diff">
            {v.difficulty_name}
            {#if (v.runs_to_next ?? 0) > 0}
              <span class="chip-run">▶ {v.runs_to_next} to next tier</span>
            {:else if v.plateaued}
              <span class="chip-plateau">plateaued</span>
            {:else if (v.runs_to_next ?? 0) === 0 && v.rank}
              <span class="chip-run">complete</span>
            {/if}
          </span>
          {#if v.tier_names?.length}
            <span class="rung-bar">
              {#each v.tier_names as t, i}
                <span
                  class="rung"
                  class:lit={i <= v.current_rank}
                  style={`--c:${TIER_COLORS[t] ?? '#566b85'}`}
                  title={t}
                ></span>
              {/each}
            </span>
          {/if}
          <RankBadge tier={v.rank} />
        </button>
      {/each}
    </div>
  {/if}
</article>

<style>
  .card {
    position: relative;
    display: flex;
    align-items: center;
    flex-direction: column;
    padding: 14px 16px 12px 20px;
    cursor: pointer;
    border-radius: var(--radius);
    transition: border-color 0.15s, box-shadow 0.15s, transform 0.15s;
  }
  /* fluted left edge of a marble tablet */
  .card::before {
    content: '';
    position: absolute;
    left: 0;
    top: 0;
    bottom: 0;
    width: 7px;
    background: repeating-linear-gradient(180deg, color-mix(in srgb, var(--text) 9%, transparent) 0 2px, transparent 2px 7px);
    pointer-events: none;
  }
    /* deterministic marble veining, unique per tablet (seeded by benchmark id) */
  .card::after {
    content: '';
    position: absolute;
    inset: 0;
    pointer-events: none;
    border-radius: inherit;
    opacity: 0.55;
    mix-blend-mode: multiply;
    background-image:
      radial-gradient(140% 60% at var(--vx, 50%) var(--vy, 40%), transparent 52%, color-mix(in srgb, #7d715c 30%, transparent) 76%, transparent 96%),
      radial-gradient(220% 90% at calc(100% - var(--vx, 50%)) calc(100% - var(--vy, 30%)), transparent 58%, color-mix(in srgb, #a5988a 34%, transparent) 80%, transparent 97%),
      repeating-linear-gradient(var(--va, 15deg), transparent 0 10px, color-mix(in srgb, #8a7f6d 12%, transparent) 10px 11.5px, transparent 11.5px 24px);
  }

  /* deterministic marble veining, unique per tablet (seeded by benchmark id) */
  .card::after {
    content: '';
    position: absolute;
    inset: 0;
    pointer-events: none;
    border-radius: inherit;
    opacity: 0.5;
    mix-blend-mode: multiply;
    background-image:
      radial-gradient(140% 60% at var(--vx, 50%) var(--vy, 40%), transparent 55%, color-mix(in srgb, #7d715c 26%, transparent) 78%, transparent 96%),
      radial-gradient(220% 90% at calc(100% - var(--vx, 50%)) calc(100% - var(--vy, 30%)), transparent 60%, color-mix(in srgb, #a5988a 30%, transparent) 80%, transparent 97%),
      repeating-linear-gradient(var(--va, 15deg), transparent 0 11px, color-mix(in srgb, #8a7f6d 9%, transparent) 11px 12px, transparent 12px 26px);
  }

  .card:hover {
    transform: translateY(-2px);
    border-color: var(--accent-2);
    box-shadow: 0 10px 22px -14px color-mix(in srgb, var(--text) 55%, transparent);
  }

  .variants {
    width: 100%;
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin-top: 8px;
    padding: 6px 0 2px 12px;
    border-top: 1px solid var(--border);
    position: relative;
  }

  /* carved inscription rows: rule + hover engraving, no boxes */
  .variant-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    background: transparent;
    border: none;
    border-bottom: 1px dotted var(--border);
    border-radius: 0;
    padding: 6px 8px 6px 10px;
    color: inherit;
    cursor: pointer;
    text-align: left;
  }

  .variant-row:last-child { border-bottom: none; }

  .variant-row:hover {
    background: color-mix(in srgb, var(--accent-2) 6%, transparent);
    border-radius: var(--radius);
    border-bottom-color: transparent;
  }

  .variant-row:hover {
    border-color: var(--accent-2);
  }

  .variant-diff {
    font-size: 12px;
    color: var(--muted);
    text-transform: uppercase;
    letter-spacing: 0.08em;
    flex: none;
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .chip-run { animation: chipIn 0.2s ease both;
    font-size: 10px;
    color: var(--accent-2);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 1px 6px;
  }

  .chip-plateau { animation: chipIn 0.2s ease both;
    font-size: 10px;
    color: var(--danger);
    border: 1px solid color-mix(in srgb, var(--danger) 55%, transparent);
    border-radius: 4px;
    padding: 1px 6px;
  }

  .rung-bar {
    flex: 1;
    display: flex;
    gap: 3px;
  }

  .rung {
    flex: 1;
    height: 5px;
    border-radius: 0;
    position: relative;
    background: var(--panel-sunken);
    border: 1px solid var(--border);
    transition: background 0.25s ease;
  }

  .rung.lit {
    background: var(--c);
    box-shadow: 0 1px 0 color-mix(in srgb, #fff 30%, transparent) inset;
  }

  .card.favorited {
    border-color: color-mix(in srgb, var(--accent-2) 60%, transparent);
  }

  .card:hover {
    border-color: var(--accent-2);
    box-shadow: var(--glow-accent-2);
  }

  .card:active {
    transform: scale(0.99);
  }

  .fav-btn {
    position: absolute;
    top: 6px;
    right: 8px;
    background: none;
    border: none;
    padding: 2px;
    font-size: 15px;
    line-height: 1;
    color: var(--muted);
    opacity: 0.35;
    cursor: pointer;
    transition: opacity 0.15s, color 0.15s, text-shadow 0.15s;
  }

  .card:hover .fav-btn {
    opacity: 0.8;
  }

  .fav-btn:hover {
    opacity: 1 !important;
    color: var(--accent);
  }

  .fav-btn.active {
    opacity: 1;
    color: #ffd246;
    text-shadow: 0 0 8px rgba(255, 210, 70, 0.65);
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    width: 100%;
    min-width: 0;
  }

  .names {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }

  h3 {
    font-size: 15px;
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    padding-right: 18px;
  }

  .diff {
    font-size: 11px;
    color: var(--muted);
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  .muted-label {
    opacity: 0.75;
  }

  .diff-count {
    font-size: 10px;
    color: var(--accent-2);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 1px 5px;
  }

  @keyframes chipIn {
    from { opacity: 0; transform: translateY(2px); }
    to { opacity: 1; transform: none; }
  }
</style>
