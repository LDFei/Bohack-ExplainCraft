<script setup lang="ts">
/**
 * 全局左侧导航：rail 负责在工作台、素材库、历史、模板之间切换；
 * 展开面板在工作台模块里只提供素材概述与快捷操作，完整管理在素材库模块。
 */
import { computed, ref, watch } from 'vue';
import type { Asset, Project, Shot } from '../domain/types.ts';
import { imagePath, inferTheme, themes } from '../domain/content.ts';
import Icon from './Icon.vue';
import PanelDragHandle from './PanelDragHandle.vue';
import type { WorkspacePanel } from '../composables/useWorkspaceLayout.ts';

const props = defineProps<{
  projects: Project[];
  selectedId: string;
  open: boolean;
  shots: Shot[];
  assets: Asset[];
  busy: boolean;
  layoutEnabled: boolean;
  /** 素材库模块中只显示 rail，不渲染展开面板。 */
  railOnly?: boolean;
  /** 当前模块；决定 rail 高亮。 */
  module?: 'workbench' | 'library';
}>();

const emit = defineEmits<{
  new: [];
  select: [id: string];
  templates: [];
  settings: [];
  workbench: [];
  library: [];
  history: [];
  panelDragStart: [panel: WorkspacePanel, event: DragEvent];
  panelDragEnd: [];
  panelMove: [panel: WorkspacePanel, direction: number];
  rename: [project: Project];
  delete: [project: Project];
  close: [];
  upload: [files: FileList | File[]];
  resize: [expanded: boolean];
}>();

type Panel = 'overview' | 'history';
const pinned = ref(true),
  hovered = ref(false),
  focused = ref(false);
const expanded = computed(() => pinned.value || hovered.value || focused.value);
watch(expanded, (value) => emit('resize', value), { immediate: true });
const panel = ref<Panel>('overview');
const query = ref(''),
  menuId = ref('');

function pinOpen() {
  pinned.value = true;
}
/** 从素材库模块返回时直接展示历史面板，不额外占用一次点击。 */
function openHistoryPanel() {
  panel.value = 'history';
}
defineExpose({ pinOpen, openHistoryPanel });

function togglePinned() {
  pinned.value = !pinned.value;
  hovered.value = false;
  focused.value = false;
}

function focusIn(event: FocusEvent) {
  if (!(event.target as HTMLElement).closest('.sidebar-collapse-control')) focused.value = true;
}

function focusOut(event: FocusEvent) {
  if (!(event.currentTarget as HTMLElement).contains(event.relatedTarget as Node | null))
    focused.value = false;
}

/** 历史是工作台里的面板；已经在素材库模块时先回到工作台。 */
function openHistory() {
  emit('history');
  if (!props.railOnly) {
    panel.value = 'history';
    query.value = '';
    menuId.value = '';
  }
}

const uploadAssets = computed(() => props.assets.filter((asset) => asset.source === 'upload'));
const countOf = (kind: Asset['kind']) =>
  uploadAssets.value.filter((asset) => asset.kind === kind).length;
const overview = computed(() => [
  { kind: 'image' as const, label: '图片', icon: 'image', count: countOf('image') },
  { kind: 'video' as const, label: '视频', icon: 'video', count: countOf('video') },
  { kind: 'audio' as const, label: '音频', icon: 'music', count: countOf('audio') },
]);

const draggedFile = ref(false);
const fileInput = ref<HTMLInputElement | null>(null);

function dragOver(event: DragEvent) {
  if (!event.dataTransfer?.types.includes('Files')) return;
  event.preventDefault();
  draggedFile.value = true;
}

function dragLeave(event: DragEvent) {
  if ((event.currentTarget as HTMLElement).contains(event.relatedTarget as Node | null)) return;
  draggedFile.value = false;
}

function dropFiles(event: DragEvent) {
  if (!event.dataTransfer?.files.length) return;
  event.preventDefault();
  draggedFile.value = false;
  emit('upload', event.dataTransfer.files);
}

