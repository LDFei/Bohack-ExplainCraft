#!/usr/bin/env bash
# Rust 检查与正式打包共用同一套 FFmpeg 8 SDK 和原生构建依赖。
set -euo pipefail
sudo apt-get update
sudo apt-get install --no-install-recommends -y \
  libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf \
  clang cmake nasm pkg-config libmpg123-dev libzstd-dev \
  libfontconfig1-dev libfreetype6-dev libasound2-dev libjack-jackd2-dev \
  libpipewire-0.3-dev libx264-dev zlib1g-dev fonts-noto-cjk ffmpeg
sdk="${EXPLAINCRAFT_FFMPEG_SDK:-$HOME/.cache/explaincraft-ffmpeg-8}"
if ! test -f "$sdk/lib/pkgconfig/libavutil.pc"; then
  scratch=$(mktemp -d)
  trap 'rm -rf "$scratch"' EXIT
  curl -fsSL https://ffmpeg.org/releases/ffmpeg-8.0.1.tar.xz -o "$scratch/ffmpeg.tar.xz"
  tar -xJf "$scratch/ffmpeg.tar.xz" -C "$scratch"
  (
    cd "$scratch/ffmpeg-8.0.1"
    # 禁用自动探测时必须显式打开 zlib，否则 PNG 字幕导入缺少解码器。
    ./configure --prefix="$sdk" --disable-shared --enable-static --enable-gpl --enable-libx264 --enable-zlib \
      --disable-programs --disable-doc --disable-debug --disable-network --disable-autodetect
    make -j"${CARGO_BUILD_JOBS:-2}"
    make install
  )
fi
if test -n "${GITHUB_ENV:-}"; then
  echo "FFMPEG_DIR=$sdk" >> "$GITHUB_ENV"
fi
# SDK 的绝对路径不变时 Cargo 无法感知编解码配置变化，清掉嵌入旧静态库的两个中间产物。
cargo clean -p ffmpeg-sys-next -p quercus-ffmpeg-link
PKG_CONFIG_PATH="$sdk/lib/pkgconfig" pkg-config --modversion libavutil
