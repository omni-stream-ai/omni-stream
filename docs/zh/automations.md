# 长期任务

OmniStream 是通用助手工作空间。普通请求在对话中完成；任务是对话结束后仍然有效、由时间或周期检查触发的长期委托。编程、消息汇总和代码统计属于助手能力，不是创建专用任务类型的理由。

## 边界与创建

- 即时查询、资料分析和修复直接在当前对话处理，需要执行能力时调用 Agent，不创建长期任务。
- 定时或重复执行的委托必须明确目标、时间、时区和交付方式，不猜测缺失信息。
- 对话中的 `propose_automation` 只保存 `pending` 草案，绑定当前会话。用户在长期任务面板确认前不会执行，模型没有启用工具。
- 面板“创建并启用”或保存配置即授权，无需二次确认。界面展示外部应用、账号、动作、固定目标和内容模板；标题可由委托内容生成。
- 工作目录可选；未指定时使用 Core 状态目录下的 `workspace` 和全局 Agent 配置，指定时使用该目录配置。目录须为已有绝对路径。缺少 Agent、来源或权限时明确失败，不能编造实时信息。

## 调度与日历

基础触发兼容旧配置：

| 触发 | 字段 |
| --- | --- |
| 每日 | `kind=daily`，`hour` 0–23，`minute` 0–59，`second` 0–59（默认 0），`utc_offset_minutes` −720 到 840 |
| 固定间隔 | `kind=interval`，`seconds` 60–31536000，从创建、启用或恢复时计算下一次 |

每日时间使用固定 UTC 偏移，不自动跟随夏令时。`calendar` 可组合 `weekdays`、`include_dates`、`exclude_dates`。星期为 ISO 1（周一）到 7（周日），空列表表示每天；选一个星期即每周，周一至周五即普通工作周，周六和周日即周末。

覆盖项为 `{date:"YYYY-MM-DD",source:"来源",reason:"原因"}`，须为有效公历日期，来源和原因必填，最多共 4096 项。优先级为 **显式排除 > 显式增加 > 星期规则**。每日按该任务偏移判断日期，间隔按 UTC 日期过滤并保留间隔相位；增加日期不会凭空增加间隔触发。间隔与星期永不相交时，下一次为 `null`，可修改规则或手动执行。

固定间隔保存调度基准，延迟处理、手动额外运行和只修改日期覆盖不会改变后续节奏；整体保存配置、启用或恢复时重新计算基准。旧记录缺少基准时，将已保存的下次计划时刻作为间隔相位参考；它不是最早允许时间，新增更早的有效日期仍须参与下一次计算。

平台不硬编码国家假日和调休。Agent 可查询可靠来源，将假日或调休日表达为带来源的覆盖项；不自带官方假日数据库，也不声称自动保持其最新。未到有效日期不启动 Agent、不发送消息。手动“立即执行”是明确的一次额外运行，可绕过日历过滤，但不能绕过窗口截止检查。

手动请求持久化为独立的 manual_run_at，周期时间仍保存在 next_run_at；日历更新、配置保存和并发等待不覆盖已受理请求。未认领前重复请求合并为一次，保留首次请求时间，窗口按该时刻和认领时的日历计算。同一任务优先认领手动请求，在同一事务中清除请求并创建运行；周期时间保持不变，已到期周期仍可在手动运行结束后合并执行一次。暂停、取消会清除尚未认领的手动请求。运行上下文的 trigger 区分 manual 与 schedule；GUI 在等待认领时显示“排队中”并禁用重复执行按钮。

`allow_calendar_updates` 默认 false。开启后，管理工具 `update_automation_calendar({id,calendar,reason})` 可修改未运行任务的日期覆盖；运行中的 Agent 使用 `automation_calendar({calendar,reason})` 修改自己的覆盖，无参数时读取当前日历。运行工具只接受 calendar 和 reason，更新时两者须同时提供；由 Core 绑定任务与运行，不接受任务 ID 或其他字段，结束后失效。Agent 不能改变基础星期规则。用户可通过 `/calendar` 接口修改完整日历。服务端保存时间、操作者、原因和完整日历快照；修改只影响未来调度，本次上下文不变。没有共享日历写接口，普通任务不能隐式影响其他任务。

## 统计窗口与生命周期

可选 `time_window`：

- `{kind:"lookback",seconds:86400}`：计划触发时刻之前的固定时长，1 秒到一年。
- `{kind:"previous_effective_day",hour:12,minute:0,second:0}`：同一日历下，上一个有效日期的截止时刻至本次日期截止时刻。

