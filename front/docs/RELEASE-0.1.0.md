ExplainCraft v0.1.0 · 跨平台桌面版

应用名称为 Lingjing（灵镜），固定标识
`com.explaincraft.lingjing`。前端、Rust、安装包及 Release 统一使用版本 `0.1.0`。

提供 Vue 创作工作台、进程内 Rust 后端、SQLite 项目保存、引导式视频制作、SVG/HTML 镜头生成、配音与背景音乐，以及 FFmpeg 初版输出。初版输出后，可由用户确认继续使用 Quercus 剪辑并保存新版本。

下载对应系统的安装包：

- Windows x64：`ExplainCraft_0.1.0_windows_x64-setup.exe`。
- Apple Silicon Mac：`ExplainCraft_0.1.0_macos_arm64.dmg`。
- Intel Mac：`ExplainCraft_0.1.0_macos_x86_64.dmg`。

macOS 最低要求为 macOS 15。Windows 安装包在 Windows Server 2022 构建机验证；Windows
ARM 不在本次发行范围内。

每个平台均运行 Rust 测试、前端构建、真实安装包自检及 H.264/AAC 视频编码。正式版要求 Windows 和两种 Mac 的验证产物齐全，并核对同一提交、版本、构建编号与文件 SHA256。

每个安装包附有 `BUILD-平台.json`、`SELF-CHECK-平台.json`、`SHA256SUMS-平台.txt`
和许可文件，正式版另附 `SHA256SUMS.txt`。本次验证不代表已在所有用户设备上完成人工界面及云模型测试。

首次启动请在设置中填写自己的 API 配置。安装包附带 FFmpeg/FFprobe，不包含开发者密钥、数据库或本地语音模型。SVG 路线无需额外安装 Node.js/Chrome；HTML 路线需要 Node.js
24.12+ 和 Chrome，本地 Breeze 配音需要另行配置模型与服务。

本版未提供 Windows 商业代码签名和 Apple 公证，首次安装或打开时系统可能要求确认来源。

Quercus/Oak、FFmpeg 和其他依赖的许可见仓库及随包许可文件。FFmpeg 构建配方来自
[BtbN](https://github.com/BtbN/FFmpeg-Builds) 与
[Homebrew](https://github.com/Homebrew/homebrew-core)。
