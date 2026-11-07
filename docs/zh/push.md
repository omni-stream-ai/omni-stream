# 远程推送

远程推送沿用[系统通知](gui.md#系统通知)的事件、过滤条件与隐私约束，让 Core 在客户端页面或 App 未运行时投递通知。Core 必须保持在线。Core 默认提供推送服务，客户端通知默认关闭；用户启用通知并授予系统权限后，支持的客户端自动向当前连接的 Core 注册，无需配对码。iOS 原生推送暂缓，等待 Apple APNs 密钥配置。

| 客户端 | 通道 | 条件 |
| --- | --- | --- |
| Android | Firebase Cloud Messaging HTTP v1 | 包名匹配 Firebase Android 应用；Core 发布包内置发送账号或由部署者覆盖；设备具有 Google Play 服务 |
| 浏览器 / PWA | 标准 Web Push、VAPID 和现有 Service Worker | HTTPS 或 localhost、Push API、已激活的 Service Worker；正式 Web 构建包含推送处理器 |
| 桌面、iOS 原生 | 本地系统通知 | 保留原有能力，需要客户端运行，不接入远程推送 |

浏览器端不依赖 Firebase Web Messaging 或控制台的 Web VAPID 密钥。每个 Core 生成自己的 VAPID 密钥；同一浏览器站点的 Service Worker 只允许启用一个 Core 的远程订阅，切换前先关闭原绑定。Android 可分别绑定多个 Core。

## Core 配置

官方 Core 发布包内置 `omnistream-9ed10` 项目的共享 FCM 发送凭据，各自运行的 Core 直接向 Google OAuth / FCM 发送请求，没有官方推送转发服务。下载者无需创建 Firebase 账号、设置发送环境变量或准备密钥文件。凭据仅授予 FCM 发送权限，设备 token 只向用户选择连接的 Core 注册，设备列表和状态接口不返回 token。

共享发送凭据属于发布产物的一部分，可从 Core 二进制提取；它不能在 FCM 侧限制每个 Core 只能向自己的设备发送，发送者还需要目标设备 token。凭据轮换后须重新构建并分发 Core。构建注入和 Actions secret 统一见[发布配置](releases.md#远程推送发送账号)。

全局或项目配置的覆盖规则见[配置参考](configuration.md#模型配置)。可在运行 Core 的主机覆盖发送凭据：

```toml
[push]
enabled = true
firebase_project = "omnistream-9ed10"
credentials_file = "/absolute/private/path/firebase-service-account.json"
```

发送凭据按以下顺序选择：`credentials_file`、运行时 `OMNISTREAM_FCM_SERVICE_ACCOUNT_JSON`、运行时 `OMNISTREAM_FCM_SERVICE_ACCOUNT_PATH`、发布包内置凭据。显式覆盖无效时不会回退到共享账号。`firebase_project` 可省略，由凭据读取；显式设置时必须与凭据的 `project_id` 一致，使用自己的 Firebase 项目时还须重新构建对应项目的 Android 客户端。

仅使用浏览器推送时不需要 Firebase 凭据。自行构建且未注入凭据时，浏览器推送仍可用，Android 显示不可用。自己的账号需要启用项目的 FCM HTTP v1 API，授予 `roles/firebasecloudmessaging.admin` 发送权限，将 JSON 密钥置于仓库外并限制读取权限（Unix `0600`）；不使用 Firebase CLI 的个人登录令牌发送推送。Core 在内存缓存短期 OAuth token，不把私钥或 token 放入 API、会话及日志。将 `[push]` 中的 `enabled` 设置为 `false` 可关闭当前 Core 的推送服务。

配置后重启 Core。VAPID 私钥首次启用时写入数据库旁的 `push-vapid.key`，使用私有文件权限并在后续启动复用；备份和迁移 Core 时同时保留数据库、VAPID 密钥及发送凭据。更换 VAPID 密钥需关闭浏览器远程推送后重新绑定。不要提交服务账号密钥或 VAPID 私钥。

Core 需要出站访问 Google OAuth、FCM 及设备所属浏览器推送服务。Web Push 仅接受受支持的 HTTPS 推送服务地址（Google、Mozilla、Apple、Windows），不跟随重定向；不允许设备把推送目标设为任意内网 HTTP 服务。客户端须先满足[Core 访问授权](access.md)，推送接口沿用同一 Bearer 凭据；推送本身不引入额外配对码。后台订阅或 FCM token 刷新也携带该凭据。Core 在同一事务中撤销客户端并停用其关联推送设备，旧凭据不能重新激活设备。

## 设备绑定与关闭

1. 连接自己的 Core，打开客户端顶部铃铛，启用系统通知并允许权限。Android 和支持 Web Push 的浏览器自动注册，已有有效绑定会在启动时同步。
2. 注册失败时保留设备身份，用户可手动重试；网络响应丢失也复用同一身份。在注册成功前保留运行期间的本地通知，不把注册失败当成推送就绪。
3. 设置中查看当前设备的绑定和投递状态，也可关闭或重新启用。显式关闭过的绑定不会在刷新或重连时自动开启。首次绑定或停用后重新启用从当前通知游标开始，跳过历史通知；注册成功且启用远程推送的设备停止本地通知轮询，防止重复提醒。

设备生成随机身份和 256 位秘密；Core 只保存秘密的哈希，后续更新 token、检查状态、关闭及重试必须验证该设备凭据。其他客户端不能仅凭设备 ID 修改绑定。浏览器在站点存储与 IndexedDB 保存有限权限的设备凭据，Android 同步至 App 私有存储以支持后台 token 更新；Android 禁用应用备份，避免恢复旧设备绑定。服务账号凭据始终只在 Core。

关闭远程推送先在当前 Core 停用设备，再移除本机后台更新绑定及浏览器订阅，恢复运行期间的本地通知。关闭总通知开关会逐个停用此设备在所有 Core 的远程绑定；任一 Core 不可达时保留尚未完成的绑定并显示对应地址和失败，用户重试后才显示总开关已关闭。已被推送服务排队或已显示的通知不保证能撤回。浏览器多个标签页共享绑定，刷新和跨标签页操作不新增设备。

管理员可执行 `omnistream push devices` 查看不含 token、订阅地址和设备秘密的列表，使用 `omnistream push revoke <device-id>` 撤销丢失设备。已撤销设备在设置中手动重新启用，不自动恢复。Android 在 `onNewToken` 更新已有绑定；浏览器在支持 `pushsubscriptionchange` 的环境更新订阅；客户端再次启动也同步已有有效绑定。后台更新失败不会循环重试，下次打开设置可检查并手动恢复。

## 投递与失败

Core 每 3 秒扫描已启用设备，复用通知流的账号、审批及会话过滤条件。通知只发送类别、任务标题或频道 ID，以及点击所需的会话/频道路径、Core 地址和事件标识，不发送消息正文、执行输出及审批参数。发送前在 SQLite 持久标记事件；成功后推进设备游标。订阅更新与撤销增加版本，旧发送结果不得覆盖新状态。

明确拒绝时暂停设备，保留失败位置；网络异常、服务端 5xx 或 Core 在发送期间退出属于结果不明，暂停并跳过该条，避免重启后自动重复提醒。这意味着通知采用尽力投递，可能漏掉一条结果不明的通知，不保证恰好送达。失败不自动重发，设置使用公用失败组件提供手动重试；已失效订阅自动停用，必须重新绑定。Core 的数据库异常同样暂停设备并保留可检查的错误状态。

提供给推送服务的消息有效期为 24 小时。通知发送后，后续已解决审批或已归档会话无法撤回服务商队列中的旧通知，点击后的业务状态以 Core 为准。系统权限、勿扰、厂商省电、网络及浏览器后台策略都可能延迟或阻止展示；Android 被系统“强行停止”后通常需要重新打开 App。关闭浏览器页面可由 Service Worker 接收，完全退出浏览器进程后的行为取决于浏览器和操作系统。

点击前台/后台通知或冷启动 App 时，先校验通知来源与当前保存的 Core 一致，再打开对应会话或收件箱频道，不自动切换 Core。前台与后台使用稳定事件标签，重复到达覆盖同一通知。

## 已读与通知清除

已读触发条件、快照边界、设备隔离和失败重试统一见[通知已读](gui.md#通知已读)。Core 用设备 ID、数据库实例和目标范围保存单调递增的已读游标，发送前跳过覆盖范围内的事件，同时推进该设备的投递游标。已读不能修改其他设备的状态，也不会将待响应请求改为已处理。

Android 使用高优先级 FCM data-only 消息，由应用在前台和后台统一检查绑定及本机已读记录后展示，避免通知消息被系统直接展示而绕过检查。本机已读先持久写入原生私有存储，再清除通知栏中匹配 Core、范围、实例且不晚于已读游标的通知；检查、展示和清除使用同一进程锁，延迟到达的已读推送不会重新展示。仍受系统省电和后台限制影响。

浏览器 Service Worker 的 IndexedDB 升级到版本 2，保留原有绑定并增加已读记录。推送展示和页面更新已读在支持 Web Locks 时串行化；收到已读事件后跳过展示，页面清除匹配的 Service Worker 通知及本窗口普通 Notification。已读记录只保存标识和游标，不保存消息内容或服务账号。已发往供应商的消息无法从队列撤回；浏览器对无需展示的推送可能采用自己的提示策略，公网实际效果须在对应浏览器验收。

原生本地通知通过通知插件清除匹配的活动通知。Linux 和原生移动端使用其可用的清除接口；当前桌面插件在 Windows/macOS 上的 `remove_active` 不支持撤销已显示通知，因此这两端仍会抑制已读事件的后续投递，操作中心已有条目的清除不作保证。不会为清除某个会话而删除其他会话的通知。

## 接口与验证

`GET /push/status` 仅返回启用状态、Android/Web 可用性和 VAPID 公钥。`POST /push/devices/register` 注册或更新当前设备，无需配对码；已有设备的更新和 `/push/devices/status`、`/push/devices/disable`、`/push/devices/retry` 使用设备 ID 与秘密。状态响应不暴露推送目标或秘密。重试仅恢复暂停设备，不恢复已失效订阅。

`POST /push/devices/read` 同样验证设备 ID 与秘密，接收 `read: { instance, scope, cursor }`。数据库实例必须匹配，游标不能为负或超过当前事件上限；同范围重复或乱序确认只保留最大游标。该接口不执行审批，也不改变投递失败的暂停状态。

验证命令和实际覆盖范围见[测试指南](testing.md#远程推送验证)。移动构建和 Firebase 公开客户端标识见[移动端指南](mobile.md#firebase-客户端)。
