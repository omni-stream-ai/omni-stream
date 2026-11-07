# Android / iOS 原生 App

移动端使用 Tauri 2 原生容器与共享 React 界面，支持 Android 7.0 / API 24 及以上、iOS 14 及以上。平台构建与真机验证范围见[测试](testing.md#移动端原生验证)。

应用标识（Android 包名 / iOS Bundle ID）统一为 `com.omnistreamai.app`，在 [tauri.conf.json](../../apps/gui/src-tauri/tauri.conf.json) 的 `identifier` 中配置，Android 与 iOS 平台配置沿用该值。

GitHub Actions 提供 Android APK/AAB 和可选的 iOS 签名 IPA，触发方式、仓库 secrets 和下载产物见[打包与发布](releases.md)。

## 运行与连接

- Core、模型请求、插件和 Agent 进程在电脑或服务器运行，App 通过 HTTP API 与流式接口访问；手机不运行 Core、shell 或 Codex，也不打包桌面 sidecar。
- 未保存 Core 地址且未设置构建变量 `VITE_OMNISTREAM_URL` 时，首次启动自动打开连接设置。地址保存到当前 App 的 `localStorage`，再次启动继续使用；顶部连接设置可随时修改。
- 局域网填写电脑的地址，如 `http://192.168.1.100:4317`。手机上的 `127.0.0.1` / `localhost` 指向手机自身；Android 模拟器访问宿主机可用 `http://10.0.2.2:4317`。电脑的 Core 需监听可达地址，并放行对应防火墙端口，桌面操作见[桌面设置](configuration.md#桌面设置)。
- 原生包允许 WebView 连接 HTTP Core，以兼容可信局域网；iOS 同时声明本地网络权限用途，首次连接局域网需允许该权限。远程 Core 需完成[客户端访问授权](access.md)：填写设备名称申请，在其他已授权客户端或服务端终端核对并批准；批准前不能使用业务功能。公网连接使用 HTTPS。
- 窄屏侧栏、输入区与弹窗复用[GUI 规则](gui.md#界面样式)，支持屏幕安全区域。桌面专用的局域网开关与开机自启不在手机展示，iPad 即使使用桌面浏览器标识也按原生移动端处理。
- 原生本地通知使用系统通知插件；Android 另外支持 FCM 远程推送，权限、设备自动注册、后台接收与限制见[远程推送](push.md)。iOS 原生推送等待 APNs 凭据，暂保留运行期间的本地通知。

## Firebase 客户端

Firebase 项目 ID 为 `omnistream-9ed10`，Android 包名与 iOS Bundle ID 均沿用文首的应用标识。已登记的应用如下：

| 平台 | 显示名称 | Firebase App ID |
| --- | --- | --- |
| Android | OmniStream Android | `1:618055154194:android:24d96aa203abe4da5a36cb` |
| iOS | OmniStream iOS | `1:618055154194:ios:8f51605bd7d03c0f5a36cb` |
| Web | OmniStream Web | `1:618055154194:web:b65e59b9e66799ec5a36cb` |

共享 React 入口使用 Firebase JavaScript SDK，在 src/lib/firebase.ts 初始化上述 Web 应用，浏览器与 Tauri WebView 共用该初始化。配置为公开的客户端标识，不包含服务账号凭据。

生产构建在 Analytics 支持的环境中初始化统计，每次启动上报 `app_open`；开发构建不采集，初始化失败不阻止启动。关闭自动 `page_view`，页面地址固定为当前 origin 的根路径，标题固定为 OmniStream，来源留空；不添加会话正文、会话 ID、Core 地址或审批参数作为事件属性。Firebase 控制台的 Web 数据流需关闭增强型衡量（包括浏览器历史变更页面浏览），避免额外自动采集路由及交互数据。Tauri WebView 的支持由运行时检测，不能视为原生统计已完成。

Android 原生接入 Firebase Messaging，公开配置维护在 `apps/gui/native/android/google-services.json`，构建脚本按包名生成资源并接入 SDK、消息服务与点击桥接。SDK 仅在用户启用远程推送时允许自动初始化；不启用原生 Analytics 自动采集。Kotlin 编译插件固定为 2.1.21，以兼容 Firebase Messaging 26.0.1 的元数据。生成目录可重建，原生推送源码与配置在 `apps/gui/native/android` 维护。

尚未启用 Auth、Firestore；iOS 未接入原生 Firebase/APNs 推送，等待 Apple 凭据。FCM 发送凭据由 Core 发布包内置，也可在 Core 运行时覆盖；移动应用和前端静态代码不注入服务账号，原始密钥不提交仓库。配置见[远程推送](push.md)。

## 本机客户端工具链

项目不再提供 Flutter/Dart SDK。NixOS 开发机使用已有 Android SDK（Platform 36、Build Tools 36、NDK 28.2.13676358）、JDK 17 和 rustup，Rust Android 四种架构目标已安装。Node.js 24、pnpm 和 rustup 通过以下入口使用：

```bash
nix develop
# 或直接运行命令
nix develop -c pnpm --filter @omnistream/gui tauri info
nix develop -c pnpm --filter @omnistream/gui android:dev
```

[flake.nix](../../flake.nix) 提供锁定 nixpkgs 版本的 Nix 开发环境，Linux 桌面依赖见[桌面端](README.md#桌面端)。桌面和 Web 命令无需 Android SDK；Android 命令仍需宿主机 SDK，沿用 JAVA_HOME、ANDROID_HOME（或 ANDROID_SDK_ROOT）及 NDK_HOME；配置 SDK 且未指定 NDK 时选取 SDK 内版本最高的目录，工具链不存在时提示配置 NDK_HOME，不阻止 Web 或桌面命令。其他开发机按下节准备环境；iOS 仍需 macOS/Xcode。Flutter 的项目链接已移除，Nix store 中共享安装由系统垃圾回收管理。

## 构建环境

先在仓库根目录执行 `pnpm install`。Tauri Rust crate 与 `@tauri-apps/api` 保持相同主次版本，CLI 也使用仓库锁定的版本。基础依赖和移动工具链参见 [Tauri 官方环境说明](https://v2.tauri.app/start/prerequisites/#configure-for-mobile-targets)。

| 平台 | 必需环境 |
| --- | --- |
| Android | Windows、macOS 或 Linux；Rust/rustup、Node.js/pnpm、JDK 17+、Android Studio；SDK Platform 36、Platform Tools、Build Tools、Command-line Tools 和 NDK（Side by side）。设置 `JAVA_HOME`、`ANDROID_HOME`、`NDK_HOME`，后者指向实际 NDK 版本目录。 |
| iOS | macOS、完整 Xcode 与模拟器、Rust/rustup、Node.js/pnpm、CocoaPods；真机和签名构建需配置 Apple 开发团队及对应证书。通过 `APPLE_DEVELOPMENT_TEAM` 设置团队 ID，不在仓库保存个人团队信息。 |

原生工程和平台图标由 `apps/gui/scripts/mobile.mjs` 调用 Tauri CLI 生成到 `src-tauri/gen/android`、`src-tauri/gen/apple` 和 `src-tauri/icons/{android,ios}`；这些目录忽略提交。图标的单一图形源、平台安全区和重建方式见[应用图标](configuration.md#应用图标)，源或生成器变化会在下次移动命令时自动更新，已有工程也同步资源。首次初始化安装 Rust 目标；已经安装目标时可为 `android:init` / `ios:init` 追加 `--skip-targets-install`。

使用下列平台脚本初始化、开发与构建；直接运行 `tauri android init` 会跳过本项目的 Android HTTP 配置和签名接入。`dev` / `build` 在缺少工程时自动初始化；再次显式 `init` 会重新生成工程，修改应维护在受版本控制的配置或脚本中，签名密钥保存在工程目录之外。 应用标识变化时，脚本将旧原生工程移到同级带 `-previous-时间戳` 的备份目录后重新初始化；Android 的 `keystore.properties` 会复制到新工程，密钥文件仍使用仓库外的绝对路径。

CI 通过 `ANDROID_BUILD_TOOLS_VERSION` 与 `ANDROID_NDK_VERSION` 把已安装的稳定版工具版本写入生成的 Android Gradle 配置，保证 Gradle 与 Rust 使用同一套 NDK，并避免 Gradle 按自身默认值额外下载工具。未设置这两个变量时沿用原生工程配置；版本格式为三段数字，如 `36.0.0`、`28.2.13676358`。CI 的具体环境见[打包与发布](releases.md#ci-构建环境)。

### Android

```bash
pnpm --filter @omnistream/gui android:init
pnpm --filter @omnistream/gui android:dev
pnpm --filter @omnistream/gui android:build --debug --apk --target aarch64
pnpm --filter @omnistream/gui android:build --apk --aab
```

`android:dev` 使用连接的设备或模拟器；可加 `--open` 打开 Android Studio 并保持 CLI 运行。开发服务器按 Tauri 提供的 `TAURI_DEV_HOST` 监听，前端资源与 Core 地址分别配置。

`--target aarch64` 用于 arm64 真机，`--target x86_64` 可用于对应模拟器；不指定时构建全部支持的架构。APK 位于 `apps/gui/src-tauri/gen/android/app/build/outputs/apk`，AAB 位于同级 `outputs/bundle`，架构、构建类型和具体文件名以 CLI 输出为准。

调试 APK 使用 Android 调试签名。发布构建在 `apps/gui/src-tauri/gen/android/keystore.properties` 存在时读取以下字段；密钥文件放在仓库外，`storeFile` 使用绝对路径，Windows 路径使用正斜杠：

```properties
storeFile=/absolute/path/to/omnistream-release.jks
storePassword=YOUR_STORE_PASSWORD
keyAlias=omnistream
keyPassword=YOUR_KEY_PASSWORD
```

未提供该文件时生成未签名发布产物，不能直接作为已签名安装包分发；密钥生成和 Play 发布要求参见 [Android 签名](https://v2.tauri.app/distribute/sign/android/)。`keystore.properties` 位于已忽略的生成目录中，不提交密钥和密码。

### iOS

在 macOS 终端配置自己的团队 ID，再执行：

```bash
export APPLE_DEVELOPMENT_TEAM=YOUR_TEAM_ID
pnpm --filter @omnistream/gui ios:init
pnpm --filter @omnistream/gui ios:dev
pnpm --filter @omnistream/gui ios:build
```

`ios:dev` 可追加设备或模拟器名称；`--open` 打开 Xcode 并保持 CLI 运行。模拟器调试可使用 `ios:dev`，不等同于签名后的真机安装包。App Store 导出使用 `ios:build --export-method app-store-connect`；IPA 默认位于 `apps/gui/src-tauri/gen/apple/build/arm64`，具体文件名以 CLI 输出为准。发布方式、签名和归档导出依照 [Tauri App Store 指南](https://v2.tauri.app/distribute/app-store/)。

`src-tauri/tauri.ios.conf.json` 配置最低系统版本并移除桌面 sidecar；`Info.ios.plist` 声明局域网访问用途和 WebView HTTP 访问规则。团队、证书、描述文件及 App Store Connect 信息使用各开发者或发布环境的配置。
