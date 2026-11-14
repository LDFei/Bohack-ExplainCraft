# 版本与发行约定

`v0.1.0` 对应完整桌面应用的正式发行源码，包含 `front/` 工作台、`src-tauri/`
Rust 后端与桌面壳。工作台的安装依赖、测试和构建命令均从 `front/` 执行。

| 名称                 | 用途                              |
| -------------------- | --------------------------------- |
| `v0.1.0` 标签        | 与 0.1.0 安装包对应的正式桌面源码 |
| `main` 分支          | 已整合的稳定代码及正式发布流程    |
| `develop` 或功能分支 | 后续开发、测试和改造              |

前端 `package.json`、`package-lock.json`、Rust `Cargo.toml`、`Cargo.lock`
与 Tauri 配置必须使用相同版本。安装包内版本必须与 Release 标签一致，不通过重命名文件冒充其他版本。

## 正式发布

1. 在独立功能分支完成修改、测试和版本更新，再整合至 `main`。
2. GitHub Actions 的 `Desktop release` 构建 Windows x64、Apple Silicon Mac 和 Intel
   Mac 安装包，并执行原生测试与安装后自检。
3. `Publish desktop release` 接收已完成的构建编号，重新校验提交、测试结果、版本、产物来源及 SHA256。
4. `stable` 要求上述三个平台全部通过；`preview` 只允许发布已通过验证的平台，并标记为预览版。

安装包公开后保留对应标签。后续修复增加版本号，不覆盖已有正式 Release，不重写分支历史。发行说明见
[v0.1.0](RELEASE-0.1.0.md)。

## 下载源码

直接安装应用可前往
[GitHub Releases](https://github.com/LDFei/Bohack-ExplainCraft/releases/tag/v0.1.0)
下载对应系统的安装包。

需要检查或修改这一版源码时，在新目录克隆并建立自己的分支：

```bash
git clone --branch v0.1.0 https://github.com/LDFei/Bohack-ExplainCraft.git Bohack-ExplainCraft-v1
cd Bohack-ExplainCraft-v1
git switch -c codex/my-redesign
cd front
npm ci
npm run desktop:dev
```

本地开发还需要 Rust、FFmpeg 开发包及对应系统的 Tauri 构建依赖，详见
[桌面开发说明](../README.md)。个人 API 配置、数据库、生成视频与本机缓存不提交到 Git，也不随分支切换回滚。
