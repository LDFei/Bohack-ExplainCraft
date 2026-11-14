ExplainCraft v0.1.2 修复版；保留历史 v0.1.0 标签和已公开的 v0.1.1 Release。

修复 Rust workspace 格式与 Linux FFmpeg
8 构建环境；增加版本一致性检查、原生测试、安装后离线自检，以及安装包来源和 SHA256 校验。构建与发布改为独立触发；部分平台仅可作为预览发布，正式版必须具备四个平台的验证产物。

应用名称为 Lingjing（灵镜），固定标识 `com.explaincraft.lingjing`，前端、Rust 和安装包版本均为
`0.1.2`，Release 标签为 `v0.1.2`。

下载对应系统的文件：

- Windows x64：`ExplainCraft_0.1.2_windows_x64-setup.exe`，自动安装验证环境为 Windows Server 2022。
- Ubuntu 24.04 x64：`ExplainCraft_0.1.2_linux_amd64.deb`，使用系统软件安装器或
  `sudo apt install ./ExplainCraft_0.1.2_linux_amd64.deb` 安装依赖。
- Apple Silicon Mac：`ExplainCraft_0.1.2_macos_arm64.dmg`。
- Intel Mac：`ExplainCraft_0.1.2_macos_x86_64.dmg`。

实际已验证并提供的系统以本页末尾的构建清单为准。macOS 两种芯片在 macOS
15 构建机验证，最低系统要求为 macOS 15；Linux 仅验证 Ubuntu 24.04 x64；Windows ARM、Linux
ARM、旧版发行版和其他未测试系统不在本版的验证范围。

每个平台附有 `BUILD-平台.json`、`SELF-CHECK-平台.json` 和
`SHA256SUMS-平台.txt`，记录对应提交、构建编号和安装后验证结果；完整正式版另附全体文件的
`SHA256SUMS.txt`。

首次打开请在设置中填写自己的 API 配置。安装包不包含开发者密钥、数据库或本地语音模型。Windows/macOS 附带 FFmpeg/FFprobe；Ubuntu 由安装器安装系统媒体和音频库。SVG 路线无需额外安装 Node/Chrome；HTML 路线需要 Node.js
24.12+ 和 Chrome，本地 Breeze 配音需要另行配置模型与服务。

本次验证涵盖代码测试、原生安装与真实 H.264/AAC 编码，不代表已在所有用户设备上完成人工界面及云模型测试。未提供 Windows 商业代码签名和 Apple 公证，系统可能要求确认来源。

Quercus/Oak、FFmpeg 和其他依赖的许可见仓库与随包许可文件。FFmpeg 构建配方来自
[BtbN](https://github.com/BtbN/FFmpeg-Builds)、[Homebrew](https://github.com/Homebrew/homebrew-core)
与仓库 Linux 构建脚本；Linux FFmpeg 8.0.1 源码来自 [FFmpeg 官方](https://ffmpeg.org/releases/)。
