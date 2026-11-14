<script setup lang="ts">
/**
 * 可复用弹窗：管理显示、焦点恢复和键盘循环，内容与确认按钮由调用方通过插槽提供。
 */

import { nextTick, onBeforeUnmount, ref, useId, watch } from "vue";
import Icon from "./Icon.vue";
const props = withDefaults(
  defineProps<{
    open: boolean;
    title: string;
    subtitle?: string;
    wide?: boolean;
    locked?: boolean;
  }>(),
  { subtitle: "", wide: false, locked: false },
);
const emit = defineEmits<{ close: [] }>();
const panel = ref<HTMLElement | null>(null),
  titleId = useId();
let returnFocus: HTMLElement | null = null;
// 先等待弹窗 DOM 出现再聚焦，关闭时把焦点还给打开它的控件。
watch(
  () => props.open,
  async (value) => {
    if (value) {
      returnFocus = document.activeElement as HTMLElement;
      await nextTick();
      const target =
        panel.value?.querySelector<HTMLElement>("[autofocus]") ??
        panel.value?.querySelector<HTMLElement>("button,input,select,textarea");
      target?.focus();
    } else returnFocus?.focus();
  },
);
/**
 * 管理弹窗键盘操作：未锁定时 Escape 关闭，Tab 和 Shift+Tab 在弹窗控件之间循环。
 * 只选择仍可见、未禁用的控件，避免键盘焦点跑到弹窗后面的页面。
 */
function keydown(event: KeyboardEvent) {
  if (event.key === "Escape" && !props.locked) emit("close");
  if (event.key !== "Tab") return;
  const elements = Array.from(
    panel.value?.querySelectorAll<HTMLElement>(
      'button:not([disabled]),input:not([disabled]),textarea:not([disabled]),select:not([disabled]),a[href],[tabindex="0"]',
    ) ?? [],
  ).filter((element) => element.getClientRects().length);
  const first = elements[0],
    last = elements.at(-1);
  if (event.shiftKey && document.activeElement === first) {
    event.preventDefault();
    last?.focus();
  } else if (!event.shiftKey && document.activeElement === last) {
    event.preventDefault();
    first?.focus();
  }
}
// 调用方直接卸载弹窗时，也尽量恢复之前的键盘焦点。
onBeforeUnmount(() => {
  if (props.open) returnFocus?.focus();
});
</script>
<template>
  <!-- 将弹窗挂到 body，避免被工作台内部的 overflow 或层级裁切。 -->
  <Teleport to="body">
    <Transition name="modal">
      <div
        v-if="open"
        class="modal-backdrop"
        @click.self="!locked && emit('close')"
        @keydown="keydown"
      >
        <section
          ref="panel"
          class="modal-panel"
          :class="{ wide }"
          role="dialog"
          aria-modal="true"
          :aria-labelledby="titleId"
          tabindex="-1"
        >
          <header class="modal-header">
            <div>
              <h2 :id="titleId">{{ title }}</h2>
              <p v-if="subtitle">{{ subtitle }}</p>
            </div>
            <button
              class="icon-button"
              aria-label="关闭对话框"
              :disabled="locked"
              @click="emit('close')"
            >
              <Icon name="close" />
            </button>
          </header>
          <div class="modal-body"><slot /></div>
          <footer v-if="$slots.footer" class="modal-footer">
            <slot name="footer" />
          </footer>
        </section>
      </div>
    </Transition>
  </Teleport>
</template>
