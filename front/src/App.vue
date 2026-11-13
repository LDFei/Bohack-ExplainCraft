<script setup lang="ts">
/**
 * 页面总装配：把左侧导航、中间对话、右侧预览和弹窗连接到 useStudio。业务数据由组合函数管理，此文件主要协调界面入口。
 */

import { computed, ref, watch } from 'vue';
import { useRouter } from 'vue-router';
import { useStudio } from './composables/useStudio.ts';
import { themes, imagePath } from './domain/content.ts';
import type { Project } from './domain/types.ts';
import Sidebar from './components/Sidebar.vue';
import Conversation from './components/Conversation.vue';
import PreviewPanel from './components/PreviewPanel.vue';
import Modal from './components/Modal.vue';
import Icon from './components/Icon.vue';

// 所有主要数据取自一个组合函数，三栏共享同一份项目、任务和版本状态。
const studio = useStudio(),
  router = useRouter();
const {
  projects,
  current,
  assets,
  versions,
  jobs,
  caps,
  selectedVersion,
  loading,
  pending,
  pageError,
  input,
  selectedAssets,
  intent,
  settings,
  uploading,
  toast,
  connection,
  projectId,
  activeJob,
  busy,
  historical,
  messages,
  playing,
} = studio;
// 已连接 Rust 后端，但后端模型仍是演示供应商（使用示例素材），需要继续向用户说明演示边界。
const backendDemo = computed(() => studio.api.mode === 'http' && !!caps.value.demo_mode);
// 本地演示或后端演示时都显示演示标识、说明和演示情景入口。
const showDemo = computed(() => studio.api.mode === 'demo' || backendDemo.value);
const sidebarOpen = ref(false),
  previewOpen = ref(false),
  moreOpen = ref(false);
const conversation = ref<InstanceType<typeof Conversation> | null>(null),
  preview = ref<InstanceType<typeof PreviewPanel> | null>(null);
const modal = ref<'templates' | 'guide' | 'rename' | 'delete' | 'export' | null>(null),
  managedProject = ref<Project | null>(null),
  titleInput = ref(''),
  modalPending = ref(false);
const exportResolution = ref('720p'),
  exportedAsset = ref(''),
  exportJobId = ref(''),
  exportError = ref('');
const title = computed(() => current.value?.project.title ?? '新的创作');
const exportJob = computed(() => (exportJobId.value ? jobs.value[exportJobId.value] : null));
// 切项目时收起临时面板和菜单，手机端才能回到新项目的对话入口。
watch(projectId, () => {
  previewOpen.value = false;
  sidebarOpen.value = false;
  moreOpen.value = false;
});
// 额外追踪导出任务的终态；成功之后把素材 ID 交给下载动作。
watch(exportJob, (job) => {
  if (job?.status === 'succeeded' && job.result?.asset_id) {
    exportedAsset.value = job.result.asset_id;
    modalPending.value = false;
  }
  if (job?.status === 'failed') {
    exportError.value = job.error?.message ?? '导出失败，请稍后重试。';
    modalPending.value = false;
  }
});
/**
 * 打开改名或删除弹窗，保存本次操作对应的项目；不会立即修改数据。
 */
function openManage(type: 'rename' | 'delete', project: Project) {
  managedProject.value = project;
  titleInput.value = project.title;
  modal.value = type;
  moreOpen.value = false;
}
/**
 * 提交改名或删除操作。只有服务层确认成功才关闭弹窗，失败时保留输入方便重试。
 */
async function confirmManage() {
  if (!managedProject.value) return;
  modalPending.value = true;
  try {
    const ok =
      modal.value === 'rename'
        ? await studio.rename(managedProject.value.id, titleInput.value)
        : await studio.remove(managedProject.value.id);
    if (ok) modal.value = null;
  } finally {
    modalPending.value = false;
  }
}
/**
 * 展开右侧作品面板，并通过子组件公开的方法切换到指定详情标签。
 */
function openPanel(tab = 'script') {
  previewOpen.value = true;
  preview.value?.openTab(tab);
}
/**
 * 按版本 ID 加载成片并展开预览；查看历史版本不会把旧草稿写回当前项目。
 */
