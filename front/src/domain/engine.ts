/**
 * 演示任务状态机。通过显式 now 推进项目记录，方便刷新恢复与确定性测试；不执行真实 AI 生成。
 */
import {
  copy,
  dimensions,
  emptyDraft,
  imagePath,
  inferTheme,
  stageKeys,
  stageLabels,
  themes,
  videoPath,
} from './content.ts';
import type {
  Database,
  DemoScenario,
  DraftChanges,
  Job,
  Message,
  MessageInput,
  ProjectRecord,
  Snapshot,
  Theme,
  VideoVersion,
} from './types.ts';

/** 业务异常同时携带稳定 code 与中文 message，让界面可以解释失败而不展示技术堆栈。 */
export class StudioError extends Error {
  code: string;
  /**
   * 创建带业务错误码的异常；页面使用 message 提示用户，code 用于判断冲突或可重试原因。
   */
  constructor(code: string, message: string) {
    super(message);
    this.code = code;
  }
}
/**
 * 组合业务前缀与随机 UUID，避免不同项目、消息和任务使用相同 ID。
 */
export const makeId = (prefix: string) => `${prefix}_${crypto.randomUUID()}`;
/**
 * 把毫秒时间戳转换成 UTC ISO 时间字符串，供持久化和接口对象使用。
 */
const iso = (now: number) => new Date(now).toISOString();
/**
 * 判断任务是否仍占用项目：排队、运行、待回答和正在取消均算活动状态。
 */
export const isActive = (job?: Job | null) =>
  !!job && ['queued', 'running', 'waiting_input', 'cancelling'].includes(job.status);
/**
 * 创建一个项目在演示数据库中的完整记录；初始只有空草稿，没有消息、任务或成片。
 */
export function createRecord(now = Date.now(), projectId = makeId('prj')): ProjectRecord {
  return {
    project: {
      id: projectId,
      title: '新的创作',
      status: 'draft',
      draft_revision: 1,
      active_job_id: null,
      latest_video_version_id: null,
      draft_dirty: true,
      created_at: iso(now),
      updated_at: iso(now),
    },
    draft: emptyDraft(),
    messages: [],
    assets: [],
    versions: [],
    jobs: [],
    runs: {},
    event_seq: 0,
  };
}
/**
 * 把用户或助手消息追加到记录并返回新消息；助手先处于 pending，后续由任务推进补全。
 */
function message(
  record: ProjectRecord,
  role: 'user' | 'assistant',
  text: string,
  jobId: string | null,
  now: number,
): Message {
  const result: Message = {
    id: makeId('msg'),
    project_id: record.project.id,
    role,
    text,
    job_id: jobId,
    asset_ids: [],
    blocks: [],
    status: role === 'user' ? 'completed' : 'pending',
    last_delta_seq: 0,
    created_at: iso(now),
  };
  record.messages.push(result);
  return result;
}
/**
 * 拒绝在同一个项目里并发启动任务或修改草稿，避免两次结果相互覆盖。
 */
function assertIdle(record: ProjectRecord) {
  if (record.project.active_job_id)
    throw new StudioError('PROJECT_BUSY', '当前创作仍在进行，请先完成或停止。');
}
/**
 * 检查调用者看到的草稿修订号；过期页面必须先刷新，不能直接覆盖新内容。
 */
function assertRevision(record: ProjectRecord, expected: number) {
  if (expected !== record.project.draft_revision)
    throw new StudioError('REVISION_CONFLICT', '草稿已更新，请刷新后再提交；你的输入会保留。');
}
/**
 * 更新项目修改时间并递增演示事件序号，通知页面有新的持久化状态需要读取。
 */
function touch(record: ProjectRecord, now: number) {
  record.project.updated_at = iso(now);
  record.event_seq++;
}
/**
 * 校验需求和素材后建立排队任务，记录输入快照及开始时间。
 * parent 表示重试来源：重试新建助手回复，但不重复插入原用户消息。
 * @param input 本次消息的完整输入，含期望修订号、素材和修改基底。
 * @param scenario 仅演示使用的正常、失败或追问分支。
 * @returns 已加入记录、处于 queued 状态的新任务。
 */
