# 开发与格式约定

- 默认在 `develop` 或新的功能分支开发，保留 `v0.1.0` 第一版标签。
- Vue、TypeScript、JavaScript、CSS、HTML、JSON、Markdown 和 YAML 遵循根目录的 Prettier 配置。
- 前端使用两空格缩进、单引号和语句分号；Vue 多属性逐行排布，CSS 声明逐行排布。
- Python 辅助脚本遵循 pyproject.toml 的 Ruff 格式规则，使用四空格缩进。
- 避免把多个语句、整个模板或整个样式规则挤成一行；长命令和参数列表按语义换行。
- 修改后执行 `npm run format`，提交前执行 `npm run format:check`。
- 仅格式整理时保持业务行为；修改代码后运行现有的 `npm test` 和 `npm run build`。
- 编辑器设置、提交前整理和仓库检查共用上述规则；不要为单个文件另建冲突的格式配置。
