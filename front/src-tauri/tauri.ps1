# 在 MSVC 开发环境中运行 Tauri CLI（与 back/cargo.ps1 相同：VS Installer 缺少 vswhere.exe 时 rustc 找不到 link.exe）。
# 用法（在 front/ 目录）：
#   npm run desktop:dev      开发：Vite 热更新 + 桌面窗口 + 内嵌后端（含 quercus 剪辑引擎）
#   npm run desktop:build    打包：生成 Windows 安装程序（NSIS）
$roots = @("$env:ProgramFiles\Microsoft Visual Studio", "${env:ProgramFiles(x86)}\Microsoft Visual Studio")
$vcvars = Get-ChildItem -Path $roots -Filter vcvars64.bat -Recurse -ErrorAction SilentlyContinue |
  Sort-Object FullName -Descending | Select-Object -First 1
if (-not $vcvars) {
  Write-Host '没有找到 vcvars64.bat，请安装 Visual Studio 的“使用 C++ 的桌面开发”组件。'
  exit 1
}
# quercus 依赖 FFmpeg 开发包：未设置 FFMPEG_DIR 时使用与仓库同级的 ffmpeg-msvc。
$ffmpeg = Join-Path $PSScriptRoot '..\..\..\ffmpeg-msvc'
if (-not $env:FFMPEG_DIR -and (Test-Path $ffmpeg)) { $env:FFMPEG_DIR = (Resolve-Path $ffmpeg).Path }
$msysClang = 'D:\ProgramData\MSYS2\ucrt64\bin'
if (-not $env:LIBCLANG_PATH -and (Test-Path "$msysClang\libclang.dll")) { $env:LIBCLANG_PATH = $msysClang }
$tauriArgs = $args -join ' '
Push-Location (Join-Path $PSScriptRoot '..')
cmd /c "`"$($vcvars.FullName)`" >nul 2>nul && npx tauri $tauriArgs 2>&1"
$code = $LASTEXITCODE
Pop-Location
exit $code