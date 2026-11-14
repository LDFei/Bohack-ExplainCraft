<script setup lang="ts">
/** 左侧素材与项目导航。素材仅定位预览，不改变服务层中的镜头顺序或内容。 */
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
  duration: number;
  layoutEnabled: boolean;
}>();
const emit = defineEmits<{
  new: [];
  select: [id: string];
  templates: [];
  settings: [];
  panelDragStart: [panel: WorkspacePanel, event: DragEvent];
  panelDragEnd: [];
  panelMove: [panel: WorkspacePanel, direction: number];
  rename: [project: Project];
  delete: [project: Project];
  close: [];
  upload: [];
  seek: [seconds: number];
  panel: [tab: string];
  resize: [expanded: boolean];
}>();
// 参照官方 Sidebar 的受控展开与悬停行为：300px / 60px，额外保留固定展开供编辑器长时间工作。
const pinned = ref(true),
  hovered = ref(false),
  focused = ref(false);
const expanded = computed(() => pinned.value || hovered.value || focused.value);
watch(expanded, (value) => emit('resize', value), { immediate: true });
/** 分隔线开始调整或恢复布局时固定展开，不改变素材与项目状态。 */
function pinOpen() {
  pinned.value = true;
}
defineExpose({ pinOpen });
/** 点击立即收起或固定，清除当次悬停，避免按钮位于侧栏内时无法收起。 */
function togglePinned() {
  pinned.value = !pinned.value;
  hovered.value = false;
  focused.value = false;
}
/** 键盘进入导航时展开；收起按钮自身保留直接操作能力。 */
function focusIn(event: FocusEvent) {
  if (!(event.target as HTMLElement).closest('.sidebar-collapse-control')) focused.value = true;
}
/** 焦点离开整个侧栏后取消临时展开，仍保留用户设置的固定状态。 */
function focusOut(event: FocusEvent) {
  if (!(event.currentTarget as HTMLElement).contains(event.relatedTarget as Node | null))
    focused.value = false;
}
const query = ref(''),
  menuId = ref(''),
  libraryTab = ref('clips'),
  selectedShot = ref('');
