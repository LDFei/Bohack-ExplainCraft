/** 工作台唯一数据入口：所有业务操作都调用真实 Tauri commands。 */
import { invoke } from '@tauri-apps/api/core';
import { makeId } from '../domain/id.ts';
import type {
  Asset,
  Capabilities,
  DraftChanges,
  Job,
  MessageInput,
  Project,
  Snapshot,
  VideoVersion,
} from '../domain/types.ts';

/** 可直接展示给用户的业务错误。 */
export class StudioError extends Error {
  constructor(
    readonly code: string,
    message: string,
  ) {
    super(message);
    this.name = 'StudioError';
  }
}

/** 项目是否仍被活动任务占用。 */
export function isActive(input: string | { status: string } | null | undefined) {
  const status = typeof input === 'string' ? input : input?.status;
  return !!status && ['queued', 'running', 'waiting_input', 'cancelling'].includes(status);
}

interface StudioApi {
  readonly mode: 'tauri';
  getCapabilities(): Promise<Capabilities>;
  listProjects(): Promise<Project[]>;
  createProject(): Promise<Project>;
  getSnapshot(id: string): Promise<Snapshot>;
  renameProject(id: string, title: string): Promise<void>;
  deleteProject(id: string): Promise<void>;
  sendMessage(id: string, input: MessageInput, key: string): Promise<void>;
  patchDraft(id: string, changes: DraftChanges, revision: number): Promise<void>;
  listAssets(id: string): Promise<Asset[]>;
  upload(id: string, file: File, onProgress: (percent: number) => void): Promise<Asset>;
  getJob(id: string): Promise<Job>;
  cancel(id: string): Promise<void>;
  retry(id: string, revision: number): Promise<void>;
  answer(id: string, question: string, value: string): Promise<void>;
  listVersions(id: string): Promise<VideoVersion[]>;
  getVersion(id: string, versionId: string): Promise<VideoVersion>;
  exportVideo(
    id: string,
    versionId: string,
    resolution: string,
    includeProject: boolean,
  ): Promise<{ asset_id?: string; job_id?: string; cached?: boolean }>;
  download(assetId: string): Promise<{ download_url: string; file_name: string }>;
  watch(id: string, cursor: string, onChange: () => void, onError: () => void): () => void;
}

interface CommandReply {
  status: number;
  body: { data?: unknown; error?: { code?: string; message?: string } } | null;
}

interface EventPoll {
  changed: boolean;
  cursor: string;
}

/** Tauri command 适配，不包含浏览器演示、本地伪造任务或计时生成。 */
class TauriApi implements StudioApi {
  readonly mode = 'tauri' as const;

  private headers(): Record<string, string> {
    const token =
      sessionStorage.getItem('lingjing_access_token') ||
      (import.meta.env.DEV ? import.meta.env.VITE_API_TOKEN : '');
    return token ? { Authorization: `Bearer ${token}` } : {};
  }

  private unwrap<T>(reply: CommandReply, fallback: string): T {
    if (reply.status === 204) return undefined as T;
    const payload = reply.body;
    if (reply.status >= 400 || !payload) {
      throw new StudioError(
        payload?.error?.code || (reply.status >= 500 ? 'BACKEND_UNAVAILABLE' : 'REQUEST_FAILED'),
        payload?.error?.message ||
          (reply.status >= 500 ? 'Tauri 后端暂时无法处理请求，请重试。' : fallback),
      );
    }
    return payload.data as T;
  }

  /** 调用 command，并统一处理鉴权、幂等 key 和业务错误。 */
  private async request<T>(
    command: string,
    args: Record<string, unknown> = {},
    key?: string,
    extraHeaders: Record<string, string> = {},
  ): Promise<T> {
    try {
      const reply = await invoke<CommandReply>(command, args, {
        headers: {
          ...this.headers(),
          ...(key ? { 'Idempotency-Key': key } : {}),
          ...extraHeaders,
        },
      });
      return this.unwrap<T>(reply, '请求失败，请稍后再试。');
    } catch (error) {
      if (error instanceof StudioError) throw error;
      throw new StudioError(
        'IPC_ERROR',
        typeof error === 'string'
          ? error
          : error instanceof Error
            ? error.message
            : 'Tauri 后端调用失败，请重试。',
      );
    }
  }

  getCapabilities() {
    return this.request<Capabilities>('capabilities');
  }

  async listProjects() {
    return (await this.request<{ items: Project[] }>('list_projects')).items;
  }

  async createProject() {
    return (await this.request<{ project: Project }>('create_project', {}, makeId('key'))).project;
  }

  getSnapshot(id: string) {
    return this.request<Snapshot>('get_project', { projectId: id });
  }

