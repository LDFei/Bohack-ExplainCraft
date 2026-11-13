/**
 * 数据访问边界。DemoApi 使用本机存储与状态机，HttpApi 映射 Rust 接口；页面只依赖 StudioApi。
 */
import {
  advance,
  answerJob,
  cancelJob,
  createRecord,
  isActive,
  makeId,
  patchDraft,
  retryJob,
  seedDatabase,
  snapshot,
  startJob,
  StudioError,
} from '../domain/engine.ts';
import { capabilities, copy } from '../domain/content.ts';
import type {
  Asset,
  Capabilities,
  Database,
  DemoScenario,
  DraftChanges,
  Job,
  MessageInput,
  Project,
  ProjectRecord,
  Snapshot,
  VideoVersion,
} from '../domain/types.ts';

// localStorage 只保存 JSON 元数据；实际二进制附件放在 IndexedDB，避免大文件挤占字符串存储。
const STORAGE_KEY = 'lingjing-studio-v1';
// Blob URL 只在本次页面存活，不能当作持久地址写回项目数据。
const objectUrls = new Map<string, string>();
let fallback: Database | null = null;
/**
 * 从 localStorage 读取演示项目；缺失、损坏或不可读时使用本页内存中的初始案例。
 */
function readDatabase(): Database {
  try {
    const text = localStorage.getItem(STORAGE_KEY);
    if (text) {
      const saved = JSON.parse(text) as Database;
      if (saved.schema_version === 1 && Array.isArray(saved.projects)) return saved;
    }
  } catch {
    /* 存储被禁用或内容损坏时，改用当前页面的内存副本。 */
  }
  return (fallback ??= seedDatabase());
}
/**
 * 先更新页面内存副本，再持久化 JSON 元数据；磁盘配额不足时抛出可读错误。
 */
function writeDatabase(database: Database) {
  fallback = copy(database);
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(database));
  } catch {
    throw new StudioError('STORAGE_FULL', '本机存储空间不足，请清理不需要的演示项目后重试。');
  }
}
/**
 * 按项目 ID 定位完整演示记录；不存在时统一抛出业务错误，避免后续读到 undefined。
 */
function findRecord(database: Database, projectId: string) {
  const record = database.projects.find((item) => item.project.id === projectId);
  if (!record) throw new StudioError('NOT_FOUND', '这个项目不存在或已被删除，可以新建一份创作。');
  return record;
}
/**
 * 打开版本 1 的 IndexedDB 附件库；首次升级时创建 files 对象仓库，以 asset_id 为外部键。
 */
function openFiles(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open('lingjing-media', 1);
    request.onupgradeneeded = () => request.result.createObjectStore('files');
    request.onerror = () =>
      reject(new StudioError('FILE_STORAGE_ERROR', '浏览器暂时无法保存附件，请检查本机存储权限。'));
    request.onsuccess = () => resolve(request.result);
  });
}
/**
 * 把 Blob 写入附件库，并等待整个写事务成功；无论成功失败都关闭数据库连接。
 */
async function saveFile(id: string, file: Blob) {
  const database = await openFiles();
  try {
    await new Promise<void>((resolve, reject) => {
      const transaction = database.transaction('files', 'readwrite');
      transaction.objectStore('files').put(file, id);
      transaction.oncomplete = () => resolve();
      transaction.onerror = () =>
        reject(new StudioError('FILE_STORAGE_ERROR', '附件保存失败，请检查剩余存储空间。'));
    });
  } finally {
    database.close();
  }
}
/**
 * 从 IndexedDB 读取文件并创建本次页面可用的 object URL；同一文件在本页内复用地址。
 */
async function fileUrl(id: string) {
  if (objectUrls.has(id)) return objectUrls.get(id)!;
  const database = await openFiles();
  try {
    const file = await new Promise<Blob | undefined>((resolve, reject) => {
      const request = database.transaction('files').objectStore('files').get(id);
      request.onsuccess = () => resolve(request.result as Blob | undefined);
      request.onerror = () => reject(request.error);
    });
    if (!file) return null;
    const url = URL.createObjectURL(file);
    objectUrls.set(id, url);
    return url;
  } finally {
    database.close();
  }
}
/**
 * 复制项目记录后补齐上传图片的临时地址，包含当前草稿与历史分镜的预览。
 * Blob URL 刷新后失效，因此每次打开页面需要重新绑定；缺失文件仅在返回副本中标为失败。
 */
