import { computed, onBeforeUnmount, reactive, ref, watch } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { api, isActive } from '../services/api.ts';
import { capabilities as defaults, copy } from '../domain/content.ts';
import { makeId } from '../domain/engine.ts';
import type {
  Asset,
  Capabilities,
  DemoScenario,
  DraftChanges,
  Intent,
  Job,
  Project,
  Settings,
  Snapshot,
  VideoVersion,
} from '../domain/types.ts';

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
  const projectId = computed(() =>
    typeof route.params.projectId === 'string' ? route.params.projectId : '',
  );
  const activeJob = computed(() => current.value?.active_job ?? null);
  const busy = computed(() => isActive(activeJob.value));
  const historical = computed(
    () =>
      !!selectedVersion.value &&
      selectedVersion.value.id !== current.value?.project.latest_video_version_id,
  );
  const messages = computed(() => current.value?.messages_page.items ?? []);
  let stopWatching = () => {},
    requestSequence = 0,
    toastTimer: ReturnType<typeof setTimeout>;
  let reconnectTimer: ReturnType<typeof setTimeout> | undefined;
  let reconnectAttempts = 0;
  const trackedJobs = new Map<string, string>();
  const localInputs = new Map<string, { text: string; ids: string[]; intent: Intent }>();
  let previousRoute = '';
  function notify(text: string, type: 'success' | 'error' | 'info' = 'success') {
    toast.value = { text, type };
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => {
      toast.value = null;
    }, 4600);
  }
  function errorText(error: unknown) {
    return error instanceof Error ? error.message : '操作暂时没有完成，请稍后再试。';
  }
  function stop() {
    stopWatching();
    stopWatching = () => {};
    clearTimeout(reconnectTimer);
  }
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
  async function refresh(id: string, initial = false) {
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
  void api
    .getCapabilities()
    .then((value) => {
      caps.value = value;
    })
    .catch((error) => notify(errorText(error), 'error'));
  const storageListener = () => {
    if (projectId.value) void refresh(projectId.value);
    else
      void api.listProjects().then((value) => {
        projects.value = value;
      });
  };
  window.addEventListener('storage', storageListener);
  onBeforeUnmount(() => {
    stop();
    clearTimeout(toastTimer);
    window.removeEventListener('storage', storageListener);
  });

  async function ensureProject() {
    if (projectId.value) return projectId.value;
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
  async function send(scenario: DemoScenario = 'normal', overrideText?: string) {
    const text = (overrideText ?? input.value).trim();
    if (!text || busy.value || pending.value || uploading.value) return;
    if (historical.value) {
      notify('请先回到当前草稿，再基于最新版本修改。', 'info');
      return;
    }
    pending.value = true;
    const nextSettings = copy(settings),
      chosen = [...selectedAssets.value],
      mode = intent.value;
    try {
      const id = await ensureProject();
      const state = await api.getSnapshot(id);
      if (JSON.stringify(state.draft.settings) !== JSON.stringify(nextSettings) && mode !== 'chat')
        await api.patchDraft(id, { settings: nextSettings }, state.project.draft_revision);
      const latest = await api.getSnapshot(id);
      await api.sendMessage(
        id,
        {
          intent: mode === 'chat' ? 'chat' : latest.latest_video ? 'revise' : 'generate',
          text,
          asset_ids: chosen,
          expected_draft_revision: latest.project.draft_revision,
          base_video_version_id: mode === 'chat' ? null : latest.project.latest_video_version_id,
        },
        makeId('request'),
        scenario,
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
  async function upload(files: FileList | File[]) {
    if (uploading.value || pending.value) return;
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
  function useExample(text: string) {
    input.value = text;
    intent.value = current.value?.latest_video ? 'revise' : 'generate';
  }
  async function newProject() {
    await router.push('/create');
  }
  async function scenario(kind: DemoScenario) {
    await newProject();
    input.value = '帮我做一个温暖的咖啡店宣传视频。';
    await send(kind);
  }
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
    scenario,
    trackJob,
  };
}
