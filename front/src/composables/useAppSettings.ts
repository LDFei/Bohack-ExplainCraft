/** 通用偏好和 API 预留信息独立于项目数据；此模块不发起任何网络或模型请求。 */
import { readonly, ref } from 'vue';
export const APP_SETTINGS_KEY = 'explaincraft.app-settings';
export type SendShortcut = 'enter' | 'mod-enter';
export interface GeneralSettings {
  rememberLayout: boolean;
  sendShortcut: SendShortcut;
}
export const apiModules = [
  {
    id: 'video',
    name: '文生视频模块',
    icon: 'video',
    description: '文生图 → 图生视频，分开预留两步接口。',
    slots: [
      { id: 'textToImage', name: '文生图 API' },
      { id: 'imageToVideo', name: '图生视频 API' },
    ],
  },
  {
    id: 'music',
    name: '背景音乐 API 模块',
    icon: 'music',
    description: '预留音乐生成或配乐服务。',
    slots: [{ id: 'music', name: '背景音乐 API' }],
  },
  {
    id: 'voice',
    name: '人声 API 模型',
    icon: 'mic',
    description: '预留语音合成服务与人声模型。',
    slots: [{ id: 'voice', name: '人声 API' }],
  },
  {
    id: 'vision',
    name: '视觉大语言 API 模块',
    icon: 'eye',
    description: '预留图像理解与视觉大语言模型。',
    slots: [{ id: 'vision', name: '视觉大语言 API' }],
  },
] as const;
export type ApiSlotId = (typeof apiModules)[number]['slots'][number]['id'];
export interface ApiConnection {
  endpoint: string;
  model: string;
  apiKey: string;
}
export type ApiConnections = Record<ApiSlotId, ApiConnection>;
const slotIds = apiModules.flatMap((group) => group.slots.map((slot) => slot.id));
/** 每个预留端点有独立模型与会话密钥，文生图和图生视频不会共用或覆盖配置。 */
export function emptyApiConnections(): ApiConnections {
  return Object.fromEntries(
    slotIds.map((id) => [id, { endpoint: '', model: '', apiKey: '' }]),
  ) as ApiConnections;
}
const general = ref<GeneralSettings>({ rememberLayout: true, sendShortcut: 'enter' });
const connections = ref<ApiConnections>(emptyApiConnections());
try {
  const saved = JSON.parse(localStorage.getItem(APP_SETTINGS_KEY) ?? 'null');
  if (saved?.version === 1) {
    general.value.rememberLayout = saved.general?.rememberLayout !== false;
    general.value.sendShortcut =
      saved.general?.sendShortcut === 'mod-enter' ? 'mod-enter' : 'enter';
    for (const id of slotIds) {
      const item = saved.connections?.[id];
      if (typeof item?.endpoint === 'string')
        connections.value[id].endpoint = item.endpoint.slice(0, 2048);
      if (typeof item?.model === 'string') connections.value[id].model = item.model.slice(0, 200);
      // 即使旧存储被手动塞入 apiKey，也不从浏览器持久化数据恢复密钥。
    }
  }
} catch {
  /* 偏好损坏或浏览器存储受限时使用默认值，不阻塞工作台。 */
}

/** 校验并保存非敏感配置；密钥只留在当前页面内存，绝不写入 localStorage。 */
function saveSettings(nextGeneral: GeneralSettings, nextConnections: ApiConnections) {
  for (const group of apiModules)
    for (const slot of group.slots) {
      const endpoint = nextConnections[slot.id].endpoint.trim();
      if (endpoint) {
        try {
          const url = new URL(endpoint);
          if (!['https:', 'http:'].includes(url.protocol)) throw new Error();
        } catch {
          return { error: `${slot.name} 请填写完整的 HTTP 或 HTTPS 接口地址。`, persisted: false };
        }
      }
    }
  general.value = {
    rememberLayout: nextGeneral.rememberLayout,
    sendShortcut: nextGeneral.sendShortcut === 'mod-enter' ? 'mod-enter' : 'enter',
  };
  connections.value = Object.fromEntries(
    slotIds.map((id) => [
      id,
      {
        endpoint: nextConnections[id].endpoint.trim(),
        model: nextConnections[id].model.trim(),
        apiKey: nextConnections[id].apiKey,
      },
    ]),
  ) as ApiConnections;
  const metadata = Object.fromEntries(
    slotIds.map((id) => [
      id,
      { endpoint: connections.value[id].endpoint, model: connections.value[id].model },
    ]),
  );
  try {
    localStorage.setItem(
      APP_SETTINGS_KEY,
      JSON.stringify({ version: 1, general: general.value, connections: metadata }),
    );
    return { error: '', persisted: true };
  } catch {
    return { error: '', persisted: false };
  }
}
/** 所有调用方共享通用设置，业务组件只读取需要的偏好，不访问 API 密钥。 */
export function useAppSettings() {
  return { general: readonly(general), connections: readonly(connections), saveSettings };
}
