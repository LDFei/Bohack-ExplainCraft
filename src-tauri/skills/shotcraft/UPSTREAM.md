# Video Shotcraft 来源与适配边界

- 上游：<https://github.com/Vincentwei1021/video-shotcraft>
- 本次核对提交：`6700db2ba80572ebc777b99d72049679fed9840c`，提交时间 2026-11-11。
- 上游代码和文档采用 Apache-2.0；原许可证随目录保留为 `LICENSE`。
- 本目录由 ExplainCraft 在 2026-11-14 新建，`recipes.json` 是针对本应用的改写与适配。

## 实际使用的内容

上游是面向 Agent 的制作方法、Remotion 模板、镜头配方与参考实现，不是单独的 SVG/HTML 渲染器。
本次读取了上游 `SKILL.md`、`template/TEMPLATE.md`、`references/pipeline.md`、
`references/sequences/promo-energy-arc.md`、Gallery 索引，以及五张镜头卡。
进一步读取 BrandInkOpen、PaperTitleCard、TypeAndFilter 的准确 TSX 参考实现，
以代码中的时值为准处理了镜头卡描述与实现的差异，例如品牌开场使用 104 帧，完整标题停留 33 帧。

`upstream-index.json` 保存这些镜头卡的真实 ID、变体名称、路径及 Gallery 参考样片地址。
`recipes.json` 保存时值、停留要求、动作顺序、约束与 SVG/HTML 适配说明：

| 本应用模板                | 上游内容                                                         | 适配范围                                           |
| ------------------------- | ---------------------------------------------------------------- | -------------------------------------------------- |
| Ink Press · 纸墨叙事      | Ink Press、brand-ink-open、spotlight-hero-card、paper-title-card | 开场、单一主角与呼吸字卡结构                       |
| Show the Flow · 功能演示  | type-and-filter、brand-ink-open、paper-title-card                | 输入、结果筛选、归位与结论                         |
| Data in Motion · 数据叙事 | chart-live-moves 的 axis-rescale-shock 变体、paper-title-card    | 真实坐标轴、一次变化、可读的结尾停留               |
| Vibe 知识大赏             | 本项目原创；按需借用文字与数据镜头语法                           | 单知识点、机制解释、边界说明，声音与节奏由用户确认 |

应用使用自己生成的 SVG/HTML 场景和本地渲染路径，不运行上游的 React/Remotion 工程。
不能将本次适配宣称为 Ink Press 的逐像素复刻，也不能声称上游完整的工作台、150 个动态预览、
音效工具或剪映工程导出已全部集成。`front/src/assets/templates/`
的三张矢量预览为本项目新绘制的模板示意，
没有复制上游产品截图、音频、商标、字体文件或未核对许可的媒体。

## 与上游规则的显式差异

本次用户要求每一步选择，故应用固定为共同创作模式，不采用上游可自主连续执行的模式。
本次用户要求先看成片再决定剪辑，故不采用上游交付后自动打开 Motion Workbench 的行为。
模型下载、可选剪辑和发布都不能由模板文字隐式授权。
模板中的示例时长不是最终约束；时长必须经过应用对话步骤确认。

若今后直接引入上游 Remotion 运行时，应独立检查该运行时及所用音频/素材的许可与实际部署要求。
本次适配没有添加该依赖，也没有借用上游广告、赞助、作者推广或自动发布指令。
