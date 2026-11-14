/** 短期播放地址在到期前更新，恢复播放位置；切换素材后丢弃旧请求。 */
import { nextTick, onBeforeUnmount, onMounted, ref, watch, type Ref } from 'vue';
import { api } from '../services/api.ts';

/** 共用成片与上传视频的续期逻辑，避免播放器持有过期地址直到用户手动刷新。 */
export function useSignedPlayback(
  player: Ref<HTMLVideoElement | null>,
  assetId: () => string | undefined,
  sourceUrl: () => string | undefined,
  expiresAt: () => string | undefined,
) {
  const url = ref('');
  let request = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let expires = 0;
  let disposed = false;
  /** 提前30秒续期；后台挂起后由可见性事件补检，失败保留当前地址并稍后重试。 */
  function schedule() {
    clearTimeout(timer);
    if (!assetId() || !expires || disposed) return;
    timer = setTimeout(
      () =>
        void renew().catch(() => {
          if (!disposed) timer = setTimeout(() => void renew().catch(() => undefined), 15000);
        }),
      Math.max(1000, Math.min(2_147_000_000, expires - Date.now() - 30000)),
    );
  }
  /** 续期只更换访问地址，仍展示同一文件，不重新渲染或转码已生成成片。 */
  async function renew() {
    const id = assetId();
    if (!id) return;
    const token = ++request;
    const result = await api.preview(id);
    if (token !== request || disposed || id !== assetId()) return;
    const element = player.value;
    const position = element?.currentTime ?? 0;
    const wasPlaying = element ? !element.paused : false;
    url.value = result.playback_url;
    expires = Date.parse(result.expires_at);
    await nextTick();
    if (element && element === player.value) {
      const restore = () => {
        if (token !== request || disposed) return;
        if (Number.isFinite(element.duration))
          element.currentTime = Math.min(position, element.duration);
        if (wasPlaying) void element.play().catch(() => undefined);
      };
      if (element.readyState >= 1) restore();
      else element.addEventListener('loadedmetadata', restore, { once: true });
    }
    schedule();
  }
  /** 页面恢复前台时处理被系统暂停的计时器。 */
  function checkExpiry() {
    if (document.visibilityState === 'visible' && expires && expires - Date.now() < 30000) {
      void renew().catch(() => undefined);
    }
  }
  watch(
    () => [assetId(), sourceUrl(), expiresAt()],
    () => {
      request++;
      clearTimeout(timer);
      url.value = sourceUrl() ?? '';
      expires = Date.parse(expiresAt() ?? '');
      schedule();
    },
    { immediate: true },
  );
  onMounted(() => {
    document.addEventListener('visibilitychange', checkExpiry);
    window.addEventListener('focus', checkExpiry);
  });
  onBeforeUnmount(() => {
    disposed = true;
    request++;
    clearTimeout(timer);
    document.removeEventListener('visibilitychange', checkExpiry);
    window.removeEventListener('focus', checkExpiry);
  });
  return { url, renew };
}
