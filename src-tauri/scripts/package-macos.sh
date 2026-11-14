#!/bin/bash
# 将动态库收进应用，验证安装后的 FFmpeg 无需 Homebrew 路径。
set -euo pipefail
app=target/release/bundle/macos/Lingjing.app
frameworks="$app/Contents/Frameworks"
mkdir -p "$frameworks"
# OCIO 的附属动态库位于 Cargo 构建目录，不一定在 Homebrew 中。
search_args=(-s "$(brew --prefix)/lib/" -s "$frameworks/")
while IFS= read -r directory; do
  search_args+=(-s "$(cd "$directory" && pwd)/")
done < <(find target/release/build -name '*.dylib' -exec dirname {} \; | sort -u)
for executable in "$app/Contents/MacOS/lingjing" "$app/Contents/Resources/runtime/ffmpeg" "$app/Contents/Resources/runtime/ffprobe"; do
  # 缺少库时立即报错，禁止在无人值守的构建机上等待交互输入。
  dylibbundler -ns -of -b -x "$executable" "${search_args[@]}" -d "$frameworks" -p @rpath/ <<< quit
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