export function startJob(
  record: ProjectRecord,
  input: MessageInput,
  now: number,
  scenario: DemoScenario = 'normal',
  parent?: Job,
): Job {
  assertIdle(record);
  assertRevision(record, input.expected_draft_revision);
  if (!input.text.trim() || input.text.length > 5000)
    throw new StudioError('VALIDATION_ERROR', '请输入 1～5000 字的创作想法。');
  if (
    input.intent === 'revise' &&
    input.base_video_version_id !== record.project.latest_video_version_id
  )
    throw new StudioError('BASE_VERSION_MISMATCH', '请回到最新版本后继续修改。');
  for (const id of input.asset_ids)
    if (!record.assets.some((asset) => asset.id === id && asset.status === 'ready'))
      throw new StudioError('ASSET_NOT_READY', '请等待所选素材准备完成。');
  const job: Job = {
    id: makeId('job'),
    project_id: record.project.id,
    parent_job_id: parent?.id ?? null,
    kind: input.intent,
    status: 'queued',
    current_stage: null,
    progress_percent: 0,
    stages:
      input.intent === 'chat'
        ? []
        : stageKeys.map((key) => ({
            key,
            status: 'pending',
            progress_percent: 0,
            message: stageLabels[key],
          })),
    question: null,
    result: null,
    error: null,
    created_at: iso(now),
    started_at: null,
    finished_at: null,
  };
  // 重试复用原用户需求，只增加新的助手消息，避免对话里出现重复需求。
  if (!parent) {
    const user = message(record, 'user', input.text.trim(), job.id, now);
    user.asset_ids = [...input.asset_ids];
  }
  const assistant = message(record, 'assistant', '', job.id, now + 1);
  if (input.intent !== 'chat') assistant.blocks = [{ type: 'progress', job_id: job.id }];
  const previous = record.runs[parent?.id ?? ''];
  const theme =
    previous?.theme ??
    (input.intent === 'revise' && record.draft.brief
      ? inferTheme(record.draft.brief.subject)
      : inferTheme(input.text));
  record.runs[job.id] = {
    // 冻结这次任务的输入，避免后续表单继续编辑时悄悄改变已经受理的需求。
    input: copy(input),
    scenario,
    theme,
    started_ms: now,
    answered: false,
    checkpoint_revision: record.project.draft_revision,
  };
  record.jobs.push(job);
  record.project.active_job_id = job.id;
  record.project.status = 'processing';
  if (record.project.title === '新的创作' && input.intent !== 'chat')
    record.project.title = themes[theme].projectTitle;
  touch(record, now);
  return job;
}
/**
 * 把内置主题映射到脚本和分镜，并用有限关键词演示语速、风格或镜头修改。
 * 上传素材在这里建立引用；不会把上传文件真正合成进内置视频。
 */
