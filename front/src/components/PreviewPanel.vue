<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue';
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
  onSave: (changes: DraftChanges, revision: number) => Promise<boolean>;
}>();
const emit = defineEmits<{
  select: [id: string];
  download: [id: string];
  export: [];
  upload: [];
  update: [];
  close: [];
  latest: [];
  playing: [value: boolean];
  prompt: [text: string];
  notify: [text: string];
}>();
const tab = ref('script'),
  video = ref<HTMLVideoElement | null>(null),
  playing = ref(false),
  mediaError = ref(false);
const form = ref<Draft>(emptyDraft()),
  formRevision = ref(1),
  formDirty = ref(false),
  editingScript = ref(false),
  auditionPlaying = ref(false);
const readOnly = computed(() => !!props.version && props.version.id !== props.latestVersionId);
const displayedDraft = computed(() =>
  readOnly.value ? props.version!.snapshot : (props.draft ?? emptyDraft()),
);
const tabs = [
  { id: 'script', name: '脚本', icon: 'file' },
  { id: 'assets', name: '素材', icon: 'image' },
  { id: 'audio', name: '音频', icon: 'music' },
  { id: 'settings', name: '设置', icon: 'sliders' },
];
const disableForm = computed(
  () => readOnly.value || props.busy || props.pending || !props.projectId,
);
let audition: HTMLAudioElement | null = null;
function resetForm() {
  form.value = copy(displayedDraft.value);
  formRevision.value = props.revision;
  formDirty.value = false;
  editingScript.value = false;
}
watch(
  () => [props.projectId, props.version?.id],
  () => {
    resetForm();
    mediaError.value = false;
    playing.value = false;
    emit('playing', false);
    stopAudition();
  },
  { immediate: true },
);
watch(
  () => props.revision,
  () => {
    if (!formDirty.value) resetForm();
  },
);
watch(
  () => props.draft,
  () => {
    if (!formDirty.value) resetForm();
  },
  { deep: true },
);
function stopAudition() {
  audition?.pause();
  audition = null;
  auditionPlaying.value = false;
}
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
const musicValue = computed(() =>
  form.value.audio.music_mode === 'library'
    ? form.value.audio.music_id!
    : form.value.audio.music_mode === 'upload'
      ? `upload:${form.value.audio.music_asset_id}`
      : form.value.audio.music_mode,
);
async function save() {
  const changes: DraftChanges = {},
    original = props.draft ?? emptyDraft();
  for (const key of ['settings', 'audio', 'finishing'] as const) {
    if (JSON.stringify(form.value[key]) !== JSON.stringify(original[key])) {
      if (key === 'settings') changes.settings = copy(form.value.settings);
      if (key === 'finishing') changes.finishing = copy(form.value.finishing);
      if (key === 'audio') {
        changes.audio = copy(form.value.audio);
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
function openTab(id: string) {
  tab.value = id;
}
onBeforeUnmount(stopAudition);
defineExpose({ openTab });
</script>
<template>
  <aside
    class="preview-panel"
    :class="{ 'is-open': open }"
    aria-label="视频作品与设置"
  >
    <header class="preview-heading">
      <div>
        <Icon
          name="video"
          :size="18"
        />
        <h2>作品预览</h2>
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
    <div class="preview-content">
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
            controls
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
            <span>A STORY BY LINGJING</span>
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
          <span class="preview-demo-tag">示例作品</span>
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
          <p>{{ busy ? '画面与声音准备好，就会在这里相遇' : '描述一个想法，让灵感成为画面' }}</p>
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
      <div
        class="editor-tabs"
        role="tablist"
        aria-label="作品详情"
      >
        <button
          v-for="item in tabs"
          :id="`tab-${item.id}`"
          :key="item.id"
          :class="{ active: tab === item.id }"
          role="tab"
          :aria-selected="tab === item.id"
          :aria-controls="`panel-${item.id}`"
          @click="tab = item.id"
        >
          <Icon
            :name="item.icon"
            :size="15"
          />
          {{ item.name }}
        </button>
      </div>
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
        <template v-if="tab === 'assets'">
          <div class="editor-subheading">
            <strong>
              我的素材
              <span>{{ assets.length }} 个文件</span>
            </strong>
            <button
              class="text-button"
              :disabled="pending || busy"
              @click="emit('upload')"
            >
              <Icon
                name="plus"
                :size="14"
              />
              上传
            </button>
          </div>
          <div
            v-if="assets.length"
            class="asset-grid"
          >
            <div
              v-for="asset in assets"
              :key="asset.id"
              class="asset-card"
            >
              <img
                v-if="asset.thumbnail_url"
                :src="asset.thumbnail_url"
                :alt="asset.file_name"
              />
              <span
                v-else
                class="asset-file-icon"
              >
                <Icon
                  :name="asset.kind === 'audio' ? 'music' : 'video'"
                  :size="25"
                />
              </span>
              <strong :title="asset.file_name">{{ asset.file_name }}</strong>
              <small :class="{ 'danger-text': asset.status === 'failed' }">
                {{
                  asset.status === 'ready'
                    ? `${(asset.size_bytes / 1024 / 1024).toFixed(1)} MB · 已就绪`
                    : (asset.error?.message ?? '正在处理')
                }}
              </small>
            </div>
          </div>
          <button
            v-else
            class="upload-zone"
            :disabled="pending || busy"
            @click="emit('upload')"
          >
            <Icon
              name="image"
              :size="28"
            />
            <strong>给故事添一点自己的画面</strong>
            <span>点击上传图片、视频或音频</span>
            <small>图片 20MB · 视频 100MB · 音频 50MB</small>
          </button>
          <template v-if="displayedDraft.shots.length">
            <div class="editor-subheading asset-heading">
              <strong>作品画面</strong>
              <span>演示素材</span>
            </div>
            <div class="generated-assets">
              <img
                v-for="shot in displayedDraft.shots"
                :key="shot.id"
                :src="shot.thumbnail_url || imagePath('coffee-cup')"
                :alt="shot.visual_prompt"
              />
            </div>
            <p class="editor-hint">示例预览使用内置画面；上传文件会保存在本机。</p>
          </template>
        </template>
        <template v-if="tab === 'audio'">
          <fieldset
            class="editor-form"
            :disabled="disableForm"
          >
            <label class="field-label">
              <span>
                <Icon
                  name="mic"
                  :size="15"
                />
                配音音色
              </span>
              <select
                v-model="form.audio.voice_id"
                @change="formDirty = true"
              >
                <option
                  v-for="voice in caps.voices"
                  :key="voice.id"
                  :value="voice.id"
                >
                  {{ voice.name }}
                </option>
              </select>
            </label>
            <label class="range-field">
              <span>
                说话速度
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
            <div class="form-divider" />
            <label class="field-label">
              <span>
                <Icon
                  name="music"
                  :size="15"
                />
                背景音乐
              </span>
              <select
                :value="musicValue"
                @change="musicMode"
              >
                <option value="auto">由灵镜自动匹配</option>
                <option value="none">无背景音乐</option>
                <option
                  v-for="track in caps.music_tracks"
                  :key="track.id"
                  :value="track.id"
                >
                  {{ track.name }}
                </option>
                <option
                  v-for="asset in assets.filter(
                    (item) => item.kind === 'audio' && item.status === 'ready',
                  )"
                  :key="asset.id"
                  :value="`upload:${asset.id}`"
                >
                  我的素材 · {{ asset.file_name }}
                </option>
              </select>
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
          <p class="editor-hint">演示视频使用内置音乐，配音设置会保存到草稿。</p>
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
                  v-for="seconds in [15, 30, 60].filter(
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
    </div>
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
          导出视频
        </button>
      </div>
    </footer>
  </aside>
</template>
