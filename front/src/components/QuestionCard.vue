<script setup lang="ts">
/** 当前选择或审阅卡：真实选项直接操作，修改留在当前步骤，音乐可原地上传和试听。 */
import { computed, onBeforeUnmount, ref, watch } from 'vue';
import type { Asset, Job, Draft } from '../domain/types.ts';
import { api } from '../services/api.ts';
const props = defineProps<{
  question: NonNullable<Job['question']>;
  assets: Asset[];
  pending: boolean;
  uploading: { name: string; percent: number } | null;
  draft: Draft | null;
}>();
const emit = defineEmits<{
  answer: [value: string, instruction?: string, assetId?: string];
  upload: [files: FileList | File[]];
}>();
const localShots = ref<NonNullable<Draft['shots']>>([]);
const runtime = ref<Awaited<ReturnType<typeof api.runtimeStatus>> | null>(null);
const checking = ref(false);
const runtimeError = ref('');
/** 只有选中HTML时才把浏览器和Node列为必需；本地配音也须完整就绪。 */
const requiredRuntime = computed(
  () =>
    runtime.value?.items.map((item) => ({
      ...item,
      required:
        item.required ||
        (props.draft?.settings.visual_format === 'html' &&
          ['Chrome', 'Node.js'].includes(item.name)) ||
        (props.draft?.audio.voice_source === 'breeze' && item.name === 'breezeTTS2'),
    })) ?? [],
);
const runtimeBlocked = computed(
  () =>
    checking.value ||
    !runtime.value ||
    !!runtimeError.value ||
    requiredRuntime.value.some((item) => item.required && !item.ready),
);
const instruction = ref(''),
  fileInput = ref<HTMLInputElement | null>(null),
  selected = ref(''),
  url = ref(''),
  previewError = ref('');
let request = 0;
const isMusic = computed(() => props.question.skill_id === 'music');
const localAudio = computed(() =>
  props.assets.filter(
    (a) => a.kind === 'audio' && a.source !== 'generated' && a.status === 'ready',
  ),
);
const review = computed(() => props.question.input_type === 'review');
const uploadChoice = computed(() => props.question.options.some((o) => o.value === 'upload'));
// 新问题清空临时意见和选择，防止旧卡片的输入误提交。
watch(
  () => props.question.id,
  () => {
    instruction.value = '';
    selected.value = '';
    url.value = '';
    previewError.value = '';
    request++;
    localShots.value = JSON.parse(JSON.stringify(props.draft?.shots ?? []));
    if (props.question.skill_id === 'production') void checkRuntime();
  },
  { immediate: true },
);
/** 检查真实依赖，模型本身的调用仍由后端控制。 */
async function checkRuntime() {
  checking.value = true;
  runtimeError.value = '';
  try {
    runtime.value = await api.runtimeStatus();
  } catch {
    runtimeError.value = '依赖检查未完成，请重试。';
  } finally {
    checking.value = false;
  }
}
/** 逐镜头修改在同一步骤保存并重新审阅，不隐式开始生成。 */
function saveShots() {
  emit('answer', 'save_shots', JSON.stringify(localShots.value));
}
/** 素材选择只保存引用；无素材代表使用统一几何图解。 */
function bindAsset(index: number, event: Event) {
  const id = (event.target as HTMLSelectElement).value;
  localShots.value[index]!.asset_ids = id ? [id] : [];
  localShots.value[index]!.source_in_ms = 0;
}
/** 调整镜头顺序，保持稳定ID便于后端识别复用与修改。 */
function move(index: number, delta: number) {
  const target = index + delta;
  if (target < 0 || target >= localShots.value.length) return;
  const row = localShots.value.splice(index, 1)[0]!;
  localShots.value.splice(target, 0, row);
}

