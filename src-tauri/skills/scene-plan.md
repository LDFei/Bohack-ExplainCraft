为一个镜头输出轻量 scene_plan JSON，不包裹 scene_plan 外层键。保持用户的教学重点、禁忌、自定义风格与图形要求，不能把科学编辑风格强加给用户。

必须有 objective（短教学标题，最多24字）、visual_concept、entities、relationships、events、final_state（最多28字）、style、aspect_ratio、claims。

entities 示例：{"id":"curve","kind":"plot","label":"","region":"secondary","x":0.04,"y":0.05,"width":0.90,"height":0.70,"points":[[0,0],[1,1]],"x_range":[0,1],"y_range":[0,1],"x_label":"时间","y_label":"观测量"}。
所有几何坐标 x/y/width/height 为当前 region 的 0～1 归一化值，必须在边界内。横屏 primary/secondary 左右排列，竖屏上下排列；为每个实体留标签与数值空间。
kind 只允许 path、ellipse、particles、plot、bars、vector、number、text、rect。path 必须带 path 字符串（局部0～100矢量路径），可组合实验容器、仪器、终端、复杂轮廓。particles 带 count（<=160）和 seed（整数），使用确定性轨迹；粒子运动用于解释对象内部，不能冒充实验统计。plot/bars 带 points、x_range、y_range、x_label、y_label，数据必须真实或标明示意；number 带 value 和 unit；颜色只能 #RRGGBB。允许其他字段：label、color。禁止额外字段。

relationships 为 [{"from":"实体ID","to":"实体ID","label":"关系"}]。
events 为 [{"entity_id":"curve","action":"trace","start_ms":0,"duration_ms":3000,"to":{},"reasoning":"曲线逐点展示观测结果"}]，按 start_ms 排序，全部在当前镜头时间内。action 可为 move/flow/trace/draw/transform/split/merge/spawn/remove/grow/shrink/connect/disconnect/set_value/reveal。to 仅允许 x/y/value/text/opacity/scale/color/easing；easing 仅允许 linear 或 accelerate（位移随时间平方，适用于恒加速度）；移动终点为区域内绝对归一化位置，必须容纳实体。每个动作要改变知识对象、数据或关系。数字用 set_value 的 to.value，禁止装饰闪烁和无意义移动。shape 的变化必须解释知识状态；split/merge 用多个实体的显隐和移动组合，不执行任意代码。

必须有至少一种非文字图形、至少一个作用于该图形的实质事件。不能通过增加文字框数量满足要求。不要只输出矩形文字卡。图形占主画面，文字用于短标签、数据和结论。一镜可有多个事件，不受旁白句数限制。

claims 为 [{"statement":"待核实的事实或数字","evidence":"用户提供/可检验计算或来源；未知则写待核实"}]。来源不是已核实标记。模型不能自称完成事实核验，不能编造试验结果、概率保证或物理数字。无数据时宁可定性演示并明确标示示意。

数值对象可使用 sample_from 字符串，引用同计划的 plot ID；宿主按照该图表的真实数据点、绘制事件和时间轴更新数值，不采用虚构的线性频率。其他实体 sample_from 写空字符串。palette 可为 null 或带 background/foreground/accent/muted 四个 #RRGGBB 色值的对象，依据用户风格设置。结构化 Schema 要求所有字段显式给出：无路径写空字符串，无点集写[]，无坐标范围写[0,0]，无数值写0，无改动的 to 字段写 null，easing 写空字符串。

状态文字可用 set_value 的 to.text（仅 text 实体）；数字用 to.value（仅 number 实体）。不能对 text 发送 value:null 冒充状态切换。
