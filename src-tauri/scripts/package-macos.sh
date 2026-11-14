#!/bin/bash
# 将动态库收进应用，验证安装后的 FFmpeg 无需 Homebrew 路径。
set -euo pipefail
app=target/release/bundle/macos/Lingjing.app
frameworks="$app/Contents/Frameworks"
mkdir -p "$frameworks"
for executable in "$app/Contents/MacOS/lingjing" "$app/Contents/Resources/runtime/ffmpeg" "$app/Contents/Resources/runtime/ffprobe"; do
  dylibbundler -of -b -x "$executable" -d "$frameworks" -p @rpath/
done
install_name_tool -add_rpath @executable_path/../Frameworks "$app/Contents/MacOS/lingjing"
for tool in ffmpeg ffprobe; do
  executable="$app/Contents/Resources/runtime/$tool"
  install_name_tool -add_rpath @executable_path/../../Frameworks "$executable"
  env -i PATH=/usr/bin:/bin "$executable" -version
  codesign --force --sign - "$executable"
done
find "$frameworks" -name '*.dylib' -exec codesign --force --sign - {} \;
codesign --force --deep --sign - "$app"
codesign --verify --deep --strict "$app"
mkdir -p dist-release
arch=$(uname -m)
hdiutil create -volname ExplainCraft -srcfolder "$app" -ov -format UDZO "dist-release/ExplainCraft_0.1.1_macos_${arch}.dmg"
