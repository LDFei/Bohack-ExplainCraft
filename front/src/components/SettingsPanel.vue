<script setup lang="ts">
/** 通用偏好与真实模型供应商设置；供应商通过 Tauri command 保存并热应用。 */
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { useAppSettings } from '../composables/useAppSettings.ts';
import type { GeneralSettings } from '../composables/useAppSettings.ts';
import { themeOptions, useTheme } from '../composables/useTheme.ts';
import type { ThemeId } from '../composables/useTheme.ts';
import {
  BREEZE_MODEL_BYTES,
  breezeModelStatus,
  cancelBreezeDownload,
  downloadBreezeModel,
  formatBytes,
  onBreezeModelProgress,
} from '../services/breeze.ts';
import type { BreezeModelProgress, BreezeModelStatus } from '../services/breeze.ts';
import {
  defaultCreationModel,
  getModelSettings,
  listVoices,
  listModels,
  providerPreset,
  updateModelSettings,
} from '../services/settings.ts';
import type { ModelSettings, ProviderSetting } from '../services/settings.ts';
import Icon from './Icon.vue';
import Modal from './Modal.vue';

const props = defineProps<{ open: boolean }>();
const emit = defineEmits<{ close: []; saved: [persisted: boolean]; resetLayout: [] }>();
const { general, saveSettings } = useAppSettings();
const { selectedTheme, setTheme, persistenceError } = useTheme();
const tab = ref<'general' | 'apis'>('general'),
  error = ref(''),
  resetDone = ref(false),
  loading = ref(false),
  saving = ref(false);
const modelOptions = ref<Record<string, string[]>>({
    script: [],
    visual: [],
    music: [],
    voice: [],
  }),
  modelStatus = ref<Record<string, string>>({ script: '', visual: '', music: '', voice: '' }),
  modelLoading = ref<Record<string, boolean>>({
    script: false,
    visual: false,
    music: false,
    voice: false,
  });

/** 返回服务端候选与用户手动输入的并集，确保自定义模型不会消失。 */
function modelChoices(kind: keyof ModelSettings) {
  const current = providerForm.value[kind].model;
  return [...new Set([...(modelOptions.value[kind] ?? []), ...(current ? [current] : [])])];
}

const voiceOptions = ref<string[]>([]),
  voiceStatus = ref(''),
  voiceLoading = ref(false);
// 本地 breezeTTS2 模型：只在用户选择该供应商时查询，下载必须经用户确认。
const breezeModel = ref<BreezeModelStatus | null>(null),
  breezeConfirmOpen = ref(false),
  breezeDir = ref(''),
  breezeProgress = ref<BreezeModelProgress | null>(null),
  breezeError = ref('');
const breezeDownloading = computed(() => breezeProgress.value?.phase === 'downloading');

// 打开设置时若当前配音就是本地 breezeTTS2，立即显示模型是否已下载，但不自动下载。
watch(
  () => props.open && providerForm.value.voice.provider === 'breeze',
  (active) => {
    if (active) void refreshBreezeModel();
  },
);
const voiceChoices = computed(() => {
  const current = providerForm.value.voice.voiceId;
  return [...new Set([...voiceOptions.value, ...(current ? [current] : [])])];
});
const form = ref<GeneralSettings>({ rememberLayout: true, sendShortcut: 'enter' });
const empty = (provider: string): ProviderSetting => ({
  provider,
  baseUrl: '',
  model: '',
  apiKey: null,
  hasApiKey: false,
  voiceId: provider === 'qwen' ? 'Cherry' : '',
  overridden: false,
});
const providerForm = ref<ModelSettings>({
  script: empty('siliconflow'),
  visual: { ...empty('siliconflow'), model: defaultCreationModel },
  music: empty('procedural'),
  voice: empty('qwen'),
});
/** 画面模型跟随统一模型时，页面展示统一模型的当前值。 */
const followedModel = computed(() => providerForm.value.script.model || defaultCreationModel);
const theme = ref<ThemeId>('silver');