function prepareDraft(record: ProjectRecord, job: Job, now: number) {
  const run = record.runs[job.id]!;
  const content = themes[run.theme];
  if (!record.draft.script) {
    record.draft.script = { title: content.title, text: content.lines.join('\n') };
  }
  // 保留用户已经保存的文案；只有没有有效分镜时，才根据文案重新规划示例镜头。
  if (!record.draft.shots.length) {
    const lines = record.draft.script.text
      .split(/\n+/)
      .map((line) => line.trim())
      .filter(Boolean)
      .slice(0, 8);
    record.draft.shots = lines.map((line, i) => ({
      id: makeId('shot'),
      duration_ms:
        lines.length === 4
          ? [7000, 8000, 8000, 7000][i]!
          : Math.floor(30000 / lines.length) + (i === lines.length - 1 ? 30000 % lines.length : 0),
      narration: line,
      visual_prompt: content.visuals[i % 4]!,
      shot_size: i === 0 ? '远景' : '特写',
      camera_motion: '缓慢推进',
      asset_ids: [],
      thumbnail_url: imagePath(content.images[i % 4]!),
    }));
  }
  if (!record.draft.brief)
    record.draft.brief = {
      subject: content.projectTitle,
      audience: '热爱生活的年轻人',
      purpose: content.label,
      style: '温暖自然，细腻真实',
    };
  const text = run.input.text;
  // 演示修改仅识别少量关键词并改草稿；真实内容重生成应由 Rust 编排服务负责。
  if (job.kind === 'revise') {
    if (/慢|温柔/.test(text)) record.draft.audio.speech_rate = 0.9;
    if (/轻快|快一点/.test(text)) {
      record.draft.audio.speech_rate = 1.1;
      record.draft.settings.style_id = 'energetic';
    }
    if (/电影/.test(text)) record.draft.settings.style_id = 'cinematic';
    if (/第二|第2/.test(text) && record.draft.shots[1]) {
      record.draft.shots[1].visual_prompt = `根据修改意见调整：${text}`;
      record.draft.shots[1].thumbnail_url = imagePath(content.images[2]!);
    }
  }
  if (/横屏/.test(text)) record.draft.settings.aspect_ratio = '16:9';
  if (/方形/.test(text)) record.draft.settings.aspect_ratio = '1:1';
  const seconds = text.match(/(15|30|60)\s*秒/);
  if (seconds) record.draft.settings.target_duration_seconds = Number(seconds[1]);
  for (const [i, assetId] of run.input.asset_ids.entries()) {
    const asset = record.assets.find((item) => item.id === assetId)!;
    if (asset.kind === 'audio') {
      record.draft.audio.music_mode = 'upload';
      record.draft.audio.music_asset_id = assetId;
      record.draft.audio.music_id = null;
    } else if (record.draft.shots[i]) {
      record.draft.shots[i]!.asset_ids = [assetId];
      if (asset.thumbnail_url) record.draft.shots[i]!.thumbnail_url = asset.thumbnail_url;
    }
  }
  record.project.draft_revision++;
  record.project.draft_dirty = true;
  run.checkpoint_revision = record.project.draft_revision;
  touch(record, now);
}
/**
 * 统一收尾：记录结束时间、释放活动任务并更新项目摘要；已有成片始终保留。
 */
function terminal(record: ProjectRecord, job: Job, now: number) {
  job.finished_at = iso(now);
  record.project.active_job_id = null;
  record.project.status =
    job.status === 'failed' && job.kind !== 'chat'
      ? 'failed'
      : record.project.latest_video_version_id
        ? 'ready'
        : 'draft';
  touch(record, now);
}
/**
 * 按传入时间推进演示状态机，原地更新 record；由快照读取和轮询调用。
 * 约 10 秒后绑定内置成片，追问和故障也由此模拟；不调用真实模型或渲染队列。
 * @param record 当前项目的内部记录，会被原地更新。
 * @param now 当前毫秒时间戳；测试可传入固定时间，页面通常使用 Date.now()。
 */
