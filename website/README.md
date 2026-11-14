# ExplainCraft 官网

独立的静态产品网站，与 `front/` 的 Tauri 工作台分离。只包含 Vue 页面、样式和
`shared/public/media/` 中的本地示例，不加载任何后端接口。

```bash
npm ci
npm run dev
```

开发地址为 <http://127.0.0.1:5174>。构建与检查：

```bash
npm run build
npm run check:website
npm run test:e2e
```

推送到 `main` 后由 GitHub Actions 发布到 GitHub Pages。
