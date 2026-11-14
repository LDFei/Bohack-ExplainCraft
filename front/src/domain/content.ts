/**
 * 初始能力、主题内容及纯工具函数。运行时界面选项以 capabilities 返回值为准。
 */
import type { Capabilities, Draft, StageKey, Theme } from './types.ts';

// 界面文案与稳定阶段 ID 分开保存；后端只需要按同一 ID 返回进度。
export const stageLabels: Record<StageKey, string> = {
  planning: '理解你的想法',
  script: '创作文案',
  storyboard: '规划分镜',
  visuals: '准备画面',
  audio: '配音与音乐',
  edit: '剪辑合成',
  finishing: '调色与字幕',
  render: '输出成片',
};
export const stageKeys = Object.keys(stageLabels) as StageKey[];

/** 从明确文本中读取 15～300 秒目标时长，未提及时返回 null。 */
export function requestedDurationSeconds(text: string) {
  const match = text.match(/(?:^|\D)([1-6]?\d)(?:\s*)(?:秒|s(?:ec(?:onds?)?)?)\b/i);
  if (!match) return null;
  const seconds = Number(match[1]);
  return seconds >= 15 && seconds <= 300 ? seconds : null;
}

/** 检测文本中明确超过 300 秒的请求，避免静默截断。 */
export function durationTooLong(text: string) {
  const match = text.match(/(?:^|\D)(\d+)(?:\s*)(?:秒|s(?:ec(?:onds?)?)?)\b/i);
  return !!match && Number(match[1]) > 300;
}
// 初始能力只公布已经支持的 720p 输出规格。
export const capabilities: Capabilities = {
  defaults: {
    target_duration_seconds: 30,
    aspect_ratio: '9:16',
    language: 'zh-CN',
    style_id: 'warm',
    voice_id: 'voice_warm',
    music_mode: 'auto',
    color_preset_id: 'natural',
    effect_preset_id: 'none',
    subtitles_enabled: true,
  },
  duration_range_seconds: { min: 15, max: 300 },
  aspect_ratios: ['9:16', '16:9', '1:1'],
  languages: ['zh-CN'],
  styles: [
    { id: 'warm', name: '温暖治愈' },
    { id: 'cinematic', name: '电影质感' },
    { id: 'minimal', name: '简约高级' },
    { id: 'energetic', name: '活力轻快' },
  ],
  voices: [
    { id: 'voice_warm', name: '温柔女声', sample_url: null },
    { id: 'voice_calm', name: '沉稳男声', sample_url: null },
    { id: 'voice_bright', name: '元气女声', sample_url: null },
  ],
  music_tracks: [
    { id: 'piano', name: '午后钢琴', sample_url: 'media/ambient.wav' },
    { id: 'light', name: '轻松日常', sample_url: 'media/ambient.wav' },
  ],
  color_presets: [
    { id: 'natural', name: '自然原色' },
    { id: 'warm', name: '暖调胶片' },
    { id: 'cool', name: '清冷电影' },
  ],
  effect_presets: [
    { id: 'none', name: '简洁无特效' },
    { id: 'fade', name: '柔和淡入淡出' },
  ],
  exports: { formats: ['mp4'], resolutions: ['720p'], fps: [30], preview_resolution: '720p' },
  upload_limits: {
    max_uploaded_files_per_project: 20,
    image: { max_bytes: 20 * 1024 * 1024, mime_types: ['image/jpeg', 'image/png', 'image/webp'] },
    video: { max_bytes: 100 * 1024 * 1024, mime_types: ['video/mp4', 'video/webm'] },
    audio: { max_bytes: 50 * 1024 * 1024, mime_types: ['audio/mpeg', 'audio/wav', 'audio/x-wav'] },
  },
  event_retention_seconds: 86400,
};

// 三类本地素材模板：用于展示前端闭环，不是用户任意提示词的真实 AI 生成结果。
export const themes = {
  coffee: {
    title: '一杯咖啡的慢时光',
    projectTitle: '咖啡店的温柔早晨',
    label: '店铺宣传',
    color: '#b28a60',
    images: ['coffee-shop', 'coffee-brew', 'coffee-cup', 'coffee-detail'],
    lines: [
      '城市醒来之前，先留一点时间给自己。',
      '听水流与咖啡相遇，让熟悉的香气慢慢散开。',
      '一杯刚刚好的温度，装下生活的小小美好。',
      '在这里，让忙碌慢下来。每一天，都值得好好开始。',
    ],
    visuals: [
      '清晨的咖啡店，阳光穿过窗户落在木质桌面',
      '手冲咖啡的细节，热水缓缓注入，蒸汽轻轻升起',
      '一杯精心制作的咖啡，温暖的光线与细腻的奶泡',
      '咖啡与日常的片刻，营造安静舒适的店铺氛围',
    ],
    prompt: '帮我做一个30秒的咖啡店宣传视频，风格温暖治愈，重点展示手冲咖啡和店里的日常。',
    summary: '用清晨的光线、手冲的细节和一杯咖啡的温度，讲一个让生活慢下来的故事。',
  },
  travel: {
    title: '去山野，找回自己',
    projectTitle: '把周末交给山野',
    label: '旅行记录',
    color: '#658c77',
    images: ['travel-road', 'travel-mountain', 'travel-lake', 'travel-road'],
    lines: [
      '把消息留在身后，把周末交给山野。',
      '走过的路，带我们看见更辽阔的世界。',
      '在山与湖之间，听一听自己的呼吸。',
      '不必去很远，只要向自由出发。',
    ],
    visuals: [
      '山间公路延伸向远方，阳光从树林间洒落',
      '巍峨山峰的远景，云层缓缓移动',
      '安静的高山湖泊，湖水倒映着远山',
      '回到开阔的自然风景，让画面缓缓淡出',
    ],
    prompt: '做一个30秒的山野旅行短片，有电影感，让人看完就想出去走走。',
    summary: '从山间公路出发，经过远山与湖泊，用舒缓的节奏记录一次与自然的相遇。',
  },
  product: {
    title: '把日常，装进美好',
    projectTitle: '随行水杯产品短片',
    label: '产品介绍',
    color: '#719b9b',
    images: ['product-bottle', 'product-bottle', 'product-bottle', 'product-bottle'],
    lines: [
      '好的设计，藏在每天都会用到的细节里。',
      '轻盈的手感，简单的线条，让日常多一点从容。',
      '无论工作还是出发，随时补充生活的能量。',
      '把美好带在身边，从每一口水开始。',
    ],
    visuals: [
      '水杯产品全貌，干净背景与柔和自然光',
      '产品瓶身和材质细节，突出简约设计',
      '水杯的日常使用场景，展现随行体验',
      '产品主体居中，留白呈现品牌文案',
    ],
    prompt: '给一款随行水杯做一个30秒的产品介绍，简约高级，突出日常陪伴感。',
    summary: '用干净的构图、产品细节和日常场景，让产品的设计与陪伴感自然呈现。',
  },
} as const;

/**
 * Vibe 知识大赏固定流程：BGM 固定、没有配音、按拍点切镜、只讲一个知识点、
 * 画面全部由 SVG / HTML 代码生成。页面教程与 `.claude/skills/vibe-knowledge-awards`
 * 共用这份定义，修改时两边同步。
 */
