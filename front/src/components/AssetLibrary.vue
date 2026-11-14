<script setup lang="ts">
/**
 * 素材库模块：当前项目的上传素材与最终成片集中管理。分组按图片、视频、音频切换，
 * 音频可试听并设为配乐；这里不修改镜头顺序，也不读取生成画面的临时地址。
 */
import { computed, onUnmounted, ref } from 'vue';
import type { Asset, VideoVersion } from '../domain/types.ts';
import { api } from '../services/api.ts';
import Icon from './Icon.vue';
import Modal from './Modal.vue';
import { useSignedPlayback } from '../composables/useSignedPlayback.ts';

const props = defineProps<{
  assets: Asset[];
  versions: VideoVersion[];
  busy: boolean;
  projectTitle: string;
  musicAssetId?: string | null;
}>();

const emit = defineEmits<{
  upload: [files: FileList | File[]];
  music: [assetId: string];
  view: [versionId: string];
  notify: [message: string];
}>();

type Filter = 'all' | Asset['kind'];
const filter = ref<Filter>('all');
const query = ref('');
const draggedFile = ref(false);
const fileInput = ref<HTMLInputElement | null>(null);

/** 隐藏 SVG、配音等中间文件，保留上传素材和可以回看的最终成片。 */
const uploadAssets = computed(() =>
  props.assets.filter((asset) =>
    ['upload', 'rendered', 'music_generated', 'music_procedural', 'library'].includes(asset.source),
  ),
);
const counts = computed(() => ({
  image: uploadAssets.value.filter((asset) => asset.kind === 'image').length,
  video: uploadAssets.value.filter((asset) => asset.kind === 'video').length,
  audio: uploadAssets.value.filter((asset) => asset.kind === 'audio').length,
}));
const tabs = computed(() => [
  { id: 'all' as const, label: '全部', count: uploadAssets.value.length },
  { id: 'image' as const, label: '图片', count: counts.value.image },
  { id: 'video' as const, label: '视频', count: counts.value.video },
  { id: 'audio' as const, label: '音频', count: counts.value.audio },
]);

const keyword = computed(() => query.value.trim().toLowerCase());
const visible = computed(() =>
  uploadAssets.value.filter(
    (asset) =>
      (filter.value === 'all' || asset.kind === filter.value) &&
      (!keyword.value || asset.file_name.toLowerCase().includes(keyword.value)),
  ),
);
const images = computed(() => visible.value.filter((asset) => asset.kind === 'image'));
const videos = computed(() => visible.value.filter((asset) => asset.kind === 'video'));
const audio = computed(() => visible.value.filter((asset) => asset.kind === 'audio'));
const grouped = computed(() => filter.value === 'all');

/** 最终视频通过版本关联打开，不把临时文件路径当作播放地址。 */
function assetVersion(asset: Asset) {
  return props.versions.find((version) => version.video_asset_id === asset.id);
}

const videoAsset = ref<Asset | null>(null),
  videoInitialUrl = ref(''),
  videoExpires = ref(''),
  videoError = ref(''),
  videoLoading = ref(false),
  videoPlayer = ref<HTMLVideoElement | null>(null);
let videoRequest = 0;
const { url: videoUrl } = useSignedPlayback(
  videoPlayer,
  () => videoAsset.value?.id,
  () => videoInitialUrl.value,
  () => videoExpires.value,
);

/** 关闭时停止声音并作废迟到的签名地址，切换素材不会串播。 */
function closeVideo() {
  videoRequest += 1;
  videoPlayer.value?.pause();
  videoAsset.value = null;
  videoInitialUrl.value = '';
  videoExpires.value = '';
  videoError.value = '';
  videoLoading.value = false;
}

/** 成片仍打开历史版本；上传视频用所属素材的签名地址独立播放。 */
async function openVideo(asset: Asset) {
  const version = assetVersion(asset);
  if (version) {
    emit('view', version.id);
    return;
  }
  if (asset.status !== 'ready') return;
  closeVideo();
  stopAudition();
  const request = videoRequest;
  videoAsset.value = asset;
  videoLoading.value = true;
  try {
    const result = await api.preview(asset.id);
    if (request !== videoRequest) return;
    videoInitialUrl.value = result.playback_url;
    videoExpires.value = result.expires_at;
  } catch {
    if (request === videoRequest) videoError.value = '无法获取视频地址，请关闭后重新打开。';
  } finally {
    if (request === videoRequest) videoLoading.value = false;
  }
}

