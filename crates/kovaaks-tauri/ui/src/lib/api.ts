/**
 * Typed invoke() wrappers mirroring the Rust DTOs in
 * `crates/kovaaks-tauri/src-tauri/src/lib.rs` (serde camelCase on the wire).
 */
import { invoke } from '@tauri-apps/api/core'

// --- DTO mirrors -----------------------------------------------------------

export interface RankTier {
  name: string
  color: string
}

export interface BenchmarkCard {
  benchmark_id: number
  benchmark_name: string
  abbreviation: string
  difficulty_name: string
  rank: RankTier | null
  benchmark_progress: number
  next_rank_name: string | null
  next_rank_delta: number | null
  avg_score: number
  high_score: number
  avg_improvement_pct: number | null
  high_improvement_pct: number | null
  samples: number
  last_synced: string | null
  snapshot_history: SnapshotPoint[]
}

export interface SyncReport {
  ok: number
  failed: number
  errors: string[]
}

export interface PlayerProfile {
  steam_id: string
  persona: string
  avatar_url: string
  country: string
}

export interface ScenarioRank {
  scenario: string
  score: number
  leaderboard_rank: number
  tier: RankTier | null
  /** 1-based achieved tier index from the API (0 = unplayed). */
  scenario_rank: number
  /** This scenario's tier thresholds (display units), ascending. */
  rank_maxes: number[]
}

export interface CategoryCard {
  name: string
  progress: number
  rank_tier: RankTier | null
}

export interface SnapshotPoint {
  captured_at: string
  benchmark_progress: number
}

export interface PlayPoint {
  scenario: string
  played_at: string
  score: number
}

export interface ScenarioHistoryPoint {
  captured_at: string
  score: number
}

export interface ScenarioHistorySeries {
  scenario: string
  category: string
  points: ScenarioHistoryPoint[]
}

export interface BenchmarkDetail {
  card: BenchmarkCard
  snapshot_history: SnapshotPoint[]
  plays: PlayPoint[]
  scenario_ranks: ScenarioRank[]
  categories: CategoryCard[]
  scenario_history: ScenarioHistorySeries[]
  /** Rank ladder for the difficulty (name + color, worst → best). */
  rank_tiers: RankTier[]
}

export interface IngestStatus {
  csv_seen: number
  csv_inserted: number
  last_synced_at: string | null
}

export interface AppSettings {
  stats_dir: string
  sync_interval_hours: number
}

// --- command wrappers ------------------------------------------------------

export function resolveProfile(identifier: string): Promise<PlayerProfile> {
  return invoke('resolve_profile', { identifier })
}

export function getProfile(): Promise<PlayerProfile | null> {
  return invoke('get_profile')
}

export function syncNow(deep: boolean): Promise<SyncReport> {
  return invoke('sync_now', { deep })
}

export function getOverview(): Promise<BenchmarkCard[]> {
  return invoke('get_overview')
}

export function getBenchmarkDetail(benchmarkId: number): Promise<BenchmarkDetail> {
  return invoke('get_benchmark_detail', { benchmarkId })
}

export function ingestStatus(): Promise<IngestStatus> {
  return invoke('ingest_status')
}

export function getSettings(): Promise<AppSettings> {
  return invoke('get_settings')
}

export function setSettings(settings: AppSettings): Promise<void> {
  return invoke('set_settings', { settings })
}
