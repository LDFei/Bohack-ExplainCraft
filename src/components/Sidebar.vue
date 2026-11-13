<script setup lang="ts">
import { computed, ref } from 'vue'
import type { Project } from '../domain/types.ts'
import { imagePath, inferTheme, themes } from '../domain/content.ts'
import Icon from './Icon.vue'
const props = defineProps<{ projects: Project[]; selectedId: string; open: boolean }>()
const emit = defineEmits<{ new: []; select: [id: string]; templates: []; guide: []; rename: [project: Project]; delete: [project: Project]; close: [] }>()
const query = ref(''), menuId = ref('')
const groups = computed(() => {
  const today = new Date().toDateString(), yesterday = new Date(Date.now() - 86400000).toDateString()
  const list = props.projects.filter(project => project.title.toLowerCase().includes(query.value.toLowerCase()))
  return [{ label: '今天', items: list.filter(project => new Date(project.created_at).toDateString() === today) }, { label: '昨天', items: list.filter(project => new Date(project.created_at).toDateString() === yesterday) }, { label: '更早', items: list.filter(project => ![today, yesterday].includes(new Date(project.created_at).toDateString())) }].filter(group => group.items.length)
})
function select(id: string) { menuId.value = ''; emit('select', id); emit('close') }
</script>
<template>
  <button v-if="open" class="sidebar-scrim" aria-label="关闭历史记录" @click="emit('close')" />
  <aside class="sidebar" :class="{ 'is-open': open }" aria-label="项目导航">
    <div class="brand"><a class="brand-link" href="/create" @click.prevent="emit('new'); emit('close')"><span class="brand-icon"><Icon name="sparkles" :size="25" :stroke="1.6" /></span><span class="brand-word">灵镜<span>AI 视频创作</span></span></a><button class="icon-button mobile-only" aria-label="关闭导航" @click="emit('close')"><Icon name="close" /></button></div>
    <button class="new-project" @click="emit('new'); emit('close')"><Icon name="plus" :size="19" /><span>新建创作</span><span class="key-hint" aria-hidden="true">＋</span></button>
    <nav class="main-nav"><button class="nav-item active" @click="selectedId ? select(selectedId) : emit('new')"><Icon name="video" :size="19" />创作空间<span class="nav-dot" /></button><button class="nav-item" @click="emit('templates'); emit('close')"><Icon name="grid" :size="18" />灵感模板<span class="tiny-label">NEW</span></button></nav>
    <div class="history-heading"><span>我的创作</span><span>{{ projects.length }}</span></div>
    <label class="history-search"><Icon name="search" :size="15" /><input v-model="query" aria-label="筛选已加载项目" placeholder="搜索创作记录" /></label>
    <div class="history-list">
      <section v-for="group in groups" :key="group.label" class="history-group"><h3>{{ group.label }}</h3>
        <div v-for="project in group.items" :key="project.id" class="history-project" :class="{ selected: selectedId === project.id }">
          <button class="project-link" :aria-current="selectedId === project.id ? 'page' : undefined" @click="select(project.id)"><img :src="imagePath(themes[inferTheme(project.title)].images[0])" alt="" /><span class="project-label"><span>{{ project.title }}</span><small v-if="project.status === 'processing'" class="status-processing">正在制作中</small><small v-else-if="project.status === 'waiting_input'">等待补充信息</small><small v-else-if="project.status === 'failed'" class="status-failed">制作暂时中断</small><small v-else>{{ project.latest_video_version_id ? '已完成' : '草稿' }}</small></span></button>
          <button class="project-more icon-button" :aria-label="`管理项目：${project.title}`" :aria-expanded="menuId === project.id" @click.stop="menuId = menuId === project.id ? '' : project.id"><Icon name="more" :size="16" /></button>
          <div v-if="menuId === project.id" class="project-menu"><button @click="emit('rename', project); menuId = ''"><Icon name="edit" :size="15" />重命名</button><button class="danger-text" @click="emit('delete', project); menuId = ''"><Icon name="trash" :size="15" />删除项目</button></div>
        </div>
      </section>
      <p v-if="!groups.length" class="history-empty">{{ query ? '没有找到相关创作' : '你的第一个故事，从这里开始' }}</p>
    </div>
    <div class="sidebar-bottom"><button class="idea-note" @click="emit('guide')"><span class="note-spark"><Icon name="sparkles" :size="21" /></span><strong>给灵感一个镜头</strong><span>一句话，开启你的创作</span><Icon name="arrowRight" :size="17" /></button><button class="profile" @click="emit('guide')"><span class="avatar">L</span><span><strong>我的工作空间</strong><small>个人创作 · 演示版</small></span><Icon name="down" :size="16" /></button></div>
  </aside>
</template>