/** 文件通过解析仍可能采用当前系统无法播放的编码，明确反馈而不是空白。 */
function videoPlaybackError() {
  videoError.value =
    '当前视频无法播放，可能是编码不受系统支持或地址已过期。请重新打开；仍失败可将视频转为 H.264 / AAC 的 MP4。';
}
onUnmounted(closeVideo);

/** 拖入文件时只接收文件类型，保持文本拖放的默认行为。 */
function dragOver(event: DragEvent) {
  if (!event.dataTransfer?.types.includes('Files')) return;
  event.preventDefault();
  draggedFile.value = true;
}

/** 在面板内部移动不清除拖放高亮。 */
function dragLeave(event: DragEvent) {
  if ((event.currentTarget as HTMLElement).contains(event.relatedTarget as Node | null)) return;
  draggedFile.value = false;
}

/** 文件交给现有上传流程，组件不直接写入项目数据。 */
function dropFiles(event: DragEvent) {
  if (!event.dataTransfer?.files.length) return;
  event.preventDefault();
  draggedFile.value = false;
  emit('upload', event.dataTransfer.files);
}

/** 任务空闲时打开系统文件选择器。 */
function pickFiles() {
  if (!props.busy) fileInput.value?.click();
}

/** 上传后清空 input，允许再次选择同名文件。 */
function onFilesChosen(event: Event) {
  const input = event.target as HTMLInputElement;
  if (input.files?.length) emit('upload', input.files);
  input.value = '';
}

const audition = ref<HTMLAudioElement | null>(null),
  auditionId = ref('');
let auditionRequest = 0;

/** 停止播放器并使仍在等待的签名地址失效，防止离开页面后突然发声。 */
function stopAudition() {
  auditionRequest += 1;
  audition.value?.pause();
  audition.value = null;
  auditionId.value = '';
}

/** 音频通过下载接口获取签名地址；请求序号保证快速切换时只播放最后一次选择。 */
async function auditionAsset(asset: Asset) {
  if (auditionId.value === asset.id) {
    stopAudition();
    return;
  }
  stopAudition();
  const request = auditionRequest;
  auditionId.value = asset.id;
  try {
    const { download_url: url } = await api.download(asset.id);
    if (request !== auditionRequest) return;
    const player = new Audio(url);
    player.onended = player.onerror = () => {
      if (audition.value === player) stopAudition();
    };
    audition.value = player;
    await player.play();
  } catch {
    if (request === auditionRequest) {
      stopAudition();
      emit('notify', '素材暂时无法试听，请重试。');
    }
  }
}

onUnmounted(stopAudition);

/** 把后端处理状态转换为卡片角标。 */
function assetState(asset: Asset) {
  if (asset.status === 'failed') return '处理失败';
  return asset.status === 'processing' ? '处理中' : '';
}

/** 用文件大小和可选时长补充素材信息。 */
function assetMeta(asset: Asset) {
  const parts = [`${(asset.size_bytes / 1024 / 1024).toFixed(1)} MB`];
  if (asset.duration_ms) parts.push(`${Math.round(asset.duration_ms / 1000)} 秒`);
  return parts.join(' · ');
}
</script>