export const vibeAwards = {
  title: 'Vibe 知识大赏',
  subtitle: '用代码讲清一个知识点，每一步都由你决定。',
  /** 视频风格方向；选择后写进提示词，后续视觉步骤沿用。 */
  styles: [
    { id: 'card', name: '信息卡片', detail: '深色底、大数字、图表生长，适合数据与反常识' },
    { id: 'paper', name: '编辑部手记', detail: '纸色底、墨黑大字、擦除转场，适合概念解释' },
    { id: 'doodle', name: '白板涂鸦', detail: '浅色底、线条逐笔画出，适合机制拆解' },
    { id: 'black', name: '黑金极简', detail: '纯黑底、单一强调色，适合一句结论' },
  ] as const,
  /** 画面比例候选，与 capabilities.aspect_ratios 保持一致。 */
  ratios: [
    { id: '9:16', name: '9:16 竖屏' },
    { id: '16:9', name: '16:9 横屏' },
    { id: '1:1', name: '1:1 方形' },
  ] as const,
  /** 固定背景音乐；文件已随仓库放在共享媒体目录。 */
  bgm: {
    name: 'We Never（若如初见）',
    artist: 'La Mer / LongTran Final / DJ山茶花',
    url: 'media/vibe-knowledge-awards/we-never.mp3',
  },
  rules: [
    '默认 BGM 使用《We Never（若如初见）》，也可以在这一步换成自己的音频。',
    '默认没有配音，观众只读字幕；也可以选择保留配音。',
    '按 BGM 的拍点卡点：默认每 8 拍换镜，保证字幕读得完。',
    '一支视频只讲清一个知识点，不贪多、不铺陈背景。',
    '画面全部用 SVG / HTML 代码生成，不拍实拍、不堆素材拼贴。',
    '每一步都会先给你方向选择，确认后才继续。',
  ],
  steps: [
    { title: '确认制作方式', detail: '先选比例、是否配音、用哪首 BGM，再开始动手。' },
    { title: '选一个知识点', detail: '给 3 个候选方向，你选一个；写一句能被验证的结论。' },
    { title: '拆成 6～12 句字幕', detail: '你选结构与语气，每句不超过 18 字。' },
    { title: '对 BGM 找拍点', detail: '你定时长与切镜密度，确认镜头表后再写画面。' },
    { title: '用代码画每一镜', detail: '先做 2 个代表镜头给你看，确认风格后再批量。' },
    { title: '决定要不要剪辑', detail: '转场、调色、字幕烧录都先问你；不做就只按镜头表拼接。' },
    { title: '渲染与验收', detail: '你决定接受成片，或回到某一步返工。' },
  ],
  /** 填进输入框的固定流程说明；不自动发送，让用户先改知识点。 */
  prompt:
    '用 Vibe 知识大赏流程做一支知识视频：只讲清一个知识点。' +
    '制作前先确认比例、配音和 BGM；默认使用固定 BGM《We Never（若如初见）》，默认不配音。' +
    '每一步先给我方向选择，确认后再继续：主题方向、文案结构与语气、时长与切镜密度、视觉系统。' +
    '按 BGM 的拍点卡点切镜；画面全部用 SVG / HTML 代码生成，字幕承担讲解。' +
    '知识点是：',
} as const;

/**
 * 将本地示例图片名转换成静态资源 URL；返回的是浏览器路径，不是磁盘路径。
 */
export const imagePath = (name: string) => `media/${name}.jpg`;
/**
 * 深拷贝仅含普通 JSON 字段的业务数据，隔离草稿、历史快照和页面表单。
 * 不适用于 File、Blob、DOM、函数或需要保留原类型的 Date、Map。
 */
export const copy = <T>(value: T): T => JSON.parse(JSON.stringify(value)) as T;
/**
 * 通过少量关键词选择咖啡、旅行或产品示例。
 */
export const inferTheme = (text: string): Theme =>
  /旅行|山野|风景|山峰|露营|湖泊/.test(text)
    ? 'travel'
    : /产品|水杯|保温|商品|耳机/.test(text)
      ? 'product'
      : 'coffee';
/**
 * 把业务比例 9:16 转成文件名中的 9x16，匹配内置视频命名。
 */
export const ratioKey = (ratio: string) => ratio.replace(':', 'x');
/**
 * 按示例主题与比例定位已打包的 MP4，不在浏览器执行视频生成。
 */
export const videoPath = (theme: Theme, ratio: string) => `media/${theme}-${ratioKey(ratio)}.mp4`;
/**
 * 返回 720p 示例的真实宽高，竖屏、横屏和方形以短边 720px 为基准。
 */
export const dimensions = (ratio: string) =>
  ratio === '16:9'
    ? { width: 1280, height: 720 }
    : ratio === '1:1'
      ? { width: 720, height: 720 }
      : { width: 720, height: 1280 };
/**
 * 生成一份独立空草稿，包含默认音频、字幕和失效阶段；每次调用都创建新对象。
 */
export function emptyDraft(): Draft {
  return {
    brief: null,
    settings: {
      target_duration_seconds: 30,
      aspect_ratio: '9:16',
      language: 'zh-CN',
      style_id: 'warm',
    },
    script: null,
    shots: [],
    audio: {
      voice_id: 'voice_warm',
      speech_rate: 1,
      narration_enabled: true,
      music_mode: 'auto',
      music_id: null,
      music_asset_id: null,
      resolved_music_asset_id: null,
      narration_volume: 1,
      music_volume: 0.2,
    },
    finishing: { color_preset_id: 'natural', effect_preset_id: 'none', subtitles_enabled: true },
    invalidated_stages: [...stageKeys],
  };
}
