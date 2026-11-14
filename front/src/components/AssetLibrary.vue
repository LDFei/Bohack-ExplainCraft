<script setup lang="ts">
/**
 * 素材库模块：当前项目的上传素材集中管理。分组按图片、视频、音频切换，
 * 音频可试听并设为配乐；这里不修改镜头顺序，也不读取生成画面的临时地址。
 */
import { computed, ref } from 'vue';
import type { Asset } from '../domain/types.ts';
import Icon from './Icon.vue';

const props = defineProps<{
  assets: Asset[];
  busy: boolean;
  projectTitle: string;
  musicAssetId?: string | null;
}>();

const emit = defineEmits<{
  upload: [files: FileList | File[]];
  music: [assetId: string];
}>();

type Filter = 'all' | Asset['kind'];
const filter = ref<Filter>('all');
const query = ref('');
const draggedFile = ref(false);
const fileInput = ref<HTMLInputElement | null>(null);

const uploadAssets = computed(() => props.assets.filter((asset) => asset.source === 'upload'));
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

const audition = ref<HTMLAudioElement | null>(null),
  auditionId = ref('');

/** 音频目前只有图片素材带签名地址，因此试听依赖后端提供的可选地址。 */
function auditionUrl(asset: Asset) {
  return asset.kind === 'audio' ? asset.thumbnail_url : null;
}

function auditionAsset(asset: Asset) {
  if (auditionId.value === asset.id) {
    audition.value?.pause();
    audition.value = null;
    auditionId.value = '';
    return;
  }
  audition.value?.pause();
  const url = auditionUrl(asset);
  if (!url) {
    auditionId.value = '';
    audition.value = null;
    return;
  }
  const player = new Audio(url);
  player.onended = player.onerror = () => {
    if (audition.value === player) {
      audition.value = null;
      auditionId.value = '';
    }
  };
  audition.value = player;
  auditionId.value = asset.id;
  void player.play().catch(() => {
    audition.value = null;
    auditionId.value = '';
  });
}

function assetState(asset: Asset) {
  if (asset.status === 'failed') return '处理失败';
  return asset.status === 'processing' ? '处理中' : '';
}

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
            <span class="asset-tile-fallback video">
              <Icon
                name="video"
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
              :disabled="asset.status !== 'ready' || !auditionUrl(asset)"
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
</template>
