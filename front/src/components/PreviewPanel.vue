<script setup lang="ts">
/**
 * 作品预览与可编辑草稿面板。播放器读取成片，表单编辑当前草稿；历史版本以只读快照展示。
 */

import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { getModelSettings, listVoices } from '../services/settings.ts';
import { copy, emptyDraft, imagePath } from '../domain/content.ts';
import type {
  Asset,
  Capabilities,
  Draft,
  DraftChanges,
  Job,
  VideoVersion,
} from '../domain/types.ts';
import Icon from './Icon.vue';
import VideoTimeline from './VideoTimeline.vue';
import PanelDragHandle from './PanelDragHandle.vue';
import type { WorkspacePanel } from '../composables/useWorkspaceLayout.ts';
const props = defineProps<{
  projectId: string;
  draft: Draft | null;
  revision: number;
  dirty: boolean;
  version: VideoVersion | null;
  versions: VideoVersion[];
  latestVersionId: string | null;
  assets: Asset[];
  caps: Capabilities;
  job: Job | null;
  busy: boolean;
  pending: boolean;
  open: boolean;
  layoutEnabled: boolean;
  timelineHeight?: number;
  onSave: (changes: DraftChanges, revision: number) => Promise<boolean>;
}>();

const remoteVoices = ref<string[]>([]);
const voiceChoices = computed(() =>
  remoteVoices.value.length ? remoteVoices.value : props.caps.voices.map((voice) => voice.id),
);
/** 背景音乐候选只列已就绪的上传音频，避免选中处理中或失败的素材。 */
const audioAssets = computed(() =>
  props.assets.filter(
    (asset) => asset.kind === 'audio' && asset.source === 'upload' && asset.status === 'ready',
  ),
);

/** 优先使用供应商返回的声线，接口不可用时保留 capabilities 中的配置候选。 */
onMounted(async () => {
  try {
    const settings = await getModelSettings();
    remoteVoices.value = await listVoices(settings.voice);
    if (
      remoteVoices.value.length &&
      (!form.value.audio.voice_id ||
        ['voice_warm', 'voice_calm', 'voice_bright'].includes(form.value.audio.voice_id))
    ) {
      form.value.audio.voice_id = remoteVoices.value[0]!;
    }
  } catch {
    // 不支持 /audio/voices 的供应商使用 capabilities 中的配置候选。
  }
});
const emit = defineEmits<{
  select: [id: string];
  download: [id: string];
  export: [];
  update: [];
  close: [];
  latest: [];
  playing: [value: boolean];
  prompt: [text: string];
  notify: [text: string];
  panelDragStart: [panel: WorkspacePanel, event: DragEvent];
  panelDragEnd: [];
  panelMove: [panel: WorkspacePanel, direction: number];
  timelineHeight: [height: number, save: boolean];
}>();
const detailsOpen = ref(false),
  currentTime = ref(0),
  muted = ref(false);
const undoStack = ref<string[]>([]),
  redoStack = ref<string[]>([]);
let restoring = false;
const canUndo = computed(() => !disableForm.value && undoStack.value.length > 0);
const canRedo = computed(() => !disableForm.value && redoStack.value.length > 0);
const tab = ref('script'),
  video = ref<HTMLVideoElement | null>(null),
  playing = ref(false),
  mediaError = ref(false);
// 表单使用独立副本；formRevision 保留编辑起点，formDirty 防止后台刷新冲掉未保存输入。
const form = ref<Draft>(emptyDraft()),
  formRevision = ref(1),
  formDirty = ref(false),
  editingScript = ref(false),
  auditionPlaying = ref(false);
// 历史版本只读；显示的是它自己的 snapshot，不是今天最新的草稿。
const readOnly = computed(() => !!props.version && props.version.id !== props.latestVersionId);
const displayedDraft = computed(() =>
  readOnly.value ? props.version!.snapshot : (props.draft ?? emptyDraft()),
);
const tabs = [
  { id: 'script', name: '脚本', icon: 'file' },
  { id: 'audio', name: '声音', icon: 'music' },
  { id: 'settings', name: '设置', icon: 'sliders' },
];
// 制作进行中或尚未创建项目时，不允许表单与后台任务同时修改草稿。
const disableForm = computed(
  () => readOnly.value || props.busy || props.pending || !props.projectId,
);
let audition: HTMLAudioElement | null = null;
/**
 * 把当前显示的草稿复制到本地表单，并记住修订号；编辑表单不会直接修改父级数据。
 */
