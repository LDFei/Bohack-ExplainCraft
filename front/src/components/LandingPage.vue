<script setup lang="ts">
/** 独立产品官网：展示桌面端能力与本地示例，所有创作入口统一下载桌面应用。 */
import { onBeforeUnmount, onMounted, ref } from 'vue';
import BackgroundBeams from './aceternity/BackgroundBeams.vue';
import BentoGrid from './aceternity/BentoGrid.vue';
import BentoGridItem from './aceternity/BentoGridItem.vue';
import Icon from './Icon.vue';
import Modal from './Modal.vue';
import { mediaUrl } from '../domain/assets.ts';
import { DOWNLOAD_URL } from '../domain/links.ts';
import ThemeSwitcher from './ThemeSwitcher.vue';
const previewOpen = ref(false),
  musicPlaying = ref(false),
  mediaError = ref('');
const player = ref<HTMLVideoElement | null>(null),
  heroVideo = ref<HTMLVideoElement | null>(null),
  motionVideo = ref<HTMLVideoElement | null>(null);
const heroTime = ref(0);
const media = window.matchMedia('(prefers-reduced-motion: reduce)');
const reducedMotion = ref(media.matches);
/** 装饰性视频遵循系统减少动态效果；用户主动打开的示例播放器保持可操作。 */
function syncMotion() {
  reducedMotion.value = media.matches;
  for (const video of [heroVideo.value, motionVideo.value]) {
    if (media.matches) video?.pause();
    else if (video) void video.play().catch(() => {});
  }
}
onMounted(() => media.addEventListener('change', syncMotion));
let audio: HTMLAudioElement | null = null;
/** 试听官网内置的背景音乐；桌面端的配音能力由 Rust 后端供应商提供。 */
async function toggleMusic() {
  mediaError.value = '';
  if (musicPlaying.value) {
    stopMusic();
    return;
  }
  audio = new Audio(mediaUrl('ambient.wav'));
  audio.onended = stopMusic;
  audio.onerror = () => {
    stopMusic();
    mediaError.value = '示例音乐暂时无法读取，请稍后重试。';
  };
  try {
    await audio.play();
    musicPlaying.value = true;
  } catch {
    stopMusic();
    mediaError.value = '请再次点击试听示例配乐。';
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
  media.removeEventListener('change', syncMotion);
});
</script>
<template>
  <div class="landing-page">
    <a
      class="skip-link"
      href="#main-content"
    >
      跳到主要内容
    </a>
    <header class="landing-nav">
      <a
        href="./"
        class="landing-brand"
        aria-label="ExplainCraft 首页"
      >
        <span class="brand-icon">
          <Icon
            name="aperture"
            :size="27"
          />
        </span>
        <span>
          Explain
          <span class="brand-light">Craft</span>
          <small>IDEAS INTO MOTION</small>
        </span>
      </a>
      <nav aria-label="首页导航">
        <a href="#features">创作能力</a>
        <a href="#workflow">工作流程</a>
        <a :href="DOWNLOAD_URL">
          立即下载
          <Icon
            name="arrowRight"
            :size="14"
          />
        </a>
      </nav>
      <div class="landing-nav-actions">
        <ThemeSwitcher />
        <a
          :href="DOWNLOAD_URL"
          class="button primary nav-start"
        >
          立即下载
          <Icon
            name="plus"
            :size="14"
          />
        </a>
      </div>
    </header>
    <main id="main-content">
      <section
        class="landing-hero"
        aria-labelledby="hero-title"
      >
        <BackgroundBeams />
        <div class="hero-inner">
          <div class="hero-copy">
            <span class="hero-eyebrow">
              <span />
              给每一个想法，一个好镜头
            </span>
            <h1 id="hero-title">
              让灵感，
              <br />
              <span>有声，有画。</span>
            </h1>
            <p class="hero-description">
              从一句想法到一段故事。
              <br />
              在同一个创作空间，打磨脚本、安排声音，
              <br class="hero-desktop-break" />
              让每一个镜头慢慢成形。
            </p>
            <div class="hero-actions">
              <a
                :href="DOWNLOAD_URL"
                class="button primary hero-cta"
              >
                立即下载
                <Icon
                  name="arrowRight"
                  :size="17"
                />
              </a>
              <button
                class="hero-watch"
                @click="openPreview"
              >
                <span>
                  <Icon
                    name="play"
                    :size="13"
                  />
                </span>
                观看示例
              </button>
            </div>
            <p class="hero-demo-note">
              <Icon
                name="info"
                :size="12"
              />
              桌面创作应用 · 项目与生成文件保存在本机
            </p>
          </div>
          <div
            class="hero-product"
            aria-label="ExplainCraft 工作台示例"
          >
            <div class="hero-product-top">
              <span>
                <Icon
                  name="aperture"
                  :size="14"
                />
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
                <Icon
                  name="folder"
                  :size="17"
                />
                <Icon
                  name="image"
                  :size="17"
                />
                <Icon
                  name="music"
                  :size="17"
                />
                <Icon
                  name="sliders"
                  :size="17"
                />
              </div>
              <div class="product-stage">
                <div class="product-stage-heading">
                  <span>咖啡店的温柔早晨</span>
                  <span>16:9 · 30s</span>
                </div>
                <button
                  class="product-video-button"
                  aria-label="预览咖啡店示例视频"
                  @click="openPreview"
                >
                  <video
                    ref="heroVideo"
                    :src="mediaUrl('coffee-16x9.mp4')"
                    :poster="mediaUrl('coffee-shop.jpg')"
                    :autoplay="!reducedMotion"
                    muted
                    loop
                    playsinline
                    @timeupdate="heroTime = heroVideo?.currentTime ?? 0"
                    preload="metadata"
                    aria-hidden="true"
                  />
                  <span class="product-film-title">
                    <small>A LITTLE MOMENT, A GOOD STORY</small>
                    一杯咖啡的慢时光
                  </span>
                  <span class="product-play">
                    <Icon
                      name="play"
                      :size="19"
                    />
                  </span>
                </button>
                <div class="product-transport">
                  <Icon
                    name="play"
                    :size="10"
                  />
                  <span>
                    00:{{ String(Math.floor(heroTime)).padStart(2, '0') }}
                    <small>/ 00:30</small>
                  </span>
                  <i><b :style="{ width: `${(heroTime / 30) * 100}%` }" /></i>
                  <Icon
                    name="fullscreen"
                    :size="11"
                  />
                </div>
                <div class="product-timeline">
                  <div>
                    <span>视频</span>
                    <i
                      v-for="image in ['coffee-shop', 'coffee-brew', 'coffee-cup', 'coffee-detail']"
                      :key="image"
                    >
                      <img
                        :src="`media/${image}.jpg`"
                        alt=""
                      />
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
                <Icon
                  name="sparkles"
                  :size="17"
                />
              </span>
              <div>
                <strong>你的故事，由你定义。</strong>
                <p>“第二个镜头换成咖啡特写，节奏再慢一点。”</p>
              </div>
              <Icon
                name="arrowUp"
                :size="16"
              />
            </div>
          </div>
        </div>
        <div class="hero-bottom">
          <span>从灵感出发，按你的节奏创作。</span>
          <a href="#features">
            探索创作可能
            <Icon
              name="down"
              :size="13"
            />
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
            <span class="section-kicker">A COMPLETE CREATIVE SPACE</span>
            <h2 id="features-title">
              <span>故事的每一步，</span>
              <span>都在这里。</span>
            </h2>
          </div>
          <p>
            把分散的创作步骤，放回同一个空间。
            <br />
            先把想法表达清楚，再逐个打磨细节。
          </p>
        </div>
        <BentoGrid>
          <BentoGridItem
            title="AI 脚本生成"
            description="写下一句想法，由创作方案生成分镜脚本。文案可编辑，镜头可继续调整。"
            wide
          >
            <template #header>
              <div class="feature-script">
                <div class="script-prompt">
                  <Icon
                    name="sparkles"
                    :size="14"
                  />
                  帮我做一个温暖的咖啡店短片。
                  <span>↵</span>
                </div>
                <div class="script-result">
                  <span>
                    <Icon
                      name="file"
                      :size="14"
                    />
                    分镜脚本
                    <small>4 个镜头</small>
                  </span>
                  <div
                    v-for="(line, index) in [
                      '城市醒来之前，先留一点时间给自己。',
                      '听水流与咖啡相遇，让熟悉的香气慢慢散开。',
                      '一杯刚刚好的温度，装下生活的小小美好。',
                    ]"
                    :key="line"
                  >
                    <i>{{ String(index + 1).padStart(2, '0') }}</i>
                    <p>{{ line }}</p>
                    <small>{{ index ? '08' : '07' }}s</small>
                  </div>
                </div>
              </div>
            </template>
            <template #icon>
              <Icon
                name="file"
                :size="18"
              />
            </template>
            <template #action>
              <a
                :href="DOWNLOAD_URL"
                class="bento-action"
              >
                立即下载
                <Icon
                  name="arrowRight"
                  :size="13"
                />
              </a>
            </template>
          </BentoGridItem>
          <BentoGridItem
            title="语音合成与配乐"
            description="配置音色、语速与背景音乐，由桌面端生成配音和配乐。"
          >
            <template #header>
              <div
                class="feature-audio"
                :class="{ playing: musicPlaying }"
              >
                <div class="audio-avatar">
                  <Icon
                    name="mic"
                    :size="23"
                  />
                </div>
                <span>为故事，找到合适的声音。</span>
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
              <Icon
                name="mic"
                :size="18"
              />
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
                {{ musicPlaying ? '停止试听' : '试听示例配乐' }}
              </button>
            </template>
          </BentoGridItem>
          <BentoGridItem
            title="动态视频生成"
            description="在对话中制作与修改，查看每个阶段的进度，并保留历史版本。"
          >
            <template #header>
              <div class="feature-motion">
                <video
                  ref="motionVideo"
                  :src="mediaUrl('travel-16x9.mp4')"
                  :poster="mediaUrl('travel-mountain.jpg')"
                  :autoplay="!reducedMotion"
                  muted
                  loop
                  playsinline
                  preload="metadata"
                  aria-hidden="true"
                />
                <span>
                  <Icon
                    name="video"
                    :size="12"
                  />
                  本地动态示例
                </span>
                <small>去山野，找回自己</small>
              </div>
            </template>
            <template #icon>
              <Icon
                name="video"
                :size="18"
              />
            </template>
            <template #action>
              <a
                :href="DOWNLOAD_URL"
                class="bento-action"
              >
                立即下载
                <Icon
                  name="arrowRight"
                  :size="13"
                />
              </a>
            </template>
          </BentoGridItem>
          <BentoGridItem
            title="视频预览与导出"
            description="在时间轴上浏览镜头，切换版本，预览声音与画面，再把确认好的作品导出为 MP4。"
            wide
          >
            <template #header>
              <div class="feature-preview">
                <div class="preview-example">
                  <img
                    :src="mediaUrl('coffee-cup.jpg')"
                    alt="咖啡店示例画面"
                  />
                  <button
                    aria-label="播放预览示例"
                    @click="openPreview"
                  >
                    <Icon
                      name="play"
                      :size="18"
                    />
                  </button>
                  <span>一杯咖啡的慢时光</span>
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
                    <Icon
                      name="export"
                      :size="14"
                    />
                  </a>
                </div>
              </div>
            </template>
            <template #icon>
              <Icon
                name="play"
                :size="18"
              />
            </template>
            <template #action>
              <button
                class="bento-action"
                @click="openPreview"
              >
                看看示例成片
                <Icon
                  name="arrowRight"
                  :size="13"
                />
              </button>
            </template>
          </BentoGridItem>
        </BentoGrid>
        <p
          v-if="mediaError"
          class="landing-media-error"
          role="status"
        >
          {{ mediaError }}
        </p>
      </section>
      <section
        id="workflow"
        class="landing-workflow"
        aria-labelledby="workflow-title"
      >
        <div>
          <span class="section-kicker">FROM IDEA TO STORY</span>
          <h2 id="workflow-title">
            <span>创作可以复杂，</span>
            <span>开始可以简单。</span>
          </h2>
        </div>
        <ol>
          <li>
            <span>01</span>
            <div>
              <h3>说出你的想法</h3>
              <p>一句话、一个主题，或一份自己的素材。</p>
            </div>
          </li>
          <li>
            <span>02</span>
            <div>
              <h3>一起打磨细节</h3>
              <p>查看脚本与镜头，通过对话继续调整。</p>
            </div>
          </li>
          <li>
            <span>03</span>
            <div>
              <h3>预览，再带走</h3>
              <p>确认当前版本，下载或导出你的作品。</p>
            </div>
          </li>
        </ol>
        <a
          :href="DOWNLOAD_URL"
          class="button primary"
        >
          立即下载
          <Icon
            name="arrowRight"
            :size="15"
          />
        </a>
      </section>
    </main>
    <footer class="landing-footer">
      <a
        href="./"
        class="landing-brand"
      >
        <Icon
          name="aperture"
          :size="21"
        />
        ExplainCraft
      </a>
      <span>给灵感一个镜头。</span>
      <p>Rust + Tauri 桌面应用 · 本地项目与可恢复制作任务</p>
      <a :href="DOWNLOAD_URL">
        立即下载
        <Icon
          name="arrowRight"
          :size="12"
        />
      </a>
    </footer>
    <Modal
      :open="previewOpen"
      title="一杯咖啡的慢时光"
      subtitle="内置示例成片 · 30 秒 · 720p MP4"
      wide
      @close="closePreview"
    >
      <video
        ref="player"
        class="landing-modal-video"
        :src="mediaUrl('coffee-16x9.mp4')"
        controls
        autoplay
        playsinline
        aria-label="首页示例视频"
      />
    </Modal>
  </div>
</template>
