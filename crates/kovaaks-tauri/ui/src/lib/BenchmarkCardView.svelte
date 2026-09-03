<script lang="ts">
  import type { BenchmarkCard } from '../api'
  import RankBadge from './RankBadge.svelte'

  let { card, onclick }: { card: BenchmarkCard; onclick?: () => void } = $props()
</script>

<article
  class="panel card"
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
    display: flex;
    align-items: center;
    padding: 14px 16px;
    cursor: pointer;
    transition: border-color 0.15s, box-shadow 0.15s, transform 0.15s;
  }

  .card:hover {
    border-color: var(--accent-2);
    box-shadow: var(--glow-cyan);
  }

  .card:active {
    transform: scale(0.99);
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
  }

  .diff {
    font-size: 11px;
    color: var(--muted);
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }
</style>
