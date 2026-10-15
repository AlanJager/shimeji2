# shimeji2

**用 Rust 重写的 macOS Shimeji 桌宠。** shimeji2 以原生菜单栏应用的形式运行经典 Shimeji 角色包，不需要 Java。

[English](README.md) · 简体中文

<p align="center"><img src="packaging/icons/shimeji2.png" width="160" alt="shimeji2 应用图标"></p>

> [!NOTE]
> **公开测试版：v0.1.0-beta.1**（应用内显示的版本号为 0.1.0）。
> - **平台：** Apple Silicon Mac，macOS 14 及以上。目前只在维护者自己的 Mac（macOS 15）上构建和运行过。本测试版不面向 Intel Mac。
> - **签名：** 尚未公证，首次打开需要在系统设置中批准一次，见[安装](#安装)。
> - **界面语言：** 设置窗口只有中文，菜单为英文。

## 这是什么

Shimeji 是住在桌面上的小角色。它们会沿着屏幕底部走动，爬上屏幕边缘和最前面的窗口，倒挂在天花板上，坐在那个窗口顶上，还会分裂繁殖。你可以把它拎起来扔出去。最初的 Shimeji 和流行的 Shimeji-ee 分支都是 Java 应用。

shimeji2 是 [shimeji4mac](https://github.com/AlanJager/shimeji4mac)（Shimeji for Mac）的 Rust 重写版，底层没有 Java：

- **原生：** 菜单栏应用，不占 Dock。每只宠物都是一个透明、无边框的窗口。点在宠物周围的透明像素上，点击会落到下面的应用。
- **行为与 Shimeji-ee 一致：** 引擎逐 tick 对照 Java 参考实现检查，详见[还原度如何？](#还原度如何)。
- **支持的包格式：** 可加载常见目录结构的经典（日文 XML）和 Shimeji-ee（英文 XML）角色包，以及 Codex 宠物精灵图。部分 Shimeji-ee 扩展尚未实现，见[兼容性说明](#兼容性说明)。
- **导入限制：** 角色包由纯 Rust 解码，并限制总大小、文件数和图片尺寸。链接和特殊文件会被拒绝。每次导入都先在暂存目录中检查，坏包不会替换掉能用的包。
- **不需要时停下：** 宠物暂停或隐藏、显示器休眠、屏幕锁定，或开启“减弱动态效果”时，时钟会停止。

应用自带一个角色：原版白色 **shime**。它的 46 张图片和两个行为文件与 shimeji4mac 中的文件逐字节一致。

## 安装

1. 从 [Releases](https://github.com/AlanJager/shimeji2/releases) 下载 DMG 或 ZIP，并用同一版本的 `SHA256SUMS.txt` 校验，例如 `shasum -a 256 shimeji2-*.dmg`。如果还没有发布预编译包，请[从源码构建](#从源码构建)。
2. 把 `shimeji2.app` 移到 **应用程序**。
3. 打开它。测试版未经公证，第一次会被 macOS 拦下。打开 **系统设置 → 隐私与安全性**，点 **仍要打开**，再确认 **打开** 并输入密码。不要关闭 Gatekeeper。参见 [Apple 的说明](https://support.apple.com/zh-cn/102445)。DMG 或 ZIP 里与应用放在一起的 `READ-ME-FIRST.md` 也有说明。

普通桌宠不需要任何特殊权限。只有开启[窗口恶作剧](#窗口恶作剧)时才会请求辅助功能权限。

**卸载**

1. 如果安装过智能体钩子，先在 **设置 → 智能体** 中点 **卸载…** 移除。这些条目写在 `~/.claude/settings.json` 和 `~/.codex/config.toml` 中，并指向应用。
2. 如果开启过登录时启动，到 **系统设置 → 通用 → 登录项**（macOS 15 中为 **登录项与扩展**）中移除 shimeji2。
3. 从菜单栏选 **Quit**，然后删除应用。
4. 如需一并删除设置、导入的角色包和恶作剧日志，删除 `~/Library/Application Support/shimeji2`。

## 使用

启动后，一只 shime 会落到主显示器上。

| 想要… | 操作 |
|---|---|
| 移动宠物 | 在宠物上按住鼠标左键拖动。 |
| 扔出宠物 | 移动鼠标的同时松开，它会以接近鼠标的速度飞出去。 |
| 打开宠物菜单 | 右键或按住 Control 点击宠物，见[宠物菜单](#宠物菜单)。 |
| 隐藏或显示全部宠物 | 按 **Control + Shift + S**。 |
| 其他操作 | 使用菜单栏中的 **し**。 |

### 分裂繁殖

宠物默认会分裂繁殖，最多 50 只。要关闭，在 **设置 → 外观与动作** 中关掉 **自动繁殖**，或调低 **宠物数量上限**。多出来的宠物可以用 **Remove Newest** 或 **Dismiss All** 清掉。

### 减弱动态效果

如果 macOS 开启了 **减弱动态效果**，宠物会静止地站在屏幕底部。想让它照常活动，在 **设置 → 高级** 中开启 **减少动态效果时继续动画**。

### 宠物菜单

右键或按住 Control 点击宠物：

| 菜单项 | 作用 |
|---|---|
| **Dismiss** | 移除这只宠物。 |
| **Dismiss All** | 移除这个角色的所有宠物。 |
| **Follow Cursor** | 这个角色的宠物会追着鼠标跑。 |
| **Restore Moved Windows** | 灰色，尚未启用。 |
| **Behaviors** | 开关角色包中的可选行为。自带的 shime 没有可选行为，所以是灰色。 |
| **Pause/Resume** | 暂停或继续全部宠物。 |

### 菜单栏

**し** 菜单包含：

| 菜单项 | 作用 |
|---|---|
| **Characters** | 列出已安装的角色，每个都有 **Add** 和 **Remove Newest**。 |
| **Pause/Resume** | 暂停或继续全部宠物。 |
| **Hide All/Show All** | 隐藏或显示全部宠物。 |
| **Dismiss All** | 移除所有宠物。 |
| **Mischief** | 开关[窗口恶作剧](#窗口恶作剧)。 |
| **Settings…** | 打开设置窗口。 |
| **Check for Updates…** | 在浏览器中打开 Releases 页面。没有配置仓库地址的构建（例如 `cargo run`）中为灰色。应用从不自行检查更新。 |
| **Quit** | 退出应用。 |

第一次启动时，菜单栏图标下会弹出一个小提示，告诉你快捷键，并提供 **Launch at login**（登录时启动）选项。

宠物暂停时，拖动后松手只会把它放在原地，不会扔出去。

### 设置

修改会自动保存。

| 页面 | 内容 |
|---|---|
| 宠物 | 已安装的角色包，以及其中的角色和行为。<br>角色、以及包里标为可选的行为带有启用复选框。<br>导入时有警告的包会标上 **!**，选中它即可查看警告。<br>[导入](#添加角色)也在这里。 |
| 外观与动作 | 宠物大小（25%–800%）、缩放方式、不透明度。<br>保持在屏幕内、跨显示器活动。<br>宠物数量上限（默认 50，最多 100）、自动繁殖、临时分身。<br>**允许抛掷**，见下方说明。<br>一个灰色、暂未开放的“角色变换”开关。 |
| 窗口互动 | [窗口恶作剧](#窗口恶作剧)及其窗口规则。 |
| 智能体 | [智能体任务反应](#智能体任务反应)。 |
| 高级 | 隐藏/显示快捷键：输入包含 Control 或 Command 的组合，例如 `ctrl+shift+s`，再按 Return。<br>动作声音。<br>**减少动态效果时继续动画**。<br>诊断面板。 |

关于这些设置：

- **允许抛掷：** 尽管提示文字这样写，这个开关实际控制的是开启恶作剧时宠物能否搬动和扔出 *窗口*。你随时都可以把宠物扔出去。
- **宠物数量上限：** 菜单里的 **Add** 按全部宠物计数，但繁殖是按角色分别限制的，多个角色同时繁殖时，总数可能超过上限。
- **声音：** 默认关闭。开启后只播放角色包内的无压缩（整数 PCM）WAV 文件。自带的 shime 没有声音。

## 添加角色

打开 **设置 → 宠物**，点 **导入素材…**，或把 ZIP 文件、文件夹拖进列表。然后从菜单栏选 **Characters → *角色名* → Add** 放出宠物。导入的角色不会在启动时自动出现，只有自带的 shime 会。

> [!IMPORTANT]
> 角色包的文件（`conf/`、`img/`、XML、`pet.json` 或 `pack.json`）必须位于 ZIP 或文件夹的最顶层。如果 ZIP 里多套了一层文件夹（访达的 **压缩** 就会这样），导入会失败并提示 “no actions file found”。请先解压，再导入里面那个文件夹。

| 格式 | 特征 |
|---|---|
| **经典** Shimeji | 日文标签的 XML（根元素 `<マスコット>`），通常是 `conf/動作.xml` 和 `conf/行動.xml` |
| **Shimeji-ee** | 英文标签的 XML（根元素 `<Mascot>`），通常是 `conf/actions.xml` 和 `conf/behaviors.xml` |
| **Codex 宠物** | `pet.json` 加一张 PNG 精灵图，尺寸为 1536×1872（v1）或 1536×2288（v2） |
| **原生格式** | shimeji2 自己的 `pack.json`，配固定 8×6 精灵图集（见 [`native/manifest.rs`](crates/shimeji-packs/src/native/manifest.rs)） |

经典和 Shimeji-ee 角色包可以采用以下任一常见结构：

- `conf/` 加 `img/`
- `img/<名称>/` 下的图片集文件夹
- XML 放在包的根目录
- `conf/` 旁边直接放 PNG

两种格式都可以使用任一可接受的文件名：动作文件可以是 `actions.xml`、`動作.xml`、`one.xml` 或 `1.xml`；行为文件可以是 `behaviors.xml`、`behavior.xml`、`行動.xml`、`two.xml` 或 `2.xml`。

**导入规则**

- **同名：** 如果已经安装了同名的包（以文件夹或 ZIP 文件名为准），可以选 **Replace**（替换）、**Keep Both**（保留两者，新副本加 `-2` 后缀）或 **Cancel**（取消）。
- **大小：** 一次导入最多 200 MB 文件、5,000 个文件和文件夹，嵌套最多 16 层。
- **图片：** 每张图片每边最多 4096 px；一个包的图片和声音解码后合计最多 512 MB。
- **ZIP 格式：** 只支持普通压缩方式（存储或 Deflate），不能加密码。
- **链接：** 包里任何位置出现符号链接或其他特殊文件，都会拒绝导入。
- **出错时：** 超出任何上限都会拒绝整个导入。较小的问题不影响加载，警告会显示在宠物页面。

设置可以关闭角色，但不能删除角色包。要删除导入的包，先退出应用，再删除 `~/Library/Application Support/shimeji2/packs/` 下对应的文件夹。自带的 shime 每次启动都会重新安装，所以请改为关闭它的角色。

除自带的 shime 外，本仓库和应用都不包含任何角色。导入或分享角色包前，请先确认它的使用条款。

### 兼容性说明

Java 参考实现能运行的大多数角色包，在这里的行为相同。已知差异：

- **未实现的 Shimeji-ee 动作：** 有 17 个扩展动作能被识别，但目前按普通的“停留”运行：宠物留在原地，播放该动作的动画（如果有）。这些动作大多没有设置时长，所以宠物可能一直停在其中，直到被拖动或失去立足点。它们是：
  - `Broadcast*`、`Scan*`、`Complex*` 系列
  - `BreedJump`、`BreedMove`、`Interact`、`Transform`、`SelfDestruct`、`Mute`、`Turn`、`MoveWithTurn`
- **未知的动作类：** 引擎完全不认识的动作类也会变成“停留”并给出警告，角色包仍然可以加载。Java 参考实现则会拒绝加载这样的包。
- **被忽略的属性：** 点击热区会被读取但不起作用。繁殖时的 `BornMascot` 会被忽略，所以分裂出来的总是同一个角色。

## 可选功能

以下功能都是**实验性的，默认关闭**。普通桌宠用不到它们。

### 窗口恶作剧

宠物会像原版 Shimeji 那样，抓起最前面的窗口、搬着走、把它扔出去。可以从菜单栏的 **Mischief**，或 **设置 → 窗口互动** 开启。它需要辅助功能权限。测试版使用临时（ad-hoc）签名，所以每次更新后都要重新授权。

- **移动范围：** 只移动当前最前面的窗口，而且只改位置，从不改变窗口大小。
- **受保护的窗口：** macOS 自身的登录、授权提示和系统设置永远不会被碰。1Password、Bitwarden、钥匙串访问和终端默认受保护。其他应用的登录窗口不在保护范围内，除非你自己添加禁止规则。
- **窗口规则：** 可以添加按应用名或窗口标题部分匹配（区分大小写）的允许或禁止规则。一旦添加了任何允许规则，列表就变成白名单：之后只有匹配的窗口才能被搬动。
- **窗口复原：** 每次搬动前都会先写入配置目录中的日志，日志从不记录窗口标题。关闭恶作剧或退出应用时，被搬过的窗口会回到原位，除非你之后自己移动或关闭了它。如果应用崩溃，会在 24 小时内下次开启恶作剧且已授权辅助功能时复原。
- **应用支持：** 窗口能否被移动，取决于它所属的应用。

> [!WARNING]
> 保护规则按英文应用名匹配。在系统语言为中文等其他语言的 Mac 上，对系统设置、终端和钥匙串访问的内置保护可能不起作用。请用本地化名称（例如 系统设置、终端、钥匙串访问）添加禁止规则，或者不要开启恶作剧。

在窗口上行走是另一项功能，不需要恶作剧，也不需要任何权限。

### 智能体任务反应

shimeji2 可以对 [Claude Code](https://claude.com/claude-code) 或 [Codex](https://github.com/openai/codex) 的状态做出反应：

- 智能体开始工作时，宠物坐下。
- 需要你输入时，它会晃腿。
- 出错时，它会摔一跤。
- 完成时，它会跳起来。使用自带的 shime 时，目前这一跳会冲向主显示器的左上角。

每个姿势在状态变化时播放一次，之后宠物回到平常的活动。宠物上方的小标签会一直显示当前状态，点它可以切回终端。

> [!CAUTION]
> **不要在 v0.1.0-beta.1 中安装钩子。** 安装器把钩子指向了应用本身的可执行文件，而不是随附的 `shimeji-hook`。应用遇到钩子的参数会报错退出，Claude Code 可能因此把每次提问和每次工具调用都当作被阻止。如果已经安装，请在 **设置 → 智能体** 中点 **卸载…**。

修复安装器之后，它的预期用法是：

- **分两步：** **设置 → 智能体** 中的开关只启动应用的监听。钩子要另外点 **安装…** 安装，而且只有 `shimeji2.app` 位于“应用程序”文件夹中运行时才能安装。
- **先预览：** 安装器会显示 `~/.claude/settings.json` 中 `hooks` 条目和 `~/.codex/config.toml` 中 `notify` 一行的前后对比。点 **应用更改** 之前不会写入任何内容。
- **写入时的改动：** 应用更改会把 `settings.json` 的键按字母顺序重新排列。原文件会保留为 `*.shimeji2-bak.1` 到 `.5`。
- **已有的 Codex 设置：** 如果 `~/.codex/config.toml` 已经设置了 `notify`，两个智能体的安装都会被拒绝。
- **发送的内容：** `shimeji-hook` 总是成功退出，不输出任何内容。它通过本机私有套接字，只把智能体名称、会话 ID、状态和你所用终端应用的名称发给 shimeji2，从不发送提示词、工具输入或文件路径。
- **限制：** Codex 只会报告“完成”，代码审查事件尚未接入。

### forge（仅限源码检出）

`shimeji-forge` crate 会构建一个名为 `forge` 的命令行工具，借助编程智能体自带的图像工具制作新角色：

1. 准备生成任务。
2. 按 46 个姿势的 shime 布局提取、摆放画面，并做质量检查。
3. 输出联系表（contact sheet）和 GIF 预览。
4. `forge install` 会先请运行中的应用确认，再导入任何内容。只有开启了智能体任务反应时它才能联系到应用；否则会打印文件夹路径，由你拖到宠物页面中。

目前的不足：

- forge 不包含在应用包中，请用 `cargo run -p shimeji-forge -- <命令>` 运行。
- 它还不会生成角色包的行为 XML，需要自己放入一个 `conf/` 文件夹；自带 shime 的 `conf/` 与 46 姿势布局一致。
- 在线生成服务尚未实现。

详见 [`integrations/skills/shimeji-forge/SKILL.md`](integrations/skills/shimeji-forge/SKILL.md)。

## 还原度如何？

行为参考是 [DalekCraft2/Shimeji-Desktop](https://github.com/DalekCraft2/Shimeji-Desktop)（一个 Shimeji-ee 分支），固定在 [`tools/ee-reference/PIN`](tools/ee-reference/PIN) 记录的提交上。一个无界面的 Java 测试程序用固定随机种子，让它运行一组脚本化场景：行走、骑在移动的窗口上、拖动和投掷、繁殖上限和双屏。它会记录每个 tick 的行为、动作、位置、图片、朝向和随机数抽取的累计次数。`cargo test` 用 Rust 重放同样的场景并比较轨迹，这一步不需要 Java。

角色包里的脚本由一个小型、沙箱化的 JavaScript 子集求值器运行，取代 Java 的 Nashorn。逐位一致的 `java.util.Random` 移植会以相同的顺序做出相同的随机选择。

- **已提交的轨迹：** 共 13 条，来自自带的 shime 和合成测试包。其中 12 条的每一行 tick 都与参考实现一致。第 13 条是有意的差异：非布尔条件会让 Java 的那一 tick 中止，而 shimeji2 把它当作 false 继续运行。
- **第三方角色包：** 在本地用 50 个包对照参考实现，其中 15 个参考实现本身拒绝加载。
  - 其余 35 个中，有 33 个没有用到未实现的扩展。
  - 这 33 个中有 32 个在全部七个场景中轨迹完全一致。

  这些素材和轨迹只保留在维护者本机，见 [`docs/testing/golden-traces.md`](docs/testing/golden-traces.md)。
- **脚本：** 表达式测试向量的预期结果由参考实现自己的 Nashorn 生成，从不手工修改。它们包括挑选的类型转换用例，以及自带 shime 中的全部表达式。

大部分有意的差异是安全限制：

- 每个 tick 有求值预算。
- 行为连续交接的链长有上限。
- 表达式有步数、深度和大小限制。

在 Java 应用会崩溃或无限递归的地方，shimeji2 会提前结束求值：条件按 false 处理，或移除那一只宠物，并记录诊断信息。其余有意差异是上面提到的错误处理选择：非布尔条件和未知动作类。

## 已知问题与限制

以下问题已在 v0.1.0-beta.1 中确认，等待修复：

- **多图片集角色包：** 如果角色包有多个图片集（`img/<名称>/`）且文件名重复，某个角色可能显示另一个图片集的图片和点击区域。放在图片集子文件夹里的姿势可能不显示。
- **多显示器：** 连接多个显示器时，如果键盘焦点切换到另一块屏幕上的窗口，宠物可能突然跳动或掉落。
- **宠物大小：** 大小不是 100% 时，宠物会略微浮在地面上方或陷到下方，边缘也可能被窗口裁掉。
- **动作声音：** 声音会在整个姿势期间反复播放。Shimeji-ee 的 `Volume` 值被理解得不同：`0` 和负值会静音。角色包混用单声道和立体声 WAV，或切换音频输出设备时，应用可能崩溃。暂时请保持 **播放动作声音** 关闭。
- **恶作剧复原：** 如果在搬窗口时撤销了辅助功能权限，或退出时被搬的窗口处于最小化或在另一个桌面空间，该窗口不会复原。
- **导入：** 拖入文件名以中文、日文等非 ASCII 字符结尾的非 ZIP 文件（例如 `写真`）会导致应用崩溃。以 `shime` 为名导入的角色包，会在下次启动时被自带的 shime 替换。
- **宠物数量上限：** 请用半角数字输入。输入法输入的全角数字会把上限设为 1。
- **智能体钩子：** 安装后会让 Claude Code 无法使用，见[智能体任务反应](#智能体任务反应)。

其他限制：

- **测试范围：** 目前只在维护者的 Apple Silicon Mac（macOS 15）上测试过。以下情况尚未验证：
  - 在其他 Mac 上首次打开
  - 多显示器
  - 休眠与唤醒
  - 长时间运行

  见 [`docs/testing/public-beta-checklist.md`](docs/testing/public-beta-checklist.md) 和 [`docs/release.md`](docs/release.md)。
- **Intel：** 可以从源码构建通用版本，但未经测试。
- **暂不可用的设置：** “角色变换”设置和 HQX 缩放方式会显示出来，但暂不可用。

## 参与开发

### 从源码构建

Rust 工具链由 `rust-toolchain.toml` 固定。普通构建和测试不需要 Java 或 Gradle。

```sh
cargo build --workspace
cargo run -p shimeji-macos -- --profile /tmp/shimeji2-dev   # 仅限 macOS
```

`cargo run` 会从 `assets/packs/shime` 加载 shime。不加 `--profile` 时，它与已安装的应用共用 `~/Library/Application Support/shimeji2` 文件夹。

提交改动前，以下三项必须全部通过：

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
```

在 Mac 上构建 `.app`、DMG 和 ZIP（需要 Xcode Command Line Tools）：

```sh
./scripts/bundle.sh --dev      # 依赖缓存好之后可加 --offline
```

- **输出：** 产物在 `target/bundle/`。
- **`--dev`：** 给构建使用独立的 bundle id（`ai.shimeji2.macos.dev`），因此有自己的辅助功能授权条目。只有正式发布时才去掉它。
- **架构：** 同时安装了 `aarch64-apple-darwin` 和 `x86_64-apple-darwin` 时，脚本会构建通用版本；否则只构建本机架构。
- **发布：** 完整的发布流程见 [`docs/release.md`](docs/release.md)。

想在不影响日常配置的情况下试用打包好的版本，请先退出日常使用的那份，再给试用版一个临时配置目录和智能体目录：

```sh
open -W -n target/bundle/shimeji2.app --args --seconds 30 \
  --profile /tmp/shimeji2-try --agent-home /tmp/shimeji2-try/agent-home
```

`--seconds N` 会让它到时自动退出；不加时，应用会一直运行到你选择 Quit。无论使用哪个配置目录，安装智能体钩子都会修改你真实的 `~/.claude` 和 `~/.codex` 配置。

### 项目结构

| Crate | 作用 |
|---|---|
| `shimeji-expr` | 沙箱化的表达式求值器和逐位一致的 `JavaRandom`。库本身没有依赖。 |
| `shimeji-core` | 角色包模型、编译器和行为引擎。 |
| `shimeji-packs` | 角色包加载、安全导入和导出。只使用纯 Rust 解码器。 |
| `shimeji-app` | 设置、角色库、窗口规则、恶作剧日志、智能体状态、固定步长时钟。 |
| `shimeji-macos` | 应用本体：AppKit 窗口、菜单、设置和输入。唯一接触 Apple 框架的 crate。 |
| `shimeji-proto` | 带版本号的本机套接字消息。 |
| `shimeji-hook` | 编程智能体在每个事件上调用的小程序。 |
| `shimeji-forge` | 角色生成命令行工具。 |

除 `shimeji-macos` 外，每个 crate 都是可移植的 Rust，可在任何系统上构建和测试。依赖只朝一个方向：`expr → core → packs → app → macos`。

引擎以 40 ms 的 tick（每秒 25 次）计时。macOS 外壳只提供时间戳，`shimeji-app` 中的固定步长时钟把它们换算成 tick，因此不需要 AppKit 就能测试。

完整规则见 [`AGENTS.md`](AGENTS.md)。简单来说：

- 不要提交第三方角色素材或语料数据。
- 保持自带 shime 逐字节不变。`cargo test` 会对照 `assets/packs/shime/CHECKSUMS.sha256` 检查它的 46 张 PNG 和两个 XML 文件。
- 引擎行为有疑问时，以固定版本的参考源码为准。用 `tools/ee-reference/fetch.sh` 把它取到 `tools/ee-reference/vendor/`。

## 许可与致谢

shimeji2 采用 [zlib/libpng 许可](LICENSE)。它是 Shimeji 的**修改版本**，不是原始软件。请不要把它当作 Shimeji、Shimeji for Mac 或 shimeji4mac 发布或介绍。

- Shimeji © 2009–2011 group-finity（山田祐樹 Yuki Yamada）
- Shimeji for Mac © 2011–2012 Yusaku Hashimoto
- shimeji4mac © 2016 AlanJager
- 行为参考：[Shimeji-Desktop](https://github.com/DalekCraft2/Shimeji-Desktop)，基于 Shimeji-ee（© Shimeji-ee Group；Kilkakon）。其许可见 [`tools/ee-reference/UPSTREAM-LICENSE.txt`](tools/ee-reference/UPSTREAM-LICENSE.txt)。

其他说明：

- **自带的 shime：** 使用未经修改的上游素材。上游许可文本只说明了“本软件”，没有说明是否包括图画本身；[`assets/packs/shime/ATTRIBUTION.md`](assets/packs/shime/ATTRIBUTION.md) 记录了这一不确定性。
- **应用图标：** 本项目借助 AI 制作的原创作品，见 [`packaging/icons/README.md`](packaging/icons/README.md)。
- **Rust 依赖：** 应用和钩子使用的 65 个 Rust 依赖的声明见 [`packaging/THIRD-PARTY-NOTICES.md`](packaging/THIRD-PARTY-NOTICES.md)。

另见 [`NOTICE.md`](NOTICE.md)。

## 反馈

请在 [GitHub](https://github.com/AlanJager/shimeji2/issues) 提交 issue，并附上：

- 下载的版本（例如 v0.1.0-beta.1；访达的 **显示简介** 中显示为 0.1.0）
- Mac 芯片和 macOS 版本
- 角色包的来源
- 复现步骤
- 问题发生在桌面上还是某个应用窗口上

截图或录屏会很有帮助。请不要附上个人配置文件或私密窗口内容。
