<script setup lang="ts">
/** 可定位的成片时间轴。轨道依据版本快照展示，波形仅为示意，不提供虚假的逐轨剪辑。 */
import { computed, ref } from 'vue';
import type { Draft } from '../domain/types.ts';
import { imagePath } from '../domain/content.ts';
import Icon from './Icon.vue';
const props = defineProps<{
  draft: Draft;
  duration: number;
  currentTime: number;
  playing: boolean;
  available: boolean;
}>();
const emit = defineEmits<{ seek: [seconds: number]; toggle: [] }>();
const zoom = ref(1),
  scroller = ref<HTMLElement | null>(null);
const total = computed(() => props.duration || 30);
const ticks = computed(() => Array.from({ length: 7 }, (_, index) => (index * total.value) / 6));
const shots = computed(() => {
  const sum = props.draft.shots.reduce((value, shot) => value + shot.duration_ms, 0) || 1;
  return props.draft.shots.map((shot, index) => ({
    ...shot,
    percent: (shot.duration_ms / sum) * 100,
    start:
      (props.draft.shots.slice(0, index).reduce((value, item) => value + item.duration_ms, 0) /
        sum) *
      total.value,
  }));
});
/** 格式化界面时间码；与底层毫秒时长及实际帧率无关。 */
function timecode(seconds: number) {
  return `${String(Math.floor(seconds / 60)).padStart(2, '0')}:${String(Math.floor(seconds % 60)).padStart(2, '0')}`;
}
/** 用点击在轨道内的实际位置定位，滚动和缩放后仍按完整序列宽度换算。 */
function seekAt(event: MouseEvent) {
  if (!props.available) return;
  const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
  emit(
    'seek',
    Math.min(total.value, Math.max(0, ((event.clientX - rect.left) / rect.width) * total.value)),
  );
}
</script>
<template>
  <section
    class="timeline"
    aria-label="视频时间轴"
  >
    <header class="timeline-toolbar">
      <span class="timeline-title">
        <Icon
          name="layers"
          :size="15"
        />
        时间轴
        <span>主序列</span>
      </span>
      <span class="timeline-readonly">
        <Icon
          name="lock"
          :size="11"
        />
        成片轨道预览
      </span>
      <div class="timeline-zoom">
        <Icon
          name="zoomIn"
          :size="13"
        />
        <input
          v-model.number="zoom"
          type="range"
          min="1"
          max="3"
          step="0.5"
          aria-label="时间轴缩放"
        />
        <span>{{ Math.round(zoom * 100) }}%</span>
      </div>
    </header>
    <div class="timeline-body">
      <div class="track-labels">
        <span class="timeline-time">
          {{ timecode(currentTime) }}
          <small>/ {{ timecode(total) }}</small>
        </span>
        <span>
          <Icon
            name="video"
            :size="14"
          />
          <strong>视频</strong>
          <small>V1</small>
        </span>
        <span>
          <Icon
            name="music"
            :size="14"
          />
          <strong>音频</strong>
          <small>A1</small>
        </span>
        <span>
          <Icon
            name="text"
            :size="14"
          />
          <strong>字幕</strong>
          <small>T1</small>
        </span>
      </div>
      <div
        ref="scroller"
        class="timeline-scroll"
      >
        <div
          class="timeline-sequence"
          :style="{ width: `${zoom * 100}%` }"
        >
          <div
            class="time-ruler"
            @click="seekAt"
          >
            <span
              v-for="(tick, index) in ticks"
              :key="index"
              :style="{ left: `${(index / 6) * 100}%` }"
            >
              {{ timecode(tick) }}
            </span>
          </div>
          <div
            class="video-track track"
            @click="seekAt"
          >
            <template v-if="shots.length">
              <button
                v-for="(shot, index) in shots"
                :key="shot.id"
                class="timeline-clip"
                :style="{ width: `${shot.percent}%` }"
                :aria-label="`跳转到镜头 ${index + 1}`"
                :disabled="!available"
                @click.stop="emit('seek', shot.start)"
              >
                <div>
                  <img
                    v-for="repeat in 3"
                    :key="repeat"
                    :src="shot.thumbnail_url || imagePath('coffee-cup')"
                    alt=""
                  />
                </div>
                <span>
                  <b>{{ String(index + 1).padStart(2, '0') }}</b>
                  {{ shot.visual_prompt.split('，')[0] }}
                </span>
              </button>
            </template>
            <span
              v-else
              class="empty-track"
            >
              生成视频后，在这里浏览你的镜头序列
            </span>
          </div>
          <div
            class="audio-track track"
            @click="seekAt"
          >
            <div
              v-if="available"
              class="audio-clip"
            >
              <span>
                <Icon
                  name="music"
                  :size="11"
                />
                示例配乐 · 波形示意
              </span>
              <div
                class="waveform"
                aria-hidden="true"
              >
                <i
                  v-for="bar in 140"
                  :key="bar"
                  :style="{ height: `${4 + ((bar * 17 + Math.floor(bar / 5) * 11) % 23)}px` }"
                />
              </div>
            </div>
            <span
              v-else
              class="empty-track"
            >
              等待音轨
            </span>
          </div>
          <div
            class="subtitle-track track"
            @click="seekAt"
          >
            <template v-if="available && draft.finishing.subtitles_enabled">
              <span
                v-for="shot in shots"
                :key="shot.id"
                class="subtitle-clip"
                :style="{ width: `${shot.percent}%` }"
              >
                {{ shot.narration || '纯画面' }}
              </span>
            </template>
            <span
              v-else
              class="empty-track"
            >
              {{ available ? '此版本未开启字幕' : '等待字幕' }}
            </span>
          </div>
          <div
            v-if="available"
            class="playhead"
            :style="{ left: `${Math.min(100, (currentTime / total) * 100)}%` }"
          >
            <span />
            <i />
          </div>
          <input
            class="sr-only"
            type="range"
            min="0"
            :max="total"
            :value="currentTime"
            :disabled="!available"
            step="0.1"
            aria-label="视频播放位置"
            @input="emit('seek', Number(($event.target as HTMLInputElement).value))"
          />
        </div>
      </div>
    </div>
    <footer class="timeline-footer">
      <span>
        <span class="ready-dot" />
        {{ available ? '所有媒体已就绪' : '等待你的第一个镜头' }}
      </span>
      <span>
        点击轨道定位 · 拖动上方进度条浏览
        <span class="timeline-fps">30 FPS</span>
      </span>
    </footer>
  </section>
</template>