// 打开时重新读取后端配置；密钥只返回是否存在，输入新密钥才会覆盖旧值。
watch(
  () => props.open,
  async (open) => {
    if (!open) return;
    form.value = { ...general.value };
    theme.value = selectedTheme.value;
    error.value = '';
    resetDone.value = false;
    tab.value = 'general';
    loading.value = true;
    try {
      providerForm.value = await getModelSettings();
      void loadVoices();
      void refreshBreezeModel();
    } catch (reason) {
      error.value = reason instanceof Error ? reason.message : String(reason);
    } finally {
      loading.value = false;
    }
  },
  { immediate: true },
);

/** 切换供应商时更新对应 base URL 和模型预设。 */
function applyPreset(kind: keyof ModelSettings) {
  const current = providerForm.value[kind];
  Object.assign(current, providerPreset(current.provider, kind));
  if (kind === 'script' || kind === 'visual') {
    modelOptions.value[kind] = [];
    modelStatus.value[kind] = '';
  }
  if (kind === 'voice') {
    voiceOptions.value = [];
    voiceStatus.value = '';
    void refreshBreezeModel();
  }
}

/** 查询本地模型状态；没有安装时先征询用户，绝不自动下载。 */
async function refreshBreezeModel() {
  breezeError.value = '';
  if (providerForm.value.voice.provider !== 'breeze') {
    breezeModel.value = null;
    return;
  }
  try {
    breezeModel.value = await breezeModelStatus();
  } catch (reason) {
    breezeError.value = reason instanceof Error ? reason.message : String(reason);
  }
}

/** 打开下载确认对话框，带上探测到的 ComfyUI 目录；下载中打开时保留当前进度。 */
async function askBreezeDownload() {
  await refreshBreezeModel();
  breezeDir.value = breezeModel.value?.comfyuiDir || breezeDir.value;
  if (!breezeDownloading.value) breezeProgress.value = null;
  breezeError.value = '';
  breezeConfirmOpen.value = true;
}

let stopBreezeProgress: (() => void) | null = null;

// 常驻订阅下载进度：下载在后台继续时重新打开设置也能看到当前状态。
onMounted(async () => {
  stopBreezeProgress = await onBreezeModelProgress((progress) => {
    breezeProgress.value = progress;
    if (progress.phase === 'done') void refreshBreezeModel();
  });
});

/** 用户确认后才开始下载；进度由后端事件推送。 */
async function startBreezeDownload() {
  if (breezeDownloading.value) return;
  breezeError.value = '';
  // 先切到下载态，避免连点两次触发第二次请求（后端也会拒绝并发下载）。
  breezeProgress.value = {
    phase: 'downloading',
    file: '',
    fileIndex: 0,
    fileCount: 0,
    received: 0,
    total: breezeModel.value?.totalBytes ?? BREEZE_MODEL_BYTES,
    percent: 0,
  };
  try {
    breezeModel.value = await downloadBreezeModel(breezeDir.value);
    breezeConfirmOpen.value = false;
    breezeProgress.value = null;
  } catch (reason) {
    const message = reason instanceof Error ? reason.message : String(reason);
    breezeProgress.value = null;
    // 用户主动取消不算错误，已下载的部分会保留以便续传。
    breezeError.value = message.includes('取消') ? '已暂停下载，再次下载会从断点继续。' : message;
  }
}

/** 中止下载；已下载部分保留。命令一返回就结束下载态，不必等下一次事件。 */
async function cancelBreezeDownloading() {
  try {
    await cancelBreezeDownload();
    breezeProgress.value = null;
    breezeError.value = '已暂停下载，再次下载会从断点继续。';
  } catch (reason) {
    breezeError.value = reason instanceof Error ? reason.message : String(reason);
  }
}

// 离开页面只解除监听；下载在后台继续，重开设置会显示实时进度。
onBeforeUnmount(() => {
  stopBreezeProgress?.();
  stopBreezeProgress = null;
});

/**
 * 画面模型默认跟随统一模型；开启独立配置时用当前统一模型作为起点，
 * 用户再改成专用视觉模型即可。关闭时不改动统一模型配置。
 */
