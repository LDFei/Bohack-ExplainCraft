/**
 * 组合式状态层：统一协调工作台数据和异步动作，防止页面组件各自维护互相冲突的项目状态。
 */
import { computed, onBeforeUnmount, reactive, ref, watch } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { api, isActive } from '../services/api.ts';
import {
  capabilities as defaults,
  copy,
  durationTooLong,
  requestedDurationSeconds,
} from '../domain/content.ts';
import { makeId } from '../domain/id.ts';
import type {
  Asset,
  Capabilities,
  DraftChanges,
  Intent,
  Job,
  Project,
  Settings,
  Snapshot,
  VideoVersion,
} from '../domain/types.ts';

/**
 * 工作台的状态与操作中心：连接路由、页面组件及统一 StudioApi。
 * 组件主要负责显示和发事件；项目加载、提交、上传、恢复订阅等流程都集中在这里。
 */
export function useStudio() {
  const route = useRoute(),
    router = useRouter();
  const projects = ref<Project[]>([]),
    current = ref<Snapshot | null>(null);
  const assets = ref<Asset[]>([]),
    versions = ref<VideoVersion[]>([]),
    jobs = ref<Record<string, Job>>({});
  const caps = ref<Capabilities>(defaults),
    selectedVersion = ref<VideoVersion | null>(null);
  const loading = ref(true),
    pending = ref(false),
    pageError = ref(''),
    input = ref('');
  const selectedAssets = ref<string[]>([]),
    intent = ref<Intent>('generate');
  const settings = reactive<Settings>({
    target_duration_seconds: 30,
    aspect_ratio: '9:16',
    style_id: 'warm',
    language: 'zh-CN',
  });
  const uploading = ref<{ name: string; percent: number } | null>(null);
  const toast = ref<{ text: string; type: 'success' | 'error' | 'info' } | null>(null);
  const connection = ref<'connected' | 'reconnecting'>('connected'),
    playing = ref(false);
  // 路由参数是当前项目的唯一来源；空字符串表示尚未实际创建项目的 /create 页面。
  const projectId = computed(() =>
    typeof route.params.projectId === 'string' ? route.params.projectId : '',
  );
  const activeJob = computed(() => current.value?.active_job ?? null);
  const busy = computed(() => isActive(activeJob.value));
  // 查看中的成片不等于最近成片时，进入只读历史视图，阻止把修改意见发给错误版本。
  const historical = computed(
    () =>
      !!selectedVersion.value &&
      selectedVersion.value.id !== current.value?.project.latest_video_version_id,
  );
  const messages = computed(() => current.value?.messages_page.items ?? []);
  /**
   * 订阅尚未建立时使用空清理函数，使路由切换和组件卸载都能安全调用同一个入口。
   */
  let stopWatching = () => {},
    requestSequence = 0,
    toastTimer: ReturnType<typeof setTimeout>;
  let reconnectTimer: ReturnType<typeof setTimeout> | undefined;
  let reconnectAttempts = 0;
  // 导出可能没有聊天消息；单独保存任务归属，任务结束后仍能查询最终素材 ID。
  const trackedJobs = new Map<string, string>();
  // 页面内临时保存每个项目尚未发送的输入；这是内存 Map，不会在整页刷新后保留。
  const localInputs = new Map<string, { text: string; ids: string[]; intent: Intent }>();
  let previousRoute = '';
  /**
   * 显示一条短提示，并替换上一条提示的消失计时器。
   */
  function notify(text: string, type: 'success' | 'error' | 'info' = 'success') {
    toast.value = { text, type };
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => {
      toast.value = null;
    }, 4600);
  }
  /**
   * 把未知错误收敛成可以展示给用户的文字，避免模板直接处理异常对象。
   */
  function errorText(error: unknown) {
    return error instanceof Error ? error.message : '操作暂时没有完成，请稍后再试。';
  }
  /**
   * 清理当前订阅及重连计时器；这里只停止页面监听，不会取消后端生成任务。
   */
  function stop() {
    stopWatching();
    stopWatching = () => {};
    clearTimeout(reconnectTimer);
  }
  /**
   * 从快照游标开始监听变更；收到变更先关闭旧流，再刷新快照。
   * 监听失败时按指数退避重连，上限 15 秒，避免网络故障造成密集请求。
   */
  function subscribe(id: string, cursor: string) {
    stopWatching();
    stopWatching = api.watch(
      id,
      cursor,
      () => {
        stopWatching();
        void refresh(id, false);
      },
      () => {
        stopWatching();
        connection.value = 'reconnecting';
        reconnectTimer = setTimeout(
          () => {
            if (projectId.value === id) void refresh(id, false);
          },
          Math.min(15000, 1000 * 2 ** reconnectAttempts++),
        );
      },
    );
  }
  /**
   * 并行读取一个项目的快照、素材、版本和历史列表，再补齐消息关联的任务。
   * 返回前校验请求序号和路由 ID，防止旧项目的迟到响应覆盖新页面；最后从新游标重新订阅。
   */
  async function refresh(id: string, initial = false) {
    // 每轮刷新领取序号；即使旧请求无法立刻中断，返回时也不能覆盖新项目。
    const sequence = ++requestSequence;
    if (initial) {
      loading.value = true;
      pageError.value = '';
    }
    stop();
    try {
      const [next, nextAssets, nextVersions, nextProjects] = await Promise.all([
        api.getSnapshot(id),
        api.listAssets(id),
        api.listVersions(id),
        api.listProjects(),
      ]);
      const jobIds = [
        ...new Set([
          ...next.messages_page.items
            .map((item) => item.job_id)
            .filter((value): value is string => !!value),
          ...[...trackedJobs.entries()].filter(([, owner]) => owner === id).map(([jobId]) => jobId),
        ]),
      ];
      const jobResults = await Promise.all(
        jobIds.map(async (jobId) => {
          try {
            return await api.getJob(jobId);
          } catch {
            return null;
          }
        }),
      );
      // 只有最新一轮、且仍属于当前路由的响应可以写回界面。
      if (sequence !== requestSequence || projectId.value !== id) return;
      const oldLatest = current.value?.project.latest_video_version_id;
      const wasCurrent = !selectedVersion.value || selectedVersion.value.id === oldLatest;
      current.value = next;
      assets.value = nextAssets;
      versions.value = nextVersions;
      projects.value = nextProjects;
      jobs.value = Object.fromEntries(
        jobResults.filter((job): job is Job => !!job).map((job) => [job.id, job]),
      );
      if (next.active_job) jobs.value[next.active_job.id] = next.active_job;
      // 用户正在播放时保留当前片段；新版完成只提示，不突然切走正在看的作品。
      if (initial || !selectedVersion.value || (wasCurrent && !playing.value))
        selectedVersion.value = next.latest_video;
      if (initial) {
        Object.assign(settings, next.draft.settings);
        intent.value = next.latest_video ? 'revise' : 'generate';
      }
      if (oldLatest && oldLatest !== next.project.latest_video_version_id && playing.value)
        notify('新版本已完成，可以在作品面板切换查看。', 'info');
      pageError.value = '';
      connection.value = 'connected';
      reconnectAttempts = 0;
      subscribe(id, next.event_cursor);
    } catch (error) {
      if (sequence !== requestSequence || projectId.value !== id) return;
      if (initial) pageError.value = errorText(error);
      else {
        connection.value = 'reconnecting';
        reconnectTimer = setTimeout(() => {
          if (projectId.value === id) void refresh(id);
        }, 3000);
      }
    } finally {
      if (sequence === requestSequence) loading.value = false;
    }
  }
  // 路由变化：先保存旧项目输入、清掉旧订阅与状态，再加载新项目；immediate 负责首次进入。
  watch(
    projectId,
    async (id) => {
      if (previousRoute)
        localInputs.set(previousRoute, {
          text: input.value,
          ids: [...selectedAssets.value],
          intent: intent.value,
        });
      previousRoute = id || 'create';
      const saved = localInputs.get(previousRoute);
      input.value = saved?.text ?? '';
      selectedAssets.value = saved?.ids ?? [];
      intent.value = saved?.intent ?? 'generate';
      stop();
      requestSequence++;
      current.value = null;
      selectedVersion.value = null;
      assets.value = [];
      versions.value = [];
      jobs.value = {};
      playing.value = false;
      pageError.value = '';
      if (id) {
        await refresh(id, true);
        if (saved) intent.value = saved.intent;
      } else {
        Object.assign(settings, {
          target_duration_seconds: caps.value.defaults.target_duration_seconds,
          aspect_ratio: caps.value.defaults.aspect_ratio,
          style_id: caps.value.defaults.style_id,
          language: caps.value.defaults.language,
        });
        loading.value = false;
        try {
          projects.value = await api.listProjects();
        } catch (error) {
          notify(errorText(error), 'error');
        }
      }
    },
    { immediate: true },
  );
  // 能力读取失败时保留初始默认值并提示。
  void api
    .getCapabilities()
    .then((value) => {
      caps.value = value;
    })
    .catch((error) => notify(errorText(error), 'error'));
  /**
   * 响应其他标签页对本机数据的修改，刷新当前项目或历史列表。
   */
  const storageListener = () => {
    if (projectId.value) void refresh(projectId.value);
    else
      void api.listProjects().then((value) => {
        projects.value = value;
      });
  };
  window.addEventListener('storage', storageListener);
  // 组件销毁时释放监听和计时器，避免离开工作台后继续更新页面。
  onBeforeUnmount(() => {
    stop();
    clearTimeout(toastTimer);
    window.removeEventListener('storage', storageListener);
  });

  /**
   * 首次发送或上传时才创建项目，返回可以调用接口的项目 ID。
   * 切路由会初始化页面，因此先暂存输入、模式和设置，再在新项目载入后恢复。
   */
  async function ensureProject(fresh = false) {
    // fresh：在已有成片的项目里选择「新做一个」时另建项目，新视频从空白草稿开始。
    if (!fresh && projectId.value) return projectId.value;
    const text = input.value,
      nextSettings = copy(settings),
      mode = intent.value;
    const project = await api.createProject();
    await router.push(`/projects/${project.id}`);
    await refresh(project.id, true);
    input.value = text;
    Object.assign(settings, nextSettings);
    intent.value = mode;
    return project.id;
  }
  /**
   * 发送一次生成、修改或讨论需求：必要时建项目，先保存面板设置，再提交消息。
   * 提交使用最新修订号和最新成片 ID；完成后只清理仍属于本项目的输入，避免串到其他项目。
   */
  async function send(overrideText?: string) {
    const text = (overrideText ?? input.value).trim();
    if (!text || busy.value || pending.value || uploading.value) return;
    if (durationTooLong(text)) {
      notify('单个作品最长支持 300 秒，请缩短目标时长后重试。', 'error');
      return;
    }
    if (historical.value) {
      notify('请先回到当前草稿，再基于最新版本修改。', 'info');
      return;
    }
    pending.value = true;
    const nextSettings = copy(settings),
      chosen = [...selectedAssets.value],
      mode = intent.value;
    nextSettings.target_duration_seconds =
      requestedDurationSeconds(text) ?? nextSettings.target_duration_seconds;
    try {
      // 「新做一个」必须落在新项目里；否则会沿用当前项目的草稿和成片上下文。
      const id = await ensureProject(mode === 'generate' && !!current.value?.latest_video);
      const state = await api.getSnapshot(id);
      // 参数保存会递增草稿版本，所以发送消息前必须再次取最新快照。
      if (JSON.stringify(state.draft.settings) !== JSON.stringify(nextSettings) && mode !== 'chat')
        await api.patchDraft(id, { settings: nextSettings }, state.project.draft_revision);
      const latest = await api.getSnapshot(id);
      await api.sendMessage(
        id,
        {
          // 仅讨论显式优先；修改沿用成片，其余按首次生成处理（新项目里没有成片）。
          intent: mode === 'chat' ? 'chat' : mode === 'revise' ? 'revise' : 'generate',
          text,
          asset_ids: chosen,
          expected_draft_revision: latest.project.draft_revision,
          base_video_version_id: mode === 'chat' ? null : latest.project.latest_video_version_id,
        },
        makeId('request'),
      );
      if (projectId.value === id) {
        input.value = '';
        selectedAssets.value = [];
        await refresh(id);
      }
    } catch (error) {
      notify(errorText(error), 'error');
    } finally {
      pending.value = false;
    }
  }
  /**
   * 保存子组件提交的草稿分组，并返回成功标记；只有成功才允许子组件退出编辑状态。
   * revision 来自编辑开始时，不能擅自换成最新值，否则会绕开多窗口冲突保护。
   */
  async function save(changes: DraftChanges, revision: number): Promise<boolean> {
    if (!projectId.value || pending.value) return false;
    pending.value = true;
    try {
      await api.patchDraft(projectId.value, changes, revision);
      await refresh(projectId.value);
      Object.assign(settings, current.value?.draft.settings);
      notify('修改已保存，更新视频后生效。');
      return true;
    } catch (error) {
      notify(errorText(error), 'error');
      return false;
    } finally {
      pending.value = false;
    }
  }
  /**
   * 逐个上传素材，展示进度并保留单文件错误；一个失败不阻断其余文件。
   * 上传前复制文件列表，上传后核对路由；切走时附件仍保存到原项目，不会挂到新项目上。
   */
  async function upload(files: FileList | File[]) {
    if (uploading.value || pending.value) return;
    // 原生文件列表是实时对象，跨 await 前先复制，否则控件清空后可能读不到文件。
    const chosenFiles = Array.from(files);
    pending.value = true;
    try {
      const id = await ensureProject();
      for (const file of chosenFiles) {
        uploading.value = { name: file.name, percent: 0 };
        try {
          const asset = await api.upload(id, file, (percent) => {
            if (uploading.value) uploading.value.percent = percent;
          });
          if (projectId.value === id) selectedAssets.value.push(asset.id);
        } catch (error) {
          notify(errorText(error), 'error');
        }
      }
      if (projectId.value === id) await refresh(id);
      else notify('附件已保存到原来的项目。', 'info');
    } catch (error) {
      notify(errorText(error), 'error');
    } finally {
      uploading.value = null;
      pending.value = false;
    }
  }
  /**
   * 统一处理停止、重试和回答追问，再刷新所属项目；pending 用来阻止重复点击。
   */
  async function jobAction(action: 'cancel' | 'retry' | 'answer', jobId: string, value?: string) {
    if (pending.value) return;
    const id = projectId.value;
    pending.value = true;
    try {
      if (action === 'cancel') await api.cancel(jobId);
      else if (action === 'retry') await api.retry(jobId, current.value!.project.draft_revision);
      else await api.answer(jobId, jobs.value[jobId]!.question!.id, value ?? '');
      if (projectId.value === id) await refresh(id);
    } catch (error) {
      notify(errorText(error), 'error');
    } finally {
      pending.value = false;
    }
  }
  /**
   * 加载指定成片快照并切换播放器；再次核对项目归属，避免切路由后展示旧结果。
   */
  async function selectVersion(id: string) {
    try {
      const version = await api.getVersion(projectId.value, id);
      if (version.project_id === projectId.value) {
        selectedVersion.value = version;
        playing.value = false;
      }
    } catch (error) {
      notify(errorText(error), 'error');
    }
  }
  /**
   * 向服务层获取下载地址，通过临时 a 标签发起浏览器下载，不把文件内容塞进页面状态。
   */
  async function download(assetId?: string) {
    if (!assetId) return;
    try {
      const file = await api.download(assetId);
      const anchor = document.createElement('a');
      anchor.href = file.download_url;
      anchor.download = file.file_name;
      anchor.rel = 'noopener';
      document.body.append(anchor);
      anchor.click();
      anchor.remove();
      notify('文件已准备好，开始下载。');
    } catch (error) {
      notify(errorText(error), 'error');
    }
  }
  /**
   * 修改项目名称，随后同步历史列表及当前项目标题；失败时返回 false 以保留弹窗。
   */
  async function rename(id: string, title: string) {
    try {
      await api.renameProject(id, title);
      projects.value = await api.listProjects();
      if (projectId.value === id) await refresh(id);
      notify('项目名称已更新。');
      return true;
    } catch (error) {
      notify(errorText(error), 'error');
      return false;
    }
  }
  /**
   * 删除项目并更新历史；删除的是当前项目时回到空白创作页，失败则保留现有界面。
   */
  async function remove(id: string) {
    try {
      await api.deleteProject(id);
      if (projectId.value === id) await router.push('/create');
      projects.value = await api.listProjects();
      notify('项目已删除。');
      return true;
    } catch (error) {
      notify(errorText(error), 'error');
      return false;
    }
  }
  /**
   * 只填入示例或修改建议；是否创建任务仍由用户发送决定。
   */
  function useExample(text: string) {
    input.value = text;
    intent.value = current.value?.latest_video ? 'revise' : 'generate';
  }
  /**
   * 导航到空白创作页；延迟到首次发送或上传再创建实际项目，减少空白历史。
   */
  async function newProject() {
    await router.push('/create');
  }
  /**
   * 记录不一定出现在消息中的额外任务，例如导出；终态后也继续查询它以读取下载结果。
   */
  function trackJob(jobId: string) {
    trackedJobs.set(jobId, projectId.value);
  }
  return {
    api,
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
    notify,
    refresh,
    send,
    save,
    upload,
    jobAction,
    selectVersion,
    download,
    rename,
    remove,
    useExample,
    newProject,
    trackJob,
  };
}
