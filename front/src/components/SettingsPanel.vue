<script setup lang="ts">
/** 设置仅管理通用偏好和未来 API 的配置槽，不改变项目 API 契约，也不测试或调用外部服务。 */
import { ref, watch } from 'vue';
import { apiModules, emptyApiConnections, useAppSettings } from '../composables/useAppSettings.ts';
import type { ApiConnections, GeneralSettings } from '../composables/useAppSettings.ts';
import { themeOptions, useTheme } from '../composables/useTheme.ts';
import type { ThemeId } from '../composables/useTheme.ts';
import Icon from './Icon.vue';
import Modal from './Modal.vue';
const props = defineProps<{ open: boolean }>();
const emit = defineEmits<{ close: []; saved: [persisted: boolean]; resetLayout: [] }>();
const { general, connections, saveSettings } = useAppSettings();
const { selectedTheme, setTheme, persistenceError } = useTheme();
const tab = ref<'general' | 'apis'>('general'),
  error = ref(''),
  resetDone = ref(false);
const form = ref<GeneralSettings>({ rememberLayout: true, sendShortcut: 'enter' });
const apiForm = ref<ApiConnections>(emptyApiConnections());
const theme = ref<ThemeId>('silver');
// 每次打开复制当前值，取消不会把未保存的模型、密钥或通用设置写回共享状态。
watch(
  () => props.open,
  (open) => {
    if (!open) return;
    form.value = { ...general.value };
    apiForm.value = JSON.parse(JSON.stringify(connections.value)) as ApiConnections;
    theme.value = selectedTheme.value;
    error.value = '';
    resetDone.value = false;
    tab.value = 'general';
  },
  { immediate: true },
);
/** 恢复布局是即时动作，用行内状态告知完成；不提前保存弹窗内其他设置。 */
function restoreLayout() {
  emit('resetLayout');
  resetDone.value = true;
}
/** 只有校验通过才应用外观和配置；保存失败时明确告知仅在当前页面生效。 */
function save() {
  const result = saveSettings(form.value, apiForm.value);
  if (result.error) {
    error.value = result.error;
    tab.value = 'apis';
    return;
  }
  setTheme(theme.value);
  emit('saved', result.persisted && !persistenceError.value);
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
        <span>预留</span>
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
        aria-label="API 预留配置"
      >
        <div class="api-reserved-note">
          <Icon
            name="info"
            :size="17"
          />
          <p>
            以下为配置预留，当前演示不会调用这些服务。地址和模型可保存在本机；API Key
            仅在当前页面内保留，刷新后需重新填写。
          </p>
        </div>
        <details
          v-for="group in apiModules"
          :key="group.id"
          class="api-module"
          :open="group.id === 'video'"
        >
          <summary>
            <span class="api-module-icon">
              <Icon
                :name="group.icon"
                :size="18"
              />
            </span>
            <span>
              <strong>{{ group.name }}</strong>
              <small>{{ group.description }}</small>
            </span>
            <span class="api-reserved-badge">
              {{ group.slots.length === 2 ? '2 个 API' : '待接入' }}
            </span>
            <Icon
              name="down"
              :size="14"
            />
          </summary>
          <div
            class="api-module-fields"
            :class="{ 'two-slots': group.slots.length === 2 }"
          >
            <fieldset
              v-for="slot in group.slots"
              :key="slot.id"
            >
              <legend>{{ slot.name }}</legend>
              <label class="field-label">
                <span>接口地址</span>
                <input
                  v-model="apiForm[slot.id].endpoint"
                  :aria-label="`${slot.name}接口地址`"
                  type="url"
                  placeholder="https://api.example.com/v1/..."
                  maxlength="2048"
                  autocomplete="off"
                />
              </label>
              <label class="field-label">
                <span>模型名称</span>
                <input
                  v-model="apiForm[slot.id].model"
                  :aria-label="`${slot.name}模型名称`"
                  placeholder="填写后续接入的模型标识"
                  maxlength="200"
                  autocomplete="off"
                />
              </label>
              <label class="field-label">
                <span>
                  API Key
                  <small>仅当前页面</small>
                </span>
                <input
                  v-model="apiForm[slot.id].apiKey"
                  :aria-label="`${slot.name}密钥`"
                  type="password"
                  placeholder="预留，暂不发送请求"
                  autocomplete="off"
                  maxlength="4096"
                />
              </label>
            </fieldset>
          </div>
        </details>
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
      >
        保存设置
      </button>
    </template>
  </Modal>
</template>
