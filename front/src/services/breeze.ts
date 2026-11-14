/** 本地 breezeTTS2 模型：状态查询、下载命令与进度事件。 */
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { UnlistenFn } from '@tauri-apps/api/event';

/** ComfyUI 目录的来源：用户配置、自动探测，或尚未找到。 */
export type BreezeModelSource = 'config' | 'detected' | 'unset';

export interface BreezeModelStatus {
  /** 全部模型文件是否已就绪。 */
  installed: boolean;
  /** 模型目录。 */
  dir: string;
  /** ComfyUI 安装目录；未找到时为空。 */
  comfyuiDir: string;
  source: BreezeModelSource;
  /** 需要下载的总字节数。 */
  totalBytes: number;
  /** 是否有下载正在进行。 */
  downloading: boolean;
}

export interface BreezeModelProgress {
  phase: 'downloading' | 'done' | 'cancelled';
  /** 当前文件在模型目录内的相对路径。 */
  file: string;
  fileIndex: number;
  fileCount: number;
  received: number;
  total: number;
  /** 完成百分比，0～100。 */
  percent: number;
}

export const BREEZE_MODEL_PROGRESS_EVENT = 'breeze-model-progress';

/** 模型文件合计字节数；后端查不到状态时的兜底显示值。 */
export const BREEZE_MODEL_BYTES = 5_580_631_573;

/** 读取本地模型状态；不会触发任何下载。 */
export function breezeModelStatus() {
  return invoke<BreezeModelStatus>('breeze_model_status');
}

/** 下载模型；不传目录时使用后端探测到的 ComfyUI 目录。 */
export function downloadBreezeModel(comfyuiDir?: string) {
  return invoke<BreezeModelStatus>('download_breeze_model', { comfyuiDir: comfyuiDir || null });
}

/** 中止下载；已下载的部分保留，下次接着下。 */
export function cancelBreezeDownload() {
  return invoke<void>('cancel_breeze_download');
}

export function onBreezeModelProgress(
  handler: (progress: BreezeModelProgress) => void,
): Promise<UnlistenFn> {
  return listen<BreezeModelProgress>(BREEZE_MODEL_PROGRESS_EVENT, (event) =>
    handler(event.payload),
  );
}

/** 字节数格式化，按 1024 进制，与资源管理器显示一致。 */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${unit === 0 || value >= 100 ? Math.round(value) : value.toFixed(1)} ${units[unit]}`;
}