export function advance(record: ProjectRecord, now: number) {
  const job = record.jobs.find((item) => item.id === record.project.active_job_id);
  if (!job || !isActive(job)) return;
  const run = record.runs[job.id]!;
  const assistant = record.messages.find(
    (item) => item.role === 'assistant' && item.job_id === job.id,
  )!;
  // 先处理取消，再考虑完成：确保取消请求之后的迟到结果不能发布新成片。
  if (job.status === 'cancelling') {
    if (now - (run.cancelled_ms ?? now) < 500) return;
    job.status = 'cancelled';
    job.stages.forEach((stage) => {
      if (stage.status === 'running' || stage.status === 'pending') stage.status = 'cancelled';
    });
    assistant.status = 'cancelled';
    assistant.text = '本次制作已停止。你可以调整想法后重新开始，之前的作品会为你保留。';
    terminal(record, job, now);
    return;
  }
  // 等待回答不继续推进渲染；问题过期后进入失败终态，释放项目占用。
  if (job.status === 'waiting_input') {
    if (job.question && now >= Date.parse(job.question.expires_at)) {
      job.status = 'failed';
      job.error = {
        code: 'INPUT_TIMEOUT',
        message: '这次提问已过期，可以重新开始创作。',
        retryable: true,
        failed_stage: 'planning',
      };
      assistant.status = 'failed';
      assistant.text = job.error.message;
      terminal(record, job, now);
    }
    return;
  }
  const elapsed = Math.max(0, now - run.started_ms);
  if (elapsed < 450) return;
  job.status = 'running';
  job.started_at ??= iso(run.started_ms + 450);
  assistant.status = 'streaming';
  // 仅讨论的分支只产出文本，不动草稿、分镜和成片版本。
  if (job.kind === 'chat') {
    if (elapsed < 1500) {
      assistant.text = '正在整理适合你的创作建议…';
      return;
    }
    assistant.text = /你好|怎么用/.test(run.input.text)
      ? '你好！告诉我视频的主题和想要的感觉，就可以开始。也可以上传自己的照片，或在右侧调整脚本、配音和字幕。切回「视频创作」即可生成。'
      : '建议先明确一个重点，用开头吸引注意，中段展示两三个有记忆点的细节，最后用一句话收尾。30 秒视频适合 4～6 个镜头。当前是仅讨论模式，不会修改或生成视频。';
    assistant.status = 'completed';
    job.status = 'succeeded';
    job.result = { assistant_message_id: assistant.id };
    terminal(record, job, now);
    return;
  }
  // 以下时间阈值是可演示流程的模拟节奏，不是实际模型服务的耗时估算。
  if (run.scenario === 'question' && !run.answered && elapsed >= 2500) {
    job.status = 'waiting_input';
    record.project.status = 'waiting_input';
    job.question = {
      id: makeId('question'),
      prompt: '你希望片尾展示什么店名？也可以填写“不展示”。',
      input_type: 'text',
      options: [],
      expires_at: iso(now + 86400000),
    };
    assistant.text = '内容方向已经准备好，再补充一个小信息就能继续。';
    assistant.blocks = [{ type: 'question', job_id: job.id, question_id: job.question.id }];
    touch(record, now);
    return;
  }
  const index = Math.min(7, Math.floor(elapsed / 1250));
  if (index >= 2 && !assistant.blocks.some((block) => block.type === 'plan')) {
    prepareDraft(record, job, now);
    assistant.blocks.unshift({ type: 'plan', summary: themes[run.theme].summary });
  }
  job.current_stage = stageKeys[index]!;
  job.progress_percent = Math.min(98, Math.floor(elapsed / 100));
  job.stages.forEach((stage, i) => {
    stage.status = i < index ? 'succeeded' : i === index ? 'running' : 'pending';
    stage.progress_percent =
      i < index ? 100 : i === index ? Math.floor((elapsed % 1250) / 12.5) : 0;
  });
  assistant.text = `${job.kind === 'revise' ? '收到，正在根据你的修改更新作品。' : '好呀，我会为你制作一支有故事感的短片。'}${index >= 2 ? '脚本与分镜已准备好，接下来让画面和声音相遇。' : '先整理创作方向，再把灵感拆成一个个镜头。'}`;
  // 模拟中断时保留此前已经保存的草稿，供重试或手工调整。
  if (run.scenario === 'failure' && elapsed >= 4200) {
    job.status = 'failed';
    job.error = {
      code: 'PROVIDER_UNAVAILABLE',
      message: '画面准备暂时中断，已保留脚本和分镜。重试后可以继续。',
      retryable: true,
      failed_stage: 'visuals',
    };
    job.stages[3]!.status = 'failed';
    assistant.status = 'failed';
    assistant.text = job.error.message;
    assistant.blocks.push({ type: 'error', job_id: job.id });
    terminal(record, job, now);
    return;
  }
  // 只有成功出口才追加新版本并更新 latest_video_version_id；失败或取消都不覆盖旧作品。
  if (elapsed >= 10000) {
    // 根据修改位置失效下游产物；这里只标记需要更新，保存本身不发布成片。
    record.draft.invalidated_stages = [];
    record.project.draft_revision++;
    const version: VideoVersion = {
      id: makeId('ver'),
      project_id: record.project.id,
      version_number: record.versions.length + 1,
      source_draft_revision: record.project.draft_revision,
      source_job_id: job.id,
      created_at: iso(now),
      video_asset_id: makeId('video'),
      thumbnail_url: imagePath(themes[run.theme].images[0]),
      duration_ms: 30000,
      ...dimensions(record.draft.settings.aspect_ratio),
      playback_url: videoPath(run.theme, record.draft.settings.aspect_ratio),
      url_expires_at: iso(now + 86400000),
      // 必须深拷贝：历史成片若直接引用当前草稿，后续编辑会把旧版本一起改掉。
      snapshot: copy(record.draft),
      exports: [],
    };
    version.exports.push({
      asset_id: version.video_asset_id,
      resolution: '720p',
      fps: 30,
      format: 'mp4',
      created_at: iso(now),
    });
    record.versions.push(version);
    record.project.latest_video_version_id = version.id;
    record.project.draft_dirty = false;
    job.status = 'succeeded';
    job.progress_percent = 100;
    job.stages.forEach((stage) => {
      stage.status = 'succeeded';
      stage.progress_percent = 100;
    });
    job.result = { video_version_id: version.id, draft_revision: record.project.draft_revision };
    assistant.status = 'completed';
    assistant.text =
      job.kind === 'revise'
        ? '新版本准备好了，之前的作品也已经保留。可以切换对比，或继续告诉我想调整的地方。'
        : `你的第一支短片准备好了！${run.theme === 'coffee' ? '清晨的光线、手冲的细节和温柔的音乐' : run.theme === 'travel' ? '开阔的山野、安静的湖泊和舒缓的音乐' : '干净的构图、细腻的产品画面和轻柔的音乐'}，都安排好了。还想调整哪里，继续告诉我就好。`;
    assistant.blocks = [
      { type: 'plan', summary: themes[run.theme].summary },
      { type: 'progress', job_id: job.id },
      { type: 'video', video_version_id: version.id },
    ];
    terminal(record, job, now);
  } else touch(record, now);
}
/**
 * 把活动任务标记为 cancelling 并记录取消时间；稍后的 advance 完成收尾，不发布迟到成片。
 */
