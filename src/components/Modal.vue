<script setup lang="ts">
import { nextTick, onBeforeUnmount, ref, useId, watch } from 'vue'
import Icon from './Icon.vue'
const props = withDefaults(defineProps<{ open: boolean; title: string; subtitle?: string; wide?: boolean; locked?: boolean }>(), { subtitle: '', wide: false, locked: false })
const emit = defineEmits<{ close: [] }>()
const panel = ref<HTMLElement | null>(null), titleId = useId()
let returnFocus: HTMLElement | null = null
watch(() => props.open, async value => {
  if (value) { returnFocus = document.activeElement as HTMLElement; await nextTick(); const target = panel.value?.querySelector<HTMLElement>('[autofocus]') ?? panel.value?.querySelector<HTMLElement>('button,input,select,textarea'); target?.focus() }
  else returnFocus?.focus()
})
function keydown(event: KeyboardEvent) {
  if (event.key === 'Escape' && !props.locked) emit('close')
  if (event.key !== 'Tab') return
  const elements = Array.from(panel.value?.querySelectorAll<HTMLElement>('button:not([disabled]),input:not([disabled]),textarea:not([disabled]),select:not([disabled]),a[href],[tabindex="0"]') ?? []).filter(element => element.getClientRects().length)
  const first = elements[0], last = elements.at(-1)
  if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last?.focus() }
  else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first?.focus() }
}
onBeforeUnmount(() => { if (props.open) returnFocus?.focus() })
</script>
<template>
  <Teleport to="body">
    <Transition name="modal">
      <div v-if="open" class="modal-backdrop" @click.self="!locked && emit('close')" @keydown="keydown">
        <section ref="panel" class="modal-panel" :class="{ wide }" role="dialog" aria-modal="true" :aria-labelledby="titleId" tabindex="-1">
          <header class="modal-header"><div><h2 :id="titleId">{{ title }}</h2><p v-if="subtitle">{{ subtitle }}</p></div><button class="icon-button" aria-label="关闭对话框" :disabled="locked" @click="emit('close')"><Icon name="close" /></button></header>
          <div class="modal-body"><slot /></div>
          <footer v-if="$slots.footer" class="modal-footer"><slot name="footer" /></footer>
        </section>
      </div>
    </Transition>
  </Teleport>
</template>
