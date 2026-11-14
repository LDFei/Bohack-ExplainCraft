#!/bin/bash
# 将动态库收进应用，验证安装后的 FFmpeg 无需 Homebrew 路径。
set -euo pipefail
app=target/release/bundle/macos/Lingjing.app
frameworks="$app/Contents/Frameworks"
mkdir -p "$frameworks"
for executable in "$app/Contents/MacOS/lingjing" "$app/Contents/Resources/runtime/ffmpeg" "$app/Contents/Resources/runtime/ffprobe"; do
  dylibbundler -of -b -x "$executable" -d "$frameworks" -p @rpath/
done
# 已有的 rpath 保留；重试打包时也能安全执行。
add_rpath() {
  if ! otool -l "$1" | grep -F "path $2 (offset" > /dev/null; then
    install_name_tool -add_rpath "$2" "$1"
  fi
}
add_rpath "$app/Contents/MacOS/lingjing" @executable_path/../Frameworks
for tool in ffmpeg ffprobe; do
  executable="$app/Contents/Resources/runtime/$tool"
  add_rpath "$executable" @executable_path/../../Frameworks
done
# 安装到没有 Homebrew 的电脑后也必须能找到全部动态库。
for executable in "$app/Contents/MacOS/lingjing" "$app/Contents/Resources/runtime/ffmpeg" "$app/Contents/Resources/runtime/ffprobe" "$frameworks"/*.dylib; do
  if otool -L "$executable" | grep -E '/(opt/homebrew|usr/local)/'; then
    echo "Unbundled dependency in $executable" >&2
    exit 1
  fi
done
find "$frameworks" -name '*.dylib' -exec codesign --force --sign - {} \;
for tool in ffmpeg ffprobe; do
  codesign --force --sign - "$app/Contents/Resources/runtime/$tool"
done
codesign --force --deep --sign - "$app"
codesign --verify --deep --strict "$app"
for tool in ffmpeg ffprobe; do
  env -i PATH=/usr/bin:/bin "$app/Contents/Resources/runtime/$tool" -version
done
mkdir -p dist-release
arch=$(uname -m)
hdiutil create -volname ExplainCraft -srcfolder "$app" -ov -format UDZO "dist-release/ExplainCraft_0.1.1_macos_${arch}.dmg"
