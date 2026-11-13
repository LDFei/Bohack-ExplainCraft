# 工作台布局与设置

日期：2026-11-14。所有调整位于 `front/`，不改变项目数据、后端接口或当前演示生成方式。

## 操作

- 工作台只保留顶部一个“查看使用指南”问号入口，移除侧栏和项目菜单的重复指南。
- 左下角头像替换为“工作台设置”按钮；收起侧栏时也可打开。
- 桌面三栏的标题旁有六点手柄。拖动手柄到另一个面板即可换位；Alt + 左右方向键提供等价操作。
- 拖动两栏之间的分隔线可调宽。方向键每次调整 24px，Shift + 方向键每次调整 64px。
- 预览区与时间轴之间的横向分隔线可调高时间轴，上下方向键也可调整。
- 面板顺序和比例默认自动保存在当前浏览器，切项目不改变布局。通用设置里可关闭“记住工作台布局”，或即时恢复默认布局。
- 窗口宽度不超过 1050px 时沿用原有分区方式，不压缩为三栏，不展示桌面拖拽手柄；回到大屏后恢复桌面偏好。

三栏通过 CSS
Grid 定位，不移动或重新挂载播放器、输入框组件。拖拽布局与文件上传使用不同的事件标记，原有拖入素材上传仍可使用。

## 通用设置

- 主题外观：沿用银灰、浅丁香、暖纸、雾蓝四套主题。
- 发送快捷键：Enter 发送，或 ⌘ / Ctrl + Enter 发送；Shift +
  Enter 用于换行，中文输入法选字不触发发送。
- 记住工作台布局：关闭后移除布局偏好，当前布局在本页继续生效，刷新恢复默认。
- 恢复默认布局：仅恢复面板顺序、宽度和时间轴高度，不删除项目、脚本、上传文件或成片。

除“恢复默认布局”外，弹窗内修改在点击“保存设置”后应用；取消丢弃未保存的输入。

## 四类 API 配置预留

| 模块                | 独立端点                 | 预留字段                          |
| ------------------- | ------------------------ | --------------------------------- |
| 文生视频模块        | 文生图 API、图生视频 API | 各自的接口地址、模型名称、API Key |
| 背景音乐 API 模块   | 背景音乐 API             | 接口地址、模型名称、API Key       |
| 人声 API 模型       | 人声 API                 | 接口地址、人声模型名称、API Key   |
| 视觉大语言 API 模块 | 视觉大语言 API           | 接口地址、视觉模型名称、API Key   |

当前是四类模块、五个独立配置槽。配置地址必须为完整 HTTP / HTTPS 地址，可留空。

这些字段仅为后续接入预留，不发出服务请求，不测试连通性，不会把当前演示标为已接入真实模型。地址和模型名称保存到当前浏览器；API
Key 仅保留在当前页面的内存中，关闭设置后可再次查看已填写值，整页刷新后清空，不写入 localStorage。没有改动已有 StudioApi 契约。

## 文件

- `src/components/SettingsPanel.vue`：通用设置、四类 API 预留、取消与保存。
- `src/composables/useAppSettings.ts`：通用偏好与非敏感配置保存，会话密钥隔离。
- `src/components/PanelDragHandle.vue`：可拖动与键盘移动的共用手柄。
- `src/composables/useWorkspaceLayout.ts`：网格顺序、尺寸边界、指针捕获、持久化与损坏配置回退。
- `src/workspace-layout.css`：三栏网格、分隔线和设置的响应式样式。
- `App.vue`、`Sidebar.vue`、`Conversation.vue`、`PreviewPanel.vue`：连接设置入口、拖动事件、快捷键与时间轴高度。
- `tests/e2e/layout-settings.spec.ts`：8 项新增浏览器回归。

偏好分别使用 `explaincraft.workspace-layout` 和
`explaincraft.app-settings`，独立于原有项目与主题存储。保存的布局损坏时回到默认，尺寸受最小宽度和窗口高度约束。

## 验证与截图

- 11 项业务逻辑测试与全部 34 项浏览器测试通过。
- 新增覆盖重复帮助入口移除、真实指针调宽、面板原生拖放与键盘换位、DOM 与未发送输入保持、刷新恢复、时间轴高度、通用设置、五个 API 配置槽、取消与无效地址、损坏配置和 320～1024px 设置页。
- 核对预留 API 未发出请求，API Key 未写入持久化配置。
- 构建和格式检查通过。浏览器为本机 Chrome 与移动尺寸模拟；未在实体移动设备上执行。

截图：[扩大 Copilot](screenshots/layout-settings/wider-copilot.png)、[面板换位](screenshots/layout-settings/reordered.png)、[通用设置](screenshots/layout-settings/general.png)、[API 预留](screenshots/layout-settings/api-slots.png)、[手机设置](screenshots/layout-settings/mobile-settings.png)。

本轮前端快照位于 `/private/tmp/bohack-before-layout-settings-20261113.tar.gz`。原版 Git 提交
`82365b1` 与此前快照保留；临时目录可能被系统清理。没有提交或推送 GitHub。
