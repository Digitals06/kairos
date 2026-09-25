/**
 * Navigation module: screen routing, analytics backflow, and theme state.
 *
 * Extracted from App.svelte (candidate 5 of the 2026-09 architecture
 * review): the "am I on which screen" knowledge used to live as loose
 * $state vars in the root orchestrator, threaded into children via
 * props. One module owns it now — small interface, all transitions
 * inside (locality), children consume named stores (leverage).
 */

export type Screen = 'loading' | 'setup' | 'overview'
export type ThemeChoice = 'marble' | 'system' | 'basalt'

export const nav = $state({
  screen: 'loading' as Screen,
  /// Detail page currently open (KovaaK's benchmark id), null = none.
  selectedBenchmarkId: null as number | null,
  /// Analytics entry: the benchmark the user clicked "All scenarios" from.
  analyticsBenchmarkId: null as number | null,
})

export function openDetail(benchmarkId: number) {
  nav.selectedBenchmarkId = benchmarkId
}

export function closeDetail() {
  nav.selectedBenchmarkId = null
}

export function openAnalytics(fromBenchmarkId: number | null) {
  nav.analyticsBenchmarkId = fromBenchmarkId
}

export function closeAnalytics() {
  nav.analyticsBenchmarkId = null
}

export function showOverview() {
  nav.screen = 'overview'
}

export function showSetup() {
  nav.screen = 'setup'
}

// ---- theme state ----------------------------------------------------------

/// The app's live `$state` type. True = prefers light/marble.
const themeMedia =
  typeof matchMedia === 'function'
    ? matchMedia('(prefers-color-scheme: light)')
    : null

export const themeState = $state({
  choice: 'basalt' as ThemeChoice,
  resolved: 'basalt' as 'marble' | 'basalt',
})

function currentSystemTheme(): 'marble' | 'basalt' {
  return themeMedia?.matches ? 'marble' : 'basalt'
}

export function applyTheme() {
  themeState.resolved =
    themeState.choice === 'system' ? currentSystemTheme() : themeState.choice
  document.documentElement.dataset.theme = themeState.resolved
}

/// Follow OS theme flips while the user is on "system".
themeMedia?.addEventListener('change', () => {
  if (themeState.choice === 'system') applyTheme()
})
