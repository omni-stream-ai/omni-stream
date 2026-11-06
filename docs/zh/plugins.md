# 插件市场

GUI 侧栏的“插件市场”提供 Slack、GitHub、Notion、Figma 服务连接器及 Chrome 浏览器自动化工具的按需安装。首次使用均未安装，不自动启用；可搜索、筛选已安装项目，逐项下载或卸载。下载将其 manifest 注册到本地。四款服务连接器随 Core 发布，不安装第三方桌面客户端；Chrome MCP 的固定官方 npm 包在检查或执行时获取，见下节。

## 通用 Agent 接入

Linux 上的插件转发器通过 `/proc/<Core PID>/exe` 启动当前 Core 对应的程序版本；运行期间构建替换或删除磁盘上的二进制，不影响已有执行及后续会话恢复的插件启动。该路径依赖当前 Core 进程存续及本机 `/proc` 访问权限，不解析为磁盘路径；无法访问时在创建网关前明确报错。其他平台沿用当前可执行文件路径。此机制不热更新 Core，加载新版本仍需重启。

五款插件共用 Core 的插件工具网关，不再限定 Codex 或管理助手。普通委派、配置协助和长期委托按当前安装、连接和启用设置准备工具；服务插件仍需验证令牌，Chrome 仍需单独启用及浏览器授权。Agent 必须支持原生 MCP 或本机命令执行，只有文本输入输出的 Agent 无法自行调用工具。

