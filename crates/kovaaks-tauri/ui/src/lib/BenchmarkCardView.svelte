<script lang="ts">
  import type { BenchmarkCard } from '../api'
  import RankBadge from './RankBadge.svelte'

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

<article class="panel card" class:favorited={card.is_favorite} class:expanded>
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
      <span class="diff">{card.difficulty_name}</span>
      {#if (card.difficulty_count ?? 1) > 1}
        <span class="diff-count">{expanded ? '▾' : '▸'} {card.difficulty_count}</span>
      {/if}
    </div>
    <RankBadge tier={card.rank} />
  </header>
  {#if expanded}
    <div class="variants">
      {#each card.variants ?? [] as v (v.benchmark_id)}
        <button class="variant-row" onclick={(e) => variantClick(e, v.benchmark_id)}>
          <span class="variant-diff">{v.difficulty_name}</span>
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
    padding: 14px 16px;
    cursor: pointer;
    transition: border-color 0.15s, box-shadow 0.15s, transform 0.15s;
  }

  .variants {
    width: 100%;
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin-top: 10px;
    padding-top: 8px;
    border-top: 1px solid var(--border);
  }

  .variant-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    background: transparent;
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 6px 10px;
    color: inherit;
    cursor: pointer;
    text-align: left;
  }

  .variant-row:hover {
    border-color: var(--accent-2);
  }

  .variant-diff {
    font-size: 12px;
    color: var(--muted);
    text-transform: uppercase;
    letter-spacing: 0.08em;
  }

  .card.favorited {
    border-color: rgba(255, 210, 70, 0.55);
  }

  .card:hover {
    border-color: var(--accent-2);
    box-shadow: var(--glow-cyan);
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

  .diff-count {
    font-size: 10px;
    color: var(--accent-2, #00e5ff);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 1px 5px;
  }
</style>