async function hydrate(record: ProjectRecord): Promise<ProjectRecord> {
  const result = copy(record);
  for (const asset of result.assets.filter((item) => item.source === 'upload')) {
    const url = await fileUrl(asset.id);
    if (!url) {
      asset.status = 'failed';
      asset.error = { code: 'FILE_MISSING', message: '本机附件已被清理，请重新上传。' };
      asset.thumbnail_url = null;
    } else if (asset.kind === 'image') asset.thumbnail_url = url;
  }
  for (const draft of [result.draft, ...result.versions.map((version) => version.snapshot)]) {
    for (const shot of draft.shots) {
      const asset = result.assets.find((item) => shot.asset_ids.includes(item.id));
      if (asset?.kind === 'image' && asset.thumbnail_url) shot.thumbnail_url = asset.thumbnail_url;
    }
  }
  return result;
}
/** 页面依赖的统一服务合同；切换演示与 HTTP 不需要改组件中的业务操作。 */
export interface StudioApi {
  readonly mode: 'demo' | 'http';
  /**
   * 读取界面可选项、默认值、上传限制和导出规格。
   */
  getCapabilities(): Promise<Capabilities>;
  /**
   * 列出历史项目摘要。
   */
  listProjects(): Promise<Project[]>;
  /**
   * 创建空项目并返回项目标识及初始状态。
   */
  createProject(): Promise<Project>;
  /**
   * 读取草稿、消息、活动任务、最新成片和事件游标。
   */
  getSnapshot(id: string): Promise<Snapshot>;
  /**
   * 修改项目标题。
   */
  renameProject(id: string, title: string): Promise<void>;
  /**
   * 删除没有活动任务的项目及其关联数据。
   */
  deleteProject(id: string): Promise<void>;
  /**
   * 提交一次生成、修改或仅讨论请求；key 用于幂等，scenario 仅供本地演示。
   */
  sendMessage(id: string, input: MessageInput, key: string, scenario?: DemoScenario): Promise<void>;
  /**
   * 保存变化的草稿分组并校验 revision。
   */
  patchDraft(id: string, changes: DraftChanges, revision: number): Promise<void>;
  /**
   * 列出项目素材及处理状态。
   */
  listAssets(id: string): Promise<Asset[]>;
  /**
   * 接收文件并报告处理进度，成功后返回 ready 素材。
   */
  upload(id: string, file: File, onProgress: (percent: number) => void): Promise<Asset>;
  /**
   * 取得当前或历史任务的完整状态。
   */
  getJob(id: string): Promise<Job>;
  /**
   * 请求停止任务。
   */
  cancel(id: string): Promise<void>;
  /**
   * 按当前修订号从旧任务建立新的重试。
   */
  retry(id: string, revision: number): Promise<void>;
  /**
   * 回答指定问题并恢复原任务。
   */
  answer(id: string, question: string, value: string): Promise<void>;
  /**
   * 列出已成功生成的成片版本。
   */
  listVersions(id: string): Promise<VideoVersion[]>;
  /**
   * 读取指定版本完整数据及快照。
   */
  getVersion(id: string, versionId: string): Promise<VideoVersion>;
  /**
   * 导出指定版本，返回缓存文件 ID 或异步导出任务 ID。
   */
  exportVideo(
    id: string,
    versionId: string,
    resolution: string,
  ): Promise<{ asset_id?: string; job_id?: string; cached?: boolean }>;
  /**
   * 取得素材下载地址与文件名。
   */
  download(assetId: string): Promise<{ download_url: string; file_name: string }>;
  /**
   * 监听项目变更；onChange 要求页面刷新快照，onError 请求恢复连接，返回取消监听函数。
   */
  watch(id: string, cursor: string, onChange: () => void, onError: () => void): () => void;
}