function openVersion(id: string) {
  void studio.selectVersion(id);
  previewOpen.value = true;
}
/**
 * 切回最近成功的成片，让历史只读状态回到可以继续修改的当前草稿。
 */
function latest() {
  if (current.value?.project.latest_video_version_id)
    void studio.selectVersion(current.value.project.latest_video_version_id);
}
/**
 * 用已保存的脚本和设置发起更新；与“保存设置”分开，避免把旧预览误认为新视频。
 */
function updateVideo() {
  intent.value = versions.value.length ? 'revise' : 'generate';
  void studio.send('normal', '按当前已保存的脚本与设置更新视频。');
}
/**
 * 打开导出弹窗并清空上一次导出状态；默认清晰度来自服务能力。
 */
function openExport() {
  exportedAsset.value = '';
  exportJobId.value = '';
  exportError.value = '';
  exportResolution.value = caps.value.exports.preview_resolution;
  modal.value = 'export';
}
/**
 * 导出当前查看的版本。缓存文件直接提供下载；异步结果通过任务 ID 持续追踪。
 * 导出基于成片版本快照，不把后续未生成的草稿修改混入这个文件。
 */
async function exportVideo() {
  if (!selectedVersion.value) return;
  modalPending.value = true;
  exportError.value = '';
  try {
    const result = await studio.api.exportVideo(
      projectId.value,
      selectedVersion.value.id,
      exportResolution.value,
    );
    if (result.asset_id) {
      exportedAsset.value = result.asset_id;
      modalPending.value = false;
    } else if (result.job_id) {
      exportJobId.value = result.job_id;
      studio.trackJob(result.job_id);
      await studio.refresh(projectId.value);
    }
  } catch (error) {
    exportError.value = error instanceof Error ? error.message : '导出暂时失败。';
    modalPending.value = false;
  }
}
/**
 * 先收起手机端作品和导航面板，再进入空白创作页；此时不创建后端空项目。
 */
async function newProject() {
  previewOpen.value = false;
  sidebarOpen.value = false;
  await studio.newProject();
}
/**
 * 在空白创作中填入可编辑的模板提示词，仅填输入框，不自动启动生成。
 */
async function useTemplate(text: string) {
  modal.value = null;
  await newProject();
  studio.useExample(text);
  conversation.value?.focus();
}
/**
 * 把镜头编辑建议送到对话输入框，并聚焦输入，供用户补全后发送。
 */
function prompt(text: string) {
  studio.useExample(text);
  previewOpen.value = false;
  conversation.value?.focus();
}
/**
 * 非提交状态下关闭弹窗；提交中保留弹窗，避免同一个动作被反复触发。
 */
function closeModal() {
  if (!modalPending.value) modal.value = null;
}
/**
 * 从帮助面板启动失败或追问演示；页面只在演示模式展示此入口。
 */
