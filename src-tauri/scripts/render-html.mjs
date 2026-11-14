/** 离线 HTML/CSS/SMIL 定帧渲染器，只接收本机临时文件，不执行模型脚本。 */
import { spawn } from 'node:child_process';
import { readFile, access, mkdir } from 'node:fs/promises';
import { join } from 'node:path';
import { once } from 'node:events';

const [input, output, profile, widthArg, heightArg, durationArg] = process.argv.slice(2);
const width = Number(widthArg), height = Number(heightArg), duration = Number(durationArg);
let chrome, ffmpeg, socket;
const pending = new Map();
let sequence = 0, sessionId;

/** 只清理本次启动的浏览器和编码器；退出与取消共享相同收尾。 */
function cleanup() {
  socket?.close();
  ffmpeg?.kill('SIGTERM');
  chrome?.kill('SIGTERM');
}
process.once('SIGTERM', () => { cleanup(); process.exit(143); });
process.once('SIGINT', () => { cleanup(); process.exit(130); });

/** 单个 CDP 调用有期限，失去浏览器时不能永久阻塞渲染。 */
function cdp(method, params = {}, session = sessionId) {
  const id = ++sequence;
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => { pending.delete(id); reject(new Error('CDP_TIMEOUT')); }, 20000);
    pending.set(id, { resolve, reject, timer });
    socket.send(JSON.stringify({ id, method, params, ...(session ? { sessionId: session } : {}) }));
  });
}

/** Chrome 必须来自显式配置或系统常见安装；不自动下载和执行远端程序。 */
async function chromeExecutable() {
  const candidates = [process.env.CHROME_PATH,
    '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
    '/usr/bin/google-chrome', '/usr/bin/chromium', '/usr/bin/chromium-browser',
    process.env.PROGRAMFILES && join(process.env.PROGRAMFILES, 'Google/Chrome/Application/chrome.exe'),
    process.env.LOCALAPPDATA && join(process.env.LOCALAPPDATA, 'Google/Chrome/Application/chrome.exe')].filter(Boolean);
  for (const candidate of candidates) { try { await access(candidate); return candidate; } catch {} }
  throw new Error('CHROME_NOT_FOUND');
}

/** 审阅解析后的 DOM 和 CSS，防止编码/大小写绕过前置字符串校验。 */
const validateDocument = `(() => {
  const forbidden = new Set(['SCRIPT','IFRAME','FRAME','OBJECT','EMBED','LINK','BASE','FORM','INPUT','BUTTON','AUDIO','VIDEO','CANVAS']);
  for (const element of document.querySelectorAll('*')) {
    if (forbidden.has(element.tagName.toUpperCase())) throw new Error('UNSAFE_HTML');
    for (const attr of element.attributes) {
      const name = attr.name.toLowerCase(), value = attr.value.trim();
      if (name.startsWith('on') || ['src','srcset','action','formaction','poster','srcdoc'].includes(name)) throw new Error('UNSAFE_HTML');
      if ((name === 'href' || name.endsWith(':href')) && !value.startsWith('#')) throw new Error('UNSAFE_HTML');
    }
  }
  const checkRules = rules => { for (const rule of rules) {
    if (rule.type === 3 || /url\\s*\\(/i.test(rule.cssText) && /url\\s*\\(\\s*['\"]?(?!#)/i.test(rule.cssText)) throw new Error('UNSAFE_HTML');
    if (rule.cssRules) checkRules(rule.cssRules);
  }};
  for (const sheet of document.styleSheets) checkRules(sheet.cssRules);
  return true;
})()`;

/** 通过可信的 DevTools 驱动时钟；模型 HTML 自身没有脚本执行权限。 */
async function evaluate(expression, awaitPromise = false) {
  const result = await cdp('Runtime.evaluate', { expression, awaitPromise, returnByValue: true });
  if (result.exceptionDetails) throw new Error('UNSAFE_HTML');
  return result.result.value;
}

/** 将 PNG 帧写入编码器并遵守背压，避免长视频占满内存。 */
async function writeFrame(bytes) {
  if (!ffmpeg.stdin.write(bytes)) await once(ffmpeg.stdin, 'drain');
}