function pickFiles() {
  if (!props.busy) fileInput.value?.click();
}

function onFilesChosen(event: Event) {
  const input = event.target as HTMLInputElement;
  if (input.files?.length) emit('upload', input.files);
  input.value = '';
}

const filteredProjects = computed(() =>
  props.projects.filter((project) =>
    project.title.toLowerCase().includes(query.value.toLowerCase()),
  ),
);
const shotCount = computed(() => props.shots.length);

function projectStatus(project: Project) {
  if (project.status === 'processing') return '正在制作中';
  if (project.status === 'failed') return '制作暂时中断';
  if (project.status === 'waiting_input') return '等待补充信息';
  return project.latest_video_version_id ? '已完成' : '草稿';
}

function select(id: string) {
  menuId.value = '';
  emit('select', id);
  emit('close');
}
</script>

<template>
  <button
    v-if="open"
    class="sidebar-scrim"
    aria-label="关闭导航"
    @click="emit('close')"
  />
  <aside
    class="sidebar"
    :class="{ 'is-open': open, 'is-expanded': expanded, 'is-rail-only': railOnly }"
    :data-expanded="expanded"
    @mouseenter="hovered = true"
    @mouseleave="hovered = false"
    @focusin="focusIn"
    @focusout="focusOut"
    aria-label="工作台导航"
  >
    <nav
      class="tool-rail"
      aria-label="工作台工具"
    >
      <button
        class="rail-button sidebar-collapse-control"
        :aria-label="railOnly ? '返回工作台' : pinned ? '收起侧栏' : '固定展开侧栏'"
        :aria-expanded="expanded"
        :aria-pressed="pinned"
        @click="railOnly ? emit('workbench') : togglePinned()"
      >
        <Icon
          name="sidebar"
          :size="20"
        />
        <span>{{ railOnly ? '返回' : pinned ? '收起' : '展开' }}</span>
      </button>
      <button
        class="rail-button"
        :class="{ active: module !== 'library' && panel === 'overview' }"
        aria-label="工作台"
        :aria-pressed="module !== 'library' && panel === 'overview'"
        @click="
          emit('workbench');
          panel = 'overview';
        "
      >
        <Icon
          name="aperture"
          :size="21"
        />
        <span>工作台</span>
      </button>
      <button
        class="rail-button"
        :class="{ active: module === 'library' }"
        aria-label="素材库"
        :aria-pressed="module === 'library'"
        @click="emit('library')"
      >
        <Icon
          name="folder"
          :size="21"
        />
        <span>素材库</span>
      </button>
      <button
        class="rail-button"
        :class="{ active: module !== 'library' && panel === 'history' }"
        aria-label="历史创作"
        :aria-pressed="module !== 'library' && panel === 'history'"
        @click="openHistory"
      >
        <Icon
          name="clock"
          :size="20"
        />
        <span>历史</span>
      </button>
      <button
        class="rail-button"
        aria-label="灵感模板"
        @click="
          emit('templates');
          emit('close');
        "
      >
        <Icon
          name="grid"
          :size="20"
        />
        <span>模板</span>
      </button>
      <div class="rail-spacer" />
      <button
        class="icon-button workspace-settings"
        aria-label="打开工作台设置"
        title="工作台设置"
        @click="emit('settings')"
      >
        <Icon
          name="settings"
          :size="21"
        />
      </button>
    </nav>
    <div
      v-if="!railOnly"
      class="library-panel"
      :inert="!expanded && !open"
    >
      <header class="library-heading">
        <PanelDragHandle
          panel="library"
          label="素材概述"
          :enabled="layoutEnabled"
          @start="(id, event) => emit('panelDragStart', id, event)"
          @end="emit('panelDragEnd')"
          @move="(id, direction) => emit('panelMove', id, direction)"
        />
        <h2>{{ panel === 'overview' ? '素材概述' : '历史创作' }}</h2>
        <button
          class="icon-button mobile-only"
          aria-label="关闭导航"
          @click="emit('close')"
        >
          <Icon
            name="close"
            :size="17"
          />
        </button>
        <span class="library-count">
          {{ panel === 'overview' ? uploadAssets.length : projects.length }}
        </span>
      </header>

      <template v-if="panel === 'overview'">
        <div
          class="asset-overview"
          :class="{ 'is-dragging': draggedFile }"
          @dragover="dragOver"
          @dragleave="dragLeave"
          @drop="dropFiles"
        >
          <button
            class="asset-upload"
            :disabled="busy"
            aria-label="添加素材"
            @click="pickFiles"
          >
            <Icon
              name="plus"
              :size="16"
            />
            <span>添加素材</span>
            <small>点击或拖入</small>
          </button>
          <input
            ref="fileInput"
            class="sr-only"
            type="file"
            multiple
            accept="image/*,video/*,audio/*"
            tabindex="-1"
            @change="onFilesChosen"
          />
          <div class="overview-list">
            <button
              v-for="item in overview"
              :key="item.kind"
              class="overview-row"
              :aria-label="`打开素材库：${item.label}`"
              @click="emit('library')"
            >
              <Icon
                :name="item.icon"
                :size="14"
              />
              <span>{{ item.label }}</span>
              <strong>{{ item.count }}</strong>
              <Icon
                name="arrowRight"
                :size="12"
              />
            </button>
          </div>
          <button
            class="overview-manage"
            @click="emit('library')"
          >
            打开素材库，管理全部素材
            <Icon
              name="arrowRight"
              :size="13"
            />
          </button>
          <p
            v-if="shotCount"
            class="asset-hint"
          >
            已生成 {{ shotCount }} 个镜头，可在中央「脚本」中逐个查看。
          </p>
        </div>
      </template>

      <template v-else>
        <button
          aria-label="新建创作"
          class="new-project"
          @click="
            emit('new');
            emit('close');
          "
        >
          <Icon
            name="plus"
            :size="16"
          />
          新建创作
          <kbd>＋</kbd>
        </button>
        <label class="history-search">
          <Icon
            name="search"
            :size="15"
          />
          <input
            v-model="query"
            aria-label="搜索项目"
            placeholder="搜索项目…"
          />
        </label>
        <div class="history-list history-list-full">
          <div
            v-for="project in filteredProjects"
            :key="project.id"
            class="history-project"
            :class="{ selected: selectedId === project.id }"
          >
            <button
              class="project-link"
              :aria-current="selectedId === project.id ? 'page' : undefined"
              @click="select(project.id)"
            >
              <img
                :src="imagePath(themes[inferTheme(project.title)].images[0])"
                alt=""
              />
              <span class="project-label">
                <span>{{ project.title }}</span>
                <small>{{ projectStatus(project) }}</small>
              </span>
            </button>
            <button
              class="project-more icon-button"
              :aria-label="`管理项目：${project.title}`"
              :aria-expanded="menuId === project.id"
              @click="menuId = menuId === project.id ? '' : project.id"
            >
              <Icon
                name="more"
                :size="14"
              />
            </button>
            <div
              v-if="menuId === project.id"
              class="project-menu"
            >
              <button
                @click="
                  emit('rename', project);
                  menuId = '';
                "
              >
                <Icon
                  name="edit"
                  :size="14"
                />
                重命名
              </button>
              <button
                class="danger-text"
                @click="
                  emit('delete', project);
                  menuId = '';
                "
              >
                <Icon
                  name="trash"
                  :size="14"
                />
                删除项目
              </button>
            </div>
          </div>
          <p
            v-if="!filteredProjects.length"
            class="history-empty"
          >
            {{ query ? '没有匹配的项目' : '尚无创作记录' }}
          </p>
        </div>
      </template>
    </div>
  </aside>
</template>
