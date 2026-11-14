# 应用内创作 skills

这些文件是 ExplainCraft 运行时读取的产品规则，不是安装到 Codex/Claude 客户端的个人技能。
根目录 `manifest.json` 是步骤、版本和选项值的声明；每个步骤的 Markdown 是传给模型的创作约束。
是否放行必须由后端持久化的确认状态决定，不能仅依赖模型遵守文字。

## 本次问题的原因

旧版只有 `front/src/domain/content.ts` 的注释提到 `.claude/skills/vibe-knowledge-awards`，
仓库内并没有相应文件，后端也没有读取本地 skill。
同时旧接口说明采用全自动制作，只在缺失关键信息时追问。
因此在普通聊天中写“每步先给我选项”，并不能可靠地改变流水线的执行方式。

## 运行时契约

- 固定
  `schema_version`、`version`、`mode=guided`；任务保存版本及完整规则/配方内容摘要，升级不能悄悄替换旧任务规则。
- 用户主题是起点，随后按 style、duration、visual、voice、music、plan、script、storyboard、
  production、render、post_edit、delivery 顺序保存答案与产物。
- 每次暂停把问题、选项值、任务/项目 ID、修订号与事件一起提交 SQLite。
  重启后恢复同一问题；过期问题或旧版本回答必须拒绝，不能把重复点击执行两次。
- 模型建议与用户已确认答案分别保存。没有显式答案不得跨越确认边界。
- 新制作/修改任务需要明确确认偏好；同一任务恢复沿用已保存的选择，仅重算失效产物。
- 外部服务调用必须有期限；用户取消、网络失败、可重试异常与超时降级分别记录。
- 传给页面的制作日志采用业务事件，保留步骤、开始/结束时间、耗时、状态与可理解的原因；
  API 密钥、请求头、原始供应商响应和本机个人路径不能出现在界面。
- 先用 SVG/HTML 与 FFmpeg 制作可播放初版。`post_edit=quercus` 才授权调用 Quercus，
  `skip`、未回答和取消都不能触发剪辑插件。

## 模板约束

`shotcraft/recipes.json` 提供本地镜头配方和四个模板预设；`upstream-index.json` 记录核对来源。
模板只建议叙事结构与运动语言，不能覆盖用户已选风格、时长、配音或 BGM。
Vibe 的示例音乐也是候选，不是写死的依赖。程序不能因选择模板而自动下载模型或发起视频生成。

## 设计依据

本应用借鉴
[LangGraph 的中断与持久化恢复](https://docs.langchain.com/oss/python/langgraph/interrupts)
所描述的显式暂停、保存状态与继续机制，以及
[Temporal 的 Workflow 消息处理](https://docs.temporal.io/develop/typescript/workflows/message-passing)
中可序列化请求、验证和处理工作流消息的做法。
当前桌面应用使用已有 SQLite、事件记录、幂等键和 worker fencing 实现这些边界，
没有额外部署 LangGraph/Temporal 服务，也不把添加提示词等同于完整的工作流引擎。
