# 测试

Core 与前端的职责和适配器验证边界见[适配器说明](adapters.md)，会话与长期委托的行为分别见[会话](sessions.md)和[长期任务](automations.md)。

## 检查命令

```bash
cargo fmt --all -- --check
cargo test --workspace
pnpm --filter @omnistream/gui typecheck
pnpm --filter @omnistream/gui test:unit
pnpm --filter @omnistream/gui build
```

Core 集成测试的模拟 Agent 调用 PATH 中的 `python3`，需要 Python 3.11+ 提供 `tomllib`；CI 通过共享构建动作安装 Python 3.12，并在 Linux 环境预检中验证此依赖。

运行浏览器回归前先执行 `cargo build -p omnistream`，再运行 `pnpm --filter @omnistream/gui test:e2e`。浏览器可通过 `PLAYWRIGHT_CHROMIUM_EXECUTABLE` 指定，例如使用当前系统安装的 Chrome。Playwright 自动启动独立 Vite 服务；用例启动隔离的 Core 和模拟模型、Agent 或插件服务，不使用生产数据与真实账号。

开发环境设置 HTTP(S) 代理时，将 `127.0.0.1,localhost,::1` 加入 `NO_PROXY`，确保测试中的 Core 与模拟服务通过回环直连；容器验证时也需传入，避免代理造成空响应或 502 干扰结果。

## 验证范围

