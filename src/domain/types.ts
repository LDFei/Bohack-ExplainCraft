export type JobStatus = 'queued' | 'running' | 'waiting_input' | 'cancelling' | 'succeeded' | 'failed' | 'cancelled'
export type StageKey = 'planning' | 'script' | 'storyboard' | 'visuals' | 'audio' | 'edit' | 'finishing' | 'render'
export type Intent = 'generate' | 'revise' | 'chat'
export type Theme = 'coffee' | 'travel' | 'product'
export type DemoScenario = 'normal' | 'failure' | 'question'

export interface Settings {
  target_duration_seconds: number
  aspect_ratio: string
  language: string
  style_id: string
}
export interface AudioSettings {
  voice_id: string
  speech_rate: number
  music_mode: 'auto' | 'none' | 'library' | 'upload'
  music_id: string | null
  music_asset_id: string | null
  resolved_music_asset_id?: string | null
  narration_volume: number
  music_volume: number
}
export interface Finishing {
  color_preset_id: string
  effect_preset_id: string
  subtitles_enabled: boolean
}
export interface Shot {
  id: string
  duration_ms: number
  narration: string
  visual_prompt: string
  shot_size: string
  camera_motion: string
  asset_ids: string[]
  thumbnail_url?: string
}
export interface Draft {
  brief: { subject: string; audience: string; purpose: string; style: string } | null
  settings: Settings
  script: { title: string; text: string } | null
  shots: Shot[]
  audio: AudioSettings
  finishing: Finishing
  invalidated_stages: StageKey[]
}
export interface Project {
  id: string
  title: string
  status: 'draft' | 'processing' | 'waiting_input' | 'ready' | 'failed'
  draft_revision: number
  active_job_id: string | null
  latest_video_version_id: string | null
  draft_dirty: boolean
  created_at: string
  updated_at: string
}
export interface Asset {
  id: string
  project_id: string
  kind: 'image' | 'video' | 'audio'
  source: 'upload' | 'generated' | 'library' | 'rendered'
  status: 'processing' | 'ready' | 'failed'
  file_name: string
  mime_type: string
  size_bytes: number
  thumbnail_url: string | null
  duration_ms: number | null
  width: number | null
  height: number | null
  error: { code: string; message: string } | null
}
export interface MessageBlock {
  type: 'plan' | 'progress' | 'video' | 'question' | 'error' | 'script'
  summary?: string
  job_id?: string
  video_version_id?: string
  question_id?: string
}
export interface Message {
  id: string
  project_id: string
  role: 'user' | 'assistant'
  text: string
  status: 'pending' | 'streaming' | 'completed' | 'failed' | 'cancelled'
  asset_ids: string[]
  job_id: string | null
  blocks: MessageBlock[]
  last_delta_seq: number
  created_at: string
}
export interface Stage {
  key: StageKey
  status: 'pending' | 'running' | 'succeeded' | 'failed' | 'skipped' | 'cancelled'
  progress_percent: number | null
  message: string
}
export interface Job {
  id: string
  project_id: string
  parent_job_id: string | null
  kind: Intent | 'export'
  status: JobStatus
  current_stage: StageKey | null
  progress_percent: number | null
  stages: Stage[]
  question: { id: string; prompt: string; input_type: 'text' | 'single_select'; options: { value: string; label: string }[]; expires_at: string } | null
  result: { video_version_id?: string; draft_revision?: number; assistant_message_id?: string; asset_id?: string } | null
  error: { code: string; message: string; retryable: boolean; failed_stage: StageKey | null } | null
  created_at: string
  started_at: string | null
  finished_at: string | null
}
export interface VideoVersion {
  id: string
  project_id: string
  version_number: number
  source_draft_revision: number
  source_job_id: string
  created_at: string
  video_asset_id: string
  thumbnail_url: string
  duration_ms: number
  width: number
  height: number
  playback_url: string
  url_expires_at: string
  snapshot: Draft
  exports: { asset_id: string; resolution: string; fps: number; format: string; created_at: string }[]
}
export interface Snapshot {
  project: Project
  draft: Draft
  active_job: Job | null
  latest_video: VideoVersion | null
  messages_page: { items: Message[]; next_cursor: string | null }
  event_cursor: string
}
export interface Capabilities {
  defaults: Settings & { voice_id: string; music_mode: 'auto'; color_preset_id: string; effect_preset_id: string; subtitles_enabled: boolean }
  duration_range_seconds: { min: number; max: number }
  aspect_ratios: string[]
  languages: string[]
  styles: { id: string; name: string }[]
  voices: { id: string; name: string; sample_url: string | null }[]
  music_tracks: { id: string; name: string; sample_url: string | null }[]
  color_presets: { id: string; name: string }[]
  effect_presets: { id: string; name: string }[]
  exports: { formats: string[]; resolutions: string[]; fps: number[]; preview_resolution: string }
  upload_limits: { max_uploaded_files_per_project: number; image: FileLimit; video: FileLimit; audio: FileLimit }
  event_retention_seconds: number
}
export interface FileLimit { max_bytes: number; mime_types: string[] }
export interface MessageInput {
  intent: Intent
  text: string
  asset_ids: string[]
  expected_draft_revision: number
  base_video_version_id: string | null
}
export type DraftChanges = Partial<Pick<Draft, 'settings' | 'script' | 'shots' | 'audio' | 'finishing'>>
export interface DemoRun {
  input: MessageInput
  scenario: DemoScenario
  theme: Theme
  started_ms: number
  cancelled_ms?: number
  answered: boolean
  checkpoint_revision: number
}
export interface ProjectRecord {
  project: Project
  draft: Draft
  messages: Message[]
  assets: Asset[]
  versions: VideoVersion[]
  jobs: Job[]
  runs: Record<string, DemoRun>
  event_seq: number
}
export interface Database { schema_version: 1; projects: ProjectRecord[] }
