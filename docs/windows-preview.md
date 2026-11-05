# Windows 基础体验版

目标是让朋友拿到 ZIP 后能直接运行。当前为 Windows 10 1607+ / Windows 11 x64
开发预览，尚未完成 Windows 桌面真机验收。构建通过不等于交互验收通过。

完整解压后双击 `shimeji2.exe`，保留旁边的 `packs` 文件夹。无需 Java、Rust 或
管理员权限。左键拖动并松开抛出；右键桌宠/托盘提供添加、移除、暂停、隐藏、找回、退出。
`Ctrl+Shift+S` 切换显示。默认白色 shime 不变，不附带私人或第三方角色。

将角色 ZIP/文件夹拖到 EXE 上，或通过托盘菜单打开 ZIP，即可替换当前角色。
选择只在本次运行有效。多角色包默认第一个，也可用 `--character NAME` 指定。
完整参数和限制见 [随包说明](../packaging/windows/README.txt)。

## 构建

Windows 安装固定版本 Rust 与 Visual Studio C++ Build Tools 后：

```powershell
cargo build --locked --release -p shimeji-windows --bin shimeji2
python scripts/bundle-windows.py --exe target/release/shimeji2.exe
```

macOS/Linux 可用 Rust + Zig 0.15.2 交叉编译：

```sh
rustup target add --toolchain 1.94.1 x86_64-pc-windows-gnu
./scripts/build-windows.sh --offline
```

首次下载 Cargo 依赖时去掉 `--offline`。脚本不修改 Cargo 全局配置、默认工具链或
已有安装包目录；重复构建会原子替换同名 ZIP，也可传入新的 `--out` 目录。
输出为 `target/windows-bundle/shimeji2-0.1.0-windows-x64-preview.zip` 与 SHA256SUMS。
打包核对 46 张 PNG 与 2 个 XML 的原始哈希，并生成归档内逐文件清单。

`.github/workflows/windows.yml` 用于在 Windows runner 上执行专用 lint/test、发布构建、
打包和移动目录后的 `--check-pack` 冒烟检查，上传 Actions artifact，不发布 Release。

## 实现与验收范围

`shimeji-windows` 是 `shimeji-app` 之上的并列平台 shell；Apple API 继续仅在
`shimeji-macos`。Win32 代码与依赖以 `cfg(target_os = "windows")` 隔离。
引擎继续使用 40ms 固定步长、原有 PNG 纯 Rust 解码和导入限制。

桌宠使用 Win32 layered windows 的逐像素 alpha；透明像素的命中由 Windows 处理，
参见 [Microsoft 文档](https://learn.microsoft.com/en-us/windows/win32/winmsg/window-features#layered-windows)。
渲染上传预乘 BGRA，帧图与镜像状态不变时只移动窗口。
屏幕工作区避让任务栏；前台窗口仅提供碰撞边界，不执行窗口移动。

默认关闭繁殖、限制 20 只。声音、设置窗口、窗口搬运、Agent hooks、更新和开机启动
尚未接入。物理像素缩放可用 `--scale` 调整；混合 DPI、显示器变更、休眠和长期资源
占用仍待 [Windows 手动验收](testing/windows-manual-checklist.md)。
预览 EXE 尚未进行 Windows 代码签名。

## 本机验证（2026-11-05）

固定 Rust 1.94.1、macOS ARM64 主机：工作区 fmt、全目标全特性 Clippy 通过；
基于 GitHub 最新主线的独立推送快照测试 465 通过、0 失败、7 忽略；不依赖本地未提交改动。
Windows 专用 6 项无界面测试包含在总数中。
Windows x64 GNU 目标的专用 Clippy 通过；Zig 0.15.2 交叉链接生成 PE32+ GUI EXE。
归档包含 63 个文件，逐文件哈希、46 PNG / 2 XML 原始哈希均通过验证，
EXE 已移除开发机 home 路径，导入表仅包含 Windows 系统 DLL。

尚未执行：Windows runner CI、Windows EXE 的实际启动/`--check-pack`、
Windows 桌面鼠标/托盘/DPI 验收。这个 ZIP 应以开发预览身份交给测试者。