窗口为 Unix 秒 `[start,end)`，上下文还包含任务与运行编号、偏移、计划时刻、实际开始时间、可选 project 和实际 workspace 目录。延迟执行使用保存的计划时刻；前一个有效日期不等于前一次成功运行。每日配置拒绝晚于触发时刻的截止时间，手动和间隔运行也在调用 Agent 前检查。例如 11:57 执行不能包含当天 12:00 的数据。

委托状态为 `pending/active/paused/cancelled`。暂停阻止后续触发，当前运行继续；恢复从当前时间重新计算。取消还请求终止当前 Agent，且不可恢复。每次运行独立编号，状态为 `running/succeeded/failed/cancelled/interrupted/skipped`。每个委托最多同时运行一次，委托间并发由 `max_concurrency` 限制，默认 4、最小 1。

同一状态目录只支持一个 Core。HTTP 服务启动约每秒扫描的调度器；SQLite 事务认领运行、持久化下次时间后启动 Agent。错过多个周期合并为一次，不逐次补发；执行超过周期时结束后至多补一次已到期运行。重启将旧运行标为中断并通知会话，不自动重放，未来调度继续。Core 必须持续运行。

修改内容或调度重新计算下次时间，保留执行历史；草案修改仍待确认。执行中或已取消委托禁止整体修改，任务范围内已授权日历覆盖和授权撤销除外。保留最近 100 次运行；历史会话消息继续保留。

条件监控是周期轮询，不是事件订阅。`silent_unmet` 默认 false，保持每次交付；开启后 Agent 必须返回 `{output_markdown:string,condition_met:boolean}`。明确 false 时保存 `skipped` 和原因，不发应用内或外部消息。无效结构化结果报错，不能当作条件未满足。

## 每次执行前确认

可选 `confirmation={before_seconds:600,on_timeout:"skip"}` 开启逐次确认，默认关闭。before_seconds 为 1 秒至 7 天，表示计划执行时刻之前多久发起确认；例如 12:05 执行、提前 30 分钟确认，对应 11:35。使用原任务的调度、偏移和有效日期，不为被日历排除的周期提醒。创建或恢复时已过提醒时间则立即提醒；Core 离线不会补发所有历史周期。

调度器在接收会话中发送带操作卡片的应用内确认消息，展示本次内容、计划时刻、统计窗口和固定投递目标，并提供“沟通调整”入口。请求和消息在同一事务保存，重启或重复扫描不重复提醒。确认本身不启动工作 Agent、不生成或投递业务结果；只有用户发送讨论消息时才启动沟通 Agent。通知不自动发送到外部服务或操作系统。

每个计划周期独立编号，状态为 pending、approved、ready、consumed、skipped、expired 或 invalidated。用户可直接编辑本次委托内容，也可进入该次独立会话与任务所用 Agent 沟通。Core 向沟通 Agent 绑定当前确认编号并提供 automation_confirmation 工具，允许读取和修改本次目标文本，不接受其他任务编号，不能确认执行、修改时间和投递授权或调用外部写工具。最终确认和跳过只通过用户界面/API，不提供给模型。修改内容递增 revision 并撤销旧确认，用户须审阅新版本再确认；提交旧 revision 明确失败。确认时可显式选择将目标文本用于后续执行，默认仅本次。后续生效不修改时间、目录、Agent、日历或外部目标，也不恢复已撤销的投递授权。

提前确认只授权该次运行，不提前执行。到原计划时刻，按下述未确认策略决定是否认领运行；明确确认的新目标进入运行上下文。统计窗口始终按原计划时刻计算，不因讨论、等待或并发延迟改变。认领、新运行及确认消费在同一事务提交，重复确认不导致重复执行。一次运行失败仍按原恢复规则处理，不自动重放。

on_timeout 有三种策略：skip（默认）在计划时刻仍未确认则跳过该次；wait 持续等待，用户稍后确认后立即执行；run 在原计划时刻执行，即使用户未确认。run 是用户保存或启用任务时授予的到时自动执行策略，Agent 仍不能自行批准。

