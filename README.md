# Open Desktop Pet — Tauri Edition

Open Desktop Pet 的 Tauri 2 迁移版。它复用系统 WebView，不再随安装包携带 Electron/Chromium，因此 Windows 安装包可以显著缩小。

想了解窗口、状态机和打包方案，可以阅读[《用 Tauri 做一只会“察言观色”的桌面宠物：也祝》](docs/technical-blog-zh.md)。

## 功能

- 原版七状态小猪 PNG 与爱心动画
- 20%–140% 缩放，气泡和文字同步缩放
- 50%–200% 可调散步速度，时走时停地沿屏幕四边移动
- 检测持续键盘输入并进入工作状态；低活动时依次进入空闲、困倦和睡眠
- 透明、无边框、始终置顶的桌宠窗口
- 独立设置窗口，打开或关闭设置不隐藏桌宠
- 系统托盘、开机启动、配置和窗口位置持久化
- Windows NSIS 安装包

## 开发环境

Windows 需要：

- Node.js 20+
- Rust stable MSVC toolchain
- Microsoft C++ Build Tools（Desktop development with C++）
- Microsoft Edge WebView2 Runtime

```powershell
npm install
npm run tauri:dev
```

生成 Windows 安装包：

```powershell
npm run dist:win
```

如果本机没有 Visual C++ Build Tools，但已经安装 64 位 MinGW 和 Rust GNU 工具链，也可以运行：

```powershell
npm run dist:win:gnu
```

产物位于 `src-tauri/target/` 下对应工具链的 `release/bundle/nsis/` 目录。

默认使用 Tauri 的 `downloadBootstrapper` WebView2 安装方式，因此安装包保持较小，但旧系统首次安装可能需要联网获取 WebView2。Windows 11 通常已内置 WebView2。

发布前建议为安装包配置代码签名证书；未签名的本地构建可能触发 Windows SmartScreen 提示。

## 项目结构

```text
src/                 HTML/CSS/JavaScript 与角色素材
src-tauri/src/       Rust 窗口、托盘、配置与系统集成
src-tauri/icons/     各平台应用图标
src-tauri/tauri.conf.json
```

## 许可证

[MIT](LICENSE)