/** 本机演示实现，负责存储与调用状态机，不提供真实账户、AI 生成或跨设备同步。 */
class DemoApi implements StudioApi {
  readonly mode = 'demo' as const;
  // 当前页面内的幂等记录；浏览器刷新后清空，真实后端必须提供自己的持久化实现。
  private received = new Map<string, string>();
  /**
   * 返回演示能力副本，使界面只能选择本地真实存在的比例、媒体及导出规格。
   */
  async getCapabilities() {
    return copy(capabilities);
  }
  /**
   * 推进所有演示项目到当前时间并返回按修改时间倒序的历史列表。
   */
  async listProjects() {
    const database = readDatabase();
    for (const record of database.projects) advance(record, Date.now());
    writeDatabase(database);
    return database.projects
      .map((item) => copy(item.project))
      .sort((a, b) => b.updated_at.localeCompare(a.updated_at));
  }
  /**
   * 新建空记录并写入本机数据库；真正的制作在发送第一条消息后才开始。
   */
  async createProject() {
    const database = readDatabase();
    const record = createRecord();
    database.projects.unshift(record);
    writeDatabase(database);
    return copy(record.project);
  }
  /**
   * 先推进当前任务，再补齐附件地址并返回页面快照，使刷新后的进度能够恢复。
   */
  async getSnapshot(id: string) {
    const database = readDatabase();
    const record = findRecord(database, id);
    advance(record, Date.now());
    writeDatabase(database);
    return snapshot(await hydrate(record));
  }
  /**
   * 校验标题长度并保存项目名称，不改变草稿或视频版本。
   */
  async renameProject(id: string, title: string) {
    if (!title.trim() || title.length > 100)
      throw new StudioError('VALIDATION_ERROR', '标题需要在 1～100 字之间。');
    const database = readDatabase();
    findRecord(database, id).project.title = title.trim();
    writeDatabase(database);
  }
  /**
   * 拒绝删除活动项目；从列表移除后清理附件库及缓存 URL。
   * 项目 JSON 与文件库是两个存储，清理步骤并非跨存储的原子事务。
   */
  async deleteProject(id: string) {
    const database = readDatabase();
    const record = findRecord(database, id);
    if (record.project.active_job_id)
      throw new StudioError('PROJECT_BUSY', '请先停止当前制作，再删除项目。');
    database.projects = database.projects.filter((item) => item.project.id !== id);
    writeDatabase(database);
    const files = await openFiles();
    try {
      await new Promise<void>((resolve, reject) => {
        const tx = files.transaction('files', 'readwrite');
        for (const asset of record.assets) {
          tx.objectStore('files').delete(asset.id);
          const url = objectUrls.get(asset.id);
          if (url) URL.revokeObjectURL(url);
          objectUrls.delete(asset.id);
        }
        tx.oncomplete = () => resolve();
        tx.onerror = () => reject(tx.error);
      });
    } finally {
      files.close();
    }
  }
  /**
   * 用请求 key 和输入指纹阻止本页内重复创建任务，再交给演示状态机受理。
   * 这个去重 Map 仅存活于当前页面；真实后端应按接口文档实现持久化幂等。
   */
  async sendMessage(
    id: string,
    input: MessageInput,
    key: string,
    scenario: DemoScenario = 'normal',
  ) {
    // 同一 key 必须对应同一请求内容：相同请求直接返回，不同内容则拒绝复用。
    const fingerprint = JSON.stringify({ id, input });
    if (this.received.has(key)) {
      if (this.received.get(key) !== fingerprint)
        throw new StudioError('IDEMPOTENCY_CONFLICT', '请为新的请求使用不同的标识。');
      return;
    }
    const database = readDatabase();
    startJob(findRecord(database, id), input, Date.now(), scenario);
    writeDatabase(database);
    this.received.set(key, fingerprint);
  }
  /**
   * 调用领域层的修订号校验与草稿保存规则，再持久化演示记录。
   */
  async patchDraft(id: string, changes: DraftChanges, revision: number) {
    const database = readDatabase();
    patchDraft(findRecord(database, id), changes, revision, Date.now());
    writeDatabase(database);
  }
  /**
   * 返回当前项目素材，同时把本机文件重新关联为可预览的临时地址。
   */
  async listAssets(id: string) {
    return (await hydrate(findRecord(readDatabase(), id))).assets;
  }
  /**
   * 校验大小、类型和可解码性，提取媒体元数据后把文件存入 IndexedDB。
   * onProgress 表示本机处理阶段，不是实际网络上传字节进度；文件不会上传到服务器。
   */
  async upload(id: string, file: File, onProgress: (percent: number) => void) {
    const kind = file.type.startsWith('image/')
      ? 'image'
      : file.type.startsWith('video/')
        ? 'video'
        : file.type.startsWith('audio/')
          ? 'audio'
          : null;
    if (!kind || !capabilities.upload_limits[kind].mime_types.includes(file.type))
      throw new StudioError(
        'UNSUPPORTED_FILE',
        '支持 JPG、PNG、WebP、MP4、WebM、MP3 和 WAV 文件。',
      );
    const limit = capabilities.upload_limits[kind].max_bytes;
    if (file.size > limit)
      throw new StudioError(
        'FILE_TOO_LARGE',
        `文件超过 ${Math.round(limit / 1024 / 1024)}MB，请压缩后再上传。`,
      );
    if (!file.size) throw new StudioError('INVALID_FILE', '这个文件是空的，请重新选择。');
    if (
      findRecord(readDatabase(), id).assets.length >=
      capabilities.upload_limits.max_uploaded_files_per_project
    )
      throw new StudioError('FILE_LIMIT', '每个演示项目最多上传 20 个素材。');
    onProgress(10);
    let width: number | null = null,
      height: number | null = null,
      duration: number | null = null;
    if (kind === 'image') {
      try {
        // 实际解码图片验证内容，而不只相信扩展名和客户端声明的 MIME。
        const bitmap = await createImageBitmap(file);
        width = bitmap.width;
        height = bitmap.height;
        bitmap.close();
      } catch {
        throw new StudioError('INVALID_FILE', '这张图片无法读取，请选择其他文件。');
      }
    } else {
      const url = URL.createObjectURL(file);
      try {
        const metadata = await new Promise<{
          width: number | null;
          height: number | null;
          duration: number;
        }>((resolve, reject) => {
          const element = document.createElement(kind === 'video' ? 'video' : 'audio');
          // 媒体损坏时可能迟迟不触发 metadata；超时和 error 都必须释放临时媒体对象。
          const timeout = window.setTimeout(() => {
            element.removeAttribute('src');
            element.load();
            reject(new StudioError('INVALID_FILE', '媒体解析超时，请选择其他文件。'));
          }, 10000);
          /**
           * 媒体元数据解析结束或失败时，停止超时计时并释放临时 audio/video 的资源引用。
           */
          const clear = () => {
            clearTimeout(timeout);
            element.removeAttribute('src');
            element.load();
          };
          element.onloadedmetadata = () => {
            const result = {
              width: element instanceof HTMLVideoElement ? element.videoWidth : null,
              height: element instanceof HTMLVideoElement ? element.videoHeight : null,
              duration: element.duration,
            };
            clear();
            Number.isFinite(result.duration)
              ? resolve(result)
              : reject(new StudioError('INVALID_FILE', '无法读取媒体时长。'));
          };
          element.onerror = () => {
            clear();
            reject(new StudioError('INVALID_FILE', '此媒体已损坏或当前浏览器无法解码。'));
          };
          element.preload = 'metadata';
          element.src = url;
        });
        width = metadata.width;
        height = metadata.height;
        duration = Math.round(metadata.duration * 1000);
      } finally {
        URL.revokeObjectURL(url);
      }
    }
    const asset: Asset = {
      id: makeId('asset'),
      project_id: id,
      kind,
      source: 'upload',
      status: 'ready',
      file_name: file.name,
      mime_type: file.type,
      size_bytes: file.size,
      thumbnail_url: null,
      duration_ms: duration,
      width,
      height,
      error: null,
    };
    onProgress(60);
    await saveFile(asset.id, file);
    const database = readDatabase();
    findRecord(database, id).assets.push(asset);
    writeDatabase(database);
    onProgress(100);
    return (await hydrate(findRecord(database, id))).assets.find((item) => item.id === asset.id)!;
  }
  /**
   * 跨项目查找指定任务并返回副本；用于恢复历史消息里的进度和失败状态。
   */
  async getJob(id: string) {
    const job = readDatabase()
      .projects.flatMap((item) => item.jobs)
      .find((item) => item.id === id);
    if (!job) throw new StudioError('NOT_FOUND', '没有找到制作任务。');
    return copy(job);
  }
  /**
   * 定位任务所属项目，先推进到当前时间，再统一执行修改并持久化。
   * 这样取消请求到达时，已经成功完成的任务不会被倒改成取消。
   */
  private mutateJob(id: string, action: (record: ProjectRecord) => void) {
    const database = readDatabase();
    const record = database.projects.find((item) => item.jobs.some((job) => job.id === id));
    if (!record) throw new StudioError('NOT_FOUND', '没有找到制作任务。');
    advance(record, Date.now());
    action(record);
    writeDatabase(database);
  }
  /**
   * 通过统一任务修改入口请求取消；实际 cancelled 终态由下一次推进确认。
   */
  async cancel(id: string) {
    this.mutateJob(id, (record) => cancelJob(record, id, Date.now()));
  }
  /**
   * 按当前修订号校验旧任务输入，创建新的演示重试任务。
   */
  async retry(id: string, revision: number) {
    this.mutateJob(id, (record) => retryJob(record, id, revision, Date.now()));
  }
  /**
   * 提交问题 ID 和回答，恢复该项目现有的等待任务。
   */
  async answer(id: string, question: string, value: string) {
    this.mutateJob(id, (record) => answerJob(record, id, question, value, Date.now()));
  }
  /**
   * 返回新版本在前的成片列表；复制后反转，避免改变内部保存顺序。
   */
  async listVersions(id: string) {
    return copy(findRecord(readDatabase(), id).versions).reverse();
  }
  /**
   * 读取指定历史成片及快照，同时恢复其中引用的本机图片预览。
   */
  async getVersion(id: string, versionId: string) {
    const version = (await hydrate(findRecord(readDatabase(), id))).versions.find(
      (item) => item.id === versionId,
    );
    if (!version) throw new StudioError('NOT_FOUND', '这个视频版本不存在。');
    return version;
  }
  /**
   * 演示仅支持已有的 720p MP4，直接返回缓存素材 ID，不假装进行了高清重渲染。
   */
  async exportVideo(id: string, versionId: string, resolution: string) {
    if (resolution !== '720p')
      throw new StudioError('UNSUPPORTED_EXPORT', '演示素材支持 720p 导出。');
    const version = await this.getVersion(id, versionId);
    return { asset_id: version.video_asset_id, cached: true };
  }
  /**
   * 把成片素材 ID 或上传素材 ID 解析成下载地址，并附上有意义的文件名。
   */
  async download(assetId: string) {
    for (const record of readDatabase().projects) {
      const version = record.versions.find((item) => item.video_asset_id === assetId);
      if (version)
        return {
          download_url: version.playback_url,
          file_name: `${version.snapshot.script?.title ?? record.project.title}-v${version.version_number}.mp4`,
        };
      const asset = record.assets.find((item) => item.id === assetId);
      if (asset) {
        const url = await fileUrl(asset.id);
        if (url) return { download_url: url, file_name: asset.file_name };
      }
    }
    throw new StudioError('NOT_FOUND', '文件暂时不可用，请重新选择。');
  }
  /**
   * 每 700ms 检查并推进当前演示任务，仅在事件序号变化时通知页面刷新。
   * 返回清理函数，路由离开时停止轮询；演示模式不建立网络 SSE 连接。
   */
  watch(id: string, _cursor: string, onChange: () => void, onError: () => void) {
    const timer = window.setInterval(() => {
      try {
        const database = readDatabase();
        const record = findRecord(database, id);
        if (record.project.active_job_id) {
          const before = record.event_seq;
          advance(record, Date.now());
          if (before !== record.event_seq) {
            writeDatabase(database);
            onChange();
          }
        }
      } catch {
        onError();
      }
    }, 700);
    // 上层保存这个清理函数，在切换项目或重新取快照前调用。
    return () => clearInterval(timer);
  }
}

