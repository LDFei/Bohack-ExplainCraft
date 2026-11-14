<script setup lang="ts">
/** 通用偏好与真实模型供应商设置；供应商通过 Tauri command 保存并热应用。 */
import { computed, ref, watch } from 'vue';
import { useAppSettings } from '../composables/useAppSettings.ts';
import type { GeneralSettings } from '../composables/useAppSettings.ts';
import { themeOptions, useTheme } from '../composables/useTheme.ts';
import type { ThemeId } from '../composables/useTheme.ts';
import {
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
const modelOptions = ref<Record<string, string[]>>({ script: [], music: [], voice: [] }),
  modelStatus = ref<Record<string, string>>({ script: '', music: '', voice: '' }),
  modelLoading = ref<Record<string, boolean>>({ script: false, music: false, voice: false });

/** 返回服务端候选与用户手动输入的并集，确保自定义模型不会消失。 */
function modelChoices(kind: keyof ModelSettings) {
  const current = providerForm.value[kind].model;
  return [...new Set([...(modelOptions.value[kind] ?? []), ...(current ? [current] : [])])];
}

const voiceOptions = ref<string[]>([]),
  voiceStatus = ref(''),
  voiceLoading = ref(false);
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
});
const providerForm = ref<ModelSettings>({
  script: empty('siliconflow'),
  music: empty('procedural'),
  voice: empty('qwen'),
});
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
  if (kind === 'script') {
    modelOptions.value[kind] = [];
    modelStatus.value[kind] = '';
  }
  if (kind === 'voice') {
    voiceOptions.value = [];
    voiceStatus.value = '';
  }
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
          <p>创作对话与 SVG 共用同一个 LLM。保存后新任务立即使用新配置；密钥不会回显到页面。</p>
        </div>
        <details
          class="api-module"
          open
        >
          <summary><strong>创作 LLM（对话 / 分镜 / SVG）</strong></summary>
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
</template>