  async renameProject(id: string, title: string) {
    await this.request('rename_project', { projectId: id, title });
  }

  async deleteProject(id: string) {
    await this.request('delete_project', { projectId: id });
  }

  async sendMessage(id: string, input: MessageInput, key: string) {
    await this.request('send_message', { projectId: id, input }, key);
  }

  async patchDraft(id: string, changes: DraftChanges, revision: number) {
    await this.request('patch_draft', {
      projectId: id,
      input: { changes, expected_draft_revision: revision },
    });
  }

  async listAssets(id: string) {
    return (await this.request<{ items: Asset[] }>('list_assets', { projectId: id })).items;
  }

  /** 通过原始 IPC 字节上传 multipart 文件，避免大文件 base64 编码。 */
  async upload(id: string, file: File, onProgress: (percent: number) => void) {
    onProgress(0);
    const boundary = `----ExplainCraft${makeId('boundary')}`;
    const encoder = new TextEncoder();
    const fileName = file.name.replace(/["\r\n]/g, '_');
    const prefix = encoder.encode(
      `--${boundary}\r\nContent-Disposition: form-data; name="file"; filename="${fileName}"\r\nContent-Type: ${file.type || 'application/octet-stream'}\r\n\r\n`,
    );
    const suffix = encoder.encode(`\r\n--${boundary}--\r\n`);
    const content = new Uint8Array(await file.arrayBuffer());
    const body = new Uint8Array(prefix.length + content.length + suffix.length);
    body.set(prefix);
    body.set(content, prefix.length);
    body.set(suffix, prefix.length + content.length);
    const reply = await invoke<CommandReply>('upload_asset', body, {
      headers: {
        ...this.headers(),
        'Idempotency-Key': makeId('key'),
        'Content-Type': `multipart/form-data; boundary=${boundary}`,
        'X-Project-Id': id,
      },
    }).catch((error: unknown) => {
      throw new StudioError(
        'IPC_ERROR',
        typeof error === 'string' ? error : '素材上传失败，请重试。',
      );
    });
    let asset = this.unwrap<{ asset: Asset }>(reply, '上传失败。').asset;
    for (let attempt = 0; asset.status === 'processing' && attempt < 30; attempt++) {
      await new Promise((resolve) => setTimeout(resolve, 2000));
      asset = this.unwrap<{ asset: Asset }>(
        await invoke<CommandReply>('get_asset', { assetId: asset.id }),
        '素材查询失败。',
      ).asset;
    }
    if (asset.status !== 'ready') {
      throw new StudioError(
        'ASSET_NOT_READY',
        asset.error?.message || '素材仍在解析，请稍后在素材面板查看。',
      );
    }
    onProgress(100);
    return asset;
  }

  async getJob(id: string) {
    return (await this.request<{ job: Job }>('get_job', { jobId: id })).job;
  }

  async cancel(id: string) {
    await this.request('cancel_job', { jobId: id });
  }

  async retry(id: string, revision: number) {
    await this.request('retry_job', { jobId: id, expectedDraftRevision: revision }, makeId('key'));
  }

  async answer(id: string, question: string, value: string) {
    await this.request('answer_job', { jobId: id, questionId: question, value }, makeId('key'));
  }

  async listVersions(id: string) {
    return (await this.request<{ items: VideoVersion[] }>('list_versions', { projectId: id }))
      .items;
  }

  async getVersion(id: string, versionId: string) {
    return (
      await this.request<{ version: VideoVersion }>('get_version', { projectId: id, versionId })
    ).version;
  }

  exportVideo(id: string, versionId: string, resolution: string, includeProject: boolean) {
    return this.request<{ asset_id?: string; job_id?: string; cached?: boolean }>(
      'export_video',
      { projectId: id, versionId, resolution, includeProject },
      makeId('key'),
    );
  }

  download(assetId: string) {
    return this.request<{ download_url: string; file_name: string }>('download_asset', { assetId });
  }

  /** 每 700ms 查询持久化事件，变化后由上层重新获取一致性快照。 */
  watch(id: string, cursor: string, onChange: () => void, onError: () => void) {
    let active = true,
      checking = false;
    const poll = async () => {
      if (!active || checking) return;
      checking = true;
      try {
        const result = await invoke<EventPoll>(
          'project_events',
          { projectId: id, after: cursor },
          { headers: this.headers() },
        );
        if (active && result.changed) {
          active = false;
          onChange();
        }
      } catch {
        if (active) {
          active = false;
          onError();
        }
      } finally {
        checking = false;
      }
    };
    void poll();
    const timer = window.setInterval(() => void poll(), 700);
    return () => {
      active = false;
      window.clearInterval(timer);
    };
  }
}

export const api: StudioApi = new TauriApi();
