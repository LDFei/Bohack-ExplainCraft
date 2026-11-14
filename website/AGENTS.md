# 官网项目约定

- `website/` 是独立的静态 Vite + Vue 项目，不引用 `front/` 的工作台代码或 Rust 后端。
- 所有 npm、测试和构建命令均在本目录执行。
- 两空格缩进、单引号和分号，修改后运行 `npm run format`、`npm run build` 和 `npm run check:website`。
- GitHub Pages 只发布 `website/dist/`。