run 在到时的首次调度扫描中将本次状态固定为 ready：已明确确认且未重新讨论的版本使用确认内容；其余情况使用提醒创建时的原目标快照，未确认的编辑或 Agent 建议不自动生效。正在讨论不延迟到时决策，未保存的页面输入不影响执行。保存执行来源 user 或 timeout 及选定内容，自动执行不能记为用户批准。工作 Agent 只接收选定内容和确认元数据，未确认建议及历史草稿仅留在审计记录中。ready 和 consumed 不再接受修改、批准或跳过，晚到的讨论工具调用明确失败。并发排队或重启后使用同一份已固定内容和原统计窗口，只启动一次；离线错过多个周期仍合并处理一次，不补发历史确认提醒。

等待时不积累后续周期，处理完当前请求后继续最近的未来调度。手动“立即执行一次”在启用此设置时同样需要确认，但立即发起请求并等待，不因已到当前时刻而自动过期。手动与周期确认互相独立，只在满足执行策略且到时的请求中优先认领手动请求；未确认的手动请求不阻塞已确认或允许自动执行的周期。未启用的旧任务保持原行为。

暂停、取消、整体保存配置、日历变更或撤销授权会使尚未消费的确认失效，旧卡片不能启动任务。恢复或新配置按新计划重新发起；运行中的上下文不被后续讨论改写。跳过和过期保存 skipped 运行记录及原因，不执行 Agent 或业务投递。待确认、已确认和 ready 请求重启后保留，正在执行的运行仍标记中断。保留最近 100 条确认记录及独立讨论会话；讨论和工具只受理仍有效的确认，过期或失效后拒绝修改。线程与命名 Agent 的记忆按现有规则处理，Core 工具边界不承诺隔离外部执行引擎的全部本机文件访问。

## 执行与交付