export function cancelJob(record: ProjectRecord, jobId: string, now: number) {
  const job = record.jobs.find((item) => item.id === jobId);
  if (!job) throw new StudioError('NOT_FOUND', '没有找到这次创作。');
  if (job.status === 'cancelled' || job.status === 'cancelling') return;
  if (!isActive(job)) throw new StudioError('JOB_ALREADY_FINISHED', '这次创作已经结束。');
  job.status = 'cancelling';
  record.runs[jobId]!.cancelled_ms = now;
  touch(record, now);
}
/**
 * 使用原输入建立新任务，同时保留原助手记录和已保存草稿。
 * 若失败后草稿已经改变，拒绝旧重试，要求用户按当前草稿重新生成。
 */
export function retryJob(record: ProjectRecord, jobId: string, expected: number, now: number) {
  const job = record.jobs.find((item) => item.id === jobId);
  if (!job || !(job.status === 'cancelled' || (job.status === 'failed' && job.error?.retryable)))
    throw new StudioError('JOB_NOT_RETRYABLE', '这次创作暂时不能重试。');
  assertRevision(record, expected);
  const run = record.runs[jobId]!;
  // 旧任务的输入与当前草稿不一致时，不能按旧检查点继续，避免把新编辑覆盖。
  if (record.project.draft_revision !== run.checkpoint_revision)
    throw new StudioError('RETRY_INPUT_CHANGED', '草稿已经改变，请根据当前内容重新生成。');
  return startJob(record, { ...run.input, expected_draft_revision: expected }, now, 'normal', job);
}
/**
 * 核对仍然有效的问题 ID，保存回答并恢复同一个任务。
 * 演示时调整开始时间，从追问节点之后继续；重复回答不能再开启第二次生成。
 */
export function answerJob(
  record: ProjectRecord,
  jobId: string,
  questionId: string,
  value: string,
  now: number,
) {
  const job = record.jobs.find((item) => item.id === jobId);
  if (
    !job ||
    job.status !== 'waiting_input' ||
    job.question?.id !== questionId ||
    now >= Date.parse(job.question.expires_at)
  )
    throw new StudioError('QUESTION_NOT_ACTIVE', '这个问题已失效，请刷新当前创作。');
  if (!value.trim() || value.length > 5000)
    throw new StudioError('VALIDATION_ERROR', '请填写需要补充的信息。');
  message(record, 'user', value.trim(), jobId, now);
  const run = record.runs[jobId]!;
  run.answered = true;
  // 演示恢复到追问发生之后的时间点；仍沿用原 job_id，不另开一次任务。
  run.started_ms = now - 2600;
  job.question = null;
  job.status = 'running';
  record.project.status = 'processing';
  touch(record, now);
  const assistant = record.messages.find(
    (item) => item.role === 'assistant' && item.job_id === jobId,
  );
  if (assistant) assistant.blocks = [{ type: 'progress', job_id: job.id }];
}
/**
 * 校验并保存用户编辑的顶层草稿分组，递增修订号并标记需要重算的阶段。
 * 完整脚本变动会清空旧分镜；分镜旁白变动会同步完整文案，成片要另行更新。
 * @param changes 需要整体替换的草稿顶层分组。
 * @param expected 用户开始提交时看到的草稿修订号。
 */
