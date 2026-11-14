# 12 步创作工作流状态转换与覆盖

每个确认选项均通过真实业务路由、SQLite 和 Runner 推进。模型使用确定性替身；后期步骤运行真实编码器。47 个交互选择均达到下一问题或完成状态。第 10 步为自动渲染。

| 步骤         | 选择                                                   | 实际下一可操作状态                                 |
| ------------ | ------------------------------------------------------ | -------------------------------------------------- |
| 1 style      | 科学编辑式动态图形 · 暖金纪录片 (`scientific`)         | duration                                           |
| 1 style      | 温暖手绘 (`warm`)                                      | duration                                           |
| 1 style      | 极简信息图 (`minimal`)                                 | duration                                           |
| 1 style      | 电影叙事 (`cinematic`)                                 | duration                                           |
| 1 style      | 高能卡点 (`energetic`)                                 | duration                                           |
| 1 style      | 遵从我输入的自定义风格 (`custom`)                      | duration                                           |
| 2 duration   | 15 秒 · 一个结论 (`15`)                                | visual                                             |
| 2 duration   | 30 秒 · 简短讲解 (`30`)                                | visual                                             |
| 2 duration   | 60 秒 · 完整展开 (`60`)                                | visual                                             |
| 2 duration   | 120 秒 · 深度说明 (`120`)                              | visual                                             |
| 2 duration   | 根据内容推荐时长 (`auto`)                              | visual                                             |
| 2 duration   | 返回上一项 (`back`)                                    | style                                              |
| 3 visual     | SVG 动画 · 矢量图解 (`svg`)                            | voice                                              |
| 3 visual     | HTML 动画 · 页面与排版 (`html`)                        | voice                                              |
| 3 visual     | 返回上一项 (`back`)                                    | duration                                           |
| 4 voice      | API 配音 (`api`)                                       | music                                              |
| 4 voice      | 本地 breezeTTS2 配音 (`breeze`)                        | voice / 缺本地模型恢复                             |
| 4 voice      | 不用配音，只保留 BGM (`bgm`)                           | music                                              |
| 4 voice      | 返回上一项 (`back`)                                    | visual                                             |
| 5 music      | 从本地选择音乐 / 使用素材库音频 (`upload`)             | music / 具体音频确认                               |
| 5 music      | AI 生成音乐（使用已配置的音乐 API） (`ai`)             | music / 缺配置恢复                                 |
| 5 music      | 原版 Vibe 示例音乐 (`fixed`)                           | plan                                               |
| 5 music      | 本地程序化配乐（无需 API） (`auto`)                    | plan                                               |
| 5 music      | 不使用背景音乐 (`none`)                                | plan                                               |
| 5 music      | 返回上一项 (`back`)                                    | voice                                              |
| 6 plan       | 采用方案，开始文案 (`continue`)                        | script                                             |
| 6 plan       | 按修改意见调整 (`edit`)                                | plan                                               |
| 6 plan       | 重新生成此步骤 (`regenerate`)                          | plan                                               |
| 6 plan       | 返回上一项 (`back`)                                    | music                                              |
| 7 script     | 采用文案，安排分镜 (`continue`)                        | storyboard                                         |
| 7 script     | 按修改意见调整 (`edit`)                                | script                                             |
| 7 script     | 重新生成此步骤 (`regenerate`)                          | script                                             |
| 7 script     | 返回上一项 (`back`)                                    | plan                                               |
| 8 storyboard | 采用分镜 (`continue`)                                  | production                                         |
| 8 storyboard | 保存逐镜修改并重新审阅 (`save_shots`)                  | storyboard                                         |
| 8 storyboard | 按修改意见调整 (`edit`)                                | storyboard                                         |
| 8 storyboard | 重新生成此步骤 (`regenerate`)                          | storyboard                                         |
| 8 storyboard | 返回上一项 (`back`)                                    | script                                             |
| 9 production | 开始制作与合成 (`continue`)                            | post_edit                                          |
| 9 production | 1080p 原生高清制作（等待更久） (`publish`)             | post_edit                                          |
| 9 production | 返回分镜调整 (`back`)                                  | storyboard                                         |
| 10 render    | 自动 continue                                          | 编码、技术质检后进入 post_edit；失败可在同项目重试 |
| 11 post_edit | 保留初版，不使用剪辑库 (`skip`)                        | delivery                                           |
| 11 post_edit | 添加镜头间淡入淡出（生成剪辑版，保留初版） (`quercus`) | delivery                                           |
| 11 post_edit | 返回音乐选择，重新编排节奏 (`back`)                    | music                                              |
| 12 delivery  | 完成并保存作品 (`continue`)                            | succeeded                                          |
| 12 delivery  | 重新制作画面 (`regenerate`)                            | post_edit                                          |
| 12 delivery  | 保留画面，重新选择配音 (`voice_retry`)                 | voice                                              |

子状态与恢复：

- music_asset：上传、动态列表、`ready` 刷新、具体音频确认、`back`
  返回音乐来源。后端实时核验归属、来源、ready 状态、时长和实际文件。
- music_setup / music_failed：配置后重查或改用示例、程序化、上传；返回来源会清除残留子答案。
- breeze_install / voice_failed：缺模型不自动下载，可改 API / BGM；超时回退有持久标记。
- cancelled / cancelling：旧问题返回 409；可重试任务创建后继任务并复用当前项目与作品。
- failed render：真实编码器一次失败后重试，已完成画面和明确确认均复用。
- 过期问题 ID、重复回答：服务端原子拒绝；前端 pending 抑制连续提交，冲突后刷新当前问题。
- waiting_input 重启：问题、答案与草稿由 SQLite 恢复；过期后提示同项目重试。

测试证据：[原始分支记录](media/workflow-branches.json)、[Rust 日志](logs/rust-final.log)、[真实 UI 五条完整路径](ui-contract-results.json)。