/**
 * 桌面应用（Tauri）在页面脚本运行前注入的内嵌后端地址，如 `http://127.0.0.1:52341/api/v1`。
 * 浏览器中不存在；存在时说明运行在桌面端，必须走真实后端接口。
 */
const desktopApiBase = (window as Window & { __EXPLAINCRAFT_API__?: string }).__EXPLAINCRAFT_API__;

/**
 * Rust 后端接口适配，路径对应 docs/API.md。桌面端连接内嵌后端，浏览器联调经 Vite 代理连接 back/。
 * 登录、长列表分页和播放地址过期续期尚未接入。
 */
class HttpApi implements StudioApi {
  readonly mode = 'http' as const;
  // 桌面端使用注入的绝对地址（页面来源与后端端口不同）；浏览器联调走 Vite 代理的相对地址。
  private base = (desktopApiBase || import.meta.env.VITE_API_BASE_URL || '/api/v1').replace(
    /\/$/,
    '',
  );
  /**
   * 读取登录会话令牌并生成 Bearer 请求头；开发令牌只在 DEV 模式作为后备来源。
   */
  private headers(): Record<string, string> {
    const token =
      sessionStorage.getItem('lingjing_access_token') ||
      (import.meta.env.DEV ? import.meta.env.VITE_API_TOKEN : '');
    return token ? { Authorization: `Bearer ${token}` } : {};
  }
  /**
   * 读取统一响应：成功返回 data，失败转换为 StudioError。
   * 后端未启动时开发代理返回的 502 等页面不是 JSON，这里给出可读提示而不是解析异常。
   */
  private async unwrap<T>(response: Response, fallback: string): Promise<T> {
    // 删除接口返回空响应，先处理它，避免对不存在的 JSON 正文解析报错。
    if (response.status === 204) return undefined as T;
    let payload: { data?: T; error?: { code?: string; message?: string } } | null = null;
    try {
      payload = await response.json();
    } catch {
      payload = null;
    }
    if (!response.ok || !payload)
      throw new StudioError(
        payload?.error?.code || (response.status >= 500 ? 'BACKEND_UNAVAILABLE' : 'REQUEST_FAILED'),
        payload?.error?.message ||
          (response.status >= 500 ? '暂时连不上创作服务，请确认后端已启动后重试。' : fallback),
      );
    return payload.data as T;
  }
  /**
   * 发出请求；网络层失败（后端未启动、断网）时转换为可读的业务错误。
   */
  private async send(path: string, init: RequestInit): Promise<Response> {
    try {
      return await fetch(`${this.base}${path}`, init);
    } catch {
      throw new StudioError('NETWORK_ERROR', '网络连接中断，请检查网络或后端服务后重试。');
    }
  }
  /**
   * 封装 JSON 请求、鉴权、幂等 key 和业务异常，统一取响应中的 data。
   * 任务是否成功仍需看 Job.status；extraHeaders 用于少数接口的附加请求头。
   */
  private async request<T>(
    path: string,
    method = 'GET',
    body?: unknown,
    key?: string,
    extraHeaders: Record<string, string> = {},
  ): Promise<T> {
    const response = await this.send(path, {
      method,
      headers: {
        ...this.headers(),
        ...(body ? { 'Content-Type': 'application/json' } : {}),
        ...(key ? { 'Idempotency-Key': key } : {}),
        ...extraHeaders,
      },
      body: body ? JSON.stringify(body) : undefined,
    });
    return this.unwrap<T>(response, '请求失败，请稍后再试。');
  }
  /**
   * 调用 GET /capabilities，读取真实后端已经接通的可选参数与上传限制。
   */
  getCapabilities() {
    return this.request<Capabilities>('/capabilities');
  }
  /**
   * 读取最多 100 条项目摘要；更长历史的游标翻页留待真实后端联调。
   */
  async listProjects() {
    return (await this.request<{ items: Project[] }>('/projects?limit=100')).items;
  }
  /**
   * 以独立幂等 key 创建空项目，取返回的 project 对象。
   */
  async createProject() {
    return (await this.request<{ project: Project }>('/projects', 'POST', {}, makeId('key')))
      .project;
  }
  /**
   * 读取项目一致性快照，作为恢复页面和重新订阅事件流的唯一状态基准。
   */
  getSnapshot(id: string) {
    return this.request<Snapshot>(`/projects/${id}`);
  }
  /**
   * PATCH 项目标题；不提交整个项目对象，避免覆盖服务端管理字段。
   */
  async renameProject(id: string, title: string) {
    await this.request(`/projects/${id}`, 'PATCH', { title });
  }
  /**
   * 请求删除项目，服务端负责活动任务校验及附件清理。
   */
  async deleteProject(id: string) {
    await this.request(`/projects/${id}`, 'DELETE');
  }
  /**
   * 把生成、修改或讨论意图提交到统一消息入口，复用调用者提供的幂等 key。
   */
  async sendMessage(
    id: string,
    input: MessageInput,
    key: string,
    scenario: DemoScenario = 'normal',
  ) {
    // 演示情景只在后端仍使用演示供应商时生效，接入真实模型后后端会忽略这个请求头。
    const headers: Record<string, string> =
      scenario === 'normal' ? {} : { 'X-Demo-Scenario': scenario };
    await this.request(`/projects/${id}/messages`, 'POST', input, key, headers);
  }
  /**
   * PATCH 发生变化的草稿分组，并携带编辑时的修订号供后端做冲突检测。
   */
  async patchDraft(id: string, changes: DraftChanges, revision: number) {
    await this.request(`/projects/${id}/draft`, 'PATCH', {
      changes,
      expected_draft_revision: revision,
    });
  }
  /**
   * 读取项目素材列表，当前适配最多加载 100 项。
   */
  async listAssets(id: string) {
    return (await this.request<{ items: Asset[] }>(`/projects/${id}/assets?limit=100`)).items;
  }
  /**
   * 使用 FormData 上传文件，等待后端媒体解析就绪；每 2 秒查询一次，最多 30 次。
   * 不手写 Content-Type，让浏览器补 multipart boundary；这里只提供开始和完成的进度通知。
   */
  async upload(id: string, file: File, onProgress: (percent: number) => void) {
    // 文件请求不能复用 JSON 请求体；浏览器会自动生成 multipart 的边界。
    const body = new FormData();
    body.append('file', file);
    onProgress(0);
    const response = await this.send(`/projects/${id}/assets`, {
      method: 'POST',
      headers: { ...this.headers(), 'Idempotency-Key': makeId('key') },
      body,
    });
    let asset = (await this.unwrap<{ asset: Asset }>(response, '上传失败。')).asset;
    // HTTP 201 表示文件已受理，不一定已经解析完成；只有 ready 才能参与制作。
    for (let attempt = 0; asset.status === 'processing' && attempt < 30; attempt++) {
      await new Promise((resolve) => setTimeout(resolve, 2000));
      asset = (await this.request<{ asset: Asset }>(`/assets/${asset.id}`)).asset;
    }
    if (asset.status !== 'ready')
      throw new StudioError(
        'ASSET_NOT_READY',
        asset.error?.message || '素材仍在解析，请稍后在素材面板查看。',
      );
    onProgress(100);
    return asset;
  }
  /**
   * 查询完整任务对象；HTTP 成功只表示查询成功，任务错误在 job.error 中。
   */
  async getJob(id: string) {
    return (await this.request<{ job: Job }>(`/jobs/${id}`)).job;
  }
  /**
   * POST 取消请求；与断开 SSE 区分，只有这个业务接口才请求停止制作。
   */
  async cancel(id: string) {
    await this.request(`/jobs/${id}/cancel`, 'POST');
  }
  /**
   * 携带当前草稿修订号重试任务，使用新的幂等 key 标识这次重试请求。
   */
  async retry(id: string, revision: number) {
    await this.request(
      `/jobs/${id}/retry`,
      'POST',
      { expected_draft_revision: revision },
      makeId('key'),
    );
  }
  /**
   * 提交仍在等待的问题 ID 和回答，由后端原子恢复同一个任务。
   */
  async answer(id: string, question: string, value: string) {
    await this.request(
      `/jobs/${id}/answers`,
      'POST',
      { question_id: question, value },
      makeId('key'),
    );
  }
  /**
   * 读取最多 100 个成片版本摘要；完整历史快照通过 getVersion 再取。
   */
  async listVersions(id: string) {
    return (await this.request<{ items: VideoVersion[] }>(`/projects/${id}/versions?limit=100`))
      .items;
  }
  /**
   * 取得指定成片的完整快照和最新可用的播放地址。
   */
  async getVersion(id: string, versionId: string) {
    return (await this.request<{ version: VideoVersion }>(`/projects/${id}/versions/${versionId}`))
      .version;
  }
  /**
   * 按指定不可变版本请求 MP4 导出；可能直接返回缓存素材，也可能返回异步任务 ID。
   */
  exportVideo(id: string, versionId: string, resolution: string) {
    return this.request<{ asset_id?: string; job_id?: string; cached?: boolean }>(
      `/projects/${id}/versions/${versionId}/exports`,
      'POST',
      { format: 'mp4', resolution, fps: 30 },
      makeId('key'),
    );
  }
  /**
   * 获取经过鉴权的短期下载链接和文件名，页面不拼接服务器磁盘路径。
   */
  download(assetId: string) {
    return this.request<{ download_url: string; file_name: string }>(`/assets/${assetId}/download`);
  }
  /**
   * 带鉴权从 after 游标读取 SSE，保留未完成的文本块直到遇到事件空行。
   * 当前适配收到首个数据事件就通知上层重取快照并关闭旧流；返回函数仅中断监听，不取消生成。
   */
  watch(id: string, cursor: string, onChange: () => void, onError: () => void) {
    // 中断事件连接只影响当前页面监听，不是业务上的取消生成。
    const controller = new AbortController();
    void (async () => {
      try {
        const response = await fetch(
          `${this.base}/projects/${id}/events?after=${encodeURIComponent(cursor)}`,
          {
            headers: { ...this.headers(), Accept: 'text/event-stream' },
            signal: controller.signal,
          },
        );
        if (!response.ok || !response.body) throw new Error('Event stream unavailable');
        const reader = response.body.getReader();
        const decoder = new TextDecoder();
        let buffer = '';
        while (!controller.signal.aborted) {
          const { value, done } = await reader.read();
          if (done) {
            onError();
            break;
          }
          // 网络块和 SSE 事件不是一回事：保留半条事件，遇到空行才尝试处理完整事件。
          buffer += decoder.decode(value, { stream: true }).replace(/\r\n/g, '\n');
          let boundary: number;
          while ((boundary = buffer.indexOf('\n\n')) !== -1) {
            const block = buffer.slice(0, boundary);
            buffer = buffer.slice(boundary + 2);
            if (block.split('\n').some((line) => line.startsWith('data:'))) {
              // 以持久化快照为准：通知上层重新读取，而不是直接把这条事件当作完整页面数据。
              onChange();
              return;
            }
          }
        }
      } catch {
        if (!controller.signal.aborted) onError();
      } finally {
        controller.abort();
      }
    })();
    return () => controller.abort();
  }
}

// 只在这一处选择服务实现，避免各组件自行判断环境、拼接口或写本机存储。
// 桌面端总是连接内嵌后端；浏览器中由 VITE_STUDIO_MODE 决定（demo 为纯前端演示）。
export const api: StudioApi =
  desktopApiBase || import.meta.env.VITE_STUDIO_MODE === 'http' ? new HttpApi() : new DemoApi();
export { isActive };
