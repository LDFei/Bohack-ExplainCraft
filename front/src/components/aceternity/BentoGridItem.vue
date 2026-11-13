<script setup lang="ts">
/** 对应官方 BentoGridItem：header + icon/title/description；卡片悬停时内容沿 X 轴平移 8px。 */
defineProps<{ title: string; description: string; wide?: boolean }>();
</script>
<template>
  <article
    class="ac-bento-item"
    :class="{ 'bento-wide': wide }"
  >
    <div class="ac-bento-header"><slot name="header" /></div>
    <div class="ac-bento-copy">
      <slot name="icon" />
      <h3>{{ title }}</h3>
      <p>{{ description }}</p>
      <slot name="action" />
    </div>
  </article>
</template>