<template>
  <section
    class="asset-page"
    :class="{ 'is-dragging': draggedFile }"
    aria-label="素材库"
    @dragover="dragOver"
    @dragleave="dragLeave"
    @drop="dropFiles"
  >
    <header class="asset-page-head">
      <div>
        <h1>素材库</h1>
        <p>{{ projectTitle }} · {{ uploadAssets.length }} 个素材</p>
      </div>
      <button
        class="button primary"
        :disabled="busy"
        @click="pickFiles"
      >
        <Icon
          name="upload"
          :size="15"
        />
        上传素材
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
    </header>

    <div class="asset-page-toolbar">
      <div
        class="asset-filter"
        role="tablist"
        aria-label="素材类型"
      >
        <button
          v-for="tab in tabs"
          :key="tab.id"
          role="tab"
          :class="{ active: filter === tab.id }"
          :aria-selected="filter === tab.id"
          @click="filter = tab.id"
        >
          {{ tab.label }}
          <span>{{ tab.count }}</span>
        </button>
      </div>
      <label class="asset-search">
        <Icon
          name="search"
          :size="15"
        />
        <input
          v-model="query"
          aria-label="搜索素材"
          placeholder="搜索素材…"
        />
      </label>
    </div>

    <div class="asset-page-body">
      <section
        v-if="(grouped || filter === 'image') && images.length"
        class="asset-block"
      >
        <h2>
          图片
          <small>{{ images.length }}</small>
        </h2>
        <div class="asset-page-grid">
          <figure
            v-for="asset in images"
            :key="asset.id"
            class="asset-tile"
            :title="asset.file_name"
          >
            <img
              v-if="asset.thumbnail_url"
              :src="asset.thumbnail_url"
              :alt="asset.file_name"
              loading="lazy"
            />
            <span
              v-else
              class="asset-tile-fallback"
            >
              <Icon
                name="image"
                :size="24"
              />
            </span>
            <span
              v-if="assetState(asset)"
              class="asset-badge"
              :class="{ danger: asset.status === 'failed' }"
            >
              {{ assetState(asset) }}
            </span>
            <figcaption>{{ asset.file_name }}</figcaption>
          </figure>
        </div>
      </section>

      <section
        v-if="(grouped || filter === 'video') && videos.length"
        class="asset-block"
      >
        <h2>
          视频
          <small>{{ videos.length }}</small>
        </h2>
        <div class="asset-page-grid">
          <figure
            v-for="asset in videos"
            :key="asset.id"
            class="asset-tile"
            :title="asset.file_name"
          >
            <button
              class="asset-video-preview"
              :disabled="asset.status !== 'ready'"
              :aria-label="`预览 ${asset.file_name}`"
              @click="openVideo(asset)"
            >
              <img
                v-if="asset.thumbnail_url || assetVersion(asset)?.thumbnail_url"
                :src="asset.thumbnail_url || assetVersion(asset)?.thumbnail_url"
                :alt="asset.file_name"
              />
              <span
                v-else
                class="asset-tile-fallback video"
              >
                <Icon
                  name="video"
                  :size="24"
                />
              </span>
            </button>
            <span
              v-if="assetState(asset)"
              class="asset-badge"
              :class="{ danger: asset.status === 'failed' }"
            >
              {{ assetState(asset) }}
            </span>
            <figcaption>
              {{
                assetVersion(asset) ? `生成成片 · v${assetVersion(asset)!.version_number} · ` : ''
              }}
              {{ asset.file_name }}
            </figcaption>
          </figure>
        </div>
      </section>

      <section
        v-if="(grouped || filter === 'audio') && audio.length"
        class="asset-block"
      >
        <h2>
          音频
          <small>{{ audio.length }}</small>
        </h2>
        <ul class="audio-list asset-page-audio">
          <li
            v-for="asset in audio"
            :key="asset.id"
            class="audio-row"
            :class="{ selected: musicAssetId === asset.id, pending: asset.status !== 'ready' }"
          >
            <button
              class="audio-play"
              :aria-label="auditionId === asset.id ? '停止试听' : `试听 ${asset.file_name}`"
              :disabled="asset.status !== 'ready'"
              @click="auditionAsset(asset)"
            >
              <Icon
                :name="auditionId === asset.id ? 'pause' : 'play'"
                :size="13"
              />
            </button>
            <span class="audio-info">
              <strong>{{ asset.file_name }}</strong>
              <small>{{ assetState(asset) || assetMeta(asset) }}</small>
            </span>
            <button
              class="audio-use"
              :class="{ active: musicAssetId === asset.id }"
              :disabled="asset.status !== 'ready'"
              @click="emit('music', asset.id)"
            >
              {{ musicAssetId === asset.id ? '已用作配乐' : '设为配乐' }}
            </button>
          </li>
        </ul>
      </section>

      <div
        v-if="!visible.length"
        class="asset-page-empty"
      >
        <Icon
          name="folder"
          :size="32"
        />
        <strong>{{ query ? '没有匹配的素材' : '素材库还是空的' }}</strong>
        <p>{{ query ? '换个关键词或切换类型试试' : '上传图片、视频或音频，生成时会用到它们' }}</p>
        <button
          v-if="!query"
          class="button primary small"
          :disabled="busy"
          @click="pickFiles"
        >
          上传第一个素材
        </button>
      </div>
    </div>
  </section>
  <Modal
    :open="!!videoAsset"
    :title="videoAsset?.file_name ?? '视频预览'"
    wide
    @close="closeVideo"
  >
    <p
      v-if="videoLoading"
      role="status"
    >
      正在加载视频…
    </p>
    <p
      v-if="videoError"
      role="alert"
    >
      {{ videoError }}
    </p>
    <video
      v-if="videoUrl"
      ref="videoPlayer"
      :src="videoUrl"
      controls
      playsinline
      preload="metadata"
      class="uploaded-video-player"
      aria-label="上传视频播放器"
      @error="videoPlaybackError"
    />
  </Modal>
</template>
