# 打包与 GitHub Release

[Release workflow](../../.github/workflows/release.yml) 参考 omni-code 的版本触发方式，构建 Web、桌面、独立 Core 与原生移动端产物，生成分类更新日志并发布到 GitHub Releases。当前不发布到 AUR、Homebrew、Nix 包仓库或应用商店。

## 触发与版本

- Push 涉及版本清单或发布流水线时运行。版本号变化后构建；默认分支发布稳定版本，其他分支仅发布带预发布后缀的版本，例如 `0.2.0-beta.1`。默认分支上的预发布版本会报错，其他分支上的稳定版本不发布。
- 发布流水线本身变化也会构建；当前版本标签不存在时可首次发布。标签已指向当前提交时允许重试；指向其他提交时，仅修改流水线不会重新发布旧版本，必须升级版本。版本变化但标签已被其他提交占用时直接报错。
- Actions 页面 **Run workflow** 仅构建和保存 Actions artifacts，不创建标签或 Release。它可以在未配置 Android 密钥时生成明确带 `-unsigned` 后缀的产物。
- 标签为 `v<version>`。版本采用 SemVer，不使用 `+build` 元数据；以下四处必须一致，否则在构建前失败：

| 文件 | 字段 |
| --- | --- |
| [Tauri 配置](../../apps/gui/src-tauri/tauri.conf.json) | `version`，发布版本入口 |
| [根 Cargo 清单](../../Cargo.toml) | `workspace.package.version` |
| [GUI Cargo 清单](../../apps/gui/src-tauri/Cargo.toml) | `package.version` |
| [GUI package.json](../../apps/gui/package.json) | `version` |

升级后同步两个 Rust workspace 的锁文件并提交，再推送分支：

```bash
cargo update --workspace
cargo update --workspace --manifest-path apps/gui/src-tauri/Cargo.toml
python3 .github/scripts/release.py check
```

仅调整依赖且版本不变，不触发打包发布。原生应用标识沿用[移动端指南](mobile.md)的统一配置，不能为发布临时换包名。

## CI 构建环境

共享工具在 [setup-build](../../.github/actions/setup-build/action.yml) 中配置为 Node.js 24、pnpm 12.9.0、Python 3.12，原生构建安装 Rust stable 与对应目标。前端依赖使用 frozen lockfile；Core 与桌面 GUI 使用 Cargo locked 构建。

| 任务 | Runner 与构建依赖 |
| --- | --- |
| Web、版本检查与发布 | Ubuntu 24.04，避免 `ubuntu-latest` 自动迁移系统版本 |
| Linux 桌面 | Ubuntu 22.04 保持现有运行库兼容基线；显式安装 C/C++ 编译器、make、Perl、pkg-config、D-Bus、OpenSSL、WebKitGTK 4.1、GTK/AppIndicator/X11 相关开发库，以及 curl、wget、file、librsvg、patchelf、FUSE 2 |
| Windows 桌面 | Windows Server 2022；启用 MSVC x64 / Windows SDK，安装 NASM 2.16.03，指定 runner 自带的原生 Strawberry Perl |
| macOS 桌面 | macOS 15 ARM runner，选择 Xcode 16.4；分别构建 arm64 与 x86_64，先检查对应架构的 SDK 编译和链接 |
| Android | Ubuntu 24.04、Temurin JDK 17、SDK Platform 36、Build Tools 36.0.0、NDK 28.2.13676358；生成的 Gradle 同步固定 Build Tools 与 NDK，安装 ARM64、ARMv7、x86_64 Rust 目标 |
| iOS（可选） | macOS 15、Xcode 16.4 / iPhoneOS SDK、Ruby 3.3；通过该 Ruby 安装 CocoaPods 与 xcodeproj，另安装 XcodeGen |

Core 的发布构建与 Linux workspace 测试启用 `vendored-openssl`，按目标架构从锁定源码编译并静态链接 OpenSSL，避免 Windows 缺少开发库或 DLL、macOS Intel 跨架构误用 ARM Homebrew 库，以及安装后依赖构建机的 OpenSSL 路径。普通开发仍可使用系统 OpenSSL。源码构建需 C 编译器、make 和 Perl；Windows 显式设置 `OPENSSL_SRC_PERL`，防止 Git Bash 的 MSYS Perl 被优先选中。