委托可通过可选 agent_id 绑定[命名 Agent](agents.md#插件与长期任务)，使用其角色和插件范围；未绑定时沿用下文的默认选择。助手删除或引擎失效时明确失败，不自动换用其他助手。
绑定助手的运行还会加载该助手的[自主记忆](agents.md#自主记忆)，并可通过 agent_memory 维护自身记忆；该本地能力不授予外部插件写入权限。

Core 使用配置的 default、codex 或其他可用 Agent，以独立作用域提供完整目标、运行上下文和最近成功结果。不自动生成开发计划、启动项目服务、执行代码测试或等待开发验收；具体汇总与统计由 Agent 按目标完成。执行时长遵循[适配器统一契约](adapters.md#统一契约)。JSONL 缺少取消能力，不用于长期委托。

生成阶段插件网关提供外部读取动作、任务日历及命名 Agent 自身的记忆工具；外部写入由 Core 投递阶段执行，浏览器不自动参与后台生成。CLI 成功结果使用纯 stdout，进程诊断不混入结构化条件或投递内容，执行失败仍保留诊断。文件、命令和其他来源仍受 Agent 自身权限与任务授权约束，不承诺隔离外部 Agent 的全部本机访问。需要审批或输入时在任务面板显示，主动取消覆盖等待阶段。

`delivery_targets` 默认为空，保留应用内交付。可显式配置 `{type:"in_app",id:"inbox"}`，或 `plugin_action` 目标：唯一 id、plugin、action、destination、payload 和 max_attempts（1–3，默认 1），最多 16 项。

连接器声明 `delivery_destination_fields`，目标字段须为核实过的固定 ID，不允许模板或内容覆盖。模型配置前先调用读取操作核实频道、仓库、页面或文件；用户表单输入已核实 ID。payload 仅支持 `{{run.output_markdown}}` 变量，不执行脚本。配置和实际调用都校验完整参数 schema；超长内容明确失败，不静默截断。

当前声明投递能力的动作是 Slack 发消息、GitHub 创建 Issue、Notion 创建页面、Figma 评论。新增应用在连接器声明投递能力，调度核心不加应用分支。开启私聊等其他写动作不能配置为后台投递，高风险业务操作不借此自动获得权限。

配置保存账号摘要、连接代次引用和授权时间，不复制令牌。授权配置、启用、更新和撤销留审计记录；运行上下文保存当次投递目标与授权快照，修改任务不改写旧运行。草案经用户启用后才执行，面板创建和保存即明确授权。调用前重查任务授权、插件安装、连接代次、启用及写权限。停用、断连、账号或连接设置变化使旧授权失效；`/revoke` 可撤销任务投递授权，重新授权需用户检查配置后保存。已发出的请求不能撤回。

每次投递先持久化并认领 `automation:<任务>:<运行>:<目标>` 幂等键，再调用服务；记录 `pending/succeeded/failed/unknown`、次数、时间、外部结果及错误。重复处理同一运行目标不重复调用。只有适配器确认未生效且可重试的限流错误，才按 max_attempts 和 Retry-After 重试；仅接受 1–60 秒等待，权限和参数错误不自动重试。超时、断线、5xx 或损坏响应等结果不明情况停止并记录 unknown。重启把未完成投递标为 unknown，不重放。此为本地防重和保守恢复，不承诺第三方 exactly-once 或跨服务事务。

`delivery_policy`：`independent` 默认分别投递，任一失败则运行失败；`fail_fast` 首次失败后停止后续外部投递；`fallback_in_app` 保留失败记录，以应用内结果作为完成。已成功外部操作不回滚。`generation_output` 单独保存生成内容，生成成功不等于投递成功。

应用内结果始终保留并交付，静默条件跳过除外。对话草案绑定原会话，面板绑定当前会话，未指定时生成 `automation:<id>`。结果和会话消息在同一 SQLite 事务提交，重复完成不重复发消息。GUI 展示日历、统计窗口、账号和目标、授权撤销、运行及逐目标结果、日历审计。无投递插件的渠道与操作系统通知不在本实现范围内。

## HTTP 与工具

投递是否成功还须通过连接器的[成功响应校验](plugins.md#助手调用与写入)，HTTP 成功但缺少必要成功标识时记录 unknown，保留一次尝试且不自动重发。

- `GET /automations`：委托、公开授权摘要、日历审计和最近运行记录。
- `POST /automations`：直接创建并启用；必需 title、goal、schedule；可选 project、conversation、calendar、time_window、delivery_targets、delivery_policy、silent_unmet、allow_calendar_updates、confirmation。
- `POST /automations/<id>/activate|pause|resume|cancel|run`：启用、暂停、恢复、取消或手动运行。
- `POST /automations/<id>/update`：完整委托字段，省略 conversation 保留原值，重新保存投递授权。
- `POST /automations/<id>/calendar`：`{calendar,reason}` 更新任务自己的日历并审计。
- `POST /automations/<id>/revoke`：撤销该任务全部外部投递授权。
- `GET /automations/<id>/confirmations/<confirmation_id>`：读取本次确认及内容版本。
- `POST /automations/<id>/confirmations/<confirmation_id>/update|approve|skip`：携带 revision 修改目标、确认或跳过；update 还须 goal，approve 可携带 apply_to_future=true。

管理模型使用 list_automations、propose_automation、pause_automation、resume_automation、cancel_automation、run_automation、update_automation_calendar。启用只通过用户面板/API；运行网关的 automation_calendar 仅管理当前任务日期。

通用工作周示例（目标须先核实）：

```json
{
  "title":"进度摘要",
  "goal":"根据授权数据源和工作目录汇总统计窗口内的工作进度，附来源",
  "schedule":{"kind":"daily","hour":12,"minute":5,"second":0,"utc_offset_minutes":480},
  "calendar":{"weekdays":[1,2,3,4,5],"include_dates":[],"exclude_dates":[]},
  "allow_calendar_updates":true,
  "time_window":{"kind":"previous_effective_day","hour":12,"minute":0},
  "delivery_targets":[{"type":"plugin_action","id":"report","plugin":"slack","action":"send_message","destination":{"channel":"C_RESOLVED_ID"},"payload":{"text":"{{run.output_markdown}}"},"max_attempts":2}],
  "delivery_policy":"independent"
}
```

## 验证

即时对话的 Agent 执行使用内部运行记录，不加入长期委托。旧开发任务移除后的数据边界见[测试与兼容边界](testing.md#验证范围)。

Rust 单元测试覆盖时间和覆盖优先级、窗口、间隔基准与历史记录兼容、无可用星期的间隔规则、生命周期、并发认领、结果事务、Agent 日历权限、静默跳过和中断恢复。真实 Core 配合本地模拟服务验证四个连接器投递、限流重试、未知结果不重发、撤销和连接变化、重启防重；CLI 命令网关及 App Server MCP 测试验证实际上下文、任务日历授权、生成阶段禁止插件写入和浏览器，以及条件未满足静默跳过与无效输出报错。命令为 `cargo test -p omnistream --test automation_delivery --test automation_workflow --test agent_plugins_workflow`。GUI 检查创建、编辑、启用、授权、日期配置、窗口和记录，运行类型检查、单元测试及 `automation.spec.ts`；请求失败后按[公用状态规则](gui.md#多语言设置)停止轮询并提供手动重试。模拟验证不代表真实服务账号或生产环境已验收。
