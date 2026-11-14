/** 外观偏好独立于项目数据保存，切换主题不会修改草稿、播放器或任务状态。 */
import { computed, onBeforeUnmount, onMounted, readonly, ref } from "vue";

export const themeOptions = [
  {
    id: "silver",
    name: "银灰工作室",
    subtitle: "清晰的层次，专注每一个镜头",
    tag: "默认推荐",
  },
  {
    id: "lilac",
    name: "浅丁香创意空间",
    subtitle: "轻盈的粉紫，让灵感自然发生",
    tag: "轻盈创意",
  },
  {
    id: "paper",
    name: "暖纸编辑室",
    subtitle: "温暖的纸感，留一点创作的从容",
    tag: "温暖柔和",
  },
  {
    id: "mist",
    name: "雾蓝创作台",
    subtitle: "清爽的蓝调，让思路更加清晰",
    tag: "清爽理性",
  },
] as const;
export type ThemeId = (typeof themeOptions)[number]["id"];
export const DEFAULT_THEME: ThemeId = "silver";
export const THEME_STORAGE_KEY = "explaincraft.ui-theme";
const selectedTheme = ref<ThemeId>(DEFAULT_THEME);
const persistenceError = ref(false);

/** 仅接受内置主题，兼容无偏好、旧值或被手动修改的浏览器存储。 */
function resolveTheme(value: string | null): ThemeId {
  return themeOptions.find((theme) => theme.id === value)?.id ?? DEFAULT_THEME;
}

/** 作用于文档根节点，让弹窗等挂到 body 的内容也同步换肤，不重新挂载工作台。 */
function applyTheme(id: ThemeId) {
  selectedTheme.value = id;
  document.documentElement.dataset.theme = id;
  const color = getComputedStyle(document.documentElement)
    .getPropertyValue("--surface")
    .trim();
  document
    .querySelector('meta[name="theme-color"]')
    ?.setAttribute("content", color);
}

/** 在 Vue 挂载前读取外观，减少刷新时的颜色闪动；存储受限时仍可正常打开页面。 */
export function initializeTheme() {
  let preference: string | null = null;
  try {
    preference = localStorage.getItem(THEME_STORAGE_KEY);
  } catch {
    // 浏览器禁止本地存储时使用默认主题，项目初始化不依赖此偏好读取成功。
  }
  applyTheme(resolveTheme(preference));
}

/** 提供即时切换、恢复默认与跨窗口同步，生命周期清理只影响外观事件监听。 */
export function useTheme() {
  const currentTheme = computed(() =>
    themeOptions.find((theme) => theme.id === selectedTheme.value)!,
  );

  /** 先更新当前页面，再尽力保存；保存失败不撤销用户正在查看的主题。 */
  function setTheme(id: ThemeId) {
    applyTheme(id);
    try {
      localStorage.setItem(THEME_STORAGE_KEY, id);
      persistenceError.value = false;
    } catch {
      persistenceError.value = true;
    }
  }

  /** 恢复推荐的默认外观，不清理任何项目或上传数据。 */
  function resetTheme() {
    setTheme(DEFAULT_THEME);
  }

  /** 同源窗口修改或清除偏好时只同步外观，不重复写入以免触发来回同步。 */
  function syncTheme(event: StorageEvent) {
    if (event.key === THEME_STORAGE_KEY || event.key === null) {
      applyTheme(resolveTheme(event.newValue));
    }
  }
  onMounted(() => window.addEventListener("storage", syncTheme));
  onBeforeUnmount(() => window.removeEventListener("storage", syncTheme));
  return {
    currentTheme,
    selectedTheme: readonly(selectedTheme),
    persistenceError: readonly(persistenceError),
    setTheme,
    resetTheme,
  };
}