const filteredProjects = computed(() =>
  props.projects.filter((project) =>
    project.title.toLowerCase().includes(query.value.toLowerCase()),
  ),
);
const filteredShots = computed(() =>
  props.shots
    .map((shot, index) => ({
      ...shot,
      index,
      start: props.shots.slice(0, index).reduce((sum, item) => sum + item.duration_ms / 1000, 0),
    }))
    .filter((shot) => shot.visual_prompt.includes(query.value)),
);
/** 切换项目并关闭手机导航；状态与实际项目数据由父级统一管理。 */
function select(id: string) {
  menuId.value = '';
  selectedShot.value = '';
  emit('select', id);
  emit('close');
}
</script>
<template>
  <button
    v-if="open"
    class="sidebar-scrim"
    aria-label="关闭历史记录"
    @click="emit('close')"
  />
  <aside
    class="sidebar"
    data-workspace-panel="library"
    :class="{ 'is-open': open, 'is-expanded': expanded }"
    :data-expanded="expanded"
    @mouseenter="hovered = true"
    @mouseleave="hovered = false"
    @focusin="focusIn"
    @focusout="focusOut"
    aria-label="项目导航"
  >
    <nav
      class="tool-rail"
      aria-label="工作台工具"
    >
      <button
        class="rail-button sidebar-collapse-control"
        :aria-label="pinned ? '收起项目侧栏' : '固定展开侧栏'"
        :aria-expanded="expanded"
        :aria-pressed="pinned"
        @click="togglePinned"
      >
        <Icon
          name="sidebar"
          :size="20"
        />
        <span>{{ pinned ? '收起' : '展开' }}</span>
      </button>
      <button
        class="rail-button active"
        title="项目素材"
        aria-label="项目素材"
        @click="libraryTab = 'clips'"
      >
        <Icon
          name="folder"
          :size="21"
        />
        <span>素材</span>
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
      <button
        class="rail-button"
        aria-label="上传媒体"
        :disabled="busy"
        @click="emit('upload')"
      >
        <Icon
          name="upload"
          :size="20"
        />
        <span>上传</span>
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
      class="library-panel"
      :inert="!expanded && !open"
    >
      <header class="library-heading">
        <PanelDragHandle
          panel="library"
          label="项目素材"
          :enabled="layoutEnabled"
          @start="(panel, event) => emit('panelDragStart', panel, event)"
          @end="emit('panelDragEnd')"
          @move="(panel, direction) => emit('panelMove', panel, direction)"
        />
        <h2>项目素材</h2>
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
        <span class="library-count">{{ shots.length + assets.length }}</span>
      </header>
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
          aria-label="筛选已加载项目"
          placeholder="搜索素材或项目…"
        />
      </label>
      <div class="library-tabs">
        <button
          :class="{ active: libraryTab === 'clips' }"
          @click="libraryTab = 'clips'"
        >
          视频片段
          <span>{{ shots.length }}</span>
        </button>
        <button
          :class="{ active: libraryTab === 'uploads' }"
          @click="libraryTab = 'uploads'"
        >
          已上传
          <span>{{ assets.length }}</span>
        </button>
      </div>
      <div class="library-scroll">
        <template v-if="libraryTab === 'clips'">
          <div class="library-section-title">
            当前序列
            <span>当前分镜</span>
          </div>
          <button
            v-for="shot in filteredShots"
            :key="shot.id"
            class="clip-card"
            :class="{ selected: selectedShot === shot.id }"
            :aria-label="`定位镜头 ${shot.index + 1}`"
            @click="
              selectedShot = shot.id;
              emit(
                'seek',
                (shot.start /
                  (shots.reduce((sum, item) => sum + item.duration_ms / 1000, 0) || 1)) *
                  duration,
              );
              emit('close');
            "
          >
            <div class="clip-thumbnail">
              <img
                :src="shot.thumbnail_url || imagePath('coffee-cup')"
                :alt="shot.visual_prompt"
              />
              <span class="clip-number">{{ String(shot.index + 1).padStart(2, '0') }}</span>
              <span class="clip-duration">
                00:{{ String(Math.round(shot.duration_ms / 1000)).padStart(2, '0') }}
              </span>
              <span class="clip-hover-play">
                <Icon
                  name="play"
                  :size="18"
                />
              </span>
            </div>
            <div class="clip-description">
              <strong>{{ shot.visual_prompt.split('，')[0] }}</strong>
              <span>{{ shot.shot_size }} · {{ shot.camera_motion }}</span>
            </div>
          </button>
          <div
            v-if="!filteredShots.length"
            class="library-empty"
          >
            <Icon
              name="video"
              :size="28"
            />
            <strong>{{ query ? '没有匹配的片段' : '你的故事，从这里开始' }}</strong>
            <p>生成后，镜头会出现在这里</p>
          </div>
        </template>
        <template v-else>
          <div class="library-section-title">
            上传素材
            <span>{{ assets.length }} 个文件</span>
          </div>
          <button
            v-for="asset in assets"
            :key="asset.id"
            class="upload-library-item"
            @click="
              emit('panel', 'assets');
              emit('close');
            "
          >
            <img
              v-if="asset.thumbnail_url"
              :src="asset.thumbnail_url"
              alt=""
            />
            <Icon
              v-else
              :name="asset.kind === 'audio' ? 'music' : 'video'"
              :size="22"
            />
            <span>
              <strong>{{ asset.file_name }}</strong>
              <small>
                {{
                  asset.status === 'ready'
                    ? '已就绪'
                    : asset.status === 'failed'
                      ? '处理失败'
                      : '处理中'
                }}
              </small>
            </span>
          </button>
        </template>
        <button
          class="library-upload"
          :disabled="busy"
          @click="emit('upload')"
        >
          <Icon
            name="plus"
            :size="18"
          />
          <span>
            上传媒体
            <small>图片、视频或音频</small>
          </span>
        </button>
      </div>
      <section class="project-history">
        <div class="history-heading">
          <Icon
            name="clock"
            :size="13"
          />
          <span>创作历史</span>
          <span>{{ projects.length }}</span>
        </div>
        <div class="history-list">
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
                <small>
                  {{
                    project.status === 'processing'
                      ? '正在制作中'
                      : project.status === 'failed'
                        ? '制作暂时中断'
                        : project.status === 'waiting_input'
                          ? '等待补充信息'
                          : project.latest_video_version_id
                            ? '已完成'
                            : '草稿'
                  }}
                </small>
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
      </section>
      <footer class="library-footer">
        <span class="ready-dot" />
        本地工作空间
        <Icon
          name="lock"
          :size="12"
        />
      </footer>
    </div>
  </aside>
</template>