function toggleVisualOverride(enabled: boolean) {
  const visual = providerForm.value.visual;
  visual.overridden = enabled;
  if (!enabled) return;
  if (!visual.provider) visual.provider = providerForm.value.script.provider;
  if (!visual.baseUrl) visual.baseUrl = providerForm.value.script.baseUrl;
  if (!visual.model) visual.model = followedModel.value;
  if (!visual.hasApiKey) visual.hasApiKey = providerForm.value.script.hasApiKey;
}

/** 从当前 TTS 供应商获取声线，保留用户已填写的自定义 voice ID。 */
async function loadVoices() {
  const current = providerForm.value.voice;
  if (!current.apiKey && !current.hasApiKey && !['indextts', 'breeze'].includes(current.provider)) {
    voiceStatus.value = '请先填写 API Key。';
    return;
  }
  voiceLoading.value = true;
  voiceStatus.value = '正在获取声线…';
  try {
    voiceOptions.value = await listVoices(current);
    if (!current.voiceId && voiceOptions.value.length) {
      current.voiceId = voiceOptions.value[0]!;
    }
    voiceStatus.value = `已获取 ${voiceOptions.value.length} 个声线，可选择或手动输入。`;
  } catch (reason) {
    voiceStatus.value = reason instanceof Error ? reason.message : String(reason);
  } finally {
    voiceLoading.value = false;
  }
}

/** 填写密钥后自动获取模型列表；手动输入的模型名也会保留为候选。 */
async function loadModels(kind: keyof ModelSettings = 'script') {
  const current = providerForm.value[kind];
  const localWithoutKey =
    (kind === 'voice' && ['indextts', 'breeze'].includes(current.provider)) ||
    (kind === 'music' && current.provider === 'procedural');
  if (!current.apiKey && !current.hasApiKey && !localWithoutKey) {
    modelStatus.value[kind] = '请先填写 API Key。';
    return;
  }
  modelLoading.value[kind] = true;
  modelStatus.value[kind] = '正在获取模型列表…';
  try {
    modelOptions.value[kind] = await listModels(current, kind);
    if (!current.model && modelOptions.value[kind]!.length) {
      current.model = modelOptions.value[kind]![0]!;
    }
    modelStatus.value[kind] =
      `已获取 ${modelOptions.value[kind]!.length} 个模型，可选择或手动输入。`;
  } catch (reason) {
    modelStatus.value[kind] = reason instanceof Error ? reason.message : String(reason);
  } finally {
    modelLoading.value[kind] = false;
  }
}

/** 恢复布局是即时动作，不保存其他弹窗字段。 */
function restoreLayout() {
  emit('resetLayout');
  resetDone.value = true;
}

