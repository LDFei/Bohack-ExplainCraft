# 在 MSVC 开发环境中运行 Tauri CLI（与 src-tauri/cargo.ps1 相同：VS Installer 缺少 vswhere.exe 时 rustc 找不到 link.exe）。
# 用法（在 front/ 目录）：
#   npm run desktop:dev:windows      开发：Vite 热更新 + 桌面窗口 + 内嵌后端（含 quercus 剪辑引擎）
#   npm run desktop:build:windows    打包：生成 Windows 安装程序（NSIS）
#
# PATH 顺序与 cargo.ps1 一致：
# 1. MSYS2 与 FFmpeg 的 bin 排在最前，libclang / FFmpeg 的依赖 DLL 才能加载；
# 2. 保留 vcvars 设置的编译器与 SDK 目录；
# 3. 去掉 D:\ProgramData\CMake\bin，避免其中的 ld.exe 抢占 MSVC 链接器。
$roots = @("$env:ProgramFiles\Microsoft Visual Studio", "${env:ProgramFiles(x86)}\Microsoft Visual Studio")
$vcvars = Get-ChildItem -Path $roots -Filter vcvars64.bat -Recurse -ErrorAction SilentlyContinue |
  Sort-Object FullName -Descending | Select-Object -First 1
if (-not $vcvars) {
  Write-Host '没有找到 vcvars64.bat，请安装 Visual Studio 的“使用 C++ 的桌面开发”组件。'
  exit 1
}
# .cargo/config.toml 把 FFMPEG_DIR 默认成 macOS 的 /opt/homebrew（force=false，不覆盖已有变量），
# tauri 子进程里因此总是“已设置”。它指向不存在的目录时回退到 BtbN 包。
if ($env:FFMPEG_DIR -and -not (Test-Path (Join-Path $env:FFMPEG_DIR 'include'))) {
  $env:FFMPEG_DIR = $null
}
if (-not $env:FFMPEG_DIR) {
  $candidates = @(
    (Join-Path $PSScriptRoot '..\..\ffmpeg-btbn-8.1'),
    (Join-Path $PSScriptRoot '..\..\ffmpeg-msvc')
  )
  foreach ($candidate in $candidates) {
    if (Test-Path (Join-Path $candidate 'include')) { $env:FFMPEG_DIR = (Resolve-Path $candidate).Path; break }
  }
}
$msysClang = 'D:\ProgramData\MSYS2\ucrt64\bin'
if (Test-Path "$msysClang\libclang.dll") {
  if (-not $env:LIBCLANG_PATH) { $env:LIBCLANG_PATH = $msysClang }
}
$ffmpegBin = if ($env:FFMPEG_DIR) { Join-Path $env:FFMPEG_DIR 'bin' } else { '' }
$vcPath = (& cmd /v:on /c "`"$($vcvars.FullName)`" >nul 2>nul && echo !PATH!") | Select-Object -Last 1
$pathParts = @($msysClang, $ffmpegBin) + ($vcPath -split ';')
$noLd = ($pathParts | Where-Object { $_ -and $_ -notmatch 'CMake\\bin' } | Select-Object -Unique) -join ';'
$tauriArgs = $args -join ' '
Push-Location (Join-Path $PSScriptRoot '..')
cmd /c "`"$($vcvars.FullName)`" >nul 2>nul && set `"PATH=$noLd`" && npx --prefix front tauri $tauriArgs 2>&1"
$code = $LASTEXITCODE
Pop-Location
exit $code
