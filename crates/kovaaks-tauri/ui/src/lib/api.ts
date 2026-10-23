/**
 * Typed invoke() wrappers mirroring the Rust DTOs in
 * `crates/kovaaks-tauri/src-tauri/src/lib.rs` (serde camelCase on the wire).
 */
import { invoke } from '@tauri-apps/api/core'

/**
 * PlayerProfile comes from the profile resolution command. Not ts-rs
 * generated: it predates the binding workflow and is UI-owned.
 */
export interface PlayerProfile {
  steam_id: string
  persona: string
  avatar_url: string
  country: string
}

// --- DTO mirrors -----------------------------------------------------------
// GENERATED at build time from the Rust DTOs by ts-rs (the single adapter
// between the Rust command seam and the TS client). Source of truth:
// crates/kovaaks-tauri/src-tauri/src/lib.rs — regenerate via
// `cargo test -p kovaaks-tauri export`. Do NOT hand-edit: change the Rust
// struct, rerun tests, wire drift becomes a compile error.
import type { BenchmarkCard as GenBenchmarkCard } from './bindings/BenchmarkCard'
export type BenchmarkCard = GenBenchmarkCard
import type { BenchmarkVariant as GenBenchmarkVariant } from './bindings/BenchmarkVariant'
export type BenchmarkVariant = GenBenchmarkVariant
import type { RankTier as GenRankTier } from './bindings/RankTier'
export type RankTier = GenRankTier
import type { GrindChipDto as GenGrindChip } from './bindings/GrindChipDto'
export type GrindChip = GenGrindChip
import type { GrindTargetDto as GenGrindTarget } from './bindings/GrindTargetDto'
export type GrindTarget = GenGrindTarget
import type { GrindNextDto as GenGrindNext } from './bindings/GrindNextDto'
export type GrindNext = GenGrindNext
import type { ScenarioHistorySeries as GenScenarioHistorySeries } from './bindings/ScenarioHistorySeries'
export type ScenarioHistorySeries = GenScenarioHistorySeries
import type { ScenarioHistoryPoint as GenScenarioHistoryPoint } from './bindings/ScenarioHistoryPoint'
export type ScenarioHistoryPoint = GenScenarioHistoryPoint
import type { PlateauInfo as GenPlateauInfo } from './bindings/PlateauInfo'
export type PlateauInfo = GenPlateauInfo
import type { CategoryCard as GenCategoryCard } from './bindings/CategoryCard'
export type CategoryCard = GenCategoryCard
import type { SnapshotPoint as GenSnapshotPoint } from './bindings/SnapshotPoint'
export type SnapshotPoint = GenSnapshotPoint
import type { PlayPoint as GenPlayPoint } from './bindings/PlayPoint'
export type PlayPoint = GenPlayPoint
import type { BenchmarkDetail as GenBenchmarkDetail } from './bindings/BenchmarkDetail'
export type BenchmarkDetail = GenBenchmarkDetail
import type { IngestStatus as GenIngestStatus } from './bindings/IngestStatus'
export type IngestStatus = GenIngestStatus
import type { SyncReportDto as GenSyncReport } from './bindings/SyncReportDto'
export type SyncReport = GenSyncReport
import type { WeeklyReportDto as GenWeeklyReport } from './bindings/WeeklyReportDto'
export type WeeklyReport = GenWeeklyReport
import type { ImprovementRowDto as GenImprovementRow } from './bindings/ImprovementRowDto'
export type ImprovementRow = GenImprovementRow
import type { RankChangeDto as GenRankChange } from './bindings/RankChangeDto'
export type RankChange = GenRankChange
import type { LevelStepDto as GenLevelStep } from './bindings/LevelStepDto'
export type LevelStep = GenLevelStep
import type { DashboardFamiliesDto as GenDashboardFamilies } from './bindings/DashboardFamiliesDto'
export type DashboardFamilies = GenDashboardFamilies
import type { FamilyRowDto as GenFamilyRow } from './bindings/FamilyRowDto'
export type FamilyRow = GenFamilyRow
import type { FamilyDiffDto as GenFamilyDiff } from './bindings/FamilyDiffDto'
export type FamilyDiff = GenFamilyDiff
import type { AppSettings as GenAppSettings } from './bindings/AppSettings'
export type AppSettings = GenAppSettings

