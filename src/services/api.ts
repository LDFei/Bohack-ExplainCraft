import { advance, answerJob, cancelJob, createRecord, isActive, makeId, patchDraft, retryJob, seedDatabase, snapshot, startJob, StudioError } from '../domain/engine.ts'
import { capabilities, copy } from '../domain/content.ts'
import type { Asset, Capabilities, Database, DemoScenario, DraftChanges, Job, MessageInput, Project, ProjectRecord, Snapshot, VideoVersion } from '../domain/types.ts'

const STORAGE_KEY = 'lingjing-studio-v1'
const objectUrls = new Map<string, string>()
let fallback: Database | null = null
function readDatabase(): Database {
  try {
    const text = localStorage.getItem(STORAGE_KEY)
    if (text) {
      const saved = JSON.parse(text) as Database
      if (saved.schema_version === 1 && Array.isArray(saved.projects)) return saved
    }
  } catch { /* A blocked or damaged browser store falls back to this session. */ }
  return fallback ??= seedDatabase()
}
function writeDatabase(database: Database) {
  fallback = copy(database)
  try { localStorage.setItem(STORAGE_KEY, JSON.stringify(database)) }
  catch { throw new StudioError('STORAGE_FULL', '本机存储空间不足，请清理不需要的演示项目后重试。') }
}
function findRecord(database: Database, projectId: string) {
  const record = database.projects.find(item => item.project.id === projectId)
  if (!record) throw new StudioError('NOT_FOUND', '这个项目不存在或已被删除，可以新建一份创作。')
  return record
}
function openFiles(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open('lingjing-media', 1)
    request.onupgradeneeded = () => request.result.createObjectStore('files')
    request.onerror = () => reject(new StudioError('FILE_STORAGE_ERROR', '浏览器暂时无法保存附件，请检查本机存储权限。'))
    request.onsuccess = () => resolve(request.result)
  })
}
async function saveFile(id: string, file: Blob) {
  const database = await openFiles()
  try {
    await new Promise<void>((resolve, reject) => {
      const transaction = database.transaction('files', 'readwrite')
      transaction.objectStore('files').put(file, id)
      transaction.oncomplete = () => resolve()
      transaction.onerror = () => reject(new StudioError('FILE_STORAGE_ERROR', '附件保存失败，请检查剩余存储空间。'))
    })
  } finally { database.close() }
}
async function fileUrl(id: string) {
  if (objectUrls.has(id)) return objectUrls.get(id)!
  const database = await openFiles()
  try {
    const file = await new Promise<Blob | undefined>((resolve, reject) => {
      const request = database.transaction('files').objectStore('files').get(id)
      request.onsuccess = () => resolve(request.result as Blob | undefined)
      request.onerror = () => reject(request.error)
    })
    if (!file) return null
    const url = URL.createObjectURL(file); objectUrls.set(id, url); return url
  } finally { database.close() }
}
async function hydrate(record: ProjectRecord): Promise<ProjectRecord> {
  const result = copy(record)
  for (const asset of result.assets.filter(item => item.source === 'upload')) {
    const url = await fileUrl(asset.id)
    if (!url) { asset.status = 'failed'; asset.error = { code: 'FILE_MISSING', message: '本机附件已被清理，请重新上传。' }; asset.thumbnail_url = null }
    else if (asset.kind === 'image') asset.thumbnail_url = url
  }
  for (const draft of [result.draft, ...result.versions.map(version => version.snapshot)]) {
    for (const shot of draft.shots) {
      const asset = result.assets.find(item => shot.asset_ids.includes(item.id))
      if (asset?.kind === 'image' && asset.thumbnail_url) shot.thumbnail_url = asset.thumbnail_url
    }
  }
  return result
}
export interface StudioApi {
  readonly mode: 'demo' | 'http'
  getCapabilities(): Promise<Capabilities>
  listProjects(): Promise<Project[]>
  createProject(): Promise<Project>
  getSnapshot(id: string): Promise<Snapshot>
  renameProject(id: string, title: string): Promise<void>
  deleteProject(id: string): Promise<void>
  sendMessage(id: string, input: MessageInput, key: string, scenario?: DemoScenario): Promise<void>
  patchDraft(id: string, changes: DraftChanges, revision: number): Promise<void>
  listAssets(id: string): Promise<Asset[]>
  upload(id: string, file: File, onProgress: (percent: number) => void): Promise<Asset>
  getJob(id: string): Promise<Job>
  cancel(id: string): Promise<void>
  retry(id: string, revision: number): Promise<void>
  answer(id: string, question: string, value: string): Promise<void>
  listVersions(id: string): Promise<VideoVersion[]>
  getVersion(id: string, versionId: string): Promise<VideoVersion>
  exportVideo(id: string, versionId: string, resolution: string): Promise<{ asset_id?: string; job_id?: string; cached?: boolean }>
  download(assetId: string): Promise<{ download_url: string; file_name: string }>
  watch(id: string, cursor: string, onChange: () => void, onError: () => void): () => void
}

