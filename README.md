# shimeji2

A native macOS desktop mascot runner, written in Rust. No Java runtime required.

shimeji2 把 Shimeji 桌宠运行方式重写为原生 macOS 应用。当前进入公开测试准备阶段：
可以拖动、抛起桌宠，让它沿桌面和窗口边缘活动，也可以导入自己的角色包。
这是原 Shimeji 的改写版本，署名与许可见 [LICENSE](LICENSE) 和 [NOTICE.md](NOTICE.md)。

<p align="center"><img src="packaging/icons/shimeji2.png" width="160" alt="shimeji2 application icon"></p>

## 支持情况

- 首个测试包面向 **Apple Silicon（M 系列）Mac，macOS 14+**。
- 实际运行验证使用 macOS 15；macOS 14 和其他 Mac 的首次打开仍需要用户反馈。
- 普通启动持续运行，使用菜单栏的 Quit 退出。性能测试与显式 `--seconds` 仍可定时退出。
- 当前测试包使用本地 ad-hoc 签名，尚未 Developer ID 签名或公证。

## 安装与操作

从本项目 [Releases](https://github.com/AlanJager/shimeji2/releases) 获取测试包；
预编译包尚未发布时可以按下方步骤自行构建。
打开 DMG 后把 `shimeji2.app` 拖进 Applications，或解压 ZIP 后移动应用。
首次打开被 macOS 拦截时，参见 [首次打开说明](packaging/READ-ME-FIRST.md)。

- 左键按住角色拖动；松开后按鼠标移动速度抛出。
- 右键打开角色菜单。菜单栏可暂停、隐藏、添加或移除角色、打开设置和退出。
- `Control + Shift + S` 默认切换显示/隐藏，可在设置中修改。
- Settings → 宠物：拖入角色 ZIP 或通过文件选择器导入。
- 设置提供大小、不透明度、行为、声音等选项。繁殖开启时可出现多个角色。

默认角色是上游白色 shime，原始 PNG 与 XML 保持逐字节一致。
婉儿、花阳和其他第三方角色包不随源码或通用测试包分发。
导入素材时请自行核对其作者说明与分发许可。

## 功能与测试边界

已经实现：固定 40ms / 25Hz 的行为引擎、经典包导入、表达式求值、
桌面/窗口边界、拖动和抛出、中文原生设置、菜单与快捷键、角色库、
透明像素点击处理以及本地 forge 工具。

以下功能仍按实验功能提供：

- **窗口恶作剧**：默认关闭，需辅助功能权限；窗口是否允许移动取决于目标应用。
  ToDesk / 钉钉搬运尚未完成真实窗口验收。
- **第三方包兼容性**：部分扩展动作未实现，两个已记录的包差异仍待处理。
- **Agent hooks / 在线素材生成**：默认不开启；完整端到端验收和在线生成发布门槛尚未完成。
- 多显示器、系统休眠/唤醒、长时间运行及其他机器首次安装仍需扩展测试。

发布收尾后的完整工作区验证为 455 个测试通过、0 失败、7 个忽略；详情见
[发布说明](docs/release.md)。性能数据只适用于对应测量场景，不承诺所有机器都小于 1% CPU。

## 从源码构建

Rust 工具链由 `rust-toolchain.toml` 固定。普通构建和测试不需要 Java、Gradle，
也不需要访问个人的 Claude/Codex 配置。

```sh
cargo build --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
```

在 macOS 上打包（需要 Xcode Command Line Tools；发布整理脚本使用 Python3.11+）：

```sh
./scripts/bundle.sh
# 依赖已缓存时可使用 ./scripts/bundle.sh --offline
```

得到 `target/bundle/shimeji2.app`、`shimeji2.dmg`、`shimeji2.zip`。
打包只使用已安装的 Rust targets；两种架构均已安装时会生成 universal 应用。
图标 PNG 已包含在源码中，构建时用系统 `sips` / `iconutil` 转为 ICNS。

用临时配置验证，避免修改日常设置：

```sh
open -W -n target/bundle/shimeji2.app --args \
  --seconds 30 --profile /tmp/shimeji2-review --agent-home /tmp/shimeji2-review/agent-home
```

完整发布流程、签名、公证和源码快照说明见 [docs/release.md](docs/release.md)。

## 架构

依赖方向：

`shimeji-expr → shimeji-core → shimeji-packs → shimeji-app → shimeji-macos`

`shimeji-proto` 提供本地 socket 消息，供 app、hook 和 forge 使用。
只有 `shimeji-macos` 使用 Apple 平台 API；其他 crate 保持跨平台纯 Rust。
引擎中的时长、速度使用 tick 和像素/tick，macOS shell 负责时钟与显示。

兼容性参考为 `tools/ee-reference/PIN` 固定的 Shimeji-Desktop 源码。
普通测试使用已提交的白色 shime / 合成包 golden traces；参考程序仅用于离线维护，
不是应用运行依赖。第三方 corpus 和自定义角色素材不进入源码发布。

## 许可与素材

程序沿用 [zlib/libpng 许可](LICENSE)，保留原作者 notice，并明确标注重写版本。
默认 shime 的原始许可和素材说明见
[assets/packs/shime/ATTRIBUTION.md](assets/packs/shime/ATTRIBUTION.md)；
上游文本没有单独明确图像的授权范围，该不确定性也保留在说明中。
应用图标为本项目使用 AI 辅助生成的独立素材，见 [图标说明](packaging/icons/README.md)。

应用与 hook 的65项 Rust 依赖声明和上游许可证，见
[THIRD-PARTY-NOTICES](packaging/THIRD-PARTY-NOTICES.md)。