try {
  if (Number(process.versions.node.split('.')[0]) < 24 || typeof WebSocket === 'undefined') throw new Error('NODE_VERSION');
  if (!input || !output || !profile || !Number.isFinite(duration) || duration < 1 || duration > 60000 || width < 16 || height < 16 || width * height > 2073600 || width % 2 || height % 2) throw new Error('INVALID_RENDER_INPUT');
  await mkdir(profile, { recursive: true });
  chrome = spawn(await chromeExecutable(), ['--headless=new', '--no-first-run', '--no-default-browser-check',
    '--disable-background-networking', '--disable-extensions', '--disable-sync', '--disable-default-apps',
    '--disable-component-update', '--disable-breakpad', '--no-pings', '--metrics-recording-only',
    '--remote-debugging-address=127.0.0.1', '--remote-debugging-port=0', `--user-data-dir=${profile}`, 'about:blank'],
    { stdio: ['ignore', 'ignore', 'ignore'] });
  let endpoint;
  for (let tries = 0; tries < 200; tries++) {
    if (chrome.exitCode !== null) throw new Error('CHROME_START_FAILED');
    try { const [port, path] = (await readFile(join(profile, 'DevToolsActivePort'), 'utf8')).trim().split('\n'); endpoint = `ws://127.0.0.1:${port}${path}`; break; } catch {}
    await new Promise(resolve => setTimeout(resolve, 50));
  }
  if (!endpoint) throw new Error('CHROME_START_FAILED');
  socket = new WebSocket(endpoint);
  await once(socket, 'open');
  socket.addEventListener('message', ({ data }) => {
    const message = JSON.parse(data), item = pending.get(message.id);
    if (!item) return;
    clearTimeout(item.timer); pending.delete(message.id);
    if (message.error) item.reject(new Error('CDP_FAILED')); else item.resolve(message.result);
  });
  socket.addEventListener('close', () => { for (const item of pending.values()) { clearTimeout(item.timer); item.reject(new Error('CHROME_CLOSED')); } pending.clear(); });
  const { targetId } = await cdp('Target.createTarget', { url: 'about:blank' }, null);
  ({ sessionId } = await cdp('Target.attachToTarget', { targetId, flatten: true }, null));
  await cdp('Page.enable'); await cdp('Runtime.enable'); await cdp('Network.enable');
  await cdp('Network.setBypassServiceWorker', { bypass: true });
  await cdp('Network.setBlockedURLs', { urls: ['*'] });
  await cdp('Network.emulateNetworkConditions', { offline: true, latency: 0, downloadThroughput: 0, uploadThroughput: 0 });
  await cdp('Emulation.setDeviceMetricsOverride', { width, height, deviceScaleFactor: 1, mobile: false });
  const source = await readFile(input, 'utf8');
  const csp = "default-src 'none'; style-src 'unsafe-inline'; script-src 'none'; img-src 'none'; font-src 'none'; media-src 'none'; connect-src 'none'; frame-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'";
  const html = `<!doctype html><html><head><meta http-equiv="Content-Security-Policy" content="${csp}"><style>html,body{margin:0;width:${width}px;height:${height}px;overflow:hidden;background:#fff}</style></head><body>${source}</body></html>`;
  const { frameTree } = await cdp('Page.getFrameTree');
  await cdp('Page.setDocumentContent', { frameId: frameTree.frame.id, html });
  await evaluate(validateDocument);
  await evaluate('document.fonts.ready.then(() => { for (const a of document.getAnimations()) { a.pause(); a.currentTime=0; } return true; })', true);
  const frames = Math.max(1, Math.min(1800, Math.ceil(duration / 1000 * 30))), rate = frames * 1000 / duration;
  ffmpeg = spawn(process.env.FFMPEG_PATH || 'ffmpeg', ['-v', 'error', '-nostdin', '-y', '-f', 'image2pipe', '-framerate', String(rate), '-i', 'pipe:0',
    '-an', '-c:v', 'libx264', '-preset', 'veryfast', '-crf', '18', '-pix_fmt', 'yuv420p', '-r', '30', '-t', String(duration / 1000), '-movflags', '+faststart', output], { stdio: ['pipe','ignore','pipe'] });
  let ffmpegError = ''; ffmpeg.stderr.on('data', bytes => { ffmpegError = (ffmpegError + bytes).slice(-4096); });
  const encoded = once(ffmpeg, 'exit');
  ffmpeg.stdin.on('error', () => {});
  for (let frame = 0; frame < frames; frame++) {
    const time = frame * duration / frames;
    await evaluate(`(() => { for (const a of document.getAnimations()) { a.pause(); a.currentTime=${time}; } for(const svg of document.querySelectorAll('svg')) { svg.pauseAnimations?.(); svg.setCurrentTime?.(${time / 1000}); } document.body.getBoundingClientRect(); return true; })()`);
    const { data } = await cdp('Page.captureScreenshot', { format: 'png', fromSurface: true, captureBeyondViewport: false });
    await writeFrame(Buffer.from(data, 'base64'));
  }
  ffmpeg.stdin.end();
  const [code] = await encoded;
  if (code !== 0) { console.error(ffmpegError); throw new Error('FFMPEG_FAILED'); }
  console.log(JSON.stringify({ ok: true, frames, width, height, duration_ms: duration }));
} catch (error) {
  console.error(JSON.stringify({ ok: false, code: error?.message || 'HTML_RENDER_FAILED' }));
  process.exitCode = 1;
} finally { cleanup(); }
