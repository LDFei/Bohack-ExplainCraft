/**
 * 前端与后端约定的数据类型。接口字段保持 snake_case；类型本身不负责网络传输或运行时校验。
 */
/** 任务生命周期。queued/running/waiting_input/cancelling 占用项目，其他三种状态表示任务已结束。 */
export type JobStatus =
  'queued' | 'running' | 'waiting_input' | 'cancelling' | 'succeeded' | 'failed' | 'cancelled';
/** 制作阶段的稳定标识，对应 content.ts 的中文阶段名称；配音先生成，画面再按配音长度生成。 */
export type StageKey =
  'planning' | 'script' | 'storyboard' | 'visuals' | 'audio' | 'edit' | 'finishing' | 'render';
/** 消息意图：generate 首次制作，revise 修改已有成片，chat 只讨论而不生成。 */
export type Intent = 'generate' | 'revise' | 'chat';
/** 内置示例主题，用来选择随工程打包的图片、文案和视频。 */
export type Theme = 'coffee' | 'travel' | 'product';

/** 用户可选择的创作参数；目标时长与实际成片时长分开记录。 */
export interface Settings {
  /** 期望时长，单位秒；示例成片实际固定 30 秒，以成片 duration_ms 为准。 */
  target_duration_seconds: number;
  /** 画面比例，例如 9:16；不要和像素宽高混用。 */
  aspect_ratio: string;
  /** 配音与文案语言标识，例如 zh-CN。 */
  language: string;
  /** 服务能力表中的风格 ID，不是界面展示名称。 */
  style_id: string;
}
/** 配音、音乐来源与混音设置。ID 来源要互斥，真实音色和曲目由 capabilities 提供。 */
export interface AudioSettings {
  /** 配音音色 ID。 */
  voice_id: string;
  /** 语速倍率，1 表示正常速度。 */
  speech_rate: number;
  /** auto 自动匹配，none 无音乐，library 曲库，upload 用户附件。 */
  music_mode: 'auto' | 'none' | 'library' | 'upload';
  /** 仅 library 模式使用的曲库条目 ID，其余模式为 null。 */
  music_id: string | null;
  /** 仅 upload 模式使用的本项目音频素材 ID。 */
  music_asset_id: string | null;
  /** 服务端解析出的最终音乐素材，只读，PATCH 时要移除。 */
  resolved_music_asset_id?: string | null;
  /** 旁白音量，范围 0～1。 */
  narration_volume: number;
  /** 背景音乐音量，范围 0～1。 */
  music_volume: number;
}
/** 后期预设：调色、特效及字幕开关。保存这些字段只更新草稿，更新视频后才生效。 */
export interface Finishing {
  /** 调色预设 ID；natural 表示自然原色。 */
  color_preset_id: string;
  /** 特效预设 ID；none 表示不叠加额外特效。 */
  effect_preset_id: string;
  /** 是否在真正渲染时生成字幕。 */
  subtitles_enabled: boolean;
}
/** 一个分镜。数组中的顺序决定播放顺序，旁白和画面提示词分别服务于配音与素材准备。 */
export interface Shot {
  id: string;
  /** 该镜头时长，单位毫秒。 */
  duration_ms: number;
  /** 该镜头的解说文字，可用于 TTS 或字幕。 */
  narration: string;
  /** 描述要呈现的画面内容，用于素材准备。 */
  visual_prompt: string;
  /** 景别，例如远景、特写。 */
  shot_size: string;
  /** 运镜描述，例如缓慢推进。 */
  camera_motion: string;
  /** 镜头引用的素材，必须属于当前项目且已就绪。 */
  asset_ids: string[];
  /** 可选预览图地址，可能是静态 URL 或本页有效的 Blob URL。 */
  thumbnail_url?: string;
}
/** 可修改的当前工作稿；和不可变的 VideoVersion.snapshot 区分，避免编辑污染历史版本。 */
export interface Draft {
  /** 后端整理的主题、受众、目的与风格；尚未规划时为 null。 */
  brief: { subject: string; audience: string; purpose: string; style: string } | null;
  settings: Settings;
  /** 完整标题和文案；编辑后旧分镜可能失效。 */
  script: { title: string; text: string } | null;
  /** 按播放顺序排列的分镜。 */
  shots: Shot[];
  audio: AudioSettings;
  finishing: Finishing;
  /** 后续制作需要重算的阶段，由业务层推导，页面不能擅自跳过。 */
  invalidated_stages: StageKey[];
}
/** 项目摘要：把一段对话、当前草稿、活动任务与最近成功成片关联起来。 */
export interface Project {
  id: string;
  title: string;
  status: 'draft' | 'processing' | 'waiting_input' | 'ready' | 'failed';
  /** 当前草稿修订号，用于阻止旧页面覆盖较新的编辑。 */
  draft_revision: number;
  /** 占用此项目的活动任务；无任务时为 null。 */
  active_job_id: string | null;
  /** 最近成功成片；新任务失败时仍指向旧的成功版本。 */
  latest_video_version_id: string | null;
  /** 草稿与最近成片不一致，需要再次生成才能在播放器看到修改。 */
  draft_dirty: boolean;
  /** UTC ISO 时间字符串。 */
  created_at: string;
  /** 用于历史列表排序的最近修改时间。 */
  updated_at: string;
}
/** 素材元信息；文件内容另存于服务端或 IndexedDB，不嵌入聊天 JSON。 */
export interface Asset {
  id: string;
  project_id: string;
  kind: 'image' | 'video' | 'audio';
  /** 素材来自用户上传、生成、曲库或已渲染文件。 */
  source: 'upload' | 'generated' | 'library' | 'rendered';
  /** processing 还不能用于制作，ready 可用，failed 展示错误。 */
  status: 'processing' | 'ready' | 'failed';
  file_name: string;
  mime_type: string;
  /** 文件大小，单位字节；不能与 MB 显示值直接比较。 */
  size_bytes: number;
  thumbnail_url: string | null;
  /** 音视频时长，单位毫秒；图片为 null。 */
  duration_ms: number | null;
  /** 图像或视频像素宽度；音频为 null。 */
  width: number | null;
  /** 图像或视频像素高度；音频为 null。 */
  height: number | null;
  error: { code: string; message: string } | null;
}
/** 结构化消息卡片。模板依据 type 渲染，不从 AI 自然语言里猜测任务状态。 */
export interface MessageBlock {
  /** 模板的分支标识，决定显示方案、进度、问题、错误或成片卡片。 */
  type: 'plan' | 'progress' | 'video' | 'question' | 'error' | 'script';
  summary?: string;
  job_id?: string;
  video_version_id?: string;
  question_id?: string;
}
/** 一条用户或助手消息。助手文本和卡片可以在任务执行中逐步更新。 */
export interface Message {
  id: string;
  project_id: string;
  role: 'user' | 'assistant';
  text: string;
  status: 'pending' | 'streaming' | 'completed' | 'failed' | 'cancelled';
  /** 原始附件引用，展示时通过素材列表查找名称和缩略图。 */
  asset_ids: string[];
  /** 此消息对应的任务；非任务消息可以为 null。 */
  job_id: string | null;
  /** 与自然语言分开的结构化结果。 */
  blocks: MessageBlock[];
  /** 已持久化的最后文本增量序号，用于断线恢复去重。 */
  last_delta_seq: number;
  created_at: string;
}
/** 一个制作阶段的展示状态，进度百分比可以未知，不能前端自行编造。 */
export interface Stage {
  key: StageKey;
  status: 'pending' | 'running' | 'succeeded' | 'failed' | 'skipped' | 'cancelled';
  /** 阶段估算进度，未知时为 null。 */
  progress_percent: number | null;
  message: string;
}
/** 一次生成、修改、讨论或导出任务；查询接口成功不代表任务已经成功。 */
export interface Job {
  id: string;
  project_id: string;
  /** 重试来源；一次新的正常任务为 null。 */
  parent_job_id: string | null;
  kind: Intent | 'export';
  status: JobStatus;
  /** 当前阶段；排队或普通讨论可以为 null。 */
  current_stage: StageKey | null;
  /** 整体估算百分比，未知时为空；只有终态成功才表示完成。 */
  progress_percent: number | null;
  stages: Stage[];
  /** 待回答问题；回答必须带其 ID，避免旧问题被重复消费。 */
  question: {
    id: string;
    prompt: string;
    input_type: 'text' | 'single_select';
    options: { value: string; label: string }[];
    expires_at: string;
  } | null;
  /** 成功结果，按 kind 返回成片、助手消息或导出素材 ID。 */
  result: {
    video_version_id?: string;
    draft_revision?: number;
    assistant_message_id?: string;
    asset_id?: string;
  } | null;
  /** 异步业务错误，retryable 决定是否展示重试按钮。 */
  error: {
    code: string;
    message: string;
    retryable: boolean;
    failed_stage: StageKey | null;
  } | null;
  created_at: string;
  /** 尚未执行时为 null。 */
  started_at: string | null;
  /** 任务未结束时为 null。 */
  finished_at: string | null;
}
/** 一次成功制作留下的不可变成片版本，包含当次完整草稿和可下载文件信息。 */
export interface VideoVersion {
  id: string;
  project_id: string;
  version_number: number;
  /** 生成这个版本时的草稿修订号。 */
  source_draft_revision: number;
  /** 成功产出这个版本的任务。 */
  source_job_id: string;
  created_at: string;
  /** 成片文件 ID，用于获取下载链接。 */
  video_asset_id: string;
  thumbnail_url: string;
  /** 实际成片长度，单位毫秒。 */
  duration_ms: number;
  width: number;
  height: number;
  /** 播放器使用的地址；真实后端一般返回短期签名 URL。 */
  playback_url: string;
  /** 播放地址有效期，真实服务过期后需要重新查询版本详情。 */
  url_expires_at: string;
  /** 当次完整草稿深拷贝，后续编辑当前草稿不得修改这个历史业务快照。 */
  snapshot: Draft;
  /** 同一内容版本已有的输出规格；导出不额外创建内容版本。 */
  exports: {
    asset_id: string;
    resolution: string;
    fps: number;
    format: string;
    created_at: string;
  }[];
}
/** 一次性恢复工作台的数据集合；拿到快照后，再从它的 event_cursor 订阅后续事件。 */
export interface Snapshot {
  project: Project;
  draft: Draft;
  /** 当前活动任务详情，没有活动任务时为 null。 */
  active_job: Job | null;
  /** 最近成功成片详情，与查看中的历史版本不是同一个概念。 */
  latest_video: VideoVersion | null;
  /** 最近消息与加载更早消息的游标。 */
  messages_page: { items: Message[]; next_cursor: string | null };
  /** 订阅事件的起点，保持字符串，不能转成可能丢精度的 Number。 */
  event_cursor: string;
}
/** 服务能力表：页面选项、默认值和媒体限制以这里为准，不写死某个生成供应商。 */
export interface Capabilities {
  /** 新项目和输入框的默认设置。 */
  defaults: Settings & {
    voice_id: string;
    music_mode: 'auto' | 'none' | 'library' | 'upload';
    color_preset_id: string;
    effect_preset_id: string;
    subtitles_enabled: boolean;
  };
  duration_range_seconds: { min: number; max: number };
  aspect_ratios: string[];
  languages: string[];
  styles: { id: string; name: string }[];
  voices: { id: string; name: string; sample_url: string | null }[];
  music_tracks: { id: string; name: string; sample_url: string | null }[];
  color_presets: { id: string; name: string }[];
  effect_presets: { id: string; name: string }[];
  exports: { formats: string[]; resolutions: string[]; fps: number[]; preview_resolution: string };
  /** 各媒体单文件大小、类型及项目上传数量限制。 */
  upload_limits: {
    max_uploaded_files_per_project: number;
    image: FileLimit;
    video: FileLimit;
    audio: FileLimit;
  };
  /** 后端保证可补发事件的保留秒数。 */
  event_retention_seconds: number;
}
/** 某一种媒体允许的 MIME 类型和单文件字节上限。 */
export interface FileLimit {
  /** 单文件上限，单位字节。 */
  max_bytes: number;
  mime_types: string[];
}
/** 发送消息时提交的业务输入；只传素材 ID，不把文件二进制重复放进消息。 */
export interface MessageInput {
  intent: Intent;
  text: string;
  asset_ids: string[];
  /** 发送时看到的草稿修订号，服务端需要再次校验。 */
  expected_draft_revision: number;
  /** revise 的修改基底，应为最新成功版本；chat 为 null。 */
  base_video_version_id: string | null;
}
/** 允许修改的草稿顶层分组。提交某个分组时传该分组完整可写字段，不做含糊的深层合并。 */
export type DraftChanges = Partial<
  Pick<Draft, 'settings' | 'script' | 'shots' | 'audio' | 'finishing'>
>;
