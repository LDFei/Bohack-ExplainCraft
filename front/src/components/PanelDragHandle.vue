<script setup lang="ts">
/** 只有专用手柄可拖动面板；正文、播放器和文件拖入保持原有交互。 */
import Icon from './Icon.vue';
import type { WorkspacePanel } from '../composables/useWorkspaceLayout.ts';
defineProps<{ panel: WorkspacePanel; label: string; enabled: boolean }>();
const emit = defineEmits<{
  start: [panel: WorkspacePanel, event: DragEvent];
  end: [];
  move: [panel: WorkspacePanel, direction: number];
}>();
</script>
<template>
  <button
    v-if="enabled"
    class="panel-drag-handle"
    :draggable="enabled"
    :aria-label="`拖动${label}面板`"
    :title="`拖动调整${label}位置，或按 Alt + 左右方向键移动`"
    @dragstart.stop="emit('start', panel, $event)"
    @dragend="emit('end')"
    @keydown.alt.left.prevent="emit('move', panel, -1)"
    @keydown.alt.right.prevent="emit('move', panel, 1)"
  >
    <Icon
      name="grip"
      :size="15"
    />
  </button>
</template>
