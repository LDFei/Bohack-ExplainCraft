# 在 MSVC 开发环境中运行 cargo（VS Installer 缺少 vswhere.exe 时 rustc 找不到 link.exe）。
# 用法：./cargo.ps1 check    ./cargo.ps1 test    ./cargo.ps1 build --release
#
# 本脚本只调整 PowerShell 的 PATH，不改动系统设置。三条路径要求互相牵制，缺一条都会失败：
# 1. cl.exe（ffmpeg-sys-next 的 cc 探测）来自 vcvars64.bat，必须保留 VS 的编译器与 SDK 目录。
# 2. libclang.dll（bindgen）来自 MSYS2，其依赖 libstdc++/libzstd 也只能从 MSYS2 目录加载；
#    这些目录排在 Git 自带的 MinGW DLL 之前，否则 LoadLibraryExW failed。
# 3. CMake 4.x（D:\ProgramData\CMake\bin）旁边的 ld.exe 会抢占 MSVC 链接器，ocio-sys 的
#    编译器探测直接失败；构建期间把它从 PATH 中移除，CMake 会回退到 VS 自带版本。
$roots = @("$env:ProgramFiles\Microsoft Visual Studio", "${env:ProgramFiles(x86)}\Microsoft Visual Studio")
$vcvars = Get-ChildItem -Path $roots -Filter vcvars64.bat -Recurse -ErrorAction SilentlyContinue |
  Sort-Object FullName -Descending | Select-Object -First 1
if (-not $vcvars) {
  Write-Host '没有找到 vcvars64.bat，请安装 Visual Studio 的“使用 C++ 的桌面开发”组件。'
  exit 1
}
$ffmpegCandidates = @(
  (Join-Path $PSScriptRoot '..\..\ffmpeg-btbn-8.1'),
  (Join-Path $PSScriptRoot '..\..\ffmpeg-msvc')
)
if (-not $env:FFMPEG_DIR) {
  foreach ($candidate in $ffmpegCandidates) {
    if (Test-Path (Join-Path $candidate 'include')) { $env:FFMPEG_DIR = (Resolve-Path $candidate).Path; break }
  }
}
$msysClang = 'D:\ProgramData\MSYS2\ucrt64\bin'
if (Test-Path "$msysClang\libclang.dll") {
  if (-not $env:LIBCLANG_PATH) { $env:LIBCLANG_PATH = $msysClang }
}
$ffmpegBin = if ($env:FFMPEG_DIR) { Join-Path $env:FFMPEG_DIR 'bin' } else { '' }
# 以 vcvars64.bat 设置完的 PATH 为基准，再把 MSYS2 与 FFmpeg 提到最前，最后去掉 CMake 4.x 目录。
$vcPath = (& cmd /v:on /c "`"$($vcvars.FullName)`" >nul 2>nul && echo !PATH!") | Select-Object -Last 1
$pathParts = @($msysClang, $ffmpegBin) + ($vcPath -split ';')
$noLd = ($pathParts | Where-Object { $_ -and $_ -notmatch 'CMake\\bin' } | Select-Object -Unique) -join ';'
$cargoArgs = $args -join ' '
Push-Location $PSScriptRoot
cmd /c "`"$($vcvars.FullName)`" >nul 2>nul && set `"PATH=$noLd`" && cargo $cargoArgs 2>&1"
$code = $LASTEXITCODE
Pop-Location
exit $code
