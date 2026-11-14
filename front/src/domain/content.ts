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
  edit: '安排镜头顺序',
  finishing: '字幕与输出检查',
  render: '输出成片',
};
export const stageKeys = Object.keys(stageLabels) as StageKey[];

/** 从明确文本中读取 15～300 秒目标时长，未提及时返回 null。 */
export function requestedDurationSeconds(text: string) {
  const match = text.match(/(?:^|[^\d])(\d+)\s*(?:秒|s(?:ec(?:onds?)?)?)(?=$|[^\da-z])/i);
  if (!match) return null;
  const seconds = Number(match[1]);
  return seconds >= 15 && seconds <= 300 ? seconds : null;
}

/** 检测文本中明确超过 300 秒的请求，避免静默截断。 */
export function durationTooLong(text: string) {
  const match = text.match(/(?:^|[^\d])(\d+)\s*(?:秒|s(?:ec(?:onds?)?)?)(?=$|[^\da-z])/i);
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

/** Shotcraft 镜头配方的应用内适配；保留旧主题键，避免历史示例素材索引失效。 */
export const themes = {
  coffee: {
    templateId: 'shotcraft-ink-press',
    title: 'Ink Press · 纸墨叙事',
    projectTitle: '纸墨叙事创作',
    label: '纸墨开场',
    color: '#955905',
    images: [
      'templates/ink-press.svg',
      'templates/ink-press.svg',
      'templates/ink-press.svg',
      'templates/ink-press.svg',
    ],
    lines: [
      '先用一句话提出主题。',
      '让一个核心对象成为主角。',
      '通过具体例子展示价值。',
      '留一秒让结论落定。',
    ],
    visuals: [
      '纸底上准星描画，主题文字逐字压印',
      '单一主角卡片被聚光锁定，缓慢推进',
      '一张强调词字卡连接具体示例',
      '主题与结论归位，保留清晰阅读停顿',
    ],
    recipes: ['brand-ink-open', 'spotlight-hero-card', 'paper-title-card'],
    source:
      'https://github.com/Vincentwei1021/video-shotcraft/tree/6700db2ba80572ebc777b99d72049679fed9840c/template',
    prompt:
      '[template:shotcraft-ink-press] 使用 Shotcraft Ink Press 镜头结构制作视频。先在对话中让我选择风格、时长、SVG或HTML、配音和BGM；文案与分镜各自确认后制作初版，初版交付后再询问是否使用剪辑库。主题或产品是：',
    summary: '借鉴 Ink Press 的压印开场、单一主角与呼吸字卡；画面由本应用 SVG/HTML 流程制作。',
  },
  travel: {
    templateId: 'shotcraft-product-demo',
    title: 'Show the Flow · 功能演示',
    projectTitle: '功能演示创作',
    label: '功能演示',
    color: '#6b7bce',
    images: [
      'templates/product-flow.svg',
      'templates/product-flow.svg',
      'templates/product-flow.svg',
      'templates/product-flow.svg',
    ],
    lines: [
      '从用户真正遇到的问题开始。',
      '看清输入、操作与页面回应。',
      '让结果落在真实的位置。',
      '用一句收益收尾，不堆功能名。',
    ],
    visuals: [
      '核心问题与一张产品主角卡',
      '放大输入框，逐字显示操作',
      '非目标卡片错峰退场，结果归位',
      '聚焦结果，保留阅读停顿',
    ],
    recipes: ['brand-ink-open', 'type-and-filter', 'paper-title-card'],
    source:
      'https://github.com/Vincentwei1021/video-shotcraft/blob/6700db2ba80572ebc777b99d72049679fed9840c/references/shots/interaction/type-and-filter.md',
    prompt:
      '[template:shotcraft-product-demo] 使用 Shotcraft 功能演示镜头配方，让观众看清问题、输入、操作、结果。先让我选择风格、时长、SVG或HTML、配音和BGM，逐步确认文案和分镜。没有实际页面素材时明确画成示意，不捏造产品功能。先交付初版，再询问可选剪辑。要介绍的功能是：',
    summary: '基于 type-and-filter 的操作叙事；适合解释一个功能如何解决问题，不要求固定时长。',
  },
  product: {
    templateId: 'shotcraft-data-story',
    title: 'Data in Motion · 数据叙事',
    projectTitle: '数据叙事创作',
    label: '数据叙事',
    color: '#c35e45',
    images: [
      'templates/data-story.svg',
      'templates/data-story.svg',
      'templates/data-story.svg',
      'templates/data-story.svg',
    ],
    lines: [
      '先提出一个有数据可回答的问题。',
      '给出带单位和来源的坐标轴。',
      '用一次变化讲清核心结论。',
      '说明结论成立的范围与边界。',
    ],
    visuals: [
      '一句问题字卡与关键数值',
      '带刻度和单位的真实图表',
      '曲线变化，必要时只重标一次坐标轴',
      '结论与来源清楚落定',
    ],
    recipes: ['paper-title-card', 'chart-live-moves', 'paper-title-card'],
    source:
      'https://github.com/Vincentwei1021/video-shotcraft/blob/6700db2ba80572ebc777b99d72049679fed9840c/references/shots/data/chart-live-moves.md',
    prompt:
      '[template:shotcraft-data-story] 使用 Shotcraft 数据叙事配方做一支图解视频。先让我选择风格、时长、SVG或HTML、配音和BGM，再确认文案与分镜。图表必须有标题、单位、刻度和来源；没有真实数据时先说明示意，不虚构事实。先完成初版，再让我选择是否使用剪辑库。要解释的数据或问题是：',
    summary: '基于 chart-live-moves，把图表变化变成叙事；强调数据口径、可读刻度和结论边界。',
  },
} as const;

/** Vibe 知识大赏是知识叙事模板；所有制作选择服从应用内逐步 skill，不锁定配音或 BGM。 */
export const vibeAwards = {
  title: 'Vibe 知识大赏',
  templateId: 'vibe-knowledge-awards',
  subtitle: '只讲一个知识点，从主题到成片逐步共同决定。',
  styles: [
    { id: 'card', name: '信息卡片', detail: '大数字、单一强调色、清楚的数据图解' },
    { id: 'paper', name: '编辑部手记', detail: '纸色底与墨黑文字，适合概念解释' },
    { id: 'doodle', name: '白板涂鸦', detail: '线条逐笔画出，适合机制拆解' },
    { id: 'black', name: '黑金极简', detail: '低信息密度，让一句结论落定' },
  ] as const,
  ratios: [
    { id: '9:16', name: '9:16 竖屏' },
    { id: '16:9', name: '16:9 横屏' },
    { id: '1:1', name: '1:1 方形' },
  ] as const,
  /** 本地示例音乐是候选；用户可以改用上传或程序化音乐，不会自动导入。 */
  bgm: {
    name: 'We Never（若如初见）',
    artist: 'La Mer / LongTran Final / DJ山茶花',
    url: 'media/vibe-knowledge-awards/we-never.mp3',
  },
  rules: [
    '一支视频围绕一个可验证的结论，解释机制与适用边界。',
    '主题后先给风格选项；时长、比例和视觉方式由你决定。',
    '配音可选 API、本地 breezeTTS2 或纯 BGM，音乐也可自行选择。',
    'API 或本地配音超时会提示并转为纯 BGM，保留完整字幕。',
    '画面由 SVG / HTML 代码生成；文案和分镜确认后才开始制作。',
    '先交付可播放初版；由你选择是否调用 Quercus 做进一步剪辑。',
  ],
  steps: [
    { title: '确定一个知识点', detail: '写下主题；给出可验证的一句结论，必要时补充参考资料。' },
    {
      title: '逐步选择创作方案',
      detail: '依次选择风格、时长、画面方式、配音与音乐，不默认替你跳过。',
    },
    { title: '审阅讲解文案', detail: '先看完整逻辑和字幕短句；你确认后再进入分镜。' },
    { title: '确认镜头表', detail: '逐镜头看画面、文案和时长；有音乐时只依据真实拍点安排节奏。' },
    {
      title: '制作代码画面',
      detail: '按确认的 SVG 或 HTML 方案制作，步骤中显示真实进度和失败原因。',
    },
    { title: '预览初版再选剪辑', detail: '初版不调用剪辑库；确认需要后再进入 Quercus 可选剪辑。' },
    {
      title: '验收与结束话题',
      detail: '检查画面、时长、字幕与声音；可要求返工、确认交付或结束话题。',
    },
  ],
  prompt:
    '[template:vibe-knowledge-awards] 用 Vibe 知识大赏流程做一支知识视频，只讲清一个可验证的知识点。' +
    '主题确定后依次给我风格、时长、SVG或HTML、配音和BGM选项；文案、分镜、制作与渲染逐步确认。' +
    '声音可选API、本地breezeTTS2或纯BGM；缺少本地模型先让我选择是否下载，配音超时提示并转纯BGM。' +
    '音乐可选示例、程序化或上传，不强制锁定；画面用代码生成。先交付初版，再问我是否调用剪辑库。' +
    '知识点是：',
} as const;

/**
 * 将本地示例图片名转换成静态资源 URL；返回的是浏览器路径，不是磁盘路径。
 */
const templatePreviews: Record<string, string> = {
  'templates/ink-press.svg': new URL('../assets/templates/ink-press.svg', import.meta.url).href,
  'templates/product-flow.svg': new URL('../assets/templates/product-flow.svg', import.meta.url)
    .href,
  'templates/data-story.svg': new URL('../assets/templates/data-story.svg', import.meta.url).href,
};
/** 模板随源码导入构建，避免共享 publicDir 忽略工作台新素材。 */
export const imagePath = (name: string) => templatePreviews[name] ?? `media/${name}.jpg`;
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