/** 上传只入库，用户试听后再明确使用，不自动修改当前任务的音乐来源。 */
function chooseFile(event: Event) {
  const input = event.target as HTMLInputElement;
  if (input.files?.length) emit('upload', input.files);
  input.value = '';
}
/** 签名地址请求使用序号隔离，快速更换时迟到结果不能覆盖当前试听。 */
async function audition(asset: Asset) {
  const token = ++request;
  selected.value = asset.id;
  url.value = '';
  previewError.value = '';
  try {
    const result = await api.download(asset.id);
    if (token === request) url.value = result.download_url;
  } catch {
    if (token === request) previewError.value = '无法加载音频，请重新选择或重试。';
  }
}
/** 本地音频可从当前问题直接选定，后端仍校验归属和当前问题。 */
function useAudio() {
  if (!selected.value) return;
  emit(
    'answer',
    uploadChoice.value ? 'upload' : selected.value,
    '',
    uploadChoice.value ? selected.value : undefined,
  );
}
/** 本地文件入口打开系统选择器，其余来源直接提交明确的选项。 */
function act(value: string) {
  if (value === 'upload') {
    fileInput.value?.click();
    return;
  }
  if (value === 'save_shots') {
    saveShots();
    return;
  }
  emit('answer', value, value === 'edit' ? instruction.value : undefined);
}
onBeforeUnmount(() => {
  request++;
});
</script>
<template>
  <section class="question-card">
    <span class="question-step">
      {{ review ? '审阅与决定' : '创作选择' }}
      <template v-if="question.step">· {{ question.step }} / {{ question.total_steps }}</template>
    </span>
    <p class="question-prompt">{{ question.prompt }}</p>
    <template v-if="isMusic">
      <details>
        <summary>试听原版 Vibe 示例音乐</summary>
        <audio
          src="media/vibe-knowledge-awards/we-never.mp3"
          controls
          preload="none"
          aria-label="原版示例音乐试听"
        />
      </details>
      <input
        ref="fileInput"
        type="file"
        accept="audio/mpeg,audio/wav,audio/x-wav,audio/mp4,audio/flac,audio/ogg,.mp3,.wav,.m4a,.flac,.ogg"
        hidden
        @change="chooseFile"
      />
      <p
        v-if="uploading"
        role="status"
      >
        正在上传 {{ uploading.name }} · {{ uploading.percent }}%
      </p>
      <div
        v-if="localAudio.length"
        class="local-music-list"
      >
        <strong>可用音乐（本地上传与已生成）</strong>
        <button
          v-for="asset in localAudio"
          :key="asset.id"
          class="button secondary"
          :class="{ selected: selected === asset.id }"
          :disabled="pending"
          @click="audition(asset)"
        >
          {{ asset.file_name }} · {{ ((asset.duration_ms ?? 0) / 1000).toFixed(1) }}秒
        </button>
        <audio
          v-if="url"
          :key="url"
          :src="url"
          controls
          preload="metadata"
          aria-label="所选背景音乐试听"
          @error="previewError = '当前音频无法播放，请尝试 MP3 或 WAV。'"
        />
        <p
          v-if="previewError"
          role="alert"
        >
          {{ previewError }}
        </p>
        <button
          v-if="selected"
          class="button primary"
          :disabled="pending"
          @click="useAudio"
        >
          使用这首音乐
        </button>
      </div>
    </template>
    <details
      v-if="question.skill_id === 'storyboard'"
      class="shot-review-editor"
    >
      <summary>逐镜头编辑文字、顺序、素材和视频入点</summary>
      <article
        v-for="(shot, index) in localShots"
        :key="shot.id"
      >
        <strong>镜头 {{ index + 1 }}</strong>
        <label>
          解说与字幕
          <textarea
            v-model="shot.narration"
            :aria-label="`镜头${index + 1}解说`"
            maxlength="1000"
          />
        </label>
        <label>
          镜头时长（毫秒）
          <input
            v-model.number="shot.duration_ms"
            type="number"
            min="1000"
            max="60000"
          />
        </label>
        <label>
          画面素材
          <select
            :value="shot.asset_ids[0] ?? ''"
            @change="bindAsset(index, $event)"
          >
            <option value="">生成统一几何图解</option>
            <option
              v-for="asset in assets.filter(
                (a) => a.source === 'upload' && a.kind !== 'audio' && a.status === 'ready',
              )"
              :key="asset.id"
              :value="asset.id"
            >
              {{ asset.file_name }}
            </option>
          </select>
        </label>
        <label v-if="shot.asset_ids.length">
          原视频入点（毫秒）
          <input
            v-model.number="shot.source_in_ms"
            type="number"
            min="0"
          />
        </label>
        <div class="question-actions">
          <button
            class="button secondary"
            :disabled="index === 0 || pending"
            @click="move(index, -1)"
          >
            上移
          </button>
          <button
            class="button secondary"
            :disabled="index + 1 === localShots.length || pending"
            @click="move(index, 1)"
          >
            下移
          </button>
          <button
            class="button secondary"
            :disabled="localShots.length === 1 || pending"
            @click="localShots.splice(index, 1)"
          >
            移除镜头
          </button>
        </div>
      </article>
    </details>
    <div
      v-if="question.skill_id === 'production'"
      class="runtime-summary"
    >
      <p
        v-if="checking"
        role="status"
      >
        正在检查本机工具和模型…
      </p>
      <p
        v-for="item in requiredRuntime"
        :key="item.name"
      >
        {{ item.ready ? '✓' : '○' }} {{ item.name }}：{{ item.message
        }}{{ item.required ? '（必需）' : '（按需使用）' }}
      </p>
      <p
        v-if="runtimeError"
        role="alert"
      >
        {{ runtimeError }}
      </p>
      <button
        class="button secondary"
        :disabled="checking"
        @click="checkRuntime"
      >
        重新检查运行环境
      </button>
    </div>
    <textarea
      v-if="review && question.options.some((o) => o.value === 'edit')"
      v-model="instruction"
      aria-label="当前步骤修改意见"
      placeholder="写下需要修改的地方，例如：保留结论，换一个更直观的例子。"
      rows="3"
      maxlength="10000"
      :disabled="pending"
    />
    <div
      v-if="question.options.length"
      class="question-actions"
    >
      <button
        v-for="option in question.options"
        :key="option.value"
        type="button"
        class="button"
        :class="option.value === 'continue' ? 'primary' : 'secondary'"
        :disabled="
          pending ||
          (option.value === 'edit' && !instruction.trim()) ||
          (question.skill_id === 'production' &&
            ['continue', 'publish'].includes(option.value) &&
            runtimeBlocked)
        "
        @click="act(option.value)"
      >
        {{ option.label }}
      </button>
    </div>
    <form
      v-else
      @submit.prevent="emit('answer', instruction)"
    >
      <input
        v-model="instruction"
        aria-label="补充信息"
        placeholder="填写你的回答…"
        maxlength="5000"
        :disabled="pending"
      />
      <button
        class="button primary"
        :disabled="pending || !instruction.trim()"
      >
        提交回答
      </button>
    </form>
  </section>
</template>
<style scoped>
.question-prompt {
  white-space: pre-wrap;
  line-height: 1.7;
  max-height: 440px;
  overflow: auto;
}
.question-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin-top: 14px;
}
.local-music-list {
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 12px 0;
}
.local-music-list audio {
  width: 100%;
}
.shot-review-editor article {
  border-top: 1px solid var(--border);
  padding: 12px 0;
}
.shot-review-editor label {
  display: grid;
  gap: 6px;
  margin: 10px 0;
}
textarea {
  width: 100%;
  box-sizing: border-box;
  resize: vertical;
  padding: 12px;
  background: var(--surface);
  color: inherit;
  border: 1px solid var(--border);
  border-radius: 10px;
  font: inherit;
}
.selected {
  outline: 2px solid var(--accent);
}
</style>
