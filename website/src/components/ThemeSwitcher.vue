<script setup lang="ts">
/** 外观选择只修改全局设计变量，四套小样直接复用各自的主题 Tokens。 */
import { ref } from "vue";
import {
  DEFAULT_THEME,
  themeOptions,
  useTheme,
} from "../composables/useTheme.ts";
import Icon from "./Icon.vue";
import Modal from "./Modal.vue";
import { mediaUrl } from "../domain/assets.ts";
const open = ref(false);
const { currentTheme, selectedTheme, persistenceError, setTheme, resetTheme } =
  useTheme();
/** 主题卡片按两列布局响应方向键，选择与焦点同步，并支持 Home/End 快速定位。 */
function navigateTheme(event: KeyboardEvent, index: number) {
  const steps: Record<string, number> = {
    ArrowRight: 1,
    ArrowLeft: -1,
    ArrowDown: 2,
    ArrowUp: -2,
  };
  let next = index;
  if (event.key === "Home") next = 0;
  else if (event.key === "End") next = themeOptions.length - 1;
  else if (event.key in steps)
    next =
      (index + steps[event.key]! + themeOptions.length) % themeOptions.length;
  else return;
  event.preventDefault();
  setTheme(themeOptions[next]!.id);
  (event.currentTarget as HTMLElement).parentElement
    ?.querySelectorAll<HTMLElement>('[role="radio"]')
    [next]?.focus();
}
</script>
<template>
  <button
    class="theme-trigger"
    aria-label="更换主题"
    :title="`当前主题：${currentTheme.name}`"
    :aria-expanded="open"
    aria-haspopup="dialog"
    @click="open = true"
  >
    <Icon name="palette" :size="16" />
    <span>主题</span>
    <Icon class="theme-trigger-chevron" name="down" :size="11" />
  </button>
  <Modal
    :open="open"
    title="给创作空间，换一种心情"
    subtitle="选择你喜欢的外观，让每个镜头都看得更清楚。"
    wide
    @close="open = false"
  >
    <div class="theme-grid" role="radiogroup" aria-label="选择工作台主题">
      <button
        v-for="(theme, index) in themeOptions"
        :key="theme.id"
        class="theme-option"
        :class="{ selected: selectedTheme === theme.id }"
        role="radio"
        :aria-label="theme.name"
        :aria-checked="selectedTheme === theme.id"
        :tabindex="selectedTheme === theme.id ? 0 : -1"
        :autofocus="selectedTheme === theme.id"
        @keydown="navigateTheme($event, index)"
        @click="setTheme(theme.id)"
      >
        <div class="theme-miniature" :data-theme="theme.id" aria-hidden="true">
          <div class="mini-topbar">
            <i />
            <span />
            <b />
            <b />
          </div>
          <div class="mini-workspace">
            <div class="mini-library">
              <span />
              <i />
              <i />
              <i />
            </div>
            <div class="mini-main">
              <div class="mini-canvas">
                <img :src="mediaUrl('coffee-brew.jpg')" alt="" />
                <span />
              </div>
              <div class="mini-tracks">
                <i />
                <i />
                <i />
              </div>
            </div>
            <div class="mini-copilot">
              <span />
              <i />
              <i />
              <b />
            </div>
          </div>
        </div>
        <div class="theme-option-info">
          <span>
            <strong>{{ theme.name }}</strong>
            <small>{{ theme.subtitle }}</small>
          </span>
          <span class="theme-radio-indicator">
            <Icon v-if="selectedTheme === theme.id" name="check" :size="13" />
          </span>
        </div>
        <div class="theme-option-footer">
          <span class="theme-tag">{{ theme.tag }}</span>
          <span v-if="selectedTheme === theme.id" class="theme-in-use">
            使用中
          </span>
        </div>
      </button>
    </div>
    <p class="theme-note" role="status">
      <Icon :name="persistenceError ? 'info' : 'check'" :size="13" />
      {{
        persistenceError
          ? "主题已应用；当前浏览器未允许保存，刷新后可能无法保留本次选择。"
          : "即时生效，自动记住你的选择。项目内容与创作进度保持原样。"
      }}
    </p>
    <template #footer>
      <button
        class="button secondary"
        :disabled="selectedTheme === DEFAULT_THEME"
        @click="resetTheme"
      >
        <Icon name="refresh" :size="14" />
        恢复默认
      </button>
      <button class="button primary" @click="open = false">完成</button>
    </template>
  </Modal>
</template>
