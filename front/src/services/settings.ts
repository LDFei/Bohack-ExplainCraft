/** 通过 Tauri commands 读写真实供应商配置，不在浏览器中保存 API 密钥。 */
import { invoke } from '@tauri-apps/api/core';

export interface ProviderSetting {
  provider: string;
  baseUrl: string;
  model: string;
  apiKey: string | null;
  hasApiKey: boolean;
  voiceId: string;
}

export interface ModelSettings {
  script: ProviderSetting;
  music: ProviderSetting;
  voice: ProviderSetting;
}

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
  siliconflow: 'zai-org/GLM-5.3',
  glm: 'glm-4.6v',
  stable_audio: 'stable-audio-2',
  qwen: 'qwen-tts',
  indextts: 'indextts',
  breeze: 'int8 hybrid (recommended)',
  procedural: 'deterministic-wav-v1',
  custom: '',
};

/** 切换供应商时填入对应预设，用户仍可继续编辑。 */
export function providerPreset(provider: string, kind: 'script' | 'music' | 'voice' = 'script') {
  const model =
    kind === 'music' && provider === 'minimax'
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
export function getModelSettings() {
  return invoke<ModelSettings>('get_model_settings');
}

/** 保存并热应用供应商配置。 */
export function updateModelSettings(settings: ModelSettings) {
  return invoke<ModelSettings>('update_model_settings', { settings });
}

/** 通过后端请求 OpenAI Compatible /models，密钥只经过 IPC。 */
export function listModels(
  setting: ProviderSetting,
  kind: 'script' | 'music' | 'voice' = 'script',
) {
  return invoke<string[]>('list_models', { setting, kind });
}

/** 获取当前 TTS 供应商的可选声线。 */
export function listVoices(setting: ProviderSetting) {
  return invoke<string[]>('list_voices', { setting });
}