function resetForm() {
  form.value = copy(displayedDraft.value);
  formRevision.value = props.revision;
  formDirty.value = false;
  editingScript.value = false;
  undoStack.value = [];
  redoStack.value = [];
}
// 换项目或成片版本时，清空旧播放与试听状态，并重新建立表单副本。
watch(
  () => [props.projectId, props.version?.id],
  () => {
    resetForm();
    mediaError.value = false;
    currentTime.value = 0;
    playing.value = false;
    emit('playing', false);
    stopAudition();
  },
  { immediate: true },
);
// 来自其他页面的新修订仅在本地没有未保存编辑时回填；否则由保存时的冲突检测处理。
watch(
  () => props.revision,
  () => {
    if (!formDirty.value) resetForm();
  },
);
// 草稿对象的深层更新同样不能覆盖用户尚未保存的表单。
watch(
  () => props.draft,
  () => {
    if (!formDirty.value) resetForm();
  },
  { deep: true },
);
/**
 * 停止并释放独立的音乐试听对象，防止切换版本或离开页面后声音继续播放。
 */
function stopAudition() {
  audition?.pause();
  audition = null;
  auditionPlaying.value = false;
}
// 详情面板收起时也停止试听，避免不可见的音乐控件继续发声。
watch(detailsOpen, (open) => {
  if (!open) stopAudition();
});
/**
 * 切换示例背景音乐的播放状态；只播放服务提供的 sample_url，不在前端生成配音。
 */
async function playMusic() {
  if (auditionPlaying.value) {
    stopAudition();
    return;
  }
  const track =
    props.caps.music_tracks.find((item) => item.id === form.value.audio.music_id) ??
    props.caps.music_tracks[0];
  if (!track?.sample_url) return;
  audition = new Audio(track.sample_url);
  audition.onended = () => {
    auditionPlaying.value = false;
  };
  audition.onerror = () => {
    emit('notify', '试听暂时不可用，请稍后重试。');
    stopAudition();
  };
  try {
    await audition.play();
    auditionPlaying.value = true;
  } catch {
    emit('notify', '请再次点击试听播放音乐。');
    stopAudition();
  }
}
/**
 * 把下拉值拆成自动、无音乐、曲库或上传四种来源，并清空不属于该来源的 ID。
 */
function musicMode(event: Event) {
  const value = (event.target as HTMLSelectElement).value;
  if (value === 'auto' || value === 'none') {
    form.value.audio.music_mode = value;
    form.value.audio.music_id = null;
    form.value.audio.music_asset_id = null;
  } else if (value.startsWith('upload:')) {
    form.value.audio.music_mode = 'upload';
    form.value.audio.music_id = null;
    form.value.audio.music_asset_id = value.slice(7);
  } else {
    form.value.audio.music_mode = 'library';
    form.value.audio.music_id = value;
    form.value.audio.music_asset_id = null;
  }
  formDirty.value = true;
  stopAudition();
}
// 把互斥的音乐来源编码成一个下拉框值，upload: 前缀用于区别附件 ID 和曲库 ID。
const musicValue = computed(() =>
  form.value.audio.music_mode === 'library'
    ? form.value.audio.music_id!
    : form.value.audio.music_mode === 'upload'
      ? `upload:${form.value.audio.music_asset_id}`
      : form.value.audio.music_mode,
);
/**
 * 比较本地表单和原草稿，只提交变化的顶层分组。
 * 移除服务端只读音乐字段，并携带开始编辑时的 revision；冲突时保留本地表单。
 */
async function save() {
  const changes: DraftChanges = {},
    original = props.draft ?? emptyDraft();
  for (const key of ['settings', 'audio', 'finishing'] as const) {
    if (JSON.stringify(form.value[key]) !== JSON.stringify(original[key])) {
      if (key === 'settings') changes.settings = copy(form.value.settings);
      if (key === 'finishing') changes.finishing = copy(form.value.finishing);
      if (key === 'audio') {
        changes.audio = copy(form.value.audio);
        // 真实接口不允许客户端写“最终解析出的音乐素材”，所以发送前移除只读字段。
        delete changes.audio.resolved_music_asset_id;
      }
    }
  }
  if (editingScript.value && JSON.stringify(form.value.script) !== JSON.stringify(original.script))
    changes.script = copy(form.value.script);
  if (!Object.keys(changes).length) {
    resetForm();
    return;
  }
  if (await props.onSave(changes, formRevision.value)) resetForm();
}
/**
 * 通过真实 video 元素切换播放或暂停，浏览器拒绝播放时给出提示。
 */