/** 同时保存通用偏好和真实供应商；失败时保留弹窗供用户修正。 */
async function save() {
  saving.value = true;
  error.value = '';
  try {
    const result = saveSettings(form.value);
    // 画面跟随统一模型时提交前清空覆盖字段，后端据此解除独立配置。
    if (!providerForm.value.visual.overridden) {
      providerForm.value.visual = { ...providerForm.value.script, overridden: false };
    }
    await updateModelSettings(providerForm.value);
    setTheme(theme.value);
    emit('saved', result.persisted && !persistenceError.value);
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : String(reason);
    tab.value = 'apis';
  } finally {
    saving.value = false;
  }
}
</script>
<template>
  <Modal
    :open="open"
    title="工作台设置"
    subtitle="让创作空间适合你的工作方式。"
    wide
    @close="emit('close')"
  >
    <div
      class="settings-tabs"
      role="tablist"
      aria-label="设置分类"
    >
      <button
        role="tab"
        :aria-selected="tab === 'general'"
        :class="{ active: tab === 'general' }"
        @click="tab = 'general'"
      >
        <Icon
          name="settings"
          :size="15"
        />
        通用
      </button>
      <button
        role="tab"
        :aria-selected="tab === 'apis'"
        :class="{ active: tab === 'apis' }"
        @click="tab = 'apis'"
      >
        <Icon
          name="plug"
          :size="15"
        />
        API 配置
      </button>
    </div>
    <form
      id="workspace-settings-form"
      class="workspace-settings-form"
      novalidate
      @submit.prevent="save"
    >
      <section
        v-show="tab === 'general'"
        class="general-settings"
        aria-label="通用设置"
      >
        <label class="settings-row">
          <span>
            <strong>主题外观</strong>
            <small>首页、工作台与弹窗使用统一外观。</small>
          </span>
          <select
            v-model="theme"
            aria-label="设置主题外观"
          >
            <option
              v-for="option in themeOptions"
              :key="option.id"
              :value="option.id"
            >
              {{ option.name }}
            </option>
          </select>
        </label>
        <label class="settings-row">
          <span>
            <strong>发送快捷键</strong>
            <small>Shift + Enter 始终用于换行。</small>
          </span>
          <select
            v-model="form.sendShortcut"
            aria-label="发送快捷键"
          >
            <option value="enter">Enter 发送</option>
            <option value="mod-enter">⌘ / Ctrl + Enter 发送</option>
          </select>
        </label>
        <label class="settings-row">
          <span>
            <strong>记住工作台布局</strong>
            <small>保存面板顺序、宽度和时间轴高度，刷新后恢复。</small>
          </span>
          <input
            v-model="form.rememberLayout"
            type="checkbox"
            aria-label="记住工作台布局"
          />
        </label>
        <div class="settings-layout-card">
          <Icon
            name="layers"
            :size="20"
          />
          <div>
            <strong>按你的节奏安排空间</strong>
            <p>拖动面板标题旁的手柄换位，拖动栏间分隔线调宽。时间轴上沿可以调整高度。</p>
            <small>桌面支持自定义布局；窄屏继续按区域切换。</small>
          </div>
        </div>
        <div class="settings-reset-row">
          <span>想回到熟悉的工作台？</span>
          <button
            type="button"
            class="text-button"
            @click="restoreLayout"
          >
            <Icon
              name="refresh"
              :size="13"
            />
            恢复默认布局
          </button>
        </div>
        <p
          v-if="resetDone"
          class="settings-reset-confirm"
          role="status"
        >
          <Icon
            name="check"
            :size="13"
          />
          布局已恢复默认。
        </p>
      </section>
      <section
        v-show="tab === 'apis'"
        class="api-settings"
        aria-label="模型供应商配置"
      >
        <div class="api-reserved-note">
          <Icon
            name="info"
            :size="17"
          />
          <p>
            默认一条流程共用一个模型：文案、分镜、剪辑、对话和 SVG 画面都走
            <code>{{ defaultCreationModel }}</code>
            。画面需要更快的视觉模型时，可在下方单独指定。保存后新任务立即生效；密钥不会回显到页面。
          </p>
        </div>
        <details
          class="api-module"
          open
        >
          <summary><strong>统一创作模型（文案 / 分镜 / 剪辑 / 画面）</strong></summary>
          <div class="api-module-fields">
            <p v-if="providerForm.script.provider === 'custom'">
              自定义 Endpoint 必须包含完整的 HTTP(S) 地址和版本路径，例如
              <code>https://your-host.example/v1</code>
              。
            </p>
            <label class="field-label">
              <span>供应商</span>
              <select
                v-model="providerForm.script.provider"
                aria-label="创作模型供应商"
                @change="applyPreset('script')"
              >
                <option value="siliconflow">硅基流动</option>
                <option value="openai">OpenAI</option>
                <option value="aliyun">阿里云百炼</option>
                <option value="deepseek">DeepSeek</option>
                <option value="minimax">MiniMax</option>
                <option value="glm">GLM</option>
                <option value="custom">自定义 OpenAI Compatible</option>
              </select>
            </label>
            <label class="field-label">
              <span>Base URL</span>
              <input
                v-model="providerForm.script.baseUrl"
                aria-label="创作 Base URL"
                type="url"
                @blur="loadModels('script')"
              />
            </label>
            <label class="field-label">
              <span>模型名称</span>
              <input
                v-model="providerForm.script.model"
                aria-label="创作模型名称"
                list="creation-model-options"
              />
              <datalist id="creation-model-options">
                <option
                  v-for="option in modelChoices"
                  :key="option"
                  :value="option"
                />
              </datalist>
            </label>
            <label class="field-label">
              <span>API Key</span>
              <input
                v-model="providerForm.script.apiKey"
                aria-label="创作 API Key"
                type="password"
                :placeholder="providerForm.script.hasApiKey ? '留空保留现有密钥' : '输入 API 密钥'"
                @blur="loadModels('script')"
              />
            </label>
            <button
              type="button"
              class="button secondary"
              :disabled="modelLoading.script"
              @click="loadModels('script')"
            >
              {{ modelLoading.script ? '正在获取…' : '获取模型列表' }}
            </button>
            <small v-if="modelStatus.script">{{ modelStatus.script }}</small>
          </div>
        </details>
        <details class="api-module">
          <summary>
            <strong>画面生成（SVG）</strong>
            <span class="api-module-state">
              {{ providerForm.visual.overridden ? '独立模型' : '跟随统一模型' }}
            </span>
          </summary>
          <div class="api-module-fields">
            <label class="settings-row api-inline-toggle">
              <span>
                <strong>单独配置画面模型</strong>
                <small v-if="!providerForm.visual.overridden">
                  当前使用统一模型
                  <code>{{ followedModel }}</code>
                  。
                </small>
                <small v-else>画面可以换成更便宜的专用模型，其他流程不受影响。</small>
              </span>
              <input
                type="checkbox"
                :checked="providerForm.visual.overridden"
                aria-label="单独配置画面模型"
                @change="toggleVisualOverride(($event.target as HTMLInputElement).checked)"
              />
            </label>
            <template v-if="providerForm.visual.overridden">
              <p v-if="providerForm.visual.provider === 'custom'">
                自定义 Endpoint 必须包含完整的 HTTP(S) 地址和版本路径，例如
                <code>https://your-host.example/v1</code>
                。
              </p>
              <label class="field-label">
                <span>供应商</span>
                <select
                  v-model="providerForm.visual.provider"
                  aria-label="画面模型供应商"
                  @change="applyPreset('visual')"
                >
                  <option value="siliconflow">硅基流动</option>
                  <option value="openai">OpenAI</option>
                  <option value="aliyun">阿里云百炼</option>
                  <option value="deepseek">DeepSeek</option>
                  <option value="minimax">MiniMax</option>
                  <option value="glm">GLM</option>
                  <option value="custom">自定义 OpenAI Compatible</option>
                </select>
              </label>
              <label class="field-label">
                <span>Base URL</span>
                <input
                  v-model="providerForm.visual.baseUrl"
                  aria-label="画面 Base URL"
                  type="url"
                  @blur="loadModels('visual')"
                />
              </label>
              <label class="field-label">
                <span>模型名称</span>
                <input
                  v-model="providerForm.visual.model"
                  aria-label="画面模型名称"
                  list="visual-model-options"
                />
                <datalist id="visual-model-options">
                  <option
                    v-for="option in modelChoices('visual')"
                    :key="option"
                    :value="option"
                  />
                </datalist>
              </label>
              <label class="field-label">
                <span>API Key</span>
                <input
                  v-model="providerForm.visual.apiKey"
                  aria-label="画面 API Key"
                  type="password"
                  :placeholder="
                    providerForm.visual.hasApiKey ? '留空保留现有密钥' : '输入 API 密钥'
                  "
                  @blur="loadModels('visual')"
                />
              </label>
              <button
                type="button"
                class="button secondary"
                :disabled="modelLoading.visual"
                @click="loadModels('visual')"
              >
                {{ modelLoading.visual ? '正在获取…' : '获取模型列表' }}
              </button>
              <small v-if="modelStatus.visual">{{ modelStatus.visual }}</small>
            </template>
          </div>
        </details>
        <details class="api-module">
          <summary><strong>背景音乐</strong></summary>
          <div class="api-module-fields">
            <label class="field-label">
              <span>供应商</span>
              <select
                v-model="providerForm.music.provider"
                aria-label="音乐供应商"
                @change="applyPreset('music')"
              >
                <option value="procedural">程序化合成（本地）</option>
                <option value="minimax">MiniMax</option>
                <option value="stable_audio">Stable Audio</option>
                <option value="siliconflow">硅基流动</option>
              </select>
            </label>
            <label class="field-label">
              <span>Base URL</span>
              <input
                v-model="providerForm.music.baseUrl"
                aria-label="音乐 Base URL"
                type="url"
                @blur="loadModels('music')"
              />
            </label>
            <label class="field-label">
              <span>模型名称</span>
              <input
                v-model="providerForm.music.model"
                aria-label="音乐模型名称"
                list="music-model-options"
              />
              <datalist id="music-model-options">
                <option
                  v-for="option in modelChoices('music')"
                  :key="option"
                  :value="option"
                />
              </datalist>
            </label>
            <label class="field-label">
              <span>API Key</span>
              <input
                v-model="providerForm.music.apiKey"
                aria-label="音乐 API Key"
                type="password"
                :placeholder="providerForm.music.hasApiKey ? '留空保留现有密钥' : '输入 API 密钥'"
                @blur="loadModels('music')"
              />
            </label>
            <button
              type="button"
              class="button secondary"
              :disabled="modelLoading.music"
              @click="loadModels('music')"
            >
              {{ modelLoading.music ? '正在获取…' : '获取模型列表' }}
            </button>
            <small v-if="modelStatus.music">{{ modelStatus.music }}</small>
          </div>
        </details>
        <details class="api-module">
          <summary><strong>配音</strong></summary>
          <div class="api-module-fields">
            <label class="field-label">
              <span>供应商</span>
              <select
                v-model="providerForm.voice.provider"
                aria-label="配音供应商"
                @change="applyPreset('voice')"
              >
                <option value="qwen">阿里云 Qwen-TTS</option>
                <option value="siliconflow">硅基流动 TTS</option>
                <option value="indextts">本地 IndexTTS</option>
                <option value="breeze">本地 breezeTTS2</option>
              </select>
            </label>
            <label class="field-label">
              <span>Base URL</span>
              <input
                v-model="providerForm.voice.baseUrl"
                aria-label="配音 Base URL"
                type="url"
                @blur="loadModels('voice')"
              />
            </label>
            <label class="field-label">
              <span>模型名称</span>
              <input
                v-model="providerForm.voice.model"
                aria-label="配音模型名称"
                list="voice-model-options"
              />
              <datalist id="voice-model-options">
                <option
                  v-for="option in modelChoices('voice')"
                  :key="option"
                  :value="option"
                />
              </datalist>
            </label>
            <label class="field-label">
              <span>默认声线（每次可改）</span>
              <input
                v-model="providerForm.voice.voiceId"
                aria-label="默认声线"
                placeholder="例如 Cherry / anna"
                list="default-voice-options"
              />
              <datalist id="default-voice-options">
                <option
                  v-for="option in voiceChoices"
                  :key="option"
                  :value="option"
                />
              </datalist>
            </label>
            <label
              v-if="['qwen', 'siliconflow'].includes(providerForm.voice.provider)"
              class="field-label"
            >
              <span>API Key</span>
              <input
                v-model="providerForm.voice.apiKey"
                aria-label="配音 API Key"
                type="password"
                :placeholder="providerForm.voice.hasApiKey ? '留空保留现有密钥' : '输入 API 密钥'"
                @blur="loadModels('voice')"
              />
            </label>
            <button
              type="button"
              class="button secondary"
              :disabled="modelLoading.voice"
              @click="loadModels('voice')"
            >
              {{ modelLoading.voice ? '正在获取…' : '获取模型列表' }}
            </button>
            <small v-if="modelStatus.voice">{{ modelStatus.voice }}</small>
            <button
              type="button"
              class="button secondary"
              :disabled="voiceLoading"
              @click="loadVoices"
            >
              {{ voiceLoading ? '正在获取…' : '获取声线' }}
            </button>
            <small v-if="voiceStatus">{{ voiceStatus }}</small>
            <div
              v-if="providerForm.voice.provider === 'breeze'"
              class="breeze-model-row"
            >
              <small v-if="breezeModel?.installed">本地模型已就绪：{{ breezeModel.dir }}</small>
              <template v-else-if="breezeModel?.downloading">
                <small>
                  正在下载本地配音模型
                  <template v-if="breezeProgress">
                    ：{{ breezeProgress.percent.toFixed(1) }}%（{{
                      formatBytes(breezeProgress.received)
                    }}/{{ formatBytes(breezeProgress.total) }}）
                  </template>
                  ，下载完成前无法生成配音。
                </small>
                <button
                  type="button"
                  class="button secondary"
                  @click="askBreezeDownload"
                >
                  查看进度
                </button>
              </template>
              <template v-else-if="breezeModel">
                <small>
                  本地配音模型尚未下载（约
                  {{ formatBytes(breezeModel.totalBytes) }}），下载后才能离线生成配音。
                </small>
                <button
                  type="button"
                  class="button secondary"
                  @click="askBreezeDownload"
                >
                  下载模型
                </button>
              </template>
              <small v-else-if="breezeError">{{ breezeError }}</small>
            </div>
          </div>
        </details>
        <p v-if="loading">正在读取供应商配置…</p>
      </section>
      <p
        v-if="error"
        class="error-inline"
        role="alert"
      >
        {{ error }}
      </p>
    </form>
    <template #footer>
      <span class="settings-footer-note">通用偏好独立于项目保存</span>
      <button
        class="button secondary"
        @click="emit('close')"
      >
        取消
      </button>
      <button
        class="button primary"
        form="workspace-settings-form"
        type="submit"
        :disabled="saving || loading"
      >
        {{ saving ? '正在保存…' : '保存设置' }}
      </button>
    </template>
  </Modal>
  <Modal
    :open="breezeConfirmOpen"
    title="下载本地配音模型"
    subtitle="breezeTTS2 模型只在你同意后才下载。"
    :locked="breezeDownloading"
    @close="breezeConfirmOpen = false"
  >
    <div class="breeze-confirm">
      <p>
        下载
        <strong>{{ formatBytes(breezeModel?.totalBytes ?? 0) }}</strong>
        的本地模型文件到 ComfyUI
        的模型目录，下载完成后即可离线生成配音。整个过程可能比较久，建议在网络稳定时进行。
      </p>
      <label class="field-label">
        <span>ComfyUI 安装目录</span>
        <input
          v-model="breezeDir"
          aria-label="ComfyUI 安装目录"
          placeholder="例如 D:\software\ComfyUI"
          :disabled="breezeDownloading"
        />
      </label>
      <p
        v-if="!breezeDir"
        class="breeze-hint"
      >
        没有自动找到 ComfyUI，请先在上方填写它的安装目录。
      </p>
      <div
        v-if="breezeProgress"
        class="breeze-progress"
      >
        <progress
          :value="breezeProgress.percent"
          max="100"
        />
        <small>
          {{ breezeProgress.percent.toFixed(1) }}% · {{ formatBytes(breezeProgress.received) }} /
          {{ formatBytes(breezeProgress.total) }} · 第 {{ breezeProgress.fileIndex }}/{{
            breezeProgress.fileCount
          }}
          个文件
          <template v-if="breezeProgress.file">（{{ breezeProgress.file }}）</template>
        </small>
      </div>
      <p
        v-if="breezeError"
        class="error-inline"
        role="alert"
      >
        {{ breezeError }}
      </p>
    </div>
    <template #footer>
      <button
        class="button secondary"
        :disabled="breezeDownloading"
        @click="breezeConfirmOpen = false"
      >
        暂不下载
      </button>
      <button
        v-if="!breezeDownloading"
        class="button primary"
        :disabled="!breezeDir"
        @click="startBreezeDownload"
      >
        同意并下载
      </button>
      <button
        v-else
        class="button secondary"
        @click="cancelBreezeDownloading"
      >
        取消下载
      </button>
    </template>
  </Modal>
</template>
<style scoped>
.breeze-model-row {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px 12px;
  margin-top: 8px;
}

.breeze-confirm {
  display: grid;
  gap: 12px;
}

.breeze-hint {
  color: var(--muted, #8a8f98);
  margin: 0;
}

.breeze-progress {
  display: grid;
  gap: 6px;
}

.breeze-progress progress {
  width: 100%;
  height: 8px;
}
</style>