async function scenario(kind: 'failure' | 'question') {
  modal.value = null;
  await studio.scenario(kind);
}
</script>
<template>
  <div
    class="studio-app"
    @keydown.esc="
      moreOpen = false;
      sidebarOpen = false;
    "
  >
    <!-- 左侧负责项目导航；回传的动作都进入父级流程，不在导航组件内写业务数据。 -->
    <Sidebar
      :projects="projects"
      :selected-id="projectId"
      :open="sidebarOpen"
      @new="newProject"
      @select="router.push(`/projects/${$event}`)"
      @templates="modal = 'templates'"
      @guide="modal = 'guide'"
      @rename="openManage('rename', $event)"
      @delete="openManage('delete', $event)"
      @close="sidebarOpen = false"
    />
    <!-- 中间对话和右侧作品共享 useStudio 数据，通过事件完成三栏联动。 -->
    <div class="workspace">
      <header class="topbar">
        <div class="breadcrumbs">
          <button
            class="icon-button mobile-menu"
            aria-label="打开项目导航"
            @click="sidebarOpen = true"
          >
            <Icon name="menu" />
          </button>
          <span class="breadcrumb-home">创作空间</span>
          <Icon
            class="breadcrumb-chevron"
            name="chevron"
            :size="13"
          />
          <button
            class="project-title"
            :disabled="!current"
            @click="current && openManage('rename', current.project)"
          >
            {{ title }}
            <Icon
              name="edit"
              :size="13"
            />
          </button>
        </div>
        <div class="topbar-actions">
          <span
            class="saved-status"
            :class="{ reconnecting: connection === 'reconnecting' }"
          >
            <Icon
              :name="connection === 'reconnecting' ? 'refresh' : 'cloud'"
              :size="16"
            />
            {{
              connection === 'reconnecting'
                ? '正在恢复连接'
                : pending
                  ? '正在保存'
                  : studio.api.mode === 'demo'
                    ? '已保存到本机'
                    : '已自动保存'
            }}
          </span>
          <button
            v-if="showDemo"
            class="mode-badge"
            @click="modal = 'guide'"
          >
            <span />
            {{ backendDemo ? '后端演示' : '演示模式' }}
          </button>
          <button
            class="icon-button top-help"
            aria-label="查看使用指南"
            @click="modal = 'guide'"
          >
            <Icon
              name="help"
              :size="19"
            />
          </button>
          <button
            class="icon-button preview-toggle"
            aria-label="打开作品预览"
            :aria-expanded="previewOpen"
            @click="previewOpen = !previewOpen"
          >
            <Icon
              name="video"
              :size="20"
            />
          </button>
          <div class="more-container">
            <button
              class="icon-button"
              aria-label="更多项目操作"
              :aria-expanded="moreOpen"
              @click="moreOpen = !moreOpen"
            >
              <Icon name="more" />
            </button>
            <div
              v-if="moreOpen"
              class="toolbar-menu"
            >
              <button
                @click="
                  modal = 'guide';
                  moreOpen = false;
                "
              >
                <Icon
                  name="help"
                  :size="15"
                />
                使用指南
              </button>
              <button
                v-if="current"
                @click="openManage('rename', current.project)"
              >
                <Icon
                  name="edit"
                  :size="15"
                />
                重命名项目
              </button>
              <button
                v-if="current"
                class="danger-text"
                @click="openManage('delete', current.project)"
              >
                <Icon
                  name="trash"
                  :size="15"
                />
                删除项目
              </button>
            </div>
          </div>
        </div>
      </header>
      <div class="mobile-workspace-tabs">
        <button
          :class="{ active: !previewOpen }"
          @click="previewOpen = false"
        >
          <Icon
            name="sparkles"
            :size="15"
          />
          创作对话
        </button>
        <button
          :class="{ active: previewOpen }"
          @click="previewOpen = true"
        >
          <Icon
            name="video"
            :size="15"
          />
          我的作品
          <span
            v-if="versions.length"
            class="mobile-count"
          >
            {{ versions.length }}
          </span>
        </button>
      </div>
      <div
        class="workspace-content"
        :class="{ 'show-preview': previewOpen }"
      >
        <div
          v-if="loading || pageError"
          class="workspace-placeholder"
        >
          <template v-if="loading">
            <span class="spinner" />
            <strong>正在打开你的创作空间</strong>
            <span>让灵感接着发生</span>
          </template>
          <template v-else>
            <Icon
              name="folder"
              :size="38"
            />
            <strong>{{ pageError }}</strong>
            <button
              class="button primary"
              @click="studio.newProject"
            >
              开始新的创作
              <Icon
                name="plus"
                :size="15"
              />
            </button>
          </template>
        </div>
        <Conversation
          v-else
          ref="conversation"
          :messages="messages"
          :jobs="jobs"
          :draft="current?.draft ?? null"
          :versions="versions"
          :input="input"
          :settings="settings"
          :intent="intent"
          :caps="caps"
          :assets="assets"
          :selected-assets="selectedAssets"
          :busy="busy"
          :pending="pending"
          :uploading="uploading"
          :historical="historical"
          :demo="showDemo"
          @update:input="input = $event"
          @update:intent="intent = $event"
          @update:settings="Object.assign(settings, $event)"
          @send="studio.send()"
          @upload="studio.upload"
          @remove-asset="selectedAssets = selectedAssets.filter((id) => id !== $event)"
          @stop="activeJob && studio.jobAction('cancel', activeJob.id)"
          @retry="studio.jobAction('retry', $event)"
          @answer="(jobId, value) => studio.jobAction('answer', jobId, value)"
          @example="studio.useExample"
          @view="openVersion"
          @panel="openPanel"
          @latest="latest"
        />
        <button
          v-if="previewOpen"
          class="preview-scrim"
          aria-label="关闭作品预览"
          @click="previewOpen = false"
        />
        <PreviewPanel
          ref="preview"
          :project-id="projectId"
          :draft="current?.draft ?? null"
          :revision="current?.project.draft_revision ?? 1"
          :dirty="current?.project.draft_dirty ?? false"
          :version="selectedVersion"
          :versions="versions"
          :latest-version-id="current?.project.latest_video_version_id ?? null"
          :assets="assets"
          :caps="caps"
          :job="activeJob"
          :busy="busy"
          :pending="pending"
          :open="previewOpen"
          :on-save="studio.save"
          @select="studio.selectVersion"
          @download="studio.download"
          @export="openExport"
          @upload="conversation?.attach()"
          @update="updateVideo"
          @close="previewOpen = false"
          @latest="latest"
          @playing="playing = $event"
          @prompt="prompt"
          @notify="studio.notify($event, 'info')"
        />
      </div>
    </div>
    <Transition name="toast">
      <div
        v-if="toast"
        class="toast-message"
        :class="toast.type"
        role="status"
      >
        <Icon
          :name="toast.type === 'error' ? 'info' : toast.type === 'success' ? 'check' : 'sparkles'"
          :size="17"
        />
        <span>{{ toast.text }}</span>
        <button
          aria-label="关闭通知"
          @click="toast = null"
        >
          <Icon
            name="close"
            :size="14"
          />
        </button>
      </div>
    </Transition>
    <!-- 弹窗统一复用 Modal：具体内容与确认按钮由页面传入。 -->
    <Modal
      :open="modal === 'templates'"
      title="让灵感，先走一步"
      subtitle="选一个方向，写下属于你的故事。"
      wide
      @close="closeModal"
    >
      <div class="template-grid">
        <button
          v-for="(theme, key) in themes"
          :key="key"
          class="template-card"
          @click="useTemplate(theme.prompt)"
        >
          <div>
            <img
              :src="imagePath(theme.images[0])"
              :alt="theme.label"
            />
            <span>{{ theme.label }}</span>
          </div>
          <strong>{{ theme.title }}</strong>
          <p>{{ theme.summary }}</p>
          <span class="text-button">
            使用这个灵感
            <Icon
              name="arrowRight"
              :size="14"
            />
          </span>
        </button>
      </div>
    </Modal>
    <Modal
      :open="modal === 'guide'"
      title="你的第一支视频，从一句话开始"
      subtitle="不必懂剪辑，有想法就够了。"
      @close="closeModal"
    >
      <div class="guide-steps">
        <div>
          <span>01</span>
          <section>
            <h3>告诉灵镜，你的想法</h3>
            <p>说清主题和想要的感觉，也可以带上自己的图片、视频或音乐。</p>
          </section>
        </div>
        <div>
          <span>02</span>
          <section>
            <h3>看着故事慢慢成形</h3>
            <p>从脚本分镜到画面声音，制作进度会在对话里逐步呈现。</p>
          </section>
        </div>
        <div>
          <span>03</span>
          <section>
            <h3>随时调整，直到满意</h3>
            <p>直接说“第二个镜头换一下”，或在右侧保存设置，再更新视频。</p>
          </section>
        </div>
      </div>
      <div
        v-if="showDemo"
        class="demo-explanation"
      >
        <Icon
          name="info"
          :size="18"
        />
        <p v-if="backendDemo">
          已连接 Rust
          后端，创作记录和上传文件保存在服务端。文案、画面、配音与音乐目前由演示模型提供，成片使用内置的
          30 秒示例视频。
        </p>
        <p v-else>
          当前是可交互演示版。创作记录和上传文件保存在本机，视频使用内置的 30
          秒示例画面与音乐，尚未调用真实 AI 生成服务。
        </p>
      </div>
      <div
        v-if="showDemo"
        class="demo-scenarios"
      >
        <span>也可以体验制作中的小插曲</span>
        <div>
          <button @click="scenario('failure')">
            <Icon
              name="refresh"
              :size="15"
            />
            失败后继续制作
          </button>
          <button @click="scenario('question')">
            <Icon
              name="help"
              :size="15"
            />
            补充信息后继续
          </button>
        </div>
      </div>
      <template #footer>
        <button
          class="button primary"
          @click="modal = null"
        >
          知道了，开始创作
          <Icon
            name="arrowRight"
            :size="15"
          />
        </button>
      </template>
    </Modal>
    <Modal
      :open="modal === 'rename'"
      title="给故事一个新名字"
      subtitle="方便下次一眼找到它。"
      :locked="modalPending"
      @close="closeModal"
    >
      <form
        id="rename-form"
        @submit.prevent="confirmManage"
      >
        <label class="field-label">
          <span>项目名称</span>
          <input
            v-model="titleInput"
            autofocus
            maxlength="100"
            placeholder="输入项目名称"
            required
          />
        </label>
      </form>
      <template #footer>
        <button
          class="button secondary"
          :disabled="modalPending"
          @click="closeModal"
        >
          取消
        </button>
        <button
          class="button primary"
          form="rename-form"
          type="submit"
          :disabled="modalPending || !titleInput.trim()"
        >
          保存名称
        </button>
      </template>
    </Modal>
    <Modal
      :open="modal === 'delete'"
      title="删除这份创作？"
      :locked="modalPending"
      @close="closeModal"
    >
      <p class="delete-description">
        “{{ managedProject?.title }}”的对话、草稿和视频版本将一起删除。此操作无法撤回。
      </p>
      <p
        v-if="managedProject?.active_job_id"
        class="error-inline"
      >
        该项目仍在制作中，请先停止任务再删除。
      </p>
      <template #footer>
        <button
          class="button secondary"
          :disabled="modalPending"
          @click="closeModal"
        >
          保留创作
        </button>
        <button
          class="button danger"
          :disabled="modalPending || !!managedProject?.active_job_id"
          @click="confirmManage"
        >
          确认删除
        </button>
      </template>
    </Modal>
    <Modal
      :open="modal === 'export'"
      :title="exportedAsset ? '你的故事，准备好被看见了' : '把作品带走'"
      :subtitle="`导出当前查看的 v${selectedVersion?.version_number ?? 1} 版本`"
      :locked="modalPending && !exportJobId"
      @close="closeModal"
    >
      <div
        v-if="exportedAsset"
        class="export-success"
      >
        <span>
          <Icon
            name="check"
            :size="30"
          />
        </span>
        <h3>视频文件已准备就绪</h3>
        <p>MP4 · {{ exportResolution }} · 30 fps</p>
        <button
          class="button primary"
          @click="studio.download(exportedAsset)"
        >
          <Icon
            name="download"
            :size="17"
          />
          下载视频
        </button>
      </div>
      <template v-else>
        <label class="field-label">
          <span>视频清晰度</span>
          <select
            v-model="exportResolution"
            :disabled="modalPending"
          >
            <option
              v-for="resolution in caps.exports.resolutions"
              :key="resolution"
              :value="resolution"
            >
              {{ resolution }} · 清晰流畅
            </option>
          </select>
        </label>
        <div class="export-format">
          <span>
            文件格式
            <strong>MP4</strong>
          </span>
          <span>
            帧率
            <strong>30 fps</strong>
          </span>
          <span>
            作品时长
            <strong>{{ Math.round((selectedVersion?.duration_ms ?? 30000) / 1000) }} 秒</strong>
          </span>
        </div>
        <p class="export-note">
          <Icon
            name="info"
            :size="15"
          />
          导出保留这个版本的画面与设置。
        </p>
        <div
          v-if="modalPending"
          class="export-progress"
        >
          <span class="spinner" />
          {{ exportJob?.current_stage ? '正在渲染视频文件…' : '正在准备视频文件…' }}
        </div>
        <p
          v-if="exportError"
          class="error-inline"
        >
          {{ exportError }}
        </p>
      </template>
      <template #footer>
        <button
          class="button secondary"
          :disabled="modalPending"
          @click="closeModal"
        >
          {{ exportedAsset ? '完成' : '取消' }}
        </button>
        <button
          v-if="!exportedAsset"
          class="button primary"
          :disabled="modalPending"
          @click="exportVideo"
        >
          <Icon
            name="export"
            :size="16"
          />
          {{ modalPending ? '准备中…' : '导出视频' }}
        </button>
      </template>
    </Modal>
  </div>
</template>
