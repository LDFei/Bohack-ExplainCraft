<script setup lang="ts">
/** 独立产品官网：展示桌面端能力与本地示例，所有创作入口统一下载桌面应用。 */
import { onBeforeUnmount, onMounted, ref } from "vue";
import BackgroundBeams from "./aceternity/BackgroundBeams.vue";
import BentoGrid from "./aceternity/BentoGrid.vue";
import BentoGridItem from "./aceternity/BentoGridItem.vue";
import Icon from "./Icon.vue";
import Modal from "./Modal.vue";
import { mediaUrl } from "../domain/assets.ts";
import { DOWNLOAD_URL } from "../domain/links.ts";
import ThemeSwitcher from "./ThemeSwitcher.vue";
const activePath = ref<"generic" | "vibe">("generic");
/** 模板示例分镜：用现有成片静帧，说明这一系列的镜头结构。 */
const vibeShots = [
  { label: "开场钩子", dur: "2s", img: "rel-thumb-1.jpg" },
  { label: "机制拆解", dur: "8s", img: "rel-thumb-2.jpg" },
  { label: "反差转折", dur: "6s", img: "rel-thumb-3.jpg" },
  { label: "落点与引用", dur: "4s", img: "rel-thumb-4.jpg" },
];
const previewOpen = ref(false),
  musicPlaying = ref(false),
  mediaError = ref("");
const player = ref<HTMLVideoElement | null>(null),
  heroVideo = ref<HTMLVideoElement | null>(null),
  motionVideo = ref<HTMLVideoElement | null>(null);
