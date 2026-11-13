<script setup lang="ts">
/**
 * 统一 SVG 图标。name 对应 paths 表中的路径，未知名称回退为 sparkles，避免每个组件复制一套图标。
 */

withDefaults(defineProps<{ name: string; size?: number; stroke?: number }>(), {
  size: 20,
  stroke: 1.7,
});
// 统一 24×24 的路径表，size 只改变显示尺寸，不改变每个图标的坐标系。
const paths: Record<string, string> = {
  sparkles: 'm12 3 2.8 6.2L21 12l-6.2 2.8L12 21l-2.8-6.2L3 12l6.2-2.8L12 3M20 2v4m-2-2h4',
  plus: 'M12 5v14M5 12h14',
  search: 'm20 20-4.5-4.5M17 10a7 7 0 1 1-14 0 7 7 0 0 1 14 0',
  video: 'M4 5h12a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V7a2 2 0 0 1 2-2m14 4 4-2v10l-4-2',
  grid: 'M3 3h7v7H3V3m11 0h7v7h-7V3M3 14h7v7H3v-7m11 0h7v7h-7v-7',
  chevron: 'm9 5 7 7-7 7',
  down: 'm6 9 6 6 6-6',
  close: 'm6 6 12 12M6 18 18 6',
  sidebar: 'M4 3h16a1 1 0 0 1 1 1v16a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1m4 0v18',
  check: 'm5 12 4 4L19 6',
  cloud: 'M7 18h11a4 4 0 0 0 0-8 6 6 0 0 0-11-3 4 4 0 0 0 0 11m2-5 2 2 4-4',
  more: 'M5 12h.01M12 12h.01M19 12h.01',
  arrowUp: 'M12 19V5m-6 6 6-6 6 6',
  arrowRight: 'M5 12h14m-6-6 6 6-6 6',
  attach: 'm9 12 6-6a3 3 0 0 1 4 4l-8 8a5 5 0 0 1-7-7l9-9m-6 13 8-8',
  image:
    'M4 3h16a1 1 0 0 1 1 1v16a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1m-1 14 6-6 4 4 3-3 5 5M8 7h.01',
  clock: 'M12 7v5l3 2m6-2a9 9 0 1 1-18 0 9 9 0 0 1 18 0',
  ratio: 'M3 6h18v12H3V6m4 4h10v4H7v-4',
  palette:
    'M12 3a9 9 0 1 0 0 18h1a2 2 0 0 0 0-4h-1a1 1 0 0 1 0-2h3a6 6 0 0 0 6-6c0-3-4-6-9-6M7 8h.01M12 6h.01M17 8h.01M6 13h.01',
  play: 'm9 5 11 7-11 7V5',
  pause: 'M8 5v14M16 5v14',
  stop: 'M6 6h12v12H6V6',
  download: 'M12 3v12m-5-5 5 5 5-5M4 16v5h16v-5',
  export: 'M13 3h8v8m0-8L10 14M9 4H4v16h16v-5',
  edit: 'm15 5 4 4M4 20l4-1L21 6l-4-4L4 15v5',
  trash: 'M3 6h18M9 6V3h6v3M5 6l1 15h12l1-15M10 10v7m4-7v7',
  folder: 'M3 5h6l2 2h10v13H3V5',
  music: 'M9 18V5l12-2v13M9 8l12-2M9 18c0 2-6 4-6 0s6-2 6 0m12-2c0 2-6 4-6 0s6-2 6 0',
  mic: 'M9 5a3 3 0 0 1 6 0v7a3 3 0 0 1-6 0V5m-4 6v1a7 7 0 0 0 14 0v-1m-7 8v3m-4 0h8',
  sliders: 'M4 6h16M4 12h16M4 18h16M8 3v6m8 0v6m-6 0v6',
  file: 'M14 2H4v20h16V8l-6-6m0 0v6h6M8 13h8m-8 4h6',
  refresh: 'M20 7a9 9 0 1 0 1 8M20 3v5h-5',
  info: 'M12 11v6m0-10h.01m9 5a9 9 0 1 1-18 0 9 9 0 0 1 18 0',
  help: 'M9 8a3 3 0 1 1 4 3v3m-1 3h.01m9-5a9 9 0 1 1-18 0 9 9 0 0 1 18 0',
  bolt: 'm13 2-9 12h7l-1 8 10-13h-7l1-7',
  menu: 'M4 6h16M4 12h16M4 18h16',
  volume: 'M11 4 6 8H2v8h4l5 4V4m4 4a6 6 0 0 1 0 8m3-11a10 10 0 0 1 0 14',
  mute: 'M11 4 6 8H2v8h4l5 4V4m5 5 6 6m-6 0 6-6',
  fullscreen: 'M8 3H3v5m13-5h5v5M3 16v5h5m13-5v5h-5',
  coffee: 'M3 8h14v9a4 4 0 0 1-4 4H7a4 4 0 0 1-4-4V8m14 1h2a3 3 0 0 1 0 6h-2M7 2v3m5-3v3',
  mountain: 'm2 20 7-14 6 14H2m11-6 4-9 6 15h-8m-9-6 3-2 3 2',
  bag: 'M4 7h16l1 14H3L4 7m4 0V5a4 4 0 0 1 8 0v2',
  book: 'M12 5c-3-3-7-3-10-1v16c3-2 7-2 10 1m0-16c3-3 7-3 10-1v16c-3-2-7-2-10 1V5',
  logout: 'M9 3H3v18h6m5-14 5 5-5 5m-7-5h15',
  headphones: 'M3 14v-2a9 9 0 0 1 18 0v2M3 12h4v8H3v-8m14 0h4v8h-4v-8',
};
</script>
<template>
  <svg
    :width="size"
    :height="size"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    :stroke-width="stroke"
    stroke-linecap="round"
    stroke-linejoin="round"
    aria-hidden="true"
  >
    <path :d="paths[name] || paths.sparkles" />
  </svg>
</template>
