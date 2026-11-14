// Cargo 会改写 Windows 的 DLL 搜索路径；测试使用与安装包相同的媒体 DLL。
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

assert.equal(process.platform, 'win32', '此入口只用于 Windows 原生测试');
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const runtime = path.join(root, 'src-tauri/runtime');
const executable = path.resolve(process.argv[2]);
const dlls = fs.readdirSync(runtime).filter((name) => name.endsWith('.dll'));
assert.ok(dlls.length >= 7, '测试缺少完整的 FFmpeg DLL');
// 将 DLL 放在测试程序旁，避免构建目录或系统工具中的同名库抢先加载。
for (const name of dlls)
  fs.copyFileSync(path.join(runtime, name), path.join(path.dirname(executable), name));
const env = { ...process.env };
const searchPath = env.PATH ?? env.Path ?? '';
for (const key of Object.keys(env)) if (key.toLowerCase() === 'path') delete env[key];
env.PATH = `${runtime};${searchPath}`;
env.FFMPEG_PATH = path.join(runtime, 'ffmpeg.exe');
env.FFPROBE_PATH = path.join(runtime, 'ffprobe.exe');
const result = spawnSync(executable, process.argv.slice(3), { env, stdio: 'inherit' });
if (result.error) console.error(result.error);
if (result.status !== 0)
  console.error(`Windows test exit: ${result.status}, executable: ${executable}`);
process.exit(result.status ?? 1);