const heroTime = ref(0);
const media = window.matchMedia("(prefers-reduced-motion: reduce)");
const reducedMotion = ref(media.matches);
/** 装饰性视频遵循系统减少动态效果；用户主动打开的示例播放器保持可操作。 */
function syncMotion() {
  reducedMotion.value = media.matches;
  for (const video of [heroVideo.value, motionVideo.value]) {
    if (media.matches) video?.pause();
    else if (video) void video.play().catch(() => {});
  }
}
onMounted(() => media.addEventListener("change", syncMotion));
let audio: HTMLAudioElement | null = null;
/** 试听官网内置的背景音乐；桌面端的配音能力由 Rust 后端供应商提供。 */
async function toggleMusic() {
  mediaError.value = "";
  if (musicPlaying.value) {
    stopMusic();
    return;
  }
  audio = new Audio(mediaUrl("ambient.wav"));
  audio.onended = stopMusic;
  audio.onerror = () => {
    stopMusic();
    mediaError.value = "示例音乐暂时无法读取，请稍后重试。";
  };
  try {
    await audio.play();
    musicPlaying.value = true;
  } catch {
    stopMusic();
    mediaError.value = "请再次点击试听示例配乐。";
  }
}
/** 离开首页或切换到视频预览时释放音频，避免听不到控件却仍然播放。 */
function stopMusic() {
  audio?.pause();
  audio = null;
  musicPlaying.value = false;
}
/** 用户主动点击后打开原生视频播放器；弹窗关闭时停止媒体。 */
function openPreview() {
  stopMusic();
  previewOpen.value = true;
}
/** 保留弹窗关闭过渡期间的清理，防止声音晚于画面消失。 */
function closePreview() {
  player.value?.pause();
  previewOpen.value = false;
}
onBeforeUnmount(() => {
  stopMusic();
  media.removeEventListener("change", syncMotion);
});
</script>
<template>
  <div class="landing-page">
    <a class="skip-link" href="#main-content"> 跳到主要内容 </a>
    <header class="landing-nav">
      <a href="./" class="landing-brand" aria-label="ExplainCraft 首页">
        <span class="brand-icon">
          <Icon name="aperture" :size="27" />
        </span>
        <span>
          Explain
          <span class="brand-light">Craft</span>
          <small>KNOWLEDGE INTO MOTION</small>
        </span>
      </a>
      <nav aria-label="首页导航">
        <a href="#features">能做什么</a>
        <a href="#workflow">怎么用</a>
        <a :href="DOWNLOAD_URL">
          立即下载
          <Icon name="arrowRight" :size="14" />
        </a>
      </nav>
      <div class="landing-nav-actions">
        <ThemeSwitcher />
        <a :href="DOWNLOAD_URL" class="button primary nav-start">
          立即下载
          <Icon name="plus" :size="14" />
        </a>
      </div>
    </header>
    <main id="main-content">
      <section class="landing-hero" aria-labelledby="hero-title">
        <BackgroundBeams />
        <div class="hero-inner">
          <div class="hero-copy">
            <span class="hero-eyebrow">
              <span />
              用骰子讲概率，用一束光讲相对论
            </span>
            <h1 id="hero-title">
              再难的概念，
              <br />
              <span>也能讲得有趣。</span>
            </h1>
            <p class="hero-description">
              不是把课件念一遍，
              <br />
              而是把抽象的概念，变成让人愿意看下去的画面。
            </p>
            <div class="hero-actions">
              <a :href="DOWNLOAD_URL" class="button primary hero-cta">
                立即体验
                <Icon name="arrowRight" :size="17" />
              </a>
              <button class="hero-watch" @click="openPreview">
                <span>
                  <Icon name="play" :size="13" />
                </span>
                观看示例
              </button>
            </div>
            <p class="hero-demo-note">
              <Icon name="info" :size="12" />
              桌面应用 · 交互式创作 · 随时改、随时导出
            </p>
          </div>
          <div class="hero-product" aria-label="ExplainCraft 工作台示例">
            <div class="hero-product-top">
              <span>
                <Icon name="aperture" :size="14" />
                我的创作空间
              </span>
              <span>
                <i />
                示例已就绪
              </span>
              <span class="product-dots">•••</span>
            </div>
            <div class="hero-product-content">
              <div class="product-rail">
                <Icon name="folder" :size="17" />
                <Icon name="image" :size="17" />
                <Icon name="music" :size="17" />
                <Icon name="sliders" :size="17" />
              </div>
              <div class="product-stage">
                <div class="product-stage-heading">
                  <span>一束光，讲清楚相对论</span>
                  <span>16:9 · 30s</span>
                </div>
                <button
                  class="product-video-button"
                  aria-label="预览相对论示例视频"
                  @click="openPreview"
                >
                  <video
                    ref="heroVideo"
                    :src="mediaUrl('relativity-card.mp4')"
                    :poster="mediaUrl('relativity-spacetime.jpg')"
                    :autoplay="!reducedMotion"
                    muted
                    loop
                    playsinline
                    @timeupdate="heroTime = heroVideo?.currentTime ?? 0"
                    preload="metadata"
                    aria-hidden="true"
                  />
                  <span class="product-play">
                    <Icon name="play" :size="19" />
                  </span>
                </button>
                <div class="product-transport">
                  <Icon name="play" :size="10" />
                  <span>
                    00:{{ String(Math.floor(heroTime)).padStart(2, "0") }}
                    <small>/ 00:30</small>
                  </span>
                  <i><b :style="{ width: `${(heroTime / 30) * 100}%` }" /></i>
                  <Icon name="fullscreen" :size="11" />
                </div>
                <div class="product-timeline">
                  <div>
                    <span>视频</span>
                    <i
                      v-for="image in [
                        'rel-thumb-1',
                        'rel-thumb-2',
                        'rel-thumb-3',
                        'rel-thumb-4',
                      ]"
                      :key="image"
                    >
                      <img :src="`media/${image}.jpg`" alt="" />
                    </i>
                  </div>
                  <div>
                    <span>配乐</span>
                    <b>
                      <i
                        v-for="bar in 54"
                        :key="bar"
                        :style="{ height: `${3 + ((bar * 7) % 12)}px` }"
                      />
                    </b>
                  </div>
                </div>
              </div>
            </div>
            <div class="product-copilot-note">
              <span>
                <Icon name="sparkles" :size="17" />
              </span>
              <div>
                <strong>你的讲解，由你定义。</strong>
                <p>“第二个镜头停在光速那一帧，节奏再慢一点。”</p>
              </div>
              <Icon name="arrowUp" :size="16" />
            </div>
          </div>
        </div>
        <div class="hero-bottom">
          <span>从一个知识点开始。</span>
          <a href="#features">
            看看能做什么
            <Icon name="down" :size="13" />
          </a>
          <span>01 — EXPLAINCRAFT STUDIO</span>
        </div>
      </section>
      <section
        id="features"
        class="landing-features"
        aria-labelledby="features-title"
      >
        <div class="landing-section-heading">
          <div>
            <span class="section-kicker">BUILT FOR EXPLAINING</span>
            <h2 id="features-title">
              <span>一支让人想学的片，</span>
              <span>长这样。</span>
            </h2>
          </div>
          <p>
            先想清楚讲什么，
            <br />
            再让画面跟上。
          </p>
        </div>
        <div
          id="workflow"
          class="landing-workflow landing-workflow-inline"
          aria-labelledby="workflow-title"
        >
          <h3 id="workflow-title" class="workflow-inline-title">制作流程</h3>
          <p class="workflow-lead">
            通用流程自己写；Vibe 大赏模板，照着那一系列的手感来。
          </p>
          <div class="workflow-tabs" role="tablist" aria-label="制作流程分支">
            <button
              type="button"
              role="tab"
              :aria-selected="activePath === 'generic'"
              :class="{ active: activePath === 'generic' }"
              @click="activePath = 'generic'"
            >
              通用流程图
            </button>
            <button
              type="button"
              role="tab"
              :aria-selected="activePath === 'vibe'"
              :class="{ active: activePath === 'vibe' }"
              @click="activePath = 'vibe'"
            >
              Vibe 知识大赏流程图
            </button>
          </div>
          <div
            v-show="activePath === 'generic'"
            class="workflow-panel"
            role="tabpanel"
          >
            <ol>
              <li>
                <span>01</span>
                <div>
                  <h3>说清知识点</h3>
                  <p>一句话，或一份资料。</p>
                </div>
              </li>
              <li>
                <span>02</span>
                <div>
                  <h3>边看边改</h3>
                  <p>脚本、分镜，用对话调整。</p>
                </div>
              </li>
              <li>
                <span>03</span>
                <div>
                  <h3>预览后导出</h3>
                  <p>满意了，导出 MP4。</p>
                </div>
              </li>
            </ol>
          </div>
          <div
            v-show="activePath === 'vibe'"
            class="workflow-panel"
            role="tabpanel"
          >
            <ol class="vibe-steps">
              <li>
                <span>01</span>
                <div>
                  <h3>选「Vibe 知识大赏」模板</h3>
                  <p>新建时选它，分镜、配色、字幕节奏都按这一系列。</p>
                </div>
              </li>
              <li>
                <span>02</span>
                <div>
                  <h3>
                    BGM 自动锁定
                    <em class="step-plus">+</em>
                    AI 生成风格提示词
                  </h3>
                  <p>BGM 不可更换；AI 按这一系列的语气与结构写提示词。</p>
                </div>
              </li>
              <li>
                <span>03</span>
                <div>
                  <h3>提交素材，生成成片</h3>
                  <p>文字、图片、片段按提示词归位。</p>
                </div>
              </li>
            </ol>
            <div class="vibe-board">
              <figure v-for="(shot, i) in vibeShots" :key="shot.label">
                <img :src="mediaUrl(shot.img)" alt="" loading="lazy" />
                <figcaption>
                  <b>{{ String(i + 1).padStart(2, "0") }}</b>
                  {{ shot.label }}
                  <span>{{ shot.dur }}</span>
                </figcaption>
              </figure>
            </div>
            <p class="workflow-lock">
              <Icon name="lock" :size="13" />
              背景音乐已锁定 · 这一系列每期都是同一首
            </p>
          </div>
        </div>
        <div v-show="activePath === 'generic'" class="features-cards">
          <BentoGrid>
            <BentoGridItem
              title="讲清知识点"
              description="说出知识点，生成分镜脚本；文案和镜头都能改。"
              wide
            >
              <template #header>
                <div class="feature-script">
                  <div class="script-prompt">
                    <Icon name="sparkles" :size="14" />
                    用一束光，讲清楚相对论。
                    <span>↵</span>
                  </div>
                  <div class="script-result">
                    <span>
                      <Icon name="file" :size="14" />
                      分镜脚本
                      <small>4 个镜头</small>
                    </span>
                    <div
                      v-for="(line, index) in [
                        '光速对谁都不变。',
                        '运动越快，时间走得越慢。',
                        '引力，是时空的弯曲。',
                      ]"
                      :key="line"
                    >
                      <i>{{ String(index + 1).padStart(2, "0") }}</i>
                      <p>{{ line }}</p>
                      <small>{{ index ? "08" : "07" }}s</small>
                    </div>
                  </div>
                </div>
              </template>
              <template #icon>
                <Icon name="file" :size="18" />
              </template>
              <template #action>
                <a :href="DOWNLOAD_URL" class="bento-action">
                  立即下载
                  <Icon name="arrowRight" :size="13" />
                </a>
              </template>
            </BentoGridItem>
            <BentoGridItem
              title="配好声音"
              description="配置音色、语速与背景音乐，由桌面端生成配音和配乐。"
            >
              <template #header>
                <div class="feature-audio" :class="{ playing: musicPlaying }">
                  <div class="audio-avatar">
                    <Icon name="mic" :size="23" />
                  </div>
                  <span>为讲解，配上合适的声音。</span>
                  <div class="feature-wave">
                    <i
                      v-for="bar in 42"
                      :key="bar"
                      :style="{
                        height: `${8 + ((bar * 13) % 32)}px`,
                        animationDelay: `${-bar / 9}s`,
                      }"
                    />
                  </div>
                  <small>示例配乐 · 非 TTS 音色</small>
                </div>
              </template>
              <template #icon>
                <Icon name="mic" :size="18" />
              </template>
              <template #action>
                <button
                  class="bento-action"
                  :aria-pressed="musicPlaying"
                  @click="toggleMusic"
                >
                  <Icon
                    :name="musicPlaying ? 'pause' : 'headphones'"
                    :size="13"
                  />
                  {{ musicPlaying ? "停止试听" : "试听示例配乐" }}
                </button>
              </template>
            </BentoGridItem>
            <BentoGridItem
              title="生成画面"
              description="视觉模型绘制 SVG 动画，Quercus 完成剪辑与导出，并保留历史版本。"
            >
              <template #header>
                <div class="feature-motion">
                  <video
                    ref="motionVideo"
                    :src="mediaUrl('relativity-card.mp4')"
                    :poster="mediaUrl('relativity-spacetime.jpg')"
                    :autoplay="!reducedMotion"
                    muted
                    loop
                    playsinline
                    preload="metadata"
                    aria-hidden="true"
                  />
                  <span>
                    <Icon name="video" :size="12" />
                    本地动态示例
                  </span>
                  <small>一束光，讲清楚相对论</small>
                </div>
              </template>
              <template #icon>
                <Icon name="video" :size="18" />
              </template>
              <template #action>
                <a :href="DOWNLOAD_URL" class="bento-action">
                  立即下载
                  <Icon name="arrowRight" :size="13" />
                </a>
              </template>
            </BentoGridItem>
            <BentoGridItem
              title="预览导出"
              description="按镜头预览、切换版本，满意后导出 MP4。"
              wide
            >
              <template #header>
                <div class="feature-preview">
                  <div class="preview-example">
                    <img
                      :src="mediaUrl('relativity-spacetime.jpg')"
                      alt="相对论示例画面"
                    />
                    <button aria-label="播放预览示例" @click="openPreview">
                      <Icon name="play" :size="18" />
                    </button>
                    <span>一束光，讲清楚相对论</span>
                  </div>
                  <div class="feature-export">
                    <span>
                      <i />
                      作品已就绪
                    </span>
                    <strong>每个细节，都值得确认。</strong>
                    <div>
                      <span>MP4</span>
                      <span>720p</span>
                      <span>30 fps</span>
                    </div>
                    <a :href="DOWNLOAD_URL">
                      立即下载
                      <Icon name="export" :size="14" />
                    </a>
                  </div>
                </div>
              </template>
              <template #icon>
                <Icon name="play" :size="18" />
              </template>
              <template #action>
                <button class="bento-action" @click="openPreview">
                  看看示例成片
                  <Icon name="arrowRight" :size="13" />
                </button>
              </template>
            </BentoGridItem>
          </BentoGrid>
        </div>
        <p v-if="mediaError" class="landing-media-error" role="status">
          {{ mediaError }}
        </p>
      </section>
    </main>
    <footer class="landing-footer">
      <a href="./" class="landing-brand">
        <Icon name="aperture" :size="21" />
        ExplainCraft
      </a>
      <span>把知识，讲给更多人听。</span>
      <p>桌面应用 · 本地保存 · 断点续做</p>
      <a :href="DOWNLOAD_URL">
        立即下载
        <Icon name="arrowRight" :size="12" />
      </a>
    </footer>
    <Modal
      :open="previewOpen"
      title="一束光，讲清楚相对论"
      subtitle="内置示例成片 · 30 秒 · 720p MP4"
      wide
      @close="closePreview"
    >
      <video
        ref="player"
        class="landing-modal-video"
        :src="mediaUrl('relativity-16x9.mp4')"
        controls
        autoplay
        playsinline
        aria-label="首页示例视频"
      />
    </Modal>
  </div>
</template>