命名 Agent 的网关还提供 Core 本地的 agent_memory，不依赖服务插件安装或写入开关，可主动维护本 Agent 的长期记忆；隔离、容量与用户清理规则见[自主记忆](agents.md#自主记忆)。配置协助不提供此工具，记忆不能替代外部操作授权。

| Agent 接入方式 | 工具传递 |
| --- | --- |
| App Server | 通过进程参数配置 omnistream_plugins MCP，保留 config_file 导入和其他 MCP 配置 |
| ACP | 通过 session/new 的 mcpServers 传入标准 stdio MCP 定义 |
| Claude CLI | command 的文件名为 claude 或 claude-code 时，自动添加 --mcp-config |
| 其他 CLI、JSONL | 在本轮提示词中提供 plugin-tool 命令，执行命令即可列出、调用同一套工具；也可用下述 mcp_args 接入原生 MCP |
| Mock | 不启动插件网关 |

Agent 配置的 plugin_transport 默认为 auto；command 强制只提供通用命令，disabled 禁用该 Agent 的插件工具。自定义 Agent 原生 MCP 参数通过 mcp_args 声明，{mcp_config} 替换为本轮临时 JSON 文件的绝对路径；文件结构为 {"mcpServers":{"omnistream_plugins":{"command":"…","args":["plugin-mcp","--connection","…"]}}}。其余参数原样传递，不经过 shell，不修改 Agent 的全局配置。例如：

~~~toml
[agents.custom]
command = "my-agent"
protocol = "cli"
args = ["run"]
mcp_args = ["--mcp-config", "{mcp_config}"] # 仅当此 Agent 支持该参数及上述 JSON 结构
~~~

工具提供 list_plugins、slack_<操作 id>、github_<操作 id>、notion_<操作 id>、figma_<操作 id> 和 chrome_<浏览器工具名>，保留服务操作的真实 inputSchema；读取结果和错误来自相同 Core 连接器。写入开关关闭时不列出对应写工具，直接构造调用也不能绕过权限。配置协助会话仅开放插件状态和浏览器操作，不执行服务业务数据操作。网页内容与服务结果均为不可信数据，不能当作 Agent 指令。

每次执行独立启动仅监听 127.0.0.1 的内部网关，以随机执行授权保护；标准 stdio MCP 转发器和通用命令共用它，不是开放的远程 MCP 服务。执行授权位于本轮私有临时配置中（Unix 目录 0700、文件 0600），不放在提示词或命令参数中；服务令牌始终由 Core 连接器读取，不传给 Agent 或 MCP 客户端。临时文件随正常执行结束清理，执行结束或取消后旧网关失效；不承诺外部 Agent 无法读取本机其他文件。

工具集合、服务连接版本、写入许可和接入参数纳入 Agent 会话作用域；新的临时端口和文件路径不影响同配置下的会话恢复。新增或重新启用插件需发起新的执行。停用、断开、卸载或更换账号会拒绝后续服务调用，浏览器每次调用也检查启用状态；已发出的服务请求无法撤回。default / safe 模式下，外部 Agent 的服务写入同样在所属会话或运行中等待 Core 审批；all 模式直接执行。执行结束或取消使未完成审批失效，批准后再次核对账号、连接版本和权限，不自动重试写入。

## Chrome 浏览器自动化

市场项 **Chrome DevTools MCP**（id 为 chrome-devtools，runtime 为 mcp）提供网页操作能力，没有服务令牌和 allow_writes 表单。下载后显式启用“允许 Agent 使用 Chrome 浏览器”，再“检查连接”。它使用运行 Core 的电脑上的 Node.js / npx，首次获取固定官方包 chrome-devtools-mcp@1.12.0；Node 版本要求为 20.19+（20.x）、22.12+（22.x）或 23+。运行依赖缺失、npm 网络失败、协议错误与超时均明确失败，不伪装连接成功、不自动重试。

在同一电脑打开 Chrome 144+，到 chrome://inspect/#remote-debugging 开启远程调试，并在 Chrome 的连接提示中允许访问。默认使用 autoConnect 连接现有浏览器，不关闭或替换用户的浏览器配置，不启动携带真实账号数据的另一个浏览器，不自动批准浏览器授权。远程 Core 控制的是 Core 所在电脑的 Chrome。检查先完成真实 MCP 握手和工具列表校验，再调用 list_pages；结果只返回连接状态和验证时间，不返回页面标题、URL、内容或原始进程输出。最长等待 60 秒，检查期间市场、停用和卸载仍可操作；设置变化后的旧检查返回 409，不能恢复已停用或卸载的工具。

Agent 接入方式遵循上文[通用 Agent 接入](#通用-agent-接入)。浏览器启用时，配置协助从允许使用插件工具的 Agent 中按名称选择，不再筛选 app-server；没有可用者提示检查 Agent 配置。已有协助会话继续使用原 Agent。

通用网关以 chrome_ 前缀开放页面导航、选择标签页、点击、表单填写、键盘输入、等待和可访问性快照等工具。白名单在 Core 网关中强制执行，所有 Agent 和命令调用都受约束；关闭 JavaScript 求值、网络检查、性能与内存工具，不开放截图或 Cookie / 存储读取工具，关闭包的统计与 CrUX 请求。每次执行按需启动一个 Chrome MCP 进程并保留所选页面状态；浏览器调用最长 60 秒，取消后停止等待并清理子进程。浏览器仍能读取网页内容，不能据此承诺外部工具和日志绝不接触秘密；登录、验证码、授权确认、生成和显示令牌的步骤由用户接管，Agent 不应打开无关标签页内容或复制凭据。服务连接器的写入开关仅约束服务连接器，网页操作必须遵守用户明确授权的范围。

插件配置协助首次启动时，在创建会话前检查 MCP 握手与所需工具；浏览器是否允许访问由检查连接或首次页面操作确认。指令要求先调用 chrome_list_pages 检查当前连接，不沿用旧会话中“没有浏览器工具”的结论。启停设置保存在插件目录 mcp/chrome-devtools.json；卸载删除该设置。停用和卸载阻止后续浏览器调用。

## 连接与使用

安装后点击“配置与使用”，填写服务的访问令牌，选择是否启用，并“验证并保存”。Core 使用官方身份接口验证账号后才保存；替换令牌失败保留原配置。再次编辑时令牌不回显，留空沿用原令牌。停用不要求验证成功；“测试连接”只验证，不自动启用；“断开连接”删除凭据并保留安装，卸载同时删除凭据。刷新和重启保留状态，连接同一 Core 的不同浏览器共享配置。

第三方请求期间，市场查询、其他插件和停用、断开操作仍可使用。验证期间若连接已被修改、断开或卸载，旧验证结果不能保存或恢复凭据，返回 409；需要按最新设置重新提交。前端收到其他客户端更新后的连接状态时，同步启用和写入选项并提示设置已更新。

在本界面保存连接设置后，操作表单和旧结果会重置，避免切换凭据时沿用上一次连接的数据。

| 插件 | 凭据与可用操作 |
| --- | --- |
| Slack | 保留应用 Bot Token，新增个人账号 User OAuth Token；列出成员、读取成员资料、列出频道或私信、读取消息与线程回复、开启私信或群聊、发送消息或回复线程。两种身份及权限配置见[Slack 连接方式](#slack-连接方式)。历史和线程每页最多 15 条，可通过游标继续。 |
| GitHub | 细粒度个人访问令牌，并选择目标仓库；列出仓库、读取仓库、列出 Issue / Pull Request、创建 Issue。需要 Metadata、Issues、Pull requests 的读取权限，创建 Issue 需要 Issues 写入权限。 |
| Notion | 内部集成令牌，目标页面需连接到该集成；搜索页面与数据源、读取页面元信息、读取页面或区块子内容、在页面下创建子页面。读取需要 Read content，创建需要 Insert content。创建操作接受纯文本标题和可选正文，不创建数据库属性；每个文本最多 2000 字符。 |
| Figma | 个人访问令牌；读取设计文件、读取评论、发表评论或回复。需要 `current_user:read`、`file_content:read`、`file_comments:read`，写评论需要 `file_comments:write`。读取文件默认深度 2，最多 10；新根评论位于画布坐标 `(0,0)`，回复使用已有评论 ID。 |

这四项均通过访问令牌接入，Core 不托管 OAuth 应用注册或浏览器授权回调。配置页提供各服务获取令牌的官方说明链接。具体可访问资源由服务端权限控制，安装插件不授予新的服务权限。

“使用插件”根据 Core 返回的参数 schema 展示表单，操作成功显示结果与原始数据，并支持 JSON 导出。列表查询保留服务分页信息，可点击下一页；不会自动遍历全部页。空数据、加载和失败遵循 [GUI 公用状态](gui.md)，失败不自动重试；用户可修正配置或参数后重新提交。界面同时提供中英文文案。

单次结果先展示 100 项，可继续展开本次响应中的其余项目；展开结果与请求服务的下一页独立。原始数据预览最多显示 100000 字符，JSON 导出包含完整响应。

### Agent 配置协助

四款插件的配置页均提供“让 Agent 协助获取令牌”。点击会直接启动 Core 已配置的可用 Agent 中按名称排序的第一个，打开独立、持久化的配置协助会话，无需先调用管理模型或选择项目。仅携带插件、连接方式、是否启用、监听及范围、是否申请写权限和界面语言；不会传递已填写或已保存的令牌。没有可用 Agent 时就地提示 `config.toml` 配置方法，不创建空会话、不自动重试。长说明与官方链接折叠在“手动配置说明”，始终可手动连接。

Agent 根据所选权限协助操作官方配置；Slack 区分 Bot / User Token，只有个人账号选择持续接收时才指导同一应用的 Socket Mode、App-level Token 和 User events 配置。默认只申请读取权限；选择允许写入才申请对应服务的写权限。Agent 能否操作网页取决于其实际配置的浏览器工具，缺少工具时提供分步指导，不声称已经操作。用户完成登录、验证码、管理员审批与授权确认，并直接将令牌填回插件配置的密码框，仍需“验证并保存”才建立或改变连接；协助本身不发送消息、创建业务数据或启用插件。

会话顶部显示插件、Agent、所选身份与权限，以及“返回配置”。返回时恢复非敏感选项，凭据字段为空；关闭配置框可继续原协助会话。刷新和 Core 重启保留协助历史与返回入口。后续消息直接交给原 Agent，不经过管理模型；支持恢复的适配器恢复原 session，其余适配器携带近期会话结果重新执行，适配器限制见[适配器](adapters.md)。原 Agent 不可用或插件已卸载时明确报错。执行沿用会话审批、取消和错误展示；新协助使用默认审批模式，不继承其他会话设置，不产生可见的开发任务。

协助请求拒绝令牌和任意提示词等额外字段，同一 request_id 与相同选项幂等返回原会话和首次调用。协助对话拒绝附件和可识别的标准令牌格式；Core 不保存逐段文本或原始工具进度，完成后遮蔽已知令牌前缀，审批请求中的认证字段被移除，避免分段输出泄露，结果也不交给管理模型总结。这是额外防护，不能保证任意外部 Agent 工具及自身日志的安全；Agent 指令要求不读取、复制、输出或截图凭据，遇到凭据页让用户接管，不读取本机已有密钥、CLI 登录文件或 Cookie，不直接修改连接文件。

### Slack 连接方式

配置页提供“应用机器人（Bot）”和“个人账号（User Token）”两种方式，同一插件一次使用一个连接。旧配置默认保留 Bot 方式，不改变已有令牌、权限和身份；切换需填写新令牌，验证失败保留原连接。切换表单会清空待填令牌、关闭待保存的写入选项，并暂停操作，保存成功后清空旧表单和结果。其他客户端更换连接方式也会同步到页面。

- **Bot**：使用应用 Bot Token，读取目标频道通常需先邀请机器人加入。公共频道读取需要 Bot Token Scopes 中的 `channels:read`、`channels:history`，发送需要 `chat:write`。其他会话同样受 Bot 自己的权限和成员关系限制。
- **个人账号**：使用本人授权的 User OAuth Token（`xoxp-`），通过官方 Web API 以本人身份读取和发送。读取公共频道无需邀请机器人；私有频道、私信和群聊限本人有权访问的会话。仍需在工作区安装并授权 Slack 应用，遵循工作区策略，不是免授权访问。配置时验证 `auth.test` 返回的用户身份，拒绝 Bot 身份和 CLI 配置、App-level 或浏览器会话令牌。

个人账号的权限在 Slack 应用 **OAuth & Permissions → User Token Scopes** 配置，按需选择：公共频道 `channels:read`、`channels:history`；私有频道 `groups:read`、`groups:history`；私信 `im:read`、`im:history`；群聊 `mpim:read`、`mpim:history`；发送消息 `chat:write`；开启或恢复私信、群聊分别需要 `im:write`、`mpim:write`。修改后重新授权应用，复制 User OAuth Token 到配置页，不能误用 Bot Token。权限不足由 Slack 明确报错，连接验证成功不代表已具备全部操作权限。

“列出会话”的会话类型可选公共频道（默认，兼容旧调用）、私有频道、私信或群聊，单次查询一种类型以便按需授权。通过返回的会话 ID 调用“读取会话消息”或“发送消息”；“开启私信或群聊”接受 1–8 个其他用户 ID，以逗号分隔，返回的 `data.channel.id` 可用于后续收发。开启会话属于写入，受写入开关和助手审批约束。Bot 发送使用机器人身份，个人账号发送使用本人身份。

成员查询通过只读操作 `list_users({limit?,cursor?})` 和 `user_info({user})` 提供给 GUI、管理模型及通用 Agent 网关（`slack_list_users`、`slack_user_info`），Bot 和个人账号均需 `users:read`。`list_users` 调用 `users.list`，默认每页 100 人、最多 200 人，保留 `data.members` 中的 ID、姓名、显示名和停用/机器人标记及 `next_cursor`；每次只请求一页。助手按 `real_name`、`profile.real_name`、`profile.display_name` 或账号名查找用户，未找到且有游标时继续分页，不能把单页无匹配当成全工作区无匹配。同名多人与停用账号不能直接作为收件人；需要澄清或进一步核实。`user_info` 调用 `users.info`，接受单个以 U 或 W 开头的用户 ID，不能传入频道或私聊 ID。确认收件人后，再通过已有的 `open_conversation` 和 `send_message` 完成发送，查询本身不创建会话、不发送消息，也不改变写入审批规则。成员查询不要求邮件权限。缺少权限时返回 Slack 错误，不自动修改授权或重试。

按需查询仍可使用“读取会话消息”，通过 `oldest` / `latest` 时间戳限定范围（不含端点），保持字符串精度；新消息可在上次时间戳后再次查询。线程内容使用“读取线程回复”，传入会话 ID 和主消息 `ts`，支持分页；Bot 仅支持自身有权限的私信及群聊线程，公共或私有频道线程需个人账号及对应历史权限。分页不自动追踪全部消息。

#### 持续接收与收件箱

个人账号方式另提供“持续接收新消息”，默认关闭，由用户选择监听范围：

| 范围 | 同步内容 |
| --- | --- |
| 私信、群聊、@我和相关线程 | 全部私信和群聊、频道中 `<@本人用户 ID>` 的直接提及、本人发送的消息，以及开始监听后这些消息所在的线程回复；不把普通频道消息或 `@here` / 用户组提及自动视为个人提及 |
| 所有已加入的频道、私信和群聊 | Slack 为当前用户授权的所有消息事件；实时事件仍受频道成员关系约束，不包含未加入的公共频道历史 |

设置步骤：在**同一 Slack 应用**开启 Socket Mode；在 Basic Information 的 App-level Tokens 生成具有 `connections:write` 的 `xapp-` 令牌；在 Event Subscriptions 的 **Subscribe to events on behalf of users** 中按需订阅 `message.im`、`message.mpim`、`message.channels`、`message.groups`，并授予对应 User Token 历史权限、重新授权。不要只订阅 Bot events。将 User OAuth Token 与 App-level Token 填入配置页，选择范围后验证保存；两个令牌都不回显，App-level Token 留空保留，失败保留原配置。

Core 通过官方 `apps.connections.open` 获取私有 WSS 地址，无需公网回调或 Slack CLI；关闭网页不停止接收。启停插件、关闭监听、断开或卸载会停止后续接收，保存新设置会重新连接；Core 重启后按保存设置恢复。网络断开使用有上限的退避重连，遵守 `Retry-After`；认证与缺少权限错误停止自动重试，修改设置或在收件箱点击“重新连接监听”恢复。界面显示停止、连接中、接收中、重连和异常状态。

侧栏 **Slack 收件箱**（`/slack`）按会话展示消息，保留发送者 ID、时间、线程、编辑和删除标记，提供 Slack 原会话链接；不自动下载附件，非文本消息提示在 Slack 查看。每次先读最新 50 条，可加载更早消息。当前选择的会话不会因其他会话的新消息跳转。开启写入后可直接以本人身份发送或回复线程，点击即执行；Core 检查界面所显示的工作区和用户身份，拒绝切换账号后遗留的发送请求。助手发送仍遵循会话审批。

消息写入当前 Core 的 SQLite，按工作区和本人用户 ID 隔离，先提交再确认 Socket 事件；重复事件、Socket 重连和本人发送后的回声不会重复展示，迟到的编辑不能恢复已删除的消息。个人发送也保存到同一收件箱；若服务已发送但本地保存失败，返回发送成功及同步提示，不诱发自动重发。切换账号不显示旧账号数据；断开或卸载删除凭据但保留已同步历史，重新连接同一身份可继续查看。

持续接收仅处理 Slack 实际交付的新事件，不自动补拉启用前或断线期间的历史；必要时可使用按需查询。线程跟踪从收到相关主消息或本人发送开始。只接受明确授权给当前用户的事件，忽略其他工作区、其他用户及 Bot 授权，适合为当前用户授权的应用；不会把收件消息当作助手指令，不自动调用模型、自动回复或改变长期任务交付渠道。

本实现直接调用 Slack API，无需安装 CLI；官方 [`slack api`](https://docs.slack.dev/tools/slack-cli/reference/commands/slack_api/) 也是同一 API 的调用方式，仍需相应令牌和权限。不会读取本机 CLI 凭据、浏览器 Cookie，也不会自动执行 CLI 登录或把 CLI 登录视为消息访问授权。

## 助手调用与写入

HTTP 成功不单独代表动作成功。Slack 的成功 HTTP 响应必须有布尔型 ok；发消息还须有频道和有效时间戳。GitHub 创建 Issue 须有正整数 id 和 number，Notion 创建页面须有 object=page 和非空页面 id，Figma 评论须有非空评论 id。合法 JSON 缺少这些标识或字段类型错误时，连接器返回结果不明错误；交互式调用不重试，长期任务按[投递规则](automations.md#执行与交付)记录 unknown 并停止重发。

管理模型上下文包含插件安装、连接、权限及可用操作参数；Chrome 另提供 runtime=mcp 和公开 browser 状态，管理模型不能通过 plugin_invoke 操作浏览器，也不索要令牌。`list_plugins({})` 读取真实状态，`plugin_invoke({"plugin":"github","action":"list_issues","input":{"owner":"org","repo":"repo"}})` 执行相同的 Core 连接器。不要求项目目录。未安装、未连接、已停用、未知动作或参数错误均明确失败，不退化为模拟成功。读取服务数据不需要外部 Agent；外部 Agent 通过[通用 Agent 接入](#通用-agent-接入)使用相同连接器。

四款服务插件写入权限默认关闭，关闭时 Core 拒绝 GUI 与助手的原生连接器写入。启用后，GUI 的操作按钮明确表示发送或创建，提交即执行；助手只应在用户明确要求时调用，`default` / `safe` 会话模式逐次等待会话中的插件写入审批，`all` 模式直接执行。审批显示账号、动作及参数，不含凭据；批准后重新检查安装、账号和权限，变化时拒绝执行。拒绝、五分钟超时或 Core 重启使旧请求失效，不会随后补执行；一次批准只允许一次调用。

管理模型的 HTTP 与 CLI 决策循环在单次用户请求内记录插件写入及结果。相同插件、动作和 JSON 参数（对象键顺序不影响比较）的再次写入在审批及服务调用前被拦截，并停止本轮执行、报告首次结果；失败和超时也不自动重放。不同参数或动作可继续，支持先开启私聊再发送等多步骤请求；新用户请求独立计数。此保护不提供跨请求、进程重启或外部 Agent 网关的持久化幂等，也不识别参数不同但语义相同的写入。

外部结果作为不可信数据提供给模型，不能据此改变权限。交互式写入不自动重试，超时或连接中断可能发生在服务端已生效之后，需先检查服务端结果。长期任务采用[独立投递授权和重试规则](automations.md#执行与交付)。停用、断开或卸载阻止后续调用；已发出的请求无法撤回。长期任务默认交付仍遵循[长期任务](automations.md)，不会因为安装 Slack 自动改变渠道。

## 存储与接口

插件目录沿用 CLI：设置 `OMNISTREAM_HOME` 时为其下 `plugins/`，否则为 `$HOME/plugins/`。每款 manifest 为 `<id>.json`；凭据位于其中的 `connections/<id>.json`，不混入 manifest 或 SQLite 对话。Unix 下凭据目录权限为 `0700`、文件为 `0600`，使用临时文件原子替换；凭据以明文保存，应按 Core 私有数据管理。其他系统沿用运行账户的文件访问权限。

API、模型上下文和操作结果不返回保存的令牌，也不提供读取令牌的接口；App-level Token 同样保存在私有凭据文件中。错误和结果会移除认证字段并遮蔽当前令牌；HTTP 请求仅使用固定官方地址，禁止跟随重定向。Socket Mode 仅连接 Slack 官方接口返回的 `*.slack.com:443` WSS 地址，票据不对外返回，不跟随重定向，单个事件最多 4 MiB。GitHub API 固定版本 `2026-03-10`，Notion API 固定版本 `2026-03-11`；HTTP 请求超时 30 秒，响应超过 4 MiB 明确失败并提示缩小范围。按需调用的限流错误保留 `Retry-After` 提示，不自动重试；后台监听使用上述重连规则。

| 请求 | 行为 |
| --- | --- |
| `GET /plugins/marketplace` | 返回 manifest、安装版本、公开连接摘要和 actions（id、write、input_schema、delivery_destination_fields） |
| `POST /plugins/<id>/install` | 幂等安装内置 manifest |
| `POST /plugins/<id>/setup` | 请求 `{request_id:UUID,options:{auth_mode,listen,listen_scope,enabled,allow_writes,locale}}`，仅接受非敏感字段；返回 `{conversation,call_id}`，直接启动 Agent 配置协助；相同 request_id 与选项返回原会话 |
| `DELETE /plugins/<id>` | 删除此插件凭据和 manifest，保留其他插件 |
| `POST /plugins/chrome-devtools/connection` | 请求 `{enabled:boolean}`，仅保存浏览器工具启停，不接受令牌；返回更新的市场项 |
| `POST /plugins/chrome-devtools/test` | MCP 握手、检查工具并连接现有 Chrome；返回公开连接状态，不返回页面内容 |
| `POST /plugins/<id>/connection` | 请求 `{token?:string,auth_mode?:"token"\|"slack_user",enabled:boolean,allow_writes:boolean,app_token?:string,listen?:boolean,listen_scope?:"related"\|"all"}`；监听字段仅限 Slack 个人账号，`listen` 首次默认 false、范围默认 related；省略沿用同一方式的现有设置，换方式必须提供新令牌。启用时验证后保存，返回更新的市场项 |
| `DELETE /plugins/<id>/connection` | 幂等断开，返回更新的市场项 |
| `POST /plugins/<id>/test` | 验证已有凭据，更新公开账号、验证时间和错误状态 |
| `POST /plugins/<id>/invoke` | 请求 `{action:string,input:object}`；返回 `{data,next_cursor,next_page}` |
| `GET /plugins/slack/inbox` | 返回当前连接公开摘要、listener 状态和当前身份的会话摘要，不返回令牌和 Socket 票据 |
| `POST /plugins/slack/inbox/read` | 请求 `{channel:string,before?:string,limit?:number}`，最多 100 条；返回按时间排序的 messages 和 next_before |
| `POST /plugins/slack/inbox/send` | 请求 `{team_id:string,user_id:string,channel:string,text:string,thread_ts?:string}`；核对个人身份及写入权限后发送，并保存至收件箱；返回服务结果，必要时附 sync_warning |
| `POST /plugins/slack/listener/reconnect` | 请求 Core 重新建立当前已启用监听，不自动启用插件或监听 |

未知插件或接口返回 404，不支持的方法返回 405，JSON 格式错误返回 400，未安装或未满足调用条件返回 409，服务访问失败返回 502，存储错误返回 500。损坏的 manifest 会使市场加载失败，不能当成未安装；损坏的连接配置在对应插件显示异常，可重新输入令牌修复或断开删除。市场不隐藏或删除 CLI 安装的其他插件。插件 ID 必须以字母或数字开头，后续只允许字母、数字、点、下划线和连字符。

公开连接摘要包含 `auth_mode`，不含令牌；切换身份会更新私有配置代次，使此前等待的写入审批失效。Slack 新参数为 `list_channels.types`、`channel_history.oldest/latest`；新增操作为 `thread_replies({channel,ts,limit?,cursor?})` 和 `open_conversation({users})`。动作与参数 schema 同样提供给 GUI 和管理模型。

## 验证

核心测试使用隔离注册表和本地 HTTP 服务，检查四个官方身份端点与全部操作的请求方法、路径、请求头、载荷、参数和分页，并覆盖凭据保存、失败不覆盖、启停和断开、权限与审批、错误脱敏、限流和重定向。GUI 单元测试覆盖安装、配置、调用结果和失败；浏览器用例使用真实 Core 及模拟第三方服务，检查四款插件安装、连接、调用、重启保留、移动端操作及助手读写审批。模拟服务不能代替真实账号的联网验证。

Slack 用例另检查成员查询请求、分页、资料保留、参数校验和缺少权限，以及旧配置兼容、个人账号身份验证、错误令牌不覆盖、按会话类型列举、时间范围、私信开启和读写、线程分页、切换方式及重启保留。持续接收使用本地真实 WebSocket 服务及真实 Core，验证用户选择范围、确认与去重、相关线程、编辑删除、重连、重启、停止接收、账号隔离和本人回复；不使用真实 Slack 账号发送消息。

并发用例会暂缓第三方响应，验证其他插件和市场仍可操作、断开或重新安装后旧验证结果失效，以及前端刷新后的权限同步。公开网络检查通过真实 Core 调用四个官方身份接口，只使用固定无效令牌，验证认证失败和不保存凭据；运行命令为 `cargo test -p omnistream --test plugin_live -- --ignored --nocapture`，需要联网。它不使用真实账号，也不验证授权后的读写能力。

Agent 配置协助测试覆盖四插件启动、Bot / User 身份、监听及写权限上下文、缺少 Agent、拒绝凭据字段、幂等、继续对话、重启保留与输出遮蔽；使用真实 Core 和本地模拟 Agent，不访问真实账号。通用 Agent 工作流使用真实 Core 和 stdio Agent fixture，覆盖 App Server、ACP、Claude CLI、普通 CLI、JSONL 的 MCP 或命令接入、五款插件工具白名单、服务写入审批、账号或权限变化、取消失效和临时网关清理。浏览器检查折叠说明、启动跳转、返回选项恢复及移动端。

运行 `cargo test -p omnistream --test plugin_setup`、`cargo test -p omnistream --bin omnistream plugin`、`pnpm --filter @omnistream/gui test:unit`、类型检查与构建。端到端先 `cargo build -p omnistream`，再 `pnpm --filter @omnistream/gui test:e2e plugins.spec.ts plugin-setup.spec.ts`；浏览器路径可用 `PLAYWRIGHT_CHROMIUM_EXECUTABLE` 配置。测试进程可用 `OMNISTREAM_PLUGIN_TEST_ORIGIN` 将请求定向至本地回环 HTTP 根地址（按 `/<插件 id>/` 分流），该设置不通过 API 暴露，拒绝公网地址。

协议依据：[Slack Web API](https://docs.slack.dev/reference/web-api/)、[Socket Mode](https://docs.slack.dev/apis/events-api/using-socket-mode/)、[Slack Events API](https://docs.slack.dev/apis/events-api/)、[GitHub REST API](https://docs.github.com/en/rest)、[Notion API](https://developers.notion.com/reference/intro)、[Figma REST API](https://developers.figma.com/docs/rest-api/)。

Chrome MCP 的测试使用真实 JSON-RPC 子进程及 Core，覆盖工具检测、错误和超时、实际 Agent 启动参数、配置文件保留、启停 session 隔离、重启与旧检查失效。公开 npm 握手检查：`cargo test -p omnistream --bin omnistream browser_official_mcp_exposes_the_configured_tools -- --ignored`，不连接浏览器或模型。调试构建可用 `OMNISTREAM_BROWSER_MCP_TEST_COMMAND` 指定绝对路径的本地协议 fixture；发布构建忽略该变量，API 不允许设置启动命令。官方协议与配置依据：[Chrome DevTools MCP](https://github.com/ChromeDevTools/chrome-devtools-mcp)、[Codex MCP](https://developers.openai.com/codex/mcp)。

长期任务生成阶段使用只读插件快照；delivery_destination_fields 非空的动作才可配置为后台投递，目标字段固定，内容按模板生成。后台投递以已确认任务授权代替逐次交互审批，并重查连接代次；幂等、失败分类与撤销规则统一维护在[长期任务](automations.md#执行与交付)。
