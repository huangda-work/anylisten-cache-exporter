# Any Listen Cache Exporter

一个基于 Rust 和 Tauri 的跨平台桌面工具，用于查看和导出 [Any Listen](https://github.com/any-listen/any-listen) 的音乐缓存。本项目是独立工具，并非 Any Listen 官方项目。

## 功能

- 扫描完整的本地音乐缓存，查看歌曲信息、试听并批量导出。
- 列出数据库中有音乐地址但尚无完整缓存的歌曲；用户选择后可尝试联网下载。
- 记录和核对当前导出目录中的保存状态，支持单曲保存、只选未保存、取消导出及重试失败歌曲。
- 已有同名但内容不同的文件会另存，不覆盖原文件。

程序不会修改 Any Listen 的文件。联网下载仅使用本地数据库中已有的地址；地址失效或服务器拒绝访问时，歌曲会显示为失败。

## 开发

需要 Node.js、npm 和 Rust 稳定版。macOS 还需要 Xcode Command Line Tools；Windows 需要 Visual Studio C++ Build Tools 和 WebView2；Linux 需要 Tauri 2 的系统依赖。具体版本和安装方法见 [Tauri 前置条件](https://v2.tauri.app/start/prerequisites/)。

```sh
npm ci
npm run desktop:dev
```

## 测试与构建

```sh
npm test
cargo test -p anylisten-core
npm run desktop:build
```

构建产物位于 `target/release/bundle/`。Windows 可以运行 `npm run desktop:build -- --bundles msi` 生成 MSI 安装包。

## 发布版本

将 `package.json`、`src-tauri/tauri.conf.json` 和 `src-tauri/Cargo.toml` 的版本号保持一致，推送代码并创建对应的 `v<版本号>` 标签（例如 `v0.1.3`）。随后在 GitHub 上基于该标签发布 Release。发布后，GitHub Actions 会自动构建并把 macOS（Apple Silicon 和 Intel）的 DMG、Windows x64 的 MSI 与免安装 ZIP、Linux x64 的 DEB 和 AppImage 上传到该 Release 的 Assets。版本号不一致时工作流会停止构建。

Windows 免安装版解压后可直接运行其中的 `.exe`，无需安装 MSI；系统仍需具备 WebView2 Runtime，程序设置会保存在用户的 AppData 中。

macOS 安装包目前没有签名和公证，首次打开可能需要在系统设置中手动允许运行。

macOS 扫描结果已与原版工具对照，Windows 的 Chromium blockfile 缓存已在 Any Listen 0.10.1-beta.5 上验证。其他版本和 Linux 缓存格式仍需实机验证。

## 开源协议

本项目采用 [MIT 协议](LICENSE)。
