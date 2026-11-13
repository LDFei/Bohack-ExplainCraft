<script setup lang="ts">
/** Aceternity Background Beams 的 Vue 移植：保留官方路径、渐变光束及 10～20 秒缓动，使用 SVG 原生动画替代 motion/react。 */
import { onBeforeUnmount, onMounted, ref, useId } from 'vue';
import { baseBeamPath, beamPaths } from './beamPaths.ts';
const svg = ref<SVGSVGElement | null>(null);
const reducedMotion = ref(window.matchMedia('(prefers-reduced-motion: reduce)').matches);
const id = `beams-${useId().replace(/[^a-zA-Z0-9_-]/g, '')}`;
let observer: IntersectionObserver | undefined;
const media = window.matchMedia('(prefers-reduced-motion: reduce)');
/** 系统要求减少动态效果时移除持续动画，保留低对比路径作为静态背景。 */
function updateMotion() {
  reducedMotion.value = media.matches;
}
onMounted(() => {
  media.addEventListener('change', updateMotion);
  // 离开首屏时暂停 SVG 动画，进入工作台则随组件卸载，避免后台持续绘制。
  observer = new IntersectionObserver(([entry]) => {
    if (entry?.isIntersecting) svg.value?.unpauseAnimations();
    else svg.value?.pauseAnimations();
  });
  if (svg.value) observer.observe(svg.value);
});
onBeforeUnmount(() => {
  observer?.disconnect();
  media.removeEventListener('change', updateMotion);
});
</script>
<template>
  <div
    class="background-beams"
    aria-hidden="true"
  >
    <svg
      ref="svg"
      width="100%"
      height="100%"
      viewBox="0 0 696 316"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
    >
      <path
        :d="baseBeamPath"
        :stroke="`url(#${id}-radial)`"
        stroke-opacity="0.12"
        stroke-width="0.5"
      />
      <path
        v-for="(path, index) in beamPaths"
        :key="index"
        :d="path"
        :stroke="`url(#${id}-${index})`"
        stroke-opacity="0.4"
        stroke-width="0.5"
      />
      <defs>
        <linearGradient
          v-for="(_, index) in beamPaths"
          :id="`${id}-${index}`"
          :key="index"
          x1="0%"
          x2="95%"
          y1="0%"
          :y2="`${93 + (index % 9)}%`"
        >
          <template v-if="!reducedMotion">
            <animate
              v-for="[attribute, target] in [
                ['x1', '100%'],
                ['x2', '95%'],
                ['y1', '100%'],
                ['y2', `${93 + (index % 9)}%`],
              ]"
              :key="attribute"
              :attributeName="attribute"
              :values="`0%;${target}`"
              :dur="`${10 + ((index * 7) % 100) / 10}s`"
              :begin="`${-((index * 13) % 100) / 10}s`"
              repeatCount="indefinite"
              calcMode="spline"
              keySplines="0.42 0 0.58 1"
              keyTimes="0;1"
            />
          </template>
          <stop
            stop-color="#18CCFC"
            stop-opacity="0"
          />
          <stop stop-color="#18CCFC" />
          <stop
            offset="32.5%"
            stop-color="#6344F5"
          />
          <stop
            offset="100%"
            stop-color="#AE48FF"
            stop-opacity="0"
          />
        </linearGradient>
        <radialGradient
          :id="`${id}-radial`"
          cx="0"
          cy="0"
          r="1"
          gradientUnits="userSpaceOnUse"
          gradientTransform="translate(352 34) rotate(90) scale(555 1560.62)"
        >
          <stop
            offset="0.0666667"
            stop-color="currentColor"
          />
          <stop
            offset="0.243243"
            stop-color="currentColor"
          />
          <stop
            offset="0.43594"
            stop-color="white"
            stop-opacity="0"
          />
        </radialGradient>
      </defs>
    </svg>
  </div>
</template>