各原生任务先运行 [check_environment.py](../../.github/scripts/check_environment.py)：Linux 检查 pkg-config 并实际编译、链接 D-Bus/GTK/OpenSSL；Windows 检查原生 Perl、NASM、MSVC/SDK；macOS/iOS 编译和验证目标架构的 SDK 探针；Android 检查 SDK/NDK 版本，并为三种 ABI 编译、链接探针和确认 Rust 标准库。缺少依赖时在完整 Rust 编译前直接失败。iOS 的 CocoaPods、Ruby 工程编辑器使用同一解释器，避免混用 Homebrew 的独立 Gem 环境。

依赖清单参考 [Tauri 前置环境](https://v2.tauri.app/start/prerequisites/)与 [GitHub Runner 镜像清单](https://github.com/actions/runner-images)。Ubuntu 22.04 runner 已公告于 2027 年 4 月 17 日退役，须在此前迁移构建环境并验证 Linux 兼容基线，见[官方公告](https://github.com/actions/runner-images/issues/14254)。

## 构建产物

| 平台 | Release 文件 |
| --- | --- |
| Web / PWA | `omnistream-web.tar.gz`，解压根目录即静态站点文件 |
| Linux x86_64 | `omnistream-linux-x86_64.AppImage`、`.deb`；`omnistream-core-linux-x86_64.tar.gz` |
| Windows x86_64 | `omnistream-windows-x86_64-setup.exe`；`omnistream-core-windows-x86_64.zip` |
| macOS Apple Silicon / Intel | `omnistream-macos-arm64.dmg`、`omnistream-macos-x86_64.dmg`；对应的 `omnistream-core-macos-<arch>.tar.gz` |
| Android | `omnistream-android.apk`、`omnistream-android.aab`；另有 `omnistream-android-{armeabi-v7a,arm64-v8a,x86_64}.apk` |
| iOS（可选） | `omnistream-ios.ipa`，Ad Hoc / `release-testing` 导出，仅限描述文件登记的设备 |
| 校验文件 | `manifest.json`、`SHA256SUMS`，包含版本、提交及各产物 SHA-256 |

桌面安装包内置相同目标架构的 Core sidecar，独立 Core 压缩包只包含可执行文件。Windows 安装包未做代码签名；macOS 使用 ad-hoc 签名，未公证。Linux 在 Ubuntu 22.04 构建，仍依赖目标系统兼容的运行库。Web 的路由回退和远程 Core 部署见[构建与部署](README.md#构建与部署)。移动端连接电脑或服务器上的 Core，不包含桌面 sidecar。

每个构建任务上传独立 Actions artifact，保留 14 天。发布任务检查所有必需平台、移动签名标记、版本/提交、完整文件清单与哈希；任何任务失败或产物缺失都会阻止公开发布。可选 iOS 未启用时允许跳过；启用后必须构建成功且包含 IPA。

下载后可在同一目录使用 `sha256sum -c SHA256SUMS` 校验全部文件；macOS 可用 `shasum -a 256 -c SHA256SUMS`，Windows PowerShell 可用 `Get-FileHash <文件> -Algorithm SHA256` 与清单逐项比对。

## 签名配置

在 GitHub 仓库 **Settings → Secrets and variables → Actions** 配置。只恢复本项目自己的凭据，不复制参考项目的密钥。Base64 secrets 应是二进制文件的原始 Base64 内容，可含换行，不能填文件路径或 data URL；例如用 Python 的 `base64.b64encode(Path(...).read_bytes())` 在本机生成。密钥文件、证书与密码不提交仓库。

### 远程推送发送账号

仓库 secret `OMNISTREAM_FCM_SERVICE_ACCOUNT_JSON` 保存 `omnistream-9ed10` 项目专用 FCM 发送服务账号的完整 JSON，账号仅授予 `roles/firebasecloudmessaging.admin`。桌面任务只在构建 Core 和准备 sidecar 的步骤传入该 secret，由 Core 的 `build.rs` 写入 Cargo 输出目录并编译到二进制中；独立 Core 和桌面 sidecar 因而均可直接发送 Android 通知。前端 JavaScript、Web 静态包和 Android/iOS 原生客户端不注入这份凭据。

正式 Release 设置 `OMNISTREAM_REQUIRE_BUNDLED_FCM=true`，缺少凭据时构建失败，不能发布缺少发送配置的 Core。手动构建及源码开发允许不提供凭据，此时只缺少 Android 发送能力。源码构建也可通过 `OMNISTREAM_FCM_SERVICE_ACCOUNT_PATH=/absolute/private/path/account.json cargo build -p omnistream --bin omnistream --release` 注入；修改环境变量或文件后 Cargo 会重新执行构建脚本。

原始凭据不提交 Git，也不作为单独的 release 文件上传；内置后的可提取性、共享权限边界、运行时覆盖与轮换要求统一见[远程推送](push.md#core-配置)。不要把本机包含共享凭据的 Cargo 输出缓存发布成额外下载产物。

### Android

自动发布必需以下四个 secrets，缺失或只配置一部分会失败，不会退回调试签名：

| Secret | 内容 |
| --- | --- |
| `ANDROID_KEYSTORE_BASE64` | 发布用 JKS / PKCS12 keystore 的 Base64 |
| `ANDROID_KEY_ALIAS` | 密钥别名 |
| `ANDROID_KEY_PASSWORD` | 密钥密码 |
| `ANDROID_STORE_PASSWORD` | keystore 密码 |

工作流初始化原生工程后，在临时目录恢复密钥，通过生成的 `keystore.properties` 接入签名；校验通用/分 ABI APK 与 AAB 的签名，任务结束时清理密钥和配置。手动构建不配置上述任何 secret 时可生成未签名包；未签名包不能直接安装或用于正式 Release。密钥创建与本地构建见[Android 签名](mobile.md#android)。

### iOS（可选）

默认跳过 iOS 打包。启用前配置 repository variable `ENABLE_IOS_RELEASE=true` 和以下 secrets：

| Secret | 内容 |
| --- | --- |
| `IOS_CERTIFICATE_BASE64` | 带私钥的 Apple Distribution `.p12` 证书 Base64 |
| `IOS_CERTIFICATE_PASSWORD` | `.p12` 导出密码 |
| `IOS_PROVISIONING_PROFILE_BASE64` | 对应该 Bundle ID、团队和证书的 Ad Hoc 分发 `.mobileprovision` Base64 |

描述文件须包含允许安装的设备，开发团队从中读取。工作流在 macOS 临时钥匙串中导入证书，将生成的 Xcode 工程设为手动签名，并用 `release-testing` 导出 arm64 IPA；构建号使用 Actions run number。无效证书、描述文件不匹配或缺少 secrets 时任务失败。结束时恢复钥匙串列表并清理证书、描述文件。此流程不上传 App Store Connect 或 TestFlight；IPA 也不能在任意 iPhone 上直接安装。

## 更新日志与重试

发布任务是唯一获得 `contents: write` 权限的任务，用仓库 `GITHUB_TOKEN` 创建标签和 Release，无需额外 PAT。仓库规则需允许该 token 创建版本标签和 Release。

更新日志从当前提交可达的上一版本标签生成，稳定版跳过预发布标签；首版包含全部非合并提交。按 Conventional Commits 的 `!`、`feat`、`fix`、`perf` 分类，其他提交进入 Other changes，附提交和完整比较链接。Release 同时说明下载、移动端连接方式与桌面签名限制。

先校验全部产物，再创建/验证标签、创建草稿、上传文件，最后公开 Release。中途失败可在同一提交重新运行，修复已有草稿；已公开且文件齐全的 Release 保持不变。已公开但缺文件时要求发布新版本；不会移动已有标签或覆盖已公开版本。修改代码后的修复也使用新版本。

## 验证

本地检查命令与验证范围见[发布流程验证](testing.md#发布流程验证)。辅助脚本测试和工作流语法检查通过，不等于 GitHub 上所有平台的实际编译、正式签名与安装已经验收。
