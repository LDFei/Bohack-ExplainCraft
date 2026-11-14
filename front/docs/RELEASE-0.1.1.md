ExplainCraft 首个跨平台桌面发行版。历史 v0.1.0 为前端演示基线，本次保留该标签。

下载：Windows x64 选择 setup.exe；Ubuntu 24.04 x64 选择 .deb；Apple Silicon
Mac 选择 arm64.dmg；Intel Mac 选择 x86_64.dmg。SHA256SUMS.txt 可校验文件。

本版包含知识视频工作台、逐步确认创作流程、本地音乐上传、原生横竖画幅、通用矢量知识动画、版本历史与导出。模型语义计划仍需审阅，不保证任意主题自动生成的事实与画面都正确。

首次打开请在设置中填写自己的 API 配置，安装包不包含开发者密钥或数据库。SVG 路线无需 Node/Chrome；HTML 路线需安装 Node.js
24+ 和 Chrome。本地 Breeze 配音需另行配置模型与服务。

Windows/macOS 随包附带媒体工具；Ubuntu 使用系统 FFmpeg 依赖（用软件安装器或 sudo apt install
./文件名.deb 安装）。安装包尚无商业代码签名及 Apple 公证，系统可能要求确认来源。

源代码及依赖：本仓库对应发布标签提供应用源码；Quercus/Oak 代码许可见源码内 LICENSE；FFmpeg 构建来自 BtbN（Windows）及 Homebrew（macOS），对应源代码与构建配方分别见
https://github.com/BtbN/FFmpeg-Builds 和 https://github.com/Homebrew/homebrew-core。请遵守随附许可。

v0.1.1 使用四个平台构建完成后自动公开的旧发布流程。后续 [v0.1.2](RELEASE-0.1.2.md)
将构建与发布拆开，允许先发布通过验证的平台预览，四个平台验证通过后再转为正式版。构建成功不等于已完成所有系统上的人工功能测试。