NixOS 本机使用[桌面开发环境](README.md#桌面端)后，Tauri 环境检查已识别 WebKitGTK 4.1 和 librsvg，pkg-config 可发现 D-Bus 的 `dbus-1.pc`；准备本机 Core sidecar 后桌面 `cargo check` 与 `cargo build --locked --manifest-path apps/gui/src-tauri/Cargo.toml` 均通过。本次仅验证依赖与编译，未验收真实窗口、安装包及系统通知展示。

原生选择器覆盖语言、会话审批、Agent、长期任务、插件及结构化表单，按[协作约定](../../AGENTS.md#代码与组件设计)统一检查。已用这些控件的 HTML/CSS 在隔离 WebKitGTK 中完成中英文、桌面与 390px 窄屏渲染检查，确认文字行高、箭头留白、禁用状态及多选列表展示；另放开会裁切英文审批文案的窄屏宽度限制。GUI 类型检查、构建、组件测试，以及 `mobile.spec.ts` 和 `automation.spec.ts` 中的创建/修改、日历/时间窗口回归通过；并行执行浏览器回归时使用独立 `--output` 目录，避免产物清理冲突。

完整会话回归上次通过审批切换、保存和刷新恢复后，在模型菜单的“正在加载模型”断言处失败，尚未重新验收。其他桌面系统及实际设备主题仍需对应环境验收。

通知回归运行 `cargo test -p omnistream --bin omnistream notifications`、GUI 的 `test:unit NotificationSettings.test.tsx systemNotifications.test.ts` 和 `test:e2e notifications.spec.ts`：覆盖首次连接基线、持久化游标与分页、事件去重、已处理审批及归档和删除会话过滤、Slack 本人发送和账号隔离、长期任务静默跳过、权限拒绝、手动重试、切换 Core、关闭通知和浏览器通知跳转。桌面额外执行 `cargo check --manifest-path apps/gui/src-tauri/Cargo.toml`；系统横幅、免打扰、Windows/macOS/Linux 的真实点击回传及浏览器后台节流须在对应环境验收，模拟 Notification 和单元测试不代表真实系统展示已通过。

模型继承运行 `cargo test -p omnistream --test model_inheritance`：覆盖请求快照、模型切换与线程隔离、同配置恢复、命名 Agent、不兼容 provider 拒绝和凭据不进入参数或会话。单元测试另覆盖认证刷新及配置优先级；真实 Codex 与回环服务验证继承的模型、思考等级和 Bearer 凭据，以及恢复和占用分支，命令与环境见[适配器验证](adapters.md#验证方式与覆盖范围)。

管理模型子任务续接运行 `cargo test -p omnistream --test management_subtasks`：隔离 Core、模拟模型和本地 Agent 覆盖查询回传后保存 pending 草案、失败反馈、委派前后插件写入防重、取消、新消息和会话删除、续接失败反馈、共享调用上限及直接委派兼容。

Rust 测试覆盖会话、Agent 执行与取消、审批、插件权限、长期委托及持久化；`legacy_removal` 验证旧开发接口和命令不可用、旧记录不恢复执行及新 Agent 执行不覆盖旧数据。

只读网页审批回归运行 `cargo test -p omnistream --bin omnistream policy::tests`、`cargo test -p omnistream --bin omnistream agent_host::tests` 和 `cargo test -p omnistream --test adapter_workflow`，覆盖默认/内部允许模式免审批、自动批准不产生待处理记录、写入及混合权限仍等待用户、可用决定和取消限制，以及连续执行和会话恢复。模拟请求不实际访问外部网页。

长期任务回归运行 GUI 的 AutomationPanel.test.tsx、AutomationList.test.tsx 和浏览器 automation.spec.ts：覆盖摘要列表的排序与筛选、调度和异常提示、新建及详情往返、修改不启用、明确启用、取消、历史入口读取最新状态、创建失败、加载重试和任务不存在。浏览器另检查桌面与移动端布局。

GUI 单元测试检查组件、失败重试、表单与多语言；浏览器回归检查会话输入、附件、模型设置、Agent 管理、长期任务与插件流程，包含窄屏布局。真实服务或实际 Agent 的验证应单独说明使用环境及未覆盖部分；模拟测试不能代表生产账号或所有外部协议已通过验收。

会话列表回归运行 `cargo test -p omnistream conversation_list`、GUI 的 `test:unit ConversationList.test.tsx` 和 `test:e2e session-list.spec.ts`，覆盖更新时间排序及重启保留、旧数据迁移、归档过滤、相同时间的游标分页、侧栏内滚动加载、加载失败后手动重试及不逐个请求会话历史和设置。

公用状态与路径补全回归覆盖骨架屏的可访问状态、失败与空状态互斥、仅手动触发重试及失败操作参数保留；桌面设置保存失败重试须重新提交失败的设置值，而非仅重新加载设置。

桌面设置组件回归运行 `pnpm --filter @omnistream/gui test:unit DesktopSettings.test.tsx`，覆盖浏览器隐藏设置、系统登录项状态读取、局域网保存与重启提示、保存失败保留原值和加载失败重试。桌面 Rust 使用 `cargo check --manifest-path apps/gui/src-tauri/Cargo.toml` 检查，需准备[桌面打包要求的 sidecar](README.md#桌面端)及平台开发库。系统登录后自启、真实局域网连接和正常退出清理需在对应桌面系统验收；组件模拟不能替代这些检查。

旧开发模块的任务页面、`/tasks` 与 `/task-proposals` API、`task` CLI、计划审批、服务控制、自动测试及录屏管线已移除。已有数据库记录保留，但不再加载或执行，也不自动转换为长期任务；内部 Agent 执行记录继续持久化。Core 不再管理 x11box 桌面或提供 `/recordings` 媒体接口。需要界面测试时，由执行 Agent 根据用户请求选择并运行项目自己的验证工具。

## 客户端授权验证

规范与部署见[客户端访问授权](access.md)。运行以下检查，浏览器用例使用独立 Core 和三个隔离的客户端上下文：

```bash
cargo test -p omnistream --bin omnistream client_auth
cargo test -p omnistream --test client_authorization
pnpm --filter @omnistream/gui test:unit clientFetch.test.ts ClientAccess.test.tsx
pnpm --filter @omnistream/gui test:e2e client-access.spec.ts
```

2026-11-07 已通过 8 项授权单元测试、实际 HTTP 集成测试、11 项授权客户端测试，以及 Core workspace 串行测试（`--test-threads=1`）。覆盖本机 Host/Origin/转发头边界、强制授权、终端首次批准、待授权请求隔离、凭据不外泄、并发决定、重启持久化、撤销关闭事件流及同步停用推送，旧客户端不能重新注册推送设备。

GUI 全部 141 项单元测试、类型检查、Web 构建和 15 项浏览器回归通过，包含三个客户端自动弹窗、批准/拒绝/取消、授权恢复、撤销后回到授权页，以及 390px 中英文布局。客户端测试另覆盖按 Core 完整地址隔离凭据、丢失响应恢复、失败停止轮询与手动重试、旧响应不覆盖新凭据、关闭期间撤销、旧申请清理及桌面运行期凭据隔离。Linux 桌面 Rust 编译和 Android arm64 调试 APK 构建通过；Android/iOS 真机、其他桌面系统及公网 HTTPS/代理部署的授权交互仍需对应环境验收。

## 远程推送验证

规范与部署见[远程推送](push.md)。相关检查：

```bash
cargo test -p omnistream --bin omnistream push::
pnpm --filter @omnistream/gui typecheck
pnpm --filter @omnistream/gui test:unit
pnpm --filter @omnistream/gui test:e2e notifications.spec.ts remote-push.spec.ts
pnpm --filter @omnistream/gui android:build --debug --target aarch64
```

Core 测试覆盖免配对注册、默认启用及显式停用、内置凭据解析与项目匹配、无效显式凭据不回退、设备凭据验证、敏感字段不进入状态列表、首次绑定跳过历史、通知过滤、持久游标、失败暂停与手动重试、结果不明跳过、失效订阅停用、崩溃恢复、并发撤销不被旧响应覆盖、token 更新不恢复失败投递或已撤销设备、推送地址限制与 VAPID 密钥权限。GUI 组件测试覆盖通知授权后自动注册、连接与权限门控、显式关闭不自动恢复、注册失败保留本地通知，以及远程绑定凭据复用、丢失响应恢复、停用失败保留状态和切换 Core 的订阅边界。

已读联动另外覆盖：前台/焦点/加载成功/滚动尾部/弹窗遮挡的判定、快照递增和卸载取消、本地跳过与定向清除、已读同步失败保留本机状态及手动重试、Core 设备/会话/Slack 工作区与账号隔离、旧快照不能确认未来事件、乱序确认不回退，以及已读不改变审批状态。2026-11-07 通过 15 项 Core 推送测试、6 项既有通知相关测试、130 项 GUI 单元测试、类型检查、4 项浏览器回归、Android arm64 调试 APK/AAB 构建和 Linux 桌面 Rust 编译检查。浏览器实际验证进入已加载会话后清除旧通知、后台内容保持未读、审批仍待处理，以及 Service Worker 丢弃延迟已读推送而继续展示新事件。浏览器测试使用独立端口和产物目录，避免其他工作区测试互相清理输出。

本轮未连接 Android 真机，新的 data-only FCM 前后台展示、原生清除、冷启动和省电场景仍需真机验收；Windows/macOS 清除能力的实际限制见[已读与通知清除](push.md#已读与通知清除)，不能将插件接口调用成功视为操作中心条目已移除。

验证内置发送凭据时，在上述 Cargo 命令前设置 `OMNISTREAM_FCM_SERVICE_ACCOUNT_PATH` 为仓库外的账号 JSON 路径，并设置 `OMNISTREAM_REQUIRE_BUNDLED_FCM=true`。2026-11-06 免配置版本已通过 12 项 Core 推送测试、123 项 GUI 单元测试、类型检查、22 项发布辅助脚本测试、actionlint 和 Android arm64 调试 APK/AAB 构建。另在隔离目录中移除运行时 Firebase 环境变量及配置文件，确认内置凭据可用、免配对注册、设备身份校验和撤销后不自动恢复；构建脚本拒绝缺失或无效的正式发布凭据。GitHub 仓库发送 secret 已配置。此轮未连接 Android 真机，手机端的新自动注册及实际 FCM 投递尚未复测。

浏览器回归使用真实 Service Worker、IndexedDB 和 DevTools 注入的 PushEvent，关闭订阅页面后验证展示接口调用和稳定事件标签；因 CI 可能没有通知守护进程，展示接口被记录，不代表浏览器供应商的公网 Web Push 投递及系统横幅已实测。

2026-11-06 首次接入时，Android 真机已获取 FCM token，通过临时 Core 和 Firebase 专用发送账号实际收到前台及后台通知（确认进入系统通知管理器），验证原生点击桥接；测试绑定已停用移除。图标修复为 RGBA8，避免原 16 位 PNG 导致 Tauri 真机启动崩溃。本地通知浏览器回归和 Service Worker 关闭页面回归在免配置版本再次通过。

仍需验收浏览器供应商的真实公网投递、浏览器完全退出、Android 进程回收及点击冷启动、长期断网后的 token/订阅轮换、不同厂商后台策略和正式签名包。iOS 远程推送按当前范围暂缓，不属于已通过项。

首次接入时的全量 Core 单元检查使用 `--test-threads=1` 串行通过；默认并行执行曾出现 `session_leases_isolate_scopes_and_release_without_deleting_the_lock_file` 失败，未在推送任务中修改该锁实现。

## 移动端原生验证

移动端规范、环境与签名见[移动端指南](mobile.md)。GUI 类型检查、构建与组件测试不能代替原生编译和真机验证。

- 运行 `test:unit DesktopSettings.test.tsx systemNotifications.test.ts`，检查手机/iPad 隐藏桌面专用设置，以及原生通知权限和投递路径。
- 运行 `test:e2e mobile.spec.ts`，检查缺少 crypto.randomUUID 时正常启动、首次连接提示、保存地址后恢复、连接失败手动重试及模拟安全区域下输入区和弹窗不溢出；浏览器模拟原生启动标识，不代表系统 WebView 已验收。
- Android 在准备好 SDK/NDK 后运行 `pnpm --filter @omnistream/gui android:build --debug --apk --target aarch64`，发布时另检查签名配置、APK/AAB 产物和安装。
- iOS 在 macOS/Xcode 上运行 `pnpm --filter @omnistream/gui ios:init`、`ios:dev` 和 `ios:build`，检查模拟器、真机与签名归档；Linux 不执行 iOS 编译。
- 真机验收需覆盖连接实际 Core、流式对话、审批、附件、路由恢复、后台返回、软键盘、刘海/安全区域、局域网权限，以及通知授权和点击；分别记录 Android 和 iOS 结果，不能由窄屏浏览器回归推定通过。

图标修改后运行 `pnpm --filter @omnistream/gui icons:generate` 并检查 Android 圆形/圆角方形自适应裁切、iOS/Apple touch、桌面及 PWA 普通/可裁切图标的实际 PNG 预览；确认符号居中、两侧有留白，完整背景没有白边或透明缺口。已完成上述资源预览、像素边界检查、Android 调试构建与 Web 构建；iOS、Windows、macOS 的安装后系统图标仍需对应设备验收。生成与安全区规则统一见[应用图标](configuration.md#应用图标)。

当前已在 Linux 上通过 Android arm64 调试 APK，使用临时测试密钥完成包含 arm64-v8a、armeabi-v7a 和 x86_64 的通用 Release APK/AAB 及分 ABI APK 构建、签名校验和发布产物收集；另通过桌面 Rust 编译检查、GUI 类型检查和构建，以及移动连接和会话列表的浏览器回归。临时密钥和签名配置在验证后清理。Android 完整业务真机验收（推送结果单列于上节）、正式账号的发布签名及 iOS 初始化、编译和真机运行尚未完成；iOS plist 和平台配置已做静态检查。

## 发布流程验证

[打包与发布](releases.md)的本地检查使用 Python 3.11+ 和 actionlint：

```bash
python3 .github/scripts/release.py check
python3 -m unittest discover -s .github/tests -v
actionlint .github/workflows/release.yml
```

辅助脚本测试使用临时 Git 仓库与模拟 GitHub API，覆盖版本一致性、分支/手动触发、标签冲突与草稿重试、可达标签的更新日志范围和分类、产物完整性及校验和、Core 归档权限、签名配置及清理、草稿上传后再公开，以及已公开 Release 的保护。不会向真实仓库发布或使用正式签名凭据。

当前已通过这些辅助脚本测试、actionlint、Web/独立 Core 的归档检查和上述 Android 三种 ABI 的实际打包验证。Windows/macOS 安装包及 iOS IPA 尚未在本机编译；GitHub Actions 的实际运行结果见下述记录，真实 Release 尚未公开发布。

2026-11-07 的 [Actions 运行 42335070829](https://github.com/omni-stream-ai/omni-stream/actions/runs/42335070829)（提交 `066463b`）已通过版本检查、Web、Linux 桌面与独立 Core、Core workspace 测试，以及 Android 正式签名的通用/分 ABI APK 和 AAB。macOS 两种架构在预检阶段因 `lipo` 参数顺序错误失败；Windows 因 Git Bash 的 `link.exe` 遮蔽 MSVC，在 Rust 构建脚本链接时失败，发布按规则跳过。修复将 Apple 输入文件放在 `-verify_arch` 之前，Windows 使用[构建环境规则](releases.md#ci-构建环境)中的原生 Shell 和明确链接器，并新增 Cargo 宿主/目标链接探针。25 项发布辅助测试和 actionlint 已通过；回归覆盖三种 Apple 目标的架构验证、Windows 链接器配置缺失，以及 C 探针通过但 Cargo 宿主链接失败时必须停止。

随后 [Actions 运行 42337676908](https://github.com/omni-stream-ai/omni-stream/actions/runs/42337676908)（提交 `0d8a1f3`）已通过 Windows、macOS arm64/x86_64 的完整编译与安装包、Web 和 Android；Linux 在 `management_subtasks::repeated_subtasks_share_one_operation_budget` 失败。任务保存的延迟事务先读后写，与其他写入重叠时出现即时锁错误，导致 Core 退出及客户端 `IncompleteMessage`。任务写入按[Core 持久化规则](adapters.md#core-与前端边界)修复，并增加可控的并发写入回归：旧实现稳定报 `database is locked`，修复后验证任务、事件及通知一起保存；集成测试失败时保留 Core 标准错误以便定位。

2026-11-06 的 CI 环境复核在隔离 Ubuntu 22.04 中通过 Linux 开发库的编译/链接预检、Python 3.12 下的 22 项发布辅助测试、GUI 类型检查及 117 项单元测试，以及启用 `vendored-openssl` 的独立 Core 和桌面 GUI Release 编译、`.deb` / AppImage 打包。独立 Core 的 `ldd` 检查确认没有动态 OpenSSL 依赖和缺失库，Debian 包内包含可执行的 GUI 与 Core sidecar；安装与真实窗口仍需验收。另通过 Android 三种 ABI 的 NDK 编译/链接预检，并在固定 CI 的 Build Tools/NDK 版本后完成 arm64 调试 APK 构建。各平台的依赖和预检入口见 [CI 构建环境](releases.md#ci-构建环境)。

本次任务事务修复已在上述隔离 Ubuntu 22.04 环境通过与 CI 相同的 `cargo test --workspace --locked --features omnistream/vendored-openssl`，保留默认并行执行；`management_subtasks` 连续 20 轮、共 200 项检查全部通过，失败即停止。该验证使用当前源码快照；本地通过不替代修复提交的 GitHub Actions 验证。

工作流在 Web 任务执行 GUI 类型检查、组件测试和构建，在 Linux 桌面任务执行 Core workspace 测试；各平台分别编译、打包后才进入发布任务。GitHub 多平台任务、正式 Android 签名和可选 iOS 钥匙串/归档导出需在对应 runner 配置凭据后验证，不能由本地辅助脚本测试推定通过。