async function togglePlay() {
  if (!video.value) return;
  if (video.value.paused) {
    try {
      await video.value.play();
    } catch {
      emit('notify', '视频暂时无法播放，请重试。');
    }
  } else video.value.pause();
}
/**
 * 公开给父组件的标签切换入口；对话中的“查看脚本”使用它定位中央详情内容。
 */
function openTab(id: string) {
  tab.value = id;
  detailsOpen.value = true;
}
/** 仅记录本地未保存表单，撤销不回滚已生成版本或覆盖服务端草稿。 */
watch(
  () => JSON.stringify(form.value),
  (value, previous) => {
    if (restoring || !formDirty.value || value === previous) return;
    undoStack.value.push(previous);
    if (undoStack.value.length > 50) undoStack.value.shift();
    redoStack.value = [];
  },
);
/** 恢复一次本地编辑快照；等待响应式更新后再解除保护，避免恢复动作被再次入栈。 */
async function restoreEdit(source: string[], destination: string[]) {
  const snapshot = source.pop();
  if (!snapshot || disableForm.value) return;
  restoring = true;
  destination.push(JSON.stringify(form.value));
  form.value = JSON.parse(snapshot) as Draft;
  formDirty.value = JSON.stringify(form.value) !== JSON.stringify(displayedDraft.value);
  await nextTick();
  restoring = false;
}
/** 撤销当前未保存表单的一步修改。 */
function undo() {
  return restoreEdit(undoStack.value, redoStack.value);
}
/** 重做刚撤销的表单修改。 */
function redo() {
  return restoreEdit(redoStack.value, undoStack.value);
}
/** 时间轴和素材卡共用真实播放器定位，切换项目时不会沿用旧播放位置。 */
function seek(seconds: number) {
  if (!video.value || !props.version) return;
  const bounded = Math.min(props.version.duration_ms / 1000, Math.max(0, seconds));
  video.value.currentTime = bounded;
  currentTime.value = bounded;
}
/** 选择比例只修改草稿；保存并重新生成后才会改变成片文件。 */
function changeRatio(event: Event) {
  form.value.settings.aspect_ratio = (event.target as HTMLSelectElement).value;
  formDirty.value = true;
  openTab('settings');
}
/** 全屏操作交给浏览器原生播放器，不伪造成导出或新的渲染。 */
async function fullscreen() {
  try {
    await video.value?.requestFullscreen();
  } catch {
    emit('notify', '当前浏览器暂不支持全屏播放。');
  }
}
/** 画布下方使用常见的分秒与帧数时间码。 */
function timecode(seconds: number) {
  return `${String(Math.floor(seconds / 60)).padStart(2, '0')}:${String(Math.floor(seconds % 60)).padStart(2, '0')}:${String(Math.floor((seconds % 1) * 30)).padStart(2, '0')}`;
}
let timelineDrag: { y: number; height: number; latest: number } | null = null;
/** 捕获时间轴上沿拖动，让高度跟随指针，松开后才保存布局。 */
function startTimelineResize(event: PointerEvent) {
  if (event.button !== 0) return;
  event.preventDefault();
  (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
  timelineDrag = {
    y: event.clientY,
    height: props.timelineHeight ?? 244,
    latest: props.timelineHeight ?? 244,
  };
}
/** 向上拖动增大时间轴，画布自动获得剩余高度。 */
function moveTimelineResize(event: PointerEvent) {
  if (!timelineDrag) return;
  timelineDrag.latest = timelineDrag.height + timelineDrag.y - event.clientY;
  emit('timelineHeight', timelineDrag.latest, false);
}
/** 松开或取消拖动时提交当前高度，避免刷新后丢失布局。 */
function stopTimelineResize() {
  if (timelineDrag) emit('timelineHeight', timelineDrag.latest, true);
  timelineDrag = null;
}
/** 上下方向键提供时间轴调高的等价操作。 */
function timelineKey(event: KeyboardEvent) {
  if (['ArrowUp', 'ArrowDown'].includes(event.key)) {
    event.preventDefault();
    emit(
      'timelineHeight',
      (props.timelineHeight ?? 244) + (event.key === 'ArrowUp' ? 20 : -20),
      true,
    );
  }
}
onBeforeUnmount(stopAudition);
defineExpose({ openTab, seek, canUndo, canRedo, undo, redo });
</script>
<template>
  <aside
    class="preview-panel"
    data-workspace-panel="preview"
    :class="{ 'is-open': open }"
    aria-label="视频作品与设置"
  >
    <header class="preview-heading">
      <PanelDragHandle
        panel="preview"
        label="预览画布"
        :enabled="layoutEnabled"
        @start="(panel, event) => emit('panelDragStart', panel, event)"
        @end="emit('panelDragEnd')"
        @move="(panel, direction) => emit('panelMove', panel, direction)"
      />
      <div>
        <Icon
          name="video"
          :size="18"
        />
        <h2>预览画布</h2>
        <span
          v-if="version"
          class="ready-dot"
        />
      </div>
      <div>
        <label
          v-if="versions.length"
          class="version-select"
        >
          <select
            :value="version?.id"
            aria-label="切换视频版本"
            @change="emit('select', ($event.target as HTMLSelectElement).value)"
          >
            <option
              v-for="item in versions"
              :key="item.id"
              :value="item.id"
            >
              v{{ item.version_number }}{{ item.id === latestVersionId ? ' 最新版' : ' 历史版' }}
            </option>
          </select>
          <Icon
            name="down"
            :size="12"
          />
        </label>
        <button
          class="icon-button panel-close"
          aria-label="关闭作品面板"
          @click="emit('close')"
        >
          <Icon
            name="close"
            :size="18"
          />
        </button>
      </div>
    </header>
    <!-- 播放器读取成片文件；下面的表单修改的是 Draft，两者通过再次生成才同步。 -->
    <div class="canvas-toolbar">
      <div
        class="canvas-tool-group"
        role="tablist"
        aria-label="作品详情"
      >
        <span class="active-tool">
          <Icon
            name="cursor"
            :size="15"
          />
        </span>
        <span class="toolbar-divider" />
        <button
          v-for="item in tabs"
          :key="item.id"
          :class="{ active: detailsOpen && tab === item.id }"
          role="tab"
          :aria-selected="detailsOpen && tab === item.id"
          @click="detailsOpen && tab === item.id ? (detailsOpen = false) : openTab(item.id)"
        >
          <Icon
            :name="item.icon"
            :size="14"
          />
          {{ item.name }}
        </button>
      </div>
      <label class="canvas-ratio">
        <Icon
          name="ratio"
          :size="14"
        />
        <select
          aria-label="画布比例设置"
          :value="form.settings.aspect_ratio"
          :disabled="disableForm"
          @change="changeRatio"
        >
          <option
            v-for="ratio in caps.aspect_ratios"
            :key="ratio"
          >
            {{ ratio }}
          </option>
        </select>
      </label>
    </div>
    <div class="preview-content">
      <div class="canvas-area">
        <div class="canvas-corner-label">
          <span />
          {{ version ? 'PROGRAM MONITOR' : 'YOUR NEXT STORY' }}
        </div>
        <div class="canvas-frame">
          <div
            class="preview-stage"
            :class="{
              landscape: version && version.width > version.height,
              square: version && version.width === version.height,
            }"
          >
            <template v-if="version && !mediaError">
              <video
                :key="version.id"
                ref="video"
                :src="version.playback_url"
                :poster="version.thumbnail_url"
                :muted="muted"
                @timeupdate="currentTime = video?.currentTime ?? 0"
                playsinline
                preload="metadata"
                :aria-label="`${version.snapshot?.script?.title ?? '视频作品'}，版本 ${version.version_number}`"
                @play="
                  playing = true;
                  emit('playing', true);
                "
                @pause="
                  playing = false;
                  emit('playing', false);
                "
                @ended="
                  playing = false;
                  emit('playing', false);
                "
                @error="mediaError = true"
              />
              <div
                v-if="!playing"
                class="preview-caption"
              >
                <span>A STORY BY EXPLAINCRAFT</span>
                <strong>{{ version.snapshot?.script?.title ?? '你的创作短片' }}</strong>
                <i />
              </div>
              <button
                v-if="!playing"
                class="preview-play"
                aria-label="播放视频"
                @click="togglePlay"
              >
                <Icon
                  name="play"
                  :size="21"
                />
              </button>
            </template>
            <div
              v-else-if="mediaError"
              class="preview-empty"
            >
              <Icon
                name="info"
                :size="30"
              />
              <strong>视频暂时无法加载</strong>
              <button
                class="text-button"
                @click="mediaError = false"
              >
                重新加载
              </button>
            </div>
            <div
              v-else
              class="preview-empty"
            >
              <span class="empty-frame">
                <Icon
                  name="sparkles"
                  :size="32"
                />
              </span>
              <strong>{{ busy ? '你的故事，正在成形' : '这里，等着你的第一个故事' }}</strong>
              <p>
                {{ busy ? '画面与声音准备好，就会在这里相遇' : '描述一个想法，让灵感成为画面' }}
              </p>
              <span
                v-if="busy"
                class="preview-loading"
              >
                <i />
                <i />
                <i />
              </span>
            </div>
            <span
              v-if="busy && version"
              class="new-version-status"
            >
              <span class="spinner" />
              新版本制作中
            </span>
          </div>
        </div>
        <div class="canvas-footer-label">
          <span>{{ version ? `${version.width} × ${version.height}` : '等待生成' }}</span>
          <span>适应画布</span>
        </div>
      </div>
      <div class="player-controls">
        <button
          class="icon-button"
          aria-label="视频静音"
          :aria-pressed="muted"
          :disabled="!version"
          @click="muted = !muted"
        >
          <Icon
            :name="muted ? 'mute' : 'volume'"
            :size="16"
          />
        </button>
        <span class="player-time">
          {{ timecode(currentTime) }}
          <small>/ {{ timecode((version?.duration_ms ?? 0) / 1000) }}</small>
        </span>
        <div class="transport-buttons">
          <button
            class="icon-button"
            aria-label="回到开头"
            :disabled="!version"
            @click="seek(0)"
          >
            <Icon
              name="skipBack"
              :size="16"
            />
          </button>
          <button
            class="transport-play"
            :aria-label="playing ? '暂停视频' : '播放或暂停视频'"
            :disabled="!version"
            @click="togglePlay"
          >
            <Icon
              :name="playing ? 'pause' : 'play'"
              :size="17"
            />
          </button>
          <button
            class="icon-button"
            aria-label="跳到结尾"
            :disabled="!version"
            @click="seek(Math.max(0, (version?.duration_ms ?? 0) / 1000 - 0.1))"
          >
            <Icon
              name="skipNext"
              :size="16"
            />
          </button>
        </div>
        <input
          class="player-progress"
          type="range"
          min="0"
          :max="(version?.duration_ms ?? 30000) / 1000"
          step="0.1"
          :value="currentTime"
          :disabled="!version"
          aria-label="画布播放进度"
          @input="seek(Number(($event.target as HTMLInputElement).value))"
        />
        <button
          class="icon-button"
          aria-label="全屏播放"
          :disabled="!version"
          @click="fullscreen"
        >
          <Icon
            name="fullscreen"
            :size="16"
          />
        </button>
      </div>
      <div class="preview-meta">
        <span>
          <Icon
            name="clock"
            :size="12"
          />
          {{ version ? `${Math.round(version.duration_ms / 1000)} 秒` : '等待生成' }}
        </span>
        <span v-if="version">
          {{ version.width }} × {{ version.height }}
          <span class="inline-dot">·</span>
          MP4
        </span>
        <span v-else>让想象发生</span>
      </div>
      <div
        v-if="readOnly"
        class="draft-notice"
      >
        <Icon
          name="clock"
          :size="14"
        />
        <span>历史版本 · 设置只读</span>
        <button @click="emit('latest')">回到当前草稿</button>
      </div>
      <div
        v-else-if="dirty && version"
        class="draft-notice"
      >
        <Icon
          name="edit"
          :size="14"
        />
        <span>有修改尚未生成</span>
        <button
          :disabled="busy || pending"
          @click="emit('update')"
        >
          更新视频
        </button>
      </div>
      <section
        v-show="detailsOpen"
        class="inspector-drawer"
        aria-label="作品详情面板"
      >
        <header class="inspector-heading">
          <strong>
            {{ tabs.find((item) => item.id === tab)?.name }}
            <span>作品详情</span>
          </strong>
          <button
            class="icon-button"
            aria-label="收起作品详情"
            @click="detailsOpen = false"
          >
            <Icon
              name="close"
              :size="16"
            />
          </button>
        </header>
        <div
          :id="`panel-${tab}`"
          class="editor-content"
          role="tabpanel"
          :aria-labelledby="`tab-${tab}`"
        >
          <template v-if="tab === 'script'">
            <div class="editor-subheading">
              <strong>
                分镜脚本
                <span v-if="displayedDraft.shots.length">
                  {{ displayedDraft.shots.length }} 个镜头
                </span>
              </strong>
              <button
                v-if="displayedDraft.script && !editingScript"
                class="text-button"
                :disabled="disableForm"
                @click="editingScript = true"
              >
                <Icon
                  name="edit"
                  :size="13"
                />
                编辑
              </button>
            </div>
            <div
              v-if="editingScript && form.script"
              class="script-editor"
            >
              <label>
                作品标题
                <input
                  v-model="form.script.title"
                  maxlength="100"
                  :disabled="disableForm"
                  @input="formDirty = true"
                />
              </label>
              <label>
                解说文案
                <textarea
                  v-model="form.script.text"
                  rows="7"
                  maxlength="20000"
                  :disabled="disableForm"
                  @input="formDirty = true"
                />
              </label>
              <p>修改完整文案后，下次生成将重新规划分镜。</p>
              <button
                class="text-button"
                @click="resetForm"
              >
                放弃编辑
              </button>
            </div>
            <div
              v-else-if="displayedDraft.shots.length"
              class="shot-list"
            >
              <button
                v-for="(shot, index) in displayedDraft.shots"
                :key="shot.id"
                class="shot-card"
                :disabled="readOnly || busy"
                @click="emit('prompt', `把第${index + 1}个镜头调整为：`)"
              >
                <div class="shot-image">
                  <img
                    :src="shot.thumbnail_url || imagePath('coffee-cup')"
                    :alt="shot.visual_prompt"
                  />
                  <span>{{ String(index + 1).padStart(2, '0') }}</span>
                </div>
                <div class="shot-description">
                  <div>
                    <strong>{{ shot.visual_prompt.split('，')[0] }}</strong>
                    <span>{{ shot.duration_ms / 1000 }}s</span>
                  </div>
                  <p>{{ shot.narration || '纯画面镜头' }}</p>
                  <small>
                    {{ shot.shot_size }}
                    <span>·</span>
                    {{ shot.camera_motion }}
                  </small>
                </div>
              </button>
            </div>
            <div
              v-else-if="displayedDraft.script"
              class="plain-script"
            >
              <h3>{{ displayedDraft.script.title }}</h3>
              <p>{{ displayedDraft.script.text }}</p>
              <span>分镜将在下次生成时更新</span>
            </div>
            <div
              v-else
              class="tab-empty"
            >
              <Icon
                name="file"
                :size="28"
              />
              <strong>灵感会在这里变成脚本</strong>
              <span>生成后查看文案与每个镜头的安排</span>
            </div>
            <p
              v-if="displayedDraft.shots.length && !editingScript"
              class="editor-hint"
            >
              <Icon
                name="sparkles"
                :size="13"
              />
              想改某个镜头？直接在对话中告诉我
            </p>
          </template>
          <template v-if="tab === 'audio'">
            <div class="sound-card">
              <div class="sound-card-head">
                <Icon
                  name="mic"
                  :size="15"
                />
                <strong>配音</strong>
                <span>
                  {{
                    form.audio.narration_enabled
                      ? '由生成流程按文案逐句合成'
                      : '已关闭，字幕承担全部讲解'
                  }}
                </span>
              </div>
              <fieldset
                class="editor-form"
                :disabled="disableForm"
              >
                <div class="switch-field">
                  <span>
                    <strong>启用配音</strong>
                    <small>关闭后成片只保留背景音乐，字幕承担全部讲解。</small>
                  </span>
                  <button
                    type="button"
                    class="switch"
                    :class="{ on: form.audio.narration_enabled }"
                    role="switch"
                    :aria-checked="form.audio.narration_enabled"
                    aria-label="启用配音"
                    @click="
                      form.audio.narration_enabled = !form.audio.narration_enabled;
                      formDirty = true;
                    "
                  >
                    <span />
                  </button>
                </div>
                <template v-if="form.audio.narration_enabled">
                  <label class="field-label">
                    <span>音色</span>
                    <input
                      v-model="form.audio.voice_id"
                      list="voice-options"
                      aria-label="配音音色"
                      @change="formDirty = true"
                    />
                    <datalist id="voice-options">
                      <option
                        v-for="voice in voiceChoices"
                        :key="voice"
                        :value="voice"
                      />
                    </datalist>
                  </label>
                  <label class="range-field">
                    <span>
                      语速
                      <strong>{{ form.audio.speech_rate.toFixed(1) }}×</strong>
                    </span>
                    <input
                      v-model.number="form.audio.speech_rate"
                      type="range"
                      min="0.5"
                      max="2"
                      step="0.1"
                      @input="formDirty = true"
                    />
                    <small>
                      <span>舒缓</span>
                      <span>轻快</span>
                    </small>
                  </label>
                  <label class="range-field">
                    <span>
                      旁白音量
                      <strong>{{ Math.round(form.audio.narration_volume * 100) }}%</strong>
                    </span>
                    <input
                      v-model.number="form.audio.narration_volume"
                      type="range"
                      min="0"
                      max="1"
                      step="0.01"
                      @input="formDirty = true"
                    />
                  </label>
                </template>
              </fieldset>
            </div>
            <div class="sound-card">
              <div class="sound-card-head">
                <Icon
                  name="music"
                  :size="15"
                />
                <strong>背景音乐</strong>
                <span>曲库或素材库里的音频</span>
              </div>
              <fieldset
                class="editor-form"
                :disabled="disableForm"
              >
                <label class="field-label">
                  <span>来源</span>
                  <select
                    :value="musicValue"
                    @change="musicMode"
                  >
                    <option value="auto">自动生成（默认）</option>
                    <option value="none">不加背景音乐</option>
                    <optgroup label="曲库">
                      <option
                        v-for="track in caps.music_tracks"
                        :key="track.id"
                        :value="track.id"
                      >
                        {{ track.name }}
                      </option>
                    </optgroup>
                    <optgroup
                      v-if="audioAssets.length"
                      label="素材库"
                    >
                      <option
                        v-for="asset in audioAssets"
                        :key="asset.id"
                        :value="`upload:${asset.id}`"
                      >
                        {{ asset.file_name }}
                      </option>
                    </optgroup>
                  </select>
                </label>
                <label class="range-field">
                  <span>
                    音乐音量
                    <strong>{{ Math.round(form.audio.music_volume * 100) }}%</strong>
                  </span>
                  <input
                    v-model.number="form.audio.music_volume"
                    type="range"
                    min="0"
                    max="1"
                    step="0.01"
                    @input="formDirty = true"
                  />
                </label>
              </fieldset>
              <button
                v-if="caps.music_tracks.some((track) => track.sample_url)"
                class="audition-button"
                @click="playMusic"
              >
                <Icon
                  :name="auditionPlaying ? 'pause' : 'headphones'"
                  :size="15"
                />
                {{ auditionPlaying ? '停止试听' : '试听示例背景音乐' }}
              </button>
              <p
                v-if="!audioAssets.length"
                class="editor-hint"
              >
                想用自己上传的音乐？先在左侧素材库添加音频，再回到这里选择。
              </p>
            </div>
          </template>
          <template v-if="tab === 'settings'">
            <fieldset
              class="editor-form"
              :disabled="disableForm"
            >
              <div class="field-label">
                <span>画面比例</span>
                <div class="ratio-options">
                  <button
                    v-for="ratio in caps.aspect_ratios"
                    :key="ratio"
                    type="button"
                    :class="{ active: form.settings.aspect_ratio === ratio }"
                    :aria-pressed="form.settings.aspect_ratio === ratio"
                    @click="
                      form.settings.aspect_ratio = ratio;
                      formDirty = true;
                    "
                  >
                    <i
                      :class="
                        ratio === '9:16'
                          ? 'portrait-icon'
                          : ratio === '16:9'
                            ? 'landscape-icon'
                            : 'square-icon'
                      "
                    />
                    <span>{{ ratio }}</span>
                  </button>
                </div>
              </div>
              <label class="field-label">
                <span>目标时长</span>
                <select
                  v-model.number="form.settings.target_duration_seconds"
                  @change="formDirty = true"
                >
                  <option
                    v-for="seconds in [15, 30, 60, 120, 180, 300].filter(
                      (value) =>
                        value >= caps.duration_range_seconds.min &&
                        value <= caps.duration_range_seconds.max,
                    )"
                    :key="seconds"
                    :value="seconds"
                  >
                    {{ seconds }} 秒
                  </option>
                </select>
              </label>
              <label class="field-label">
                <span>画面风格</span>
                <select
                  v-model="form.settings.style_id"
                  @change="formDirty = true"
                >
                  <option
                    v-for="style in caps.styles"
                    :key="style.id"
                    :value="style.id"
                  >
                    {{ style.name }}
                  </option>
                </select>
              </label>
              <label class="field-label">
                <span>调色预设</span>
                <select
                  v-model="form.finishing.color_preset_id"
                  @change="formDirty = true"
                >
                  <option
                    v-for="preset in caps.color_presets"
                    :key="preset.id"
                    :value="preset.id"
                  >
                    {{ preset.name }}
                  </option>
                </select>
              </label>
              <label class="field-label">
                <span>画面特效</span>
                <select
                  v-model="form.finishing.effect_preset_id"
                  @change="formDirty = true"
                >
                  <option
                    v-for="preset in caps.effect_presets"
                    :key="preset.id"
                    :value="preset.id"
                  >
                    {{ preset.name }}
                  </option>
                </select>
              </label>
              <div class="switch-field">
                <span>
                  <strong>自动字幕</strong>
                  <small>让你的故事，更容易被听见</small>
                </span>
                <button
                  type="button"
                  class="switch"
                  :class="{ on: form.finishing.subtitles_enabled }"
                  role="switch"
                  :aria-checked="form.finishing.subtitles_enabled"
                  aria-label="自动字幕"
                  @click="
                    form.finishing.subtitles_enabled = !form.finishing.subtitles_enabled;
                    formDirty = true;
                  "
                >
                  <span />
                </button>
              </div>
            </fieldset>
            <p class="editor-hint">演示成片固定为 30 秒，支持三种比例预览。</p>
          </template>
          <div
            v-if="formDirty && !readOnly"
            class="save-settings"
          >
            <span>修改后需要重新生成</span>
            <button
              class="button primary small"
              :disabled="disableForm"
              @click="save"
            >
              {{ pending ? '保存中…' : '保存修改' }}
            </button>
          </div>
        </div>
      </section>
    </div>
    <div
      v-if="layoutEnabled"
      class="timeline-separator"
      role="separator"
      aria-label="调整时间轴高度"
      aria-orientation="horizontal"
      :aria-valuenow="timelineHeight ?? 244"
      aria-valuemin="140"
      aria-valuemax="420"
      tabindex="0"
      @pointerdown="startTimelineResize"
      @pointermove="moveTimelineResize"
      @pointerup="stopTimelineResize"
      @pointercancel="stopTimelineResize"
      @lostpointercapture="stopTimelineResize"
      @keydown="timelineKey"
    >
      <span />
    </div>
    <VideoTimeline
      :style="timelineHeight ? { height: `${timelineHeight}px` } : undefined"
      :draft="version?.snapshot ?? displayedDraft"
      :duration="(version?.duration_ms ?? 30000) / 1000"
      :current-time="currentTime"
      :playing="playing"
      :available="!!version && !mediaError"
      @seek="seek"
      @toggle="togglePlay"
    />
    <!-- 下载使用当前查看版本的文件；导出也必须绑定这个版本，不能猜测“最新版”。 -->
    <footer class="preview-footer">
      <div class="export-readiness">
        <span>
          <span :class="version ? 'ready-dot' : 'idle-dot'" />
          {{ busy ? '创作正在进行中' : version ? '作品已就绪，随时分享' : '好作品，即将发生' }}
        </span>
        <span v-if="version">v{{ version.version_number }}</span>
      </div>
      <div class="preview-actions">
        <button
          class="button secondary"
          :disabled="!version || pending"
          @click="version && emit('download', version.video_asset_id)"
        >
          <Icon
            name="download"
            :size="16"
          />
          下载预览
        </button>
        <button
          class="button primary"
          :disabled="!version || busy || pending"
          @click="emit('export')"
        >
          <Icon
            name="export"
            :size="16"
          />
          导出当前版本
        </button>
      </div>
    </footer>
  </aside>
</template>
