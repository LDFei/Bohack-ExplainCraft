/** 通用偏好独立于模型供应商配置保存，只包含工作台布局和发送快捷键。 */
import { readonly, ref } from 'vue';

export const APP_SETTINGS_KEY = 'explaincraft.app-settings';
export type SendShortcut = 'enter' | 'mod-enter';

export interface GeneralSettings {
  rememberLayout: boolean;
  sendShortcut: SendShortcut;
}

const general = ref<GeneralSettings>({ rememberLayout: true, sendShortcut: 'enter' });
try {
  const saved = JSON.parse(localStorage.getItem(APP_SETTINGS_KEY) ?? 'null');
  if (saved?.version === 1) {
    general.value.rememberLayout = saved.general?.rememberLayout !== false;
    general.value.sendShortcut =
      saved.general?.sendShortcut === 'mod-enter' ? 'mod-enter' : 'enter';
  }
} catch {
  /* 损坏或不可读的偏好使用默认值。 */
}

/** 保存通用偏好；模型供应商配置由 Tauri command 单独持久化。 */
function saveSettings(next: GeneralSettings) {
  general.value = {
    rememberLayout: next.rememberLayout,
    sendShortcut: next.sendShortcut === 'mod-enter' ? 'mod-enter' : 'enter',
  };
  try {
    localStorage.setItem(APP_SETTINGS_KEY, JSON.stringify({ version: 1, general: general.value }));
    return { persisted: true };
  } catch {
    return { persisted: false };
  }
}

export function useAppSettings() {
  return { general: readonly(general), saveSettings };
}
