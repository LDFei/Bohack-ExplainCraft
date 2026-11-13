"""Rebuild the bundled, original slideshow demos with local photos and a synthesized ambient bed."""
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
import array
import math
import subprocess
import wave

ROOT = Path(__file__).resolve().parents[1]
MEDIA = ROOT / 'public' / 'media'
audio = MEDIA / 'ambient.wav'
if not audio.exists():
    rate = 24000
    samples = array.array('h')
    chords = [(130.81, 164.81, 196), (110, 130.81, 164.81), (87.31, 110, 130.81), (98, 123.47, 146.83)]
    for index in range(rate * 30):
        t = index / rate
        chord = chords[min(3, int(t / 7.5))]
        envelope = min(1, t / 2, (30 - t) / 3) * (0.72 + 0.12 * math.sin(t * 0.7))
        local = t % 7.5
        value = sum(math.sin(2 * math.pi * f * t) * math.exp(-local * 0.16) for f in chord) / 3
        samples.append(int(value * envelope * 1700))
    with wave.open(str(audio), 'wb') as output:
        output.setnchannels(1)
        output.setsampwidth(2)
        output.setframerate(rate)
        output.writeframes(samples.tobytes())

themes = {
    'coffee': ['coffee-shop', 'coffee-brew', 'coffee-cup', 'coffee-detail'],
    'travel': ['travel-road', 'travel-mountain', 'travel-lake', 'travel-road'],
    'product': ['product-bottle'] * 4,
}
ratios = {'9x16': (720, 1280), '16x9': (1280, 720), '1x1': (720, 720)}

def build(task):
    theme, ratio = task
    destination = MEDIA / f'{theme}-{ratio}.mp4'
    if destination.exists():
        return f'{destination.name}: already exists'
    width, height = ratios[ratio]
    command = ['ffmpeg', '-hide_banner', '-loglevel', 'error', '-y']
    for image, duration in zip(themes[theme], [7, 8, 8, 7]):
        command += ['-loop', '1', '-framerate', '30', '-t', str(duration), '-i', str(MEDIA / f'{image}.jpg')]
    command += ['-i', str(audio)]
    filters = []
    for i, duration in enumerate([7, 8, 8, 7]):
        filters.append(f"[{i}:v]scale={width}:{height}:force_original_aspect_ratio=increase,crop={width}:{height},setsar=1,zoompan=z='min(max(zoom,pzoom)+0.00012,1.08)':d=1:s={width}x{height}:fps=30,trim=duration={duration},setpts=PTS-STARTPTS,fade=t=in:st=0:d=0.5,fade=t=out:st={duration-0.5}:d=0.5[v{i}]")
    filters.append('[v0][v1][v2][v3]concat=n=4:v=1:a=0[v]')
    command += ['-filter_complex_threads', '1', '-filter_complex', ';'.join(filters), '-map', '[v]', '-map', '4:a', '-t', '30', '-c:v', 'libx264', '-preset', 'veryfast', '-crf', '28', '-threads', '2', '-pix_fmt', 'yuv420p', '-c:a', 'aac', '-b:a', '64k', '-movflags', '+faststart', str(destination)]
    subprocess.run(command, check=True)
    return f'{destination.name}: {destination.stat().st_size} bytes'

if __name__ == '__main__':
    with ThreadPoolExecutor(max_workers=2) as pool:
        for result in pool.map(build, [(theme, ratio) for theme in themes for ratio in ratios]):
            print(result, flush=True)
