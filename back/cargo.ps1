# 在 MSVC 开发环境中运行 cargo（VS Installer 缺少 vswhere.exe 时 rustc 找不到 link.exe）。
# 用法：./cargo.ps1 run    ./cargo.ps1 test    ./cargo.ps1 build --release
#       ./cargo.ps1 run --features quercus   # 使用 quercus 剪辑引擎（需要 FFmpeg 开发包）
$roots = @("$env:ProgramFiles\Microsoft Visual Studio", "${env:ProgramFiles(x86)}\Microsoft Visual Studio")
$vcvars = Get-ChildItem -Path $roots -Filter vcvars64.bat -Recurse -ErrorAction SilentlyContinue |
  Sort-Object FullName -Descending | Select-Object -First 1
if (-not $vcvars) {
  Write-Host '没有找到 vcvars64.bat，请安装 Visual Studio 的“使用 C++ 的桌面开发”组件。'
  exit 1
}
# quercus 依赖 FFmpeg：未设置 FFMPEG_DIR 时，使用与仓库同级的 ffmpeg-msvc（见 quercus-engine/README.md）。
$ffmpeg = Join-Path $PSScriptRoot '..\..\ffmpeg-msvc'
if (-not $env:FFMPEG_DIR -and (Test-Path $ffmpeg)) { $env:FFMPEG_DIR = (Resolve-Path $ffmpeg).Path }
# bindgen 生成 FFmpeg 绑定需要 libclang；默认使用 MSYS2 自带的版本。
$msysClang = 'D:\ProgramData\MSYS2\ucrt64\bin'
if (-not $env:LIBCLANG_PATH -and (Test-Path "$msysClang\libclang.dll")) { $env:LIBCLANG_PATH = $msysClang }
$cargoArgs = $args -join ' '
Push-Location $PSScriptRoot
cmd /c "`"$($vcvars.FullName)`" >nul 2>nul && cargo $cargoArgs 2>&1"
$code = $LASTEXITCODE
Pop-Location
exit $code