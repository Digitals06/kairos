<script lang="ts">
  /**
   * Discord share card (v0.5.0).
   *
   * Composes a 1200×630 card (Discord's embed aspect) from data the stoa
   * already holds: the weekly plaque and the top benchmarks. The card is drawn
   * as SVG, previewed as an image, then rasterized on demand so it can be
   * copied straight into a chat or saved as a PNG.
   */
  import { invoke } from '@tauri-apps/api/core'
  import { saveReportPng, type BenchmarkCard } from './api'

  type Weekly = {
    days_played: number
    plays: number
    scenarios_played: number
    pb_events: number
    current_streak: number
    level_name: string
    level: number
    level_progress_pct: number
    plays_per_day: number[]
    improvements?: { scenario: string; delta: number }[]
  } | null

  let { weekly, cards, onclose }: { weekly: Weekly; cards: BenchmarkCard[]; onclose: () => void } =
    $props()

  const W = 1200
  const H = 630
  let busy = $state<'copy' | 'save' | null>(null)
  let note = $state<string | null>(null)
  let error = $state<string | null>(null)

  const GOLD = '#d4ad2f'
  const CREAM = '#f0e6d2'
  const MUTED = '#b3a488'
  const BG = '#14100c'
  const PANEL = '#1d1811'
  const LINE = '#4a3f30'

  const esc = (s: string) =>
    s
      .replace(/&/g, '&amp;')
      .replace(/</g, '&lt;')
      .replace(/>/g, '&gt;')
      .replace(/"/g, '&quot;')

  /** Up to five ranked benchmarks, in the grid's order (favorites first). */
  const topBenches = $derived(cards.filter((c) => c.rank).slice(0, 5))

  const dayBars = $derived(
    Array.from({ length: 7 }, (_, i) => ({ plays: weekly?.plays_per_day?.[i] ?? 0 })),
  )
  const maxDay = $derived(Math.max(1, ...dayBars.map((d) => d.plays)))

  const svg = $derived.by(() => {
    const parts: string[] = []
    const text = (
      x: number,
      y: number,
      value: string,
      size: number,
      fill: string,
      opts: { family?: string; weight?: string; anchor?: string; spacing?: number } = {},
    ) =>
      parts.push(
        `<text x="${x}" y="${y}" font-family="${opts.family ?? 'Georgia, serif'}" font-size="${size}" fill="${fill}"` +
          `${opts.weight ? ` font-weight="${opts.weight}"` : ''}` +
          `${opts.anchor ? ` text-anchor="${opts.anchor}"` : ''}` +
          `${opts.spacing ? ` letter-spacing="${opts.spacing}"` : ''}>${esc(value)}</text>`,
      )
    const rule = (x1: number, y1: number, x2: number, y2: number) =>
      parts.push(
        `<line x1="${x1}" y1="${y1}" x2="${x2}" y2="${y2}" stroke="${LINE}" stroke-width="1"/>`,
      )

    parts.push(`<rect width="${W}" height="${H}" fill="${BG}"/>`)
    parts.push(
      `<rect x="18" y="18" width="${W - 36}" height="${H - 36}" fill="none" stroke="${LINE}" rx="6"/>`,
    )

    // Wordmark block
    text(60, 108, 'ΚΑΙΡΟΣ', 58, GOLD, { spacing: 6 })
    text(60, 136, 'καιρός — the opportune moment', 15, MUTED)
    rule(60, 156, W - 60, 156)

    // Weekly block (left)
    text(60, 196, 'THIS WEEK — ἑβδομάς', 13, MUTED, { spacing: 3 })
    const plays = weekly?.plays ?? 0
    const playsText = String(plays)
    text(60, 268, playsText, 64, CREAM, { weight: 'bold' })
    text(60 + playsText.length * 36 + 12, 268, plays === 1 ? 'play' : 'plays', 18, MUTED)
    text(
      60,
      306,
      `${weekly?.pb_events ?? 0} PBs · streak ${weekly?.current_streak ?? 0} · ${weekly?.days_played ?? 0}/7 days`,
      16,
      CREAM,
    )

    // Level bar
    const levelName = weekly?.level_name ?? '—'
    const levelPct = weekly?.level_progress_pct ?? 0
    text(60, 356, `LEVEL ${weekly?.level ?? 0} · ${levelName}`, 13, MUTED, { spacing: 3 })
    parts.push(`<rect x="60" y="368" width="420" height="10" fill="${PANEL}" stroke="${LINE}"/>`)
    const fillW = levelPct > 0 ? Math.max(8, Math.round((420 * Math.min(100, levelPct)) / 100)) : 0
    if (fillW > 0) {
      parts.push(`<rect x="60" y="368" width="${fillW}" height="10" fill="${GOLD}"/>`)
    }
    text(60, 400, `${levelPct}% to the next step`, 12, MUTED)

    // Week strip
    const barW = 44
    const gap = 12
    dayBars.forEach((day, i) => {
      const x = 60 + i * (barW + gap)
      const h = Math.round((86 * day.plays) / maxDay)
      parts.push(
        `<rect x="${x}" y="${498 - h}" width="${barW}" height="${Math.max(h, 2)}" fill="${day.plays > 0 ? GOLD : PANEL}" stroke="${LINE}"/>`,
      )
      text(x + barW / 2, 520, String(day.plays), 14, day.plays > 0 ? CREAM : MUTED, {
        anchor: 'middle',
        family: 'ui-monospace, monospace',
      })
    })
    const stripW = 7 * barW + 6 * gap
    text(60 + stripW / 2, 546, 'plays per day', 13, MUTED, { anchor: 'middle' })

    // Benchmarks (right)
    const rx = 620
    text(rx, 196, 'BENCHMARKS', 13, MUTED, { spacing: 3 })
    if (topBenches.length === 0) {
      text(rx, 240, 'No ranked benchmarks yet.', 16, MUTED)
    }
    topBenches.forEach((bench, i) => {
      const y = 240 + i * 48
      parts.push(`<circle cx="${rx + 5}" cy="${y - 6}" r="5" fill="${bench.rank?.color ?? MUTED}"/>`)
      const name =
        bench.benchmark_name.length > 30
          ? `${bench.benchmark_name.slice(0, 29)}…`
          : bench.benchmark_name
      text(rx + 20, y, name, 20, CREAM)
      text(W - 60, y, bench.rank?.name ?? '', 20, GOLD, { anchor: 'end' })
      rule(rx, y + 18, W - 60, y + 18)
    })

    // Top movers (only when the week has any, so an empty card stays clean)
    const movers = (weekly?.improvements ?? []).slice(0, 2)
    if (movers.length > 0) {
      text(rx, 500, 'TOP MOVERS', 13, MUTED, { spacing: 3 })
      movers.forEach((row, i) => {
        const y = 532 + i * 26
        const name =
          row.scenario.length > 32 ? `${row.scenario.slice(0, 31)}…` : row.scenario
        text(rx, y, name, 15, CREAM)
        text(W - 60, y, `+${Math.round(row.delta).toLocaleString()}`, 15, GOLD, { anchor: 'end' })
      })
    }

    // Footer
    text(60, H - 46, 'Free, no login · your data stays local', 14, MUTED)
    text(W - 60, H - 46, 'github.com/Digitals06/kairos', 14, GOLD, { anchor: 'end' })

    return `<svg xmlns="http://www.w3.org/2000/svg" width="${W}" height="${H}" viewBox="0 0 ${W} ${H}">${parts.join('')}</svg>`
  })

  const previewUrl = $derived(`data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`)

  async function raster(scale = 2): Promise<HTMLCanvasElement> {
    const img = new Image()
    const blob = new Blob([svg], { type: 'image/svg+xml' })
    const url = URL.createObjectURL(blob)
    await new Promise<void>((resolve, reject) => {
      img.onload = () => resolve()
      img.onerror = () => reject(new Error('card rasterize failed'))
      img.src = url
    })
    const canvas = document.createElement('canvas')
    canvas.width = W * scale
    canvas.height = H * scale
    const ctx = canvas.getContext('2d')!
    ctx.scale(scale, scale)
    ctx.drawImage(img, 0, 0)
    URL.revokeObjectURL(url)
    return canvas
  }

  async function copy() {
    if (busy) return
    busy = 'copy'
    error = null
    note = null
    try {
      const canvas = await raster()
      const data = canvas.getContext('2d')!.getImageData(0, 0, canvas.width, canvas.height)
      await invoke('copy_report_image', {
        width: canvas.width,
        height: canvas.height,
        rgba: new Uint8Array(data.data.buffer),
      })
      note = 'Copied — paste it into Discord.'
    } catch (e) {
      error = String(e)
    } finally {
      busy = null
    }
  }

  async function save() {
    if (busy) return
    busy = 'save'
    error = null
    note = null
    try {
      const canvas = await raster()
      const blob = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, 'image/png'))
      if (!blob) throw new Error('PNG encode failed')
      const bytes = new Uint8Array(await blob.arrayBuffer())
      const path = await saveReportPng(bytes)
      note = `Saved to ${path}`
    } catch (e) {
      error = String(e)
    } finally {
      busy = null
    }
  }
</script>

<div class="modal-backdrop" role="presentation">
  <div class="panel card-modal" role="dialog" aria-label="Share card">
    <header class="card-head">
      <h2 class="display">Share card</h2>
      <button class="btn btn-small" onclick={onclose}>Close</button>
    </header>

    <img class="card-preview" src={previewUrl} alt="Kairos share card preview" width={W} height={H} />

    {#if error}
      <p class="card-error">{error}</p>
    {:else if note}
      <p class="card-note">{note}</p>
    {/if}

    <footer class="card-foot">
      <span class="muted small">1200 × 630 · rendered at 2× for copy and save</span>
      <div class="card-actions">
        <button class="btn btn-small" disabled={busy !== null} onclick={copy}>
          {busy === 'copy' ? 'Copying…' : 'Copy image'}
        </button>
        <button class="btn" disabled={busy !== null} onclick={save}>
          {busy === 'save' ? 'Saving…' : 'Save PNG'}
        </button>
      </div>
    </footer>
  </div>
</div>

<style>
  .modal-backdrop {
    position: fixed;
    inset: 0;
    background: rgb(0 0 0 / 60%);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 45;
    padding: 24px;
  }
  .card-modal {
    width: min(1240px, 100%);
    max-height: 92vh;
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 16px 18px;
    overflow: auto;
  }
  .card-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .card-head h2 {
    margin: 0;
    font-size: 15px;
  }
  .card-preview {
    width: 100%;
    height: auto;
    border: 1px solid var(--border);
    border-radius: 4px;
    display: block;
  }
  .card-foot {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    flex-wrap: wrap;
  }
  .card-actions {
    display: flex;
    gap: 8px;
  }
  .card-note {
    margin: 0;
    color: var(--accent);
    font-size: 13px;
  }
  .card-error {
    margin: 0;
    color: var(--accent-2);
    font-size: 13px;
  }
  .muted {
    color: var(--muted);
  }
  .small {
    font-size: 11px;
  }
</style>
