<script lang="ts">
  import type { BenchmarkCard } from '../api'
  import RankBadge from './RankBadge.svelte'

  let {
    card,
    onclick,
    ontogglefavorite,
  }: { card: BenchmarkCard; onclick?: () => void; ontogglefavorite?: () => void } = $props()

  function onFavClick(e: MouseEvent) {
    e.stopPropagation()
    ontogglefavorite?.()
  }
</script>

<article
  class="panel card"
  class:favorited={card.is_favorite}
  role="button"
  tabindex="0"
  onclick={onclick}
  onkeydown={(e) => {
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault()
      onclick?.()
    }
  }}
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
  <header>
    <div class="names">
      <h3 title={card.benchmark_name}>{card.benchmark_name}</h3>
      <span class="diff">{card.difficulty_name}</span>
    </div>
    <RankBadge tier={card.rank} />
  </header>
</article>

<style>
  .card {
    position: relative;
    display: flex;
    align-items: center;
    padding: 14px 16px;
    cursor: pointer;
    transition: border-color 0.15s, box-shadow 0.15s, transform 0.15s;
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
</style>
