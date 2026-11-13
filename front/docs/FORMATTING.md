# 代码格式约定

## 一键整理

在前端根目录 `front/` 执行：

```bash
npm ci
npm run format
npm run format:check
```

前端工程的格式命令需要 Node.js 24.12+ 和
[uv](https://docs.astral.sh/uv/getting-started/installation/)。uv 按提交的 uv.lock 安装 Ruff 到本项目的 .venv，不修改全局 Python 环境。平时只改前端时不需要调用 Python 格式工具。

| 命令                       | 用途                                                              |
| -------------------------- | ----------------------------------------------------------------- |
| `npm run format`           | 整理前端、配置、文档和 Python 辅助脚本                            |
| `npm run format:check`     | 只检查整个项目，发现格式差异时返回失败                            |
| `npm run format:web`       | 整理 Vue、TypeScript、CSS、HTML、JSON、Markdown、YAML、SVG 等文件 |
| `npm run format:check:web` | 只检查 Prettier 支持的项目文件                                    |
| `npm run format:python`    | 按 Ruff 规则整理 Python 辅助脚本                                  |

媒体、截图、依赖、构建目录、虚拟环境和测试产物不交给文本格式化器处理。

## 可读性规则

- 前端两空格缩进，Python 四空格缩进；UTF-8 编码和 LF 换行。
- 前端使用单引号和语句分号；Python 使用双引号。
- 目标行宽为 100；不能安全拆分的字符串、路径和链接可以超过此宽度。
- Vue 的属性逐行排列，模板层级清晰。
- CSS 选择器、样式声明和媒体查询按层级展开。
- 语句、参数列表和较长对象由格式工具自动换行，避免把整个函数堆成一行。
- 生成代码及 lock 文件由其工具维护；不要手工修改锁文件中的版本或校验值。

前端规则在 `.prettierrc.json`，Python 规则在 `pyproject.toml`，基础缩进与换行在
`.editorconfig`。工具版本通过 package-lock.json 和 uv.lock 固定。

## 保存和提交时自动整理

VS Code 已提供 `.vscode/settings.json`
和推荐扩展清单。按编辑器提示安装 Vue、Prettier、Ruff 等推荐扩展后，保存文件即可格式化。

执行 `npm ci` 或 `npm install` 时，Husky 会安装本仓库的提交前入口。执行 `git commit`
时，lint-staged 只整理本次暂存的支持文件，并把格式调整纳入本次提交。它会处理部分暂存文件，避免把未选择提交的修改一起带入。

如果安装时跳过了生命周期脚本，执行 `npm run prepare` 可以单独启用本仓库的提交前入口。

Python 文件的提交整理需要 uv；如果缺少工具或格式整理失败，提交会停止并显示原因。可以修复后重新提交。

前端提交前入口位于 `front/.husky/pre-commit`，从 Git 根目录判断本次是否有前端文件变更；只有 `front/`
的暂存文件会进入格式工具。仅提交队友的后端文件时跳过，不要求安装 Node.js 或 uv。

GitHub 的 Frontend code quality 流程仅对 main、develop 上涉及 `front/`
或前端工作流的推送和合并请求执行格式检查、逻辑测试与构建。后端可单独配置自己的检查。未设置分支保护规则，检查本身不会禁止直接推送。

## 新增文件和后续开发

在 `front/`
内正常新增源文件，沿用前端根目录规则即可。无需逐文件添加格式设置。给 AI 开发工具的前端持续约定位于
`front/AGENTS.md`，仓库协作边界位于仓库根目录 AGENTS.md。

后续 Rust 后端加入后，按 Cargo 工程接入 rustfmt；当前仓库只有前端和一个 Python 媒体辅助脚本，不额外引入 Rust 工具链。
