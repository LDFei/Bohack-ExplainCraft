import type { Capabilities, Draft, StageKey, Theme } from './types.ts';

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
  duration_range_seconds: { min: 15, max: 60 },
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
    { id: 'piano', name: '午后钢琴', sample_url: '/media/ambient.wav' },
    { id: 'light', name: '轻松日常', sample_url: '/media/ambient.wav' },
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

export const imagePath = (name: string) => `/media/${name}.jpg`;
export const copy = <T>(value: T): T => JSON.parse(JSON.stringify(value)) as T;
export const inferTheme = (text: string): Theme =>
  /旅行|山野|风景|山峰|露营|湖泊/.test(text)
    ? 'travel'
    : /产品|水杯|保温|商品|耳机/.test(text)
      ? 'product'
      : 'coffee';
export const ratioKey = (ratio: string) => ratio.replace(':', 'x');
export const videoPath = (theme: Theme, ratio: string) => `/media/${theme}-${ratioKey(ratio)}.mp4`;
export const dimensions = (ratio: string) =>
  ratio === '16:9'
    ? { width: 1280, height: 720 }
    : ratio === '1:1'
      ? { width: 720, height: 720 }
      : { width: 720, height: 1280 };
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
