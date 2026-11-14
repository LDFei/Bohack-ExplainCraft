/** 通过 Tauri commands 读写真实供应商配置，不在浏览器中保存 API 密钥。 */
import { invoke } from '@tauri-apps/api/core';

export interface ProviderSetting {
  provider: string;
  baseUrl: string;
  model: string;
  apiKey: string | null;
  hasApiKey: boolean;
  voiceId: string;
  /** 是否单独指定该流程的模型；undefined 表示沿用统一模型的旧配置。 */
  overridden?: boolean;
}

export interface ModelSettings {
  /** 文案、对话、分镜、剪辑共用的统一模型。 */
  script: ProviderSetting;
  /** SVG 画面模型；overridden 为 false 时跟随 script。 */
  visual: ProviderSetting;
  music: ProviderSetting;
  voice: ProviderSetting;
}

/** 创作类流程（统一模型、画面模型）的默认模型，也是兼容旧配置时的回退值。 */
export const defaultCreationModel = 'deepseek-ai/DeepSeek-V4.1-Flash';

const bases: Record<string, string> = {
  openai: 'https://api.openai.com/v1',
  aliyun: 'https://dashscope.aliyuncs.com/compatible-mode/v1',
  deepseek: 'https://api.deepseek.com/v1',
  minimax: 'https://api.minimaxi.com/v1',
  siliconflow: 'https://api.siliconflow.cn/v1',
  glm: 'https://open.bigmodel.cn/api/paas/v4',
  stable_audio: 'https://api.stability.ai',
  qwen: 'https://dashscope.aliyuncs.com/compatible-mode/v1',
  indextts: 'http://127.0.0.1:8000/v1',
  breeze: 'http://127.0.0.1:8188',
  procedural: '',
  custom: '',
};

const models: Record<string, string> = {
  openai: 'gpt-4o-mini',
  aliyun: 'qwen-plus',
  deepseek: 'deepseek-chat',
  minimax: 'MiniMax-Text-01',
  siliconflow: defaultCreationModel,
  glm: 'glm-4.6v',
  stable_audio: 'stable-audio-2',
  qwen: 'qwen-tts',
  indextts: 'indextts',
  breeze: 'int8 hybrid (recommended)',
  procedural: 'deterministic-wav-v1',
  custom: '',
};

/** 切换供应商时填入对应预设，用户仍可继续编辑。 */
export function providerPreset(
  provider: string,
  kind: 'script' | 'visual' | 'music' | 'voice' = 'script',
) {
  // 文案与画面默认都使用同一个 DeepSeek 模型，用户仍可逐项改成专用模型。
  const creation = kind === 'script' || kind === 'visual';
  const model =
    creation && provider !== 'custom'
      ? defaultCreationModel
      : kind === 'music' && provider === 'minimax'
        ? 'music-1.5'
        : kind === 'voice' && provider === 'siliconflow'
          ? 'fnlp/MOSS-TTSD-v0.5'
          : (models[provider] ?? '');
  return {
    baseUrl: bases[provider] ?? '',
    model,
  };
}

/** 从后端读取供应商配置；后端只返回密钥是否存在。 */
export async function getModelSettings() {
  const settings = await invoke<ModelSettings>('get_model_settings');
  // 兼容旧后端：没有 visual 字段时按“跟随统一模型”补齐，避免页面报错或误删配置。
  if (!settings.visual) {
    settings.visual = { ...settings.script, overridden: false };
  }
  if (!settings.script.model) settings.script.model = defaultCreationModel;
  return settings;
}

/** 保存并热应用供应商配置。 */
export function updateModelSettings(settings: ModelSettings) {
  return invoke<ModelSettings>('update_model_settings', { settings });
}

/** 通过后端请求 OpenAI Compatible /models，密钥只经过 IPC。 */
export function listModels(
  setting: ProviderSetting,
  kind: 'script' | 'visual' | 'music' | 'voice' = 'script',
) {
  return invoke<string[]>('list_models', { setting, kind });
}

/** 获取当前 TTS 供应商的可选声线。 */
export function listVoices(setting: ProviderSetting) {
  return invoke<string[]>('list_voices', { setting });
}
