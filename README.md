# Bohack-ExplainCraft

黑客松的 AI 教育赛道项目。仓库按前后端目录划分，方便不同电脑、不同开发者独立工作。

```text
Bohack-ExplainCraft/
├── front/                  前端工程：Vue、页面、测试、文档及演示素材
├── .github/workflows/      自动检查；当前前端检查仅覆盖 front/
├── AGENTS.md               仓库协作约定
└── README.md               全仓库入口
```

后端由队友在仓库根目录添加独立目录（例如
`back/`），自行维护 Rust 工程及其工具配置。当前没有创建后端实现或占位工程。

## 前端运行

当前协作版本在 `develop` 分支：

```bash
git clone --branch develop https://github.com/LDFei/Bohack-ExplainCraft.git
cd Bohack-ExplainCraft/front
npm ci
npm run dev
```

浏览器打开
[http://127.0.0.1:5173](http://127.0.0.1:5173)。前端目前使用演示素材，真实 Rust 和 AI 生成服务尚未接通。

前端开发时可直接在编辑器中打开 `front/`，使用其中的保存格式化设置与推荐扩展。

## 开发边界

- 前端开发只修改 `front/`，后端开发只修改自己的后端目录；共享入口和接口约定按需协调。
- 前端依赖、环境配置、构建结果都留在 `front/`，不要求后端开发者安装前端工具。
- 前端提交前检查只选择 `front/` 的暂存文件；仅提交后端文件时直接跳过。
- 前端工作流仅由 `front/` 或该工作流文件的变更触发，后端可后续添加自己的检查。
- 推荐各自在功能分支开发，再将确认的改动合入 `develop`。

## 文档与版本

- [前端完整说明](front/README.md)
- [前后端接口约定](front/docs/API.md)
- [页面设计](front/docs/FRONTEND_DESIGN.md)
- [格式约定](front/docs/FORMATTING.md)
- [版本保存与重新改造](front/docs/VERSIONING.md)

原始第一版仍由 `v0.1.0` 标签保存。该标签采用旧的根目录前端布局；当前版本将同一前端工程迁入了
`front/`。
