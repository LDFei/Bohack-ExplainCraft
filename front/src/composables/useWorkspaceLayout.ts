/** 布局只改变 CSS 网格位置和尺寸，不重建面板，避免打断视频、输入或生成任务。 */
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue';
import type { CSSProperties, Ref } from 'vue';
export const LAYOUT_STORAGE_KEY = 'explaincraft.workspace-layout';
export type WorkspacePanel = 'library' | 'preview' | 'copilot';
export const panelLabels: Record<WorkspacePanel, string> = {
  library: '项目素材',
  preview: '预览画布',
  copilot: 'AI 对话',
};
const allPanels: WorkspacePanel[] = ['library', 'preview', 'copilot'];
const minimumWidth: Record<WorkspacePanel, number> = { library: 210, preview: 300, copilot: 280 };
/** 每次恢复使用独立副本，避免重置操作被当前拖拽修改污染。 */
function defaultLayout() {
  return {
    order: [...allPanels],
    weights: { library: 300, preview: 794, copilot: 330 },
    timelineHeight: 244,
  };
}
/** 只接受完整、无重复的面板顺序，损坏配置回退默认，尺寸限制在可恢复范围。 */
function readLayout(remember: boolean) {
  const fallback = defaultLayout();
  if (!remember) return fallback;
  try {
    const value = JSON.parse(localStorage.getItem(LAYOUT_STORAGE_KEY) ?? 'null');
    if (
      value?.version !== 1 ||
      !Array.isArray(value.order) ||
      value.order.length !== 3 ||
      new Set(value.order).size !== 3 ||
      value.order.some((id: unknown) => !allPanels.includes(id as WorkspacePanel))
    )
      return fallback;
    for (const id of allPanels)
      if (
        typeof value.weights?.[id] !== 'number' ||
        !Number.isFinite(value.weights[id]) ||
        value.weights[id] < 1 ||
        value.weights[id] > 10000
      )
        return fallback;
    return {
      order: value.order as WorkspacePanel[],
      weights: { ...value.weights } as Record<WorkspacePanel, number>,
      timelineHeight: Math.min(420, Math.max(140, Number(value.timelineHeight) || 244)),
    };
  } catch {
    return fallback;
  }
}
/** 提供原生拖放、指针与键盘调宽；窄屏沿用分区布局，不挤压成桌面多栏。 */
export function useWorkspaceLayout(
  root: Ref<HTMLElement | null>,
  expanded: Ref<boolean>,
  remember: Ref<boolean>,
  expandLibrary: () => void,
) {
  const state = reactive(readLayout(remember.value));
  const media = window.matchMedia('(min-width: 1051px)');
  const viewportHeight = ref(window.innerHeight);
  const maximumTimelineHeight = computed(() =>
    Math.max(140, Math.min(420, viewportHeight.value * 0.46, viewportHeight.value - 410)),
  );
  const timelineDisplayHeight = computed(() =>
    Math.min(state.timelineHeight, maximumTimelineHeight.value),
  );
  const isDesktop = ref(media.matches),
    resizing = ref(false),
    draggedPanel = ref<WorkspacePanel | null>(null),
    dropTarget = ref<WorkspacePanel | null>(null);
  let activeResize: {
    index: number;
    x: number;
    widths: Record<WorkspacePanel, number>;
    collapsed: boolean;
  } | null = null;
  const workspaceStyle = computed<CSSProperties>(() => {
    if (!isDesktop.value) return {};
    const tracks = state.order.map((id) =>
      id === 'library' && !expanded.value
        ? '60px'
        : `minmax(${minimumWidth[id]}px, ${state.weights[id]}fr)`,
    );
    return {
      gridTemplateColumns: tracks.join(' 8px '),
      '--library-column': state.order.indexOf('library') * 2 + 1,
      '--preview-column': state.order.indexOf('preview') * 2 + 1,
      '--copilot-column': state.order.indexOf('copilot') * 2 + 1,
    } as CSSProperties;
  });
  /** 拖动结束后才持久化，避免每一帧写存储；关闭记忆时清理的只有布局偏好。 */
  function persist() {
    try {
      if (remember.value)
        localStorage.setItem(LAYOUT_STORAGE_KEY, JSON.stringify({ version: 1, ...state }));
      else localStorage.removeItem(LAYOUT_STORAGE_KEY);
    } catch {
      /* 存储受限时本页仍可调整，刷新使用默认布局。 */
    }
  }
  watch(remember, persist);
  /** 恢复面板顺序、宽度和时间轴高度，不修改项目、脚本或素材。 */
  function resetLayout() {
    Object.assign(state, defaultLayout());
    expandLibrary();
    persist();
  }
  /** 将指定面板移动到目标面板的位置，保留每个面板自己的宽度偏好。 */
  function moveTo(id: WorkspacePanel, target: WorkspacePanel) {
    if (id === target) return;
    const next = [...state.order];
    next.splice(next.indexOf(id), 1);
    next.splice(state.order.indexOf(target), 0, id);
    state.order = next;
    persist();
  }
  /** Alt + 方向键提供与拖放等价的位置调整，便于键盘用户操作。 */
  function movePanel(id: WorkspacePanel, direction: number) {
    const target = state.order[state.order.indexOf(id) + direction];
    if (isDesktop.value && target) moveTo(id, target);
  }
  /** 仅用独立的面板 MIME 标记拖放，不读取用户文件或冒充素材上传。 */
  function startDrag(id: WorkspacePanel, event: DragEvent) {
    if (!isDesktop.value || resizing.value) {
      event.preventDefault();
      return;
    }
    draggedPanel.value = id;
    if (event.dataTransfer) {
      event.dataTransfer.setData('application/x-explaincraft-panel', id);
      event.dataTransfer.effectAllowed = 'move';
    }
  }
  /** 从实际命中的面板识别放置目标，内部按钮和 SVG 都能正确落到所属面板。 */
  function targetPanel(event: DragEvent): WorkspacePanel | undefined {
    const element =
      event.target instanceof Element
        ? event.target.closest<HTMLElement>('[data-workspace-panel]')
        : null;
    const id = element?.dataset.workspacePanel as WorkspacePanel | undefined;
    return id && allPanels.includes(id) ? id : undefined;
  }
  /** 只拦截正在进行的面板拖动，让原有文件拖入继续交给上传流程。 */
  function dragOver(event: DragEvent) {
    if (!draggedPanel.value || !isDesktop.value) return;
    const id = targetPanel(event);
    if (!id) return;
    event.preventDefault();
    dropTarget.value = id;
    if (event.dataTransfer) event.dataTransfer.dropEffect = 'move';
  }
  /** 结束原生拖放并清理高亮，取消拖动不会更改顺序。 */
  function endDrag() {
    draggedPanel.value = null;
    dropTarget.value = null;
  }
  /** 放置后只修改网格列号，video 与对话组件的 DOM 身份不变。 */
  function drop(event: DragEvent) {
    const id = targetPanel(event);
    if (draggedPanel.value && id && isDesktop.value) {
      event.preventDefault();
      event.stopPropagation();
      moveTo(draggedPanel.value, id);
    }
    endDrag();
  }
  /** 读取当前真实宽度，拖动从当前布局出发，不受窗口大小或面板顺序影响。 */
  function measure() {
    return Object.fromEntries(
      allPanels.map((id) => [
        id,
        root.value
          ?.querySelector<HTMLElement>(`[data-workspace-panel="${id}"]`)
          ?.getBoundingClientRect().width ?? minimumWidth[id],
      ]),
    ) as Record<WorkspacePanel, number>;
  }
  /** 将相邻两栏限定在最小可用宽度内，其余面板保持当前实际大小。 */
  function resizePair(
    index: number,
    delta: number,
    widths: Record<WorkspacePanel, number>,
    collapsed: boolean,
  ) {
    const left = state.order[index]!,
      right = state.order[index + 1]!;
    const total = widths[left] + widths[right];
    const leftWidth = Math.min(
      total - minimumWidth[right],
      Math.max(minimumWidth[left], widths[left] + delta),
    );
    const next = { ...widths, [left]: leftWidth, [right]: total - leftWidth };
    if (collapsed) {
      const scale = (state.weights.preview + state.weights.copilot) / (next.preview + next.copilot);
      state.weights.preview = next.preview * scale;
      state.weights.copilot = next.copilot * scale;
    } else Object.assign(state.weights, next);
  }
  /** 指针捕获保证拖出分隔线仍可继续调宽；拖动收起素材栏旁的分隔线时先展开。 */
  async function beginResize(index: number, event: PointerEvent) {
    if (!isDesktop.value || event.button !== 0) return;
    event.preventDefault();
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    resizing.value = true;
    const adjacentLibrary =
      state.order[index] === 'library' || state.order[index + 1] === 'library';
    if (adjacentLibrary && !expanded.value) {
      expandLibrary();
      await nextTick();
    }
    if (!resizing.value) return;
    activeResize = { index, x: event.clientX, widths: measure(), collapsed: !expanded.value };
  }
  /** 拖动中仅更新网格变量，停止后统一保存偏好。 */
  function resizeMove(event: PointerEvent) {
    if (activeResize)
      resizePair(
        activeResize.index,
        event.clientX - activeResize.x,
        activeResize.widths,
        activeResize.collapsed,
      );
  }
  /** 指针松开、取消或窗口离焦都结束拖动，避免遗留全页调整光标。 */
  function endResize() {
    if (resizing.value) persist();
    activeResize = null;
    resizing.value = false;
  }
  /** 方向键按 24px 调宽，Shift 按 64px 调整，提供鼠标以外的等价操作。 */
  function resizeKey(index: number, event: KeyboardEvent) {
    if (!isDesktop.value || !['ArrowLeft', 'ArrowRight'].includes(event.key)) return;
    event.preventDefault();
    if (!expanded.value && [state.order[index], state.order[index + 1]].includes('library')) {
      expandLibrary();
      return;
    }
    resizePair(
      index,
      (event.key === 'ArrowLeft' ? -1 : 1) * (event.shiftKey ? 64 : 24),
      measure(),
      !expanded.value,
    );
    persist();
  }
  /** 时间轴在可用窗口高度内调整，防止把画布完全挤出屏幕。 */
  function setTimelineHeight(height: number, save = false) {
    state.timelineHeight = Math.round(Math.max(140, Math.min(maximumTimelineHeight.value, height)));
    if (save) persist();
  }
  /** 窄屏切回分区时结束拖动，保留桌面偏好供回到大屏时恢复。 */
  function updateViewport() {
    viewportHeight.value = window.innerHeight;
    isDesktop.value = media.matches;
    endResize();
    endDrag();
  }
  onMounted(() => {
    media.addEventListener('change', updateViewport);
    window.addEventListener('blur', endResize);
    window.addEventListener('resize', updateViewport);
  });
  onBeforeUnmount(() => {
    media.removeEventListener('change', updateViewport);
    window.removeEventListener('blur', endResize);
    window.removeEventListener('resize', updateViewport);
    endResize();
  });
  return {
    state,
    timelineDisplayHeight,
    isDesktop,
    workspaceStyle,
    resizing,
    draggedPanel,
    dropTarget,
    resetLayout,
    startDrag,
    dragOver,
    drop,
    endDrag,
    movePanel,
    beginResize,
    resizeMove,
    endResize,
    resizeKey,
    setTimelineHeight,
  };
}