export function patchDraft(
  record: ProjectRecord,
  changes: DraftChanges,
  expected: number,
  now: number,
) {
  assertIdle(record);
  assertRevision(record, expected);
  if (changes.script && changes.shots)
    throw new StudioError('VALIDATION_ERROR', '请分别保存完整脚本与分镜旁白。');
  if (
    changes.settings &&
    (!['9:16', '16:9', '1:1'].includes(changes.settings.aspect_ratio) ||
      changes.settings.target_duration_seconds < 15 ||
      changes.settings.target_duration_seconds > 60)
  )
    throw new StudioError('VALIDATION_ERROR', '请选择支持的比例与 15～60 秒的目标时长。');
  if (
    changes.script &&
    (!changes.script.text.trim() ||
      changes.script.text.length > 20000 ||
      !changes.script.title.trim())
  )
    throw new StudioError('VALIDATION_ERROR', '请填写标题和 1～20000 字的脚本。');
  if (changes.shots?.some((shot) => !shot.narration.trim() || shot.duration_ms <= 0))
    throw new StudioError('VALIDATION_ERROR', '分镜旁白不能为空，时长必须大于 0。');
  if (changes.audio) {
    const audio = changes.audio;
    if (
      audio.speech_rate < 0.5 ||
      audio.speech_rate > 2 ||
      [audio.narration_volume, audio.music_volume].some((volume) => volume < 0 || volume > 1)
    )
      throw new StudioError('VALIDATION_ERROR', '请检查语速和音量范围。');
    if (
      audio.music_mode === 'upload' &&
      !record.assets.some(
        (asset) =>
          asset.id === audio.music_asset_id && asset.kind === 'audio' && asset.status === 'ready',
      )
    )
      throw new StudioError('ASSET_NOT_READY', '请选择已上传的音频。');
  }
  // 顶层分组整体替换；复制输入可避免保存后继续修改表单影响数据库内对象。
  Object.assign(record.draft, copy(changes));
  if (changes.script) record.draft.shots = [];
  if (changes.shots && record.draft.script)
    record.draft.script.text = changes.shots.map((shot) => shot.narration).join('\n');
  record.draft.invalidated_stages =
    changes.script || changes.shots || changes.settings
      ? [...stageKeys].slice(1)
      : changes.audio
        ? ['audio', 'edit', 'finishing', 'render']
        : ['finishing', 'render'];
  record.project.draft_revision++;
  record.project.draft_dirty = true;
  touch(record, now);
}
/**
 * 从演示内部记录构造页面快照，并深拷贝隔离引用；不暴露内部 runs 调度信息。
 */
export function snapshot(record: ProjectRecord): Snapshot {
  return copy({
    project: record.project,
    draft: record.draft,
    active_job: record.jobs.find((job) => job.id === record.project.active_job_id) ?? null,
    latest_video:
      record.versions.find((version) => version.id === record.project.latest_video_version_id) ??
      null,
    messages_page: { items: record.messages, next_cursor: null },
    event_cursor: String(record.event_seq),
  });
}
/**
 * 首次使用时构造三份已完成案例，复用同一状态机产生成片数据，便于直接展示工作台。
 */
export function seedDatabase(now = Date.now()): Database {
  const seeded: { theme: Theme; id: string; ago: number }[] = [
    { theme: 'coffee', id: 'demo-coffee', ago: 1800000 },
    { theme: 'travel', id: 'demo-travel', ago: 7200000 },
    { theme: 'product', id: 'demo-product', ago: 86400000 },
  ];
  return {
    schema_version: 1,
    projects: seeded.map(({ theme, id, ago }) => {
      const time = now - ago;
      const record = createRecord(time, id);
      startJob(
        record,
        {
          intent: 'generate',
          text: themes[theme].prompt,
          asset_ids: [],
          expected_draft_revision: 1,
          base_video_version_id: null,
        },
        time,
      );
      advance(record, time + 11000);
      return record;
    }),
  };
}