class DemoApi implements StudioApi {
  readonly mode = 'demo' as const
  private received = new Map<string, string>()
  async getCapabilities() { return copy(capabilities) }
  async listProjects() {
    const database = readDatabase()
    for (const record of database.projects) advance(record, Date.now())
    writeDatabase(database)
    return database.projects.map(item => copy(item.project)).sort((a, b) => b.updated_at.localeCompare(a.updated_at))
  }
  async createProject() { const database = readDatabase(); const record = createRecord(); database.projects.unshift(record); writeDatabase(database); return copy(record.project) }
  async getSnapshot(id: string) {
    const database = readDatabase(); const record = findRecord(database, id)
    advance(record, Date.now()); writeDatabase(database)
    return snapshot(await hydrate(record))
  }
  async renameProject(id: string, title: string) {
    if (!title.trim() || title.length > 100) throw new StudioError('VALIDATION_ERROR', '标题需要在 1～100 字之间。')
    const database = readDatabase(); findRecord(database, id).project.title = title.trim(); writeDatabase(database)
  }
  async deleteProject(id: string) {
    const database = readDatabase(); const record = findRecord(database, id)
    if (record.project.active_job_id) throw new StudioError('PROJECT_BUSY', '请先停止当前制作，再删除项目。')
    database.projects = database.projects.filter(item => item.project.id !== id); writeDatabase(database)
    const files = await openFiles()
    try {
      await new Promise<void>((resolve, reject) => {
        const tx = files.transaction('files', 'readwrite')
        for (const asset of record.assets) { tx.objectStore('files').delete(asset.id); const url = objectUrls.get(asset.id); if (url) URL.revokeObjectURL(url); objectUrls.delete(asset.id) }
        tx.oncomplete = () => resolve(); tx.onerror = () => reject(tx.error)
      })
    } finally { files.close() }
  }
  async sendMessage(id: string, input: MessageInput, key: string, scenario: DemoScenario = 'normal') {
    const fingerprint = JSON.stringify({ id, input })
    if (this.received.has(key)) { if (this.received.get(key) !== fingerprint) throw new StudioError('IDEMPOTENCY_CONFLICT', '请为新的请求使用不同的标识。'); return }
    const database = readDatabase(); startJob(findRecord(database, id), input, Date.now(), scenario); writeDatabase(database); this.received.set(key, fingerprint)
  }
  async patchDraft(id: string, changes: DraftChanges, revision: number) { const database = readDatabase(); patchDraft(findRecord(database, id), changes, revision, Date.now()); writeDatabase(database) }
  async listAssets(id: string) { return (await hydrate(findRecord(readDatabase(), id))).assets }
  async upload(id: string, file: File, onProgress: (percent: number) => void) {
    const kind = file.type.startsWith('image/') ? 'image' : file.type.startsWith('video/') ? 'video' : file.type.startsWith('audio/') ? 'audio' : null
    if (!kind || !capabilities.upload_limits[kind].mime_types.includes(file.type)) throw new StudioError('UNSUPPORTED_FILE', '支持 JPG、PNG、WebP、MP4、WebM、MP3 和 WAV 文件。')
    const limit = capabilities.upload_limits[kind].max_bytes
    if (file.size > limit) throw new StudioError('FILE_TOO_LARGE', `文件超过 ${Math.round(limit / 1024 / 1024)}MB，请压缩后再上传。`)
    if (!file.size) throw new StudioError('INVALID_FILE', '这个文件是空的，请重新选择。')
    if (findRecord(readDatabase(), id).assets.length >= capabilities.upload_limits.max_uploaded_files_per_project) throw new StudioError('FILE_LIMIT', '每个演示项目最多上传 20 个素材。')
    onProgress(10)
    let width: number | null = null, height: number | null = null, duration: number | null = null
    if (kind === 'image') {
      try { const bitmap = await createImageBitmap(file); width = bitmap.width; height = bitmap.height; bitmap.close() }
      catch { throw new StudioError('INVALID_FILE', '这张图片无法读取，请选择其他文件。') }
    } else {
      const url = URL.createObjectURL(file)
      try {
        const metadata = await new Promise<{ width: number | null; height: number | null; duration: number }>((resolve, reject) => {
          const element = document.createElement(kind === 'video' ? 'video' : 'audio')
          const timeout = window.setTimeout(() => { element.removeAttribute('src'); element.load(); reject(new StudioError('INVALID_FILE', '媒体解析超时，请选择其他文件。')) }, 10000)
          const clear = () => { clearTimeout(timeout); element.removeAttribute('src'); element.load() }
          element.onloadedmetadata = () => { const result = { width: element instanceof HTMLVideoElement ? element.videoWidth : null, height: element instanceof HTMLVideoElement ? element.videoHeight : null, duration: element.duration }; clear(); Number.isFinite(result.duration) ? resolve(result) : reject(new StudioError('INVALID_FILE', '无法读取媒体时长。')) }
          element.onerror = () => { clear(); reject(new StudioError('INVALID_FILE', '此媒体已损坏或当前浏览器无法解码。')) }
          element.preload = 'metadata'; element.src = url
        })
        width = metadata.width; height = metadata.height; duration = Math.round(metadata.duration * 1000)
      } finally { URL.revokeObjectURL(url) }
    }
    const asset: Asset = { id: makeId('asset'), project_id: id, kind, source: 'upload', status: 'ready', file_name: file.name, mime_type: file.type, size_bytes: file.size, thumbnail_url: null, duration_ms: duration, width, height, error: null }
    onProgress(60); await saveFile(asset.id, file)
    const database = readDatabase(); findRecord(database, id).assets.push(asset); writeDatabase(database); onProgress(100)
    return (await hydrate(findRecord(database, id))).assets.find(item => item.id === asset.id)!
  }
  async getJob(id: string) { const job = readDatabase().projects.flatMap(item => item.jobs).find(item => item.id === id); if (!job) throw new StudioError('NOT_FOUND', '没有找到制作任务。'); return copy(job) }
  private mutateJob(id: string, action: (record: ProjectRecord) => void) { const database = readDatabase(); const record = database.projects.find(item => item.jobs.some(job => job.id === id)); if (!record) throw new StudioError('NOT_FOUND', '没有找到制作任务。'); advance(record, Date.now()); action(record); writeDatabase(database) }
  async cancel(id: string) { this.mutateJob(id, record => cancelJob(record, id, Date.now())) }
  async retry(id: string, revision: number) { this.mutateJob(id, record => retryJob(record, id, revision, Date.now())) }
  async answer(id: string, question: string, value: string) { this.mutateJob(id, record => answerJob(record, id, question, value, Date.now())) }
  async listVersions(id: string) { return copy(findRecord(readDatabase(), id).versions).reverse() }
  async getVersion(id: string, versionId: string) { const version = (await hydrate(findRecord(readDatabase(), id))).versions.find(item => item.id === versionId); if (!version) throw new StudioError('NOT_FOUND', '这个视频版本不存在。'); return version }
  async exportVideo(id: string, versionId: string, resolution: string) { if (resolution !== '720p') throw new StudioError('UNSUPPORTED_EXPORT', '演示素材支持 720p 导出。'); const version = await this.getVersion(id, versionId); return { asset_id: version.video_asset_id, cached: true } }
  async download(assetId: string) {
    for (const record of readDatabase().projects) {
      const version = record.versions.find(item => item.video_asset_id === assetId)
      if (version) return { download_url: version.playback_url, file_name: `${version.snapshot.script?.title ?? record.project.title}-v${version.version_number}.mp4` }
      const asset = record.assets.find(item => item.id === assetId)
      if (asset) { const url = await fileUrl(asset.id); if (url) return { download_url: url, file_name: asset.file_name } }
    }
    throw new StudioError('NOT_FOUND', '文件暂时不可用，请重新选择。')
  }
  watch(id: string, _cursor: string, onChange: () => void, onError: () => void) {
    const timer = window.setInterval(() => {
      try { const database = readDatabase(); const record = findRecord(database, id); if (record.project.active_job_id) { const before = record.event_seq; advance(record, Date.now()); if (before !== record.event_seq) { writeDatabase(database); onChange() } } }
      catch { onError() }
    }, 700)
    return () => clearInterval(timer)
  }
}

