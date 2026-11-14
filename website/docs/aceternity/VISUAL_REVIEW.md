# 真实页面视觉审查记录

日期：2026-11-13。浏览器为本机 Google
Chrome，由 Playwright 在独立浏览器上下文中打开实际 Vite 页面，不使用设计效果图作为截图。共完成三轮调整，并保存第四次终验。

## 第一轮：初版截图与卡片密度修正

- 实际问题：工作台根字号为 13px，官方 18rem 网格行高在此环境只有 234px。卡片扣除内容与内边距后，插图区只剩 44～70px；脚本示意实际需要 213px，产生明显裁切。
- 调整：保留公共 BentoGrid 的官方默认值，首页通过变量使用 400px 行高，手机脚本插图单独设为 200px；收起侧栏时隐藏文字，保留 60px 图标态。
- 指标：[round-1/metrics.json](../screenshots/aceternity/round-1/metrics.json)。桌面、手机与收起工作台均无横向溢出，页面异常为 0。
- 截图：[桌面首页](../screenshots/aceternity/round-1/home-desktop.png)、[手机首页](../screenshots/aceternity/round-1/home-mobile.png)、[收起工作台](../screenshots/aceternity/round-1/workspace-collapsed.png)。

## 第二轮：光束全幅显示与媒体状态修正

- 复查：卡片实际高度 398px，插图区恢复到 210～236px，文字与插图分层正常。
- 实际问题：光束只在左侧窄条可见；原因是官方组件的 `mask-size: 40px` 与首页添加的渐变蒙版叠加。
- 调整：Hero 蒙版明确覆盖 100%×100%，保留原 SVG 路径与渐变动画。产品小样的时间码与进度同步真实视频，减少动态效果时暂停装饰视频，并消除视频默认行内基线多出的 3px。
- 指标：[round-2/metrics.json](../screenshots/aceternity/round-2/metrics.json)。桌面与手机无横向溢出，页面异常为 0。
- 截图：[桌面首页](../screenshots/aceternity/round-2/home-desktop.png)、[手机首页](../screenshots/aceternity/round-2/home-mobile.png)、[工作台](../screenshots/aceternity/round-2/workspace.png)。

## 第三轮：窄屏排版与点击区域修正

- 复查：光束现在横跨整个 Hero；页面保留 50 条动态路径、1 条基础路径和 200 个原生 SVG 属性动画。
- 扩展尺寸：320、768、1024、1920px，均无页面横向溢出。
- 实际问题：320px 下功能标题末尾仅剩少量字，视觉断行不平衡；手机主题按钮的点击区域偏小。
- 调整：标题采用均衡断行；手机主题入口改为 44×44px，观看按钮至少 44px 高，适配顶部品牌与 CTA 间距。手机卡片入口增加点击内边距，桌面网格尺寸保持稳定。
- 指标：[round-3/metrics.json](../screenshots/aceternity/round-3/metrics.json)。页面异常为 0。
- 截图：[320px](../screenshots/aceternity/round-3/home-320.png)、[768px](../screenshots/aceternity/round-3/home-768.png)、[桌面首页](../screenshots/aceternity/round-3/home-desktop.png)。

## 第四次终验

- 卡片实际高度 398px，插图区 210～236px，插图没有内容溢出；视频示意高度与容器一致。
- 桌面与手机无横向溢出，收起侧栏实测宽度为 60px，打开宽度为 300px；页面异常为 0。
- SVG 属性动画的实时值通过浏览器测试确认在变化，减少动态效果时移除持续动画。
- 指标：[round-4/metrics.json](../screenshots/aceternity/round-4/metrics.json)。
- 最终截图：[桌面首页](../screenshots/aceternity/round-4/home-desktop.png)、[手机首页](../screenshots/aceternity/round-4/home-mobile.png)、[工作台](../screenshots/aceternity/round-4/workspace.png)、[收起工作台](../screenshots/aceternity/round-4/workspace-collapsed.png)。

## 自动化验收

- 11 项业务逻辑测试通过。
- 26 项真实浏览器测试通过：原有 20 项业务与主题回归，新增 6 项官方路径 / 光束运动 /
  Bento 网格 / 首页媒体与路由 / 侧栏 / 响应式与减少动态效果检查。
- 一次新测试最初错误地读取 CSS shorthand 的 `grid-column-end`；实际跨列在
  `grid-column-start`，修正断言后通过，生产网格未因此变更。
- TypeScript 与 Vite 生产构建通过，统一格式检查通过。没有真实 AI 或 TTS 接入验证，也未在实体手机或 Safari/Firefox 上执行。

复现：从 front/ 运行 `node scripts/review-aceternity.mjs 4` 可重新生成终验截图，`npm run test:e2e`
执行浏览器回归。