export function resolveProfile(identifier: string): Promise<PlayerProfile> {
  return invoke('resolve_profile', { identifier })
}

export function getProfile(): Promise<PlayerProfile | null> {
  return invoke('get_profile')
}

export function syncNow(deep: boolean): Promise<SyncReport> {
  return invoke('sync_now', { deep })
}

export function rankChanges(): Promise<RankChange[]> {
  return invoke('rank_changes')
}

export function grindOverview(): Promise<GrindChipDto[]> {
  return invoke('grind_overview')
}

export function grindNext(benchmarkId: number): Promise<GrindNext | null> {
  return invoke('grind_next', { benchmarkId })
}

export function launchScenario(scenario: string): Promise<void> {
  return invoke('launch_scenario', { scenario })
}

import type { ReconcileDto } from './bindings/ReconcileDto'
import type { CustomBenchDto as GenCustomBench } from './bindings/CustomBenchDto'
import type { ExportBenchDto as GenExportBench } from './bindings/ExportBenchDto'
export type ExportBench = GenExportBench
import type { ImportBenchDto as GenImportBench } from './bindings/ImportBenchDto'
export type ImportBench = GenImportBench
export type CustomBench = GenCustomBench
import type { CustomScenarioDto as GenCustomScenario } from './bindings/CustomScenarioDto'
export type CustomScenario = GenCustomScenario
import type { ScenarioChoiceDto as GenScenarioChoice } from './bindings/ScenarioChoiceDto'
export type ScenarioChoice = GenScenarioChoice

// Launch the whole difficulty (its shared scenario set) in KovaaK's — one
// click starts the session's full benchmark pass.
export function launchBenchmark(benchmarkId: number): Promise<void> {
  return invoke('launch_benchmark', { benchmarkId })
}

export function exportSeriesCsv(): Promise<string> {
  return invoke('export_series_csv')
}

export function weeklyReport(): Promise<WeeklyReport> {
  return invoke('weekly_report')
}

export function exportBackup(): Promise<string> {
  return invoke('export_backup')
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

export function refreshLocal(): Promise<IngestStatus> {
  return invoke('refresh_local')
}

export function getSettings(): Promise<AppSettings> {
  return invoke('get_settings')
}

export function setSettings(settings: AppSettings): Promise<void> {
  return invoke('set_settings', { settings })
}

export function toggleFavorite(benchmarkId: number): Promise<boolean> {
  return invoke('toggle_favorite', { benchmarkId })
}

export function dashboardRollup(): Promise<DashboardFamilies> {
  return invoke('dashboard_rollup')
}
export function reconcile(benchmarkId: number): Promise<ReconcileDto> {
  return invoke('reconcile', { benchmarkId })
}

// --- custom benchmarks (v0.5.0) --------------------------------------------

export function customBenches(): Promise<CustomBench[]> {
  return invoke('custom_benches')
}

/** Create (`benchmark_id === 0`) or update (`benchmark_id < 0`) a bench. */
export function saveCustomBench(bench: CustomBench): Promise<CustomBench> {
  return invoke('save_custom_bench', { bench })
}

export function deleteCustomBench(benchmarkId: number): Promise<void> {
  return invoke('delete_custom_bench', { benchmarkId })
}

export function knownScenarios(): Promise<ScenarioChoice[]> {
  return invoke('known_scenarios')
}

/** Shareable JSON for one custom bench (also written to Documents). */
export function exportCustomBench(benchmarkId: number): Promise<ExportBench> {
  return invoke('export_custom_bench', { benchmarkId })
}

/** Import a custom bench from exported JSON; returns the saved bench + warnings. */
export function importCustomBench(json: string): Promise<ImportBench> {
  return invoke('import_custom_bench', { json })
}

/** Write a composed PNG (from a canvas) to Pictures/Kairos; returns the path. */
export function saveReportPng(bytes: Uint8Array): Promise<string> {
  return invoke('save_report_png', { bytes })
}