class HttpApi implements StudioApi {
  readonly mode = 'http' as const
  private base = (import.meta.env.VITE_API_BASE_URL || '/api/v1').replace(/\/$/, '')
  private headers(): Record<string, string> { const token = sessionStorage.getItem('lingjing_access_token') || (import.meta.env.DEV ? import.meta.env.VITE_API_TOKEN : ''); return token ? { Authorization: `Bearer ${token}` } : {} }
  private async request<T>(path: string, method = 'GET', body?: unknown, key?: string): Promise<T> {
    const response = await fetch(`${this.base}${path}`, { method, headers: { ...this.headers(), ...(body ? { 'Content-Type': 'application/json' } : {}), ...(key ? { 'Idempotency-Key': key } : {}) }, body: body ? JSON.stringify(body) : undefined })
    if (response.status === 204) return undefined as T
    const payload = await response.json()
    if (!response.ok) throw new StudioError(payload.error?.code || 'REQUEST_FAILED', payload.error?.message || '请求失败，请稍后再试。')
    return payload.data as T
  }
  getCapabilities() { return this.request<Capabilities>('/capabilities') }
  async listProjects() { return (await this.request<{ items: Project[] }>('/projects?limit=100')).items }
  async createProject() { return (await this.request<{ project: Project }>('/projects', 'POST', {}, makeId('key'))).project }
  getSnapshot(id: string) { return this.request<Snapshot>(`/projects/${id}`) }
  async renameProject(id: string, title: string) { await this.request(`/projects/${id}`, 'PATCH', { title }) }
  async deleteProject(id: string) { await this.request(`/projects/${id}`, 'DELETE') }
  async sendMessage(id: string, input: MessageInput, key: string) { await this.request(`/projects/${id}/messages`, 'POST', input, key) }
  async patchDraft(id: string, changes: DraftChanges, revision: number) { await this.request(`/projects/${id}/draft`, 'PATCH', { changes, expected_draft_revision: revision }) }
  async listAssets(id: string) { return (await this.request<{ items: Asset[] }>(`/projects/${id}/assets?limit=100`)).items }
  async upload(id: string, file: File, onProgress: (percent: number) => void) {
    const body = new FormData(); body.append('file', file); onProgress(0)
    const response = await fetch(`${this.base}/projects/${id}/assets`, { method: 'POST', headers: { ...this.headers(), 'Idempotency-Key': makeId('key') }, body })
    const payload = await response.json()
    if (!response.ok) throw new StudioError(payload.error?.code || 'UPLOAD_FAILED', payload.error?.message || '上传失败。')
    let asset = payload.data.asset as Asset
    for (let attempt = 0; asset.status === 'processing' && attempt < 30; attempt++) {
      await new Promise(resolve => setTimeout(resolve, 2000))
      asset = (await this.request<{ asset: Asset }>(`/assets/${asset.id}`)).asset
    }
    if (asset.status !== 'ready') throw new StudioError('ASSET_NOT_READY', asset.error?.message || '素材仍在解析，请稍后在素材面板查看。')
    onProgress(100); return asset
  }
  async getJob(id: string) { return (await this.request<{ job: Job }>(`/jobs/${id}`)).job }
  async cancel(id: string) { await this.request(`/jobs/${id}/cancel`, 'POST') }
  async retry(id: string, revision: number) { await this.request(`/jobs/${id}/retry`, 'POST', { expected_draft_revision: revision }, makeId('key')) }
  async answer(id: string, question: string, value: string) { await this.request(`/jobs/${id}/answers`, 'POST', { question_id: question, value }, makeId('key')) }
  async listVersions(id: string) { return (await this.request<{ items: VideoVersion[] }>(`/projects/${id}/versions?limit=100`)).items }
  async getVersion(id: string, versionId: string) { return (await this.request<{ version: VideoVersion }>(`/projects/${id}/versions/${versionId}`)).version }
  exportVideo(id: string, versionId: string, resolution: string) { return this.request<{ asset_id?: string; job_id?: string; cached?: boolean }>(`/projects/${id}/versions/${versionId}/exports`, 'POST', { format: 'mp4', resolution, fps: 30 }, makeId('key')) }
  download(assetId: string) { return this.request<{ download_url: string; file_name: string }>(`/assets/${assetId}/download`) }
  watch(id: string, cursor: string, onChange: () => void, onError: () => void) {
    const controller = new AbortController()
    void (async () => {
      try {
        const response = await fetch(`${this.base}/projects/${id}/events?after=${encodeURIComponent(cursor)}`, { headers: { ...this.headers(), Accept: 'text/event-stream' }, signal: controller.signal })
        if (!response.ok || !response.body) throw new Error('Event stream unavailable')
        const reader = response.body.getReader(); const decoder = new TextDecoder(); let buffer = ''
        while (!controller.signal.aborted) {
          const { value, done } = await reader.read()
          if (done) { onError(); break }
          buffer += decoder.decode(value, { stream: true }).replace(/\r\n/g, '\n')
          let boundary: number
          while ((boundary = buffer.indexOf('\n\n')) !== -1) {
            const block = buffer.slice(0, boundary); buffer = buffer.slice(boundary + 2)
            if (block.split('\n').some(line => line.startsWith('data:'))) { onChange(); return }
          }
        }
      } catch { if (!controller.signal.aborted) onError() }
      finally { controller.abort() }
    })()
    return () => controller.abort()
  }
}

export const api: StudioApi = import.meta.env.VITE_STUDIO_MODE === 'http' ? new HttpApi() : new DemoApi()
export { isActive }
