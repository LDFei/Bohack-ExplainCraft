"""用本地图片和自行合成的背景音重建演示媒体；不调用 AI 或下载外部音乐。"""

from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
import array
import math
import subprocess
import wave

# 以脚本所在的 front/ 为资源根，避免从不同终端目录启动时找错图片。
ROOT = Path(__file__).resolve().parents[1]
MEDIA = ROOT / "public" / "media"
audio = MEDIA / "ambient.wav"
# 音乐只合成一次，之后生成九种视频时复用同一个本地音频文件。
if not audio.exists():
    # 单声道 24kHz、16 位 PCM；每个样本都是一个有符号整数幅值。
    rate = 24000
    samples = array.array("h")
    # 每组数值是三条基频（Hz），轮换形成轻量背景和声，不包含人声。
    chords = [
        (130.81, 164.81, 196),
        (110, 130.81, 164.81),
        (87.31, 110, 130.81),
        (98, 123.47, 146.83),
    ]
    for index in range(rate * 30):
        t = index / rate
        chord = chords[min(3, int(t / 7.5))]
        # 包络控制开头淡入、结尾淡出，降低突然出现或消失时的突兀感。
        envelope = min(1, t / 2, (30 - t) / 3) * (0.72 + 0.12 * math.sin(t * 0.7))
        local = t % 7.5
        # 叠加三个正弦波并归一化，再乘较小振幅，避免输出削波。
        value = sum(math.sin(2 * math.pi * f * t) * math.exp(-local * 0.16) for f in chord) / 3
        samples.append(int(value * envelope * 1700))
    with wave.open(str(audio), "wb") as output:
        output.setnchannels(1)
        output.setsampwidth(2)
        output.setframerate(rate)
        output.writeframes(samples.tobytes())

# 主题图片与页面 content.ts 对应；三个主题乘三种比例，共九个示例文件。
themes = {
    "coffee": ["coffee-shop", "coffee-brew", "coffee-cup", "coffee-detail"],
    "travel": ["travel-road", "travel-mountain", "travel-lake", "travel-road"],
    "product": ["product-bottle"] * 4,
}
ratios = {"9x16": (720, 1280), "16x9": (1280, 720), "1x1": (720, 720)}


def build(task):
    """构建一个主题和比例的 30 秒 MP4，已有文件直接复用。

    task 为 (theme, ratio)；返回文件名和体积，便于批量生成时检查产物。
    """
    theme, ratio = task
    destination = MEDIA / f"{theme}-{ratio}.mp4"
    if destination.exists():
        return f"{destination.name}: already exists"
    width, height = ratios[ratio]
    command = ["ffmpeg", "-hide_banner", "-loglevel", "error", "-y"]
    # 四张静态图片作为四路视频输入，分段长度合计 30 秒。
    for image, duration in zip(themes[theme], [7, 8, 8, 7]):
        command += [
            "-loop",
            "1",
            "-framerate",
            "30",
            "-t",
            str(duration),
            "-i",
            str(MEDIA / f"{image}.jpg"),
        ]
    # 第五路输入是背景音，后面 -map 4:a 按从零开始的输入编号选择它。
    command += ["-i", str(audio)]
    # 每个镜头先等比铺满并裁切，再做轻微中心放大、时间戳归零与片头片尾渐隐。
    filters = []
    for i, duration in enumerate([7, 8, 8, 7]):
        filters.append(
            f"[{i}:v]scale={width}:{height}:force_original_aspect_ratio=increase,"
            f"crop={width}:{height},setsar=1,"
            "zoompan=z='min(max(zoom,pzoom)+0.00012,1.08)':"
            f"d=1:s={width}x{height}:fps=30,trim=duration={duration},"
            "setpts=PTS-STARTPTS,fade=t=in:st=0:d=0.5,"
            f"fade=t=out:st={duration - 0.5}:d=0.5[v{i}]"
        )
    # 拼接四段视频，保证它们按脚本顺序形成单一输出流。
    filters.append("[v0][v1][v2][v3]concat=n=4:v=1:a=0[v]")
    # H.264 + yuv420p 兼容常见浏览器；faststart 把媒体索引前置，方便边加载边播放。
    command += [
        "-filter_complex_threads",
        "1",
        "-filter_complex",
        ";".join(filters),
        "-map",
        "[v]",
        "-map",
        "4:a",
        "-t",
        "30",
        "-c:v",
        "libx264",
        "-preset",
        "veryfast",
        "-crf",
        "28",
        "-threads",
        "2",
        "-pix_fmt",
        "yuv420p",
        "-c:a",
        "aac",
        "-b:a",
        "64k",
        "-movflags",
        "+faststart",
        str(destination),
    ]
    subprocess.run(command, check=True)
    return f"{destination.name}: {destination.stat().st_size} bytes"


# 最多并行两次 FFmpeg 编码，避免一次启动九个编码器占满电脑。
if __name__ == "__main__":
    with ThreadPoolExecutor(max_workers=2) as pool:
        for result in pool.map(build, [(theme, ratio) for theme in themes for ratio in ratios]):
            print(result, flush=True)
