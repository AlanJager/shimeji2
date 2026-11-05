shimeji2 — Windows 基础体验版（x64）

面向 Windows 10 1607+ / Windows 11 的 Intel、AMD 64 位电脑。
这是开发预览包；尚未完成 Windows 真机交互验收。暂不承诺 ARM64 支持。

1. 把整个 ZIP 解压到普通文件夹，保留 shimeji2.exe 与 packs 文件夹在一起。
2. 双击 shimeji2.exe，默认出现白色 shime。无需安装 Java 或 Rust。
3. 左键按住桌宠拖动，松开抛出；右键桌宠或任务栏通知区域图标打开菜单。
4. 菜单提供添加、暂停、隐藏、找回、移除和退出。
   Ctrl+Shift+S 切换显示/隐藏（如果没有被其他软件占用）。
   退出请用托盘菜单；隐藏桌宠不会退出程序。
5. 打开自己的角色：把 ZIP 或角色文件夹拖到 shimeji2.exe 上，
   或在托盘菜单选“打开角色 ZIP…”，或把 ZIP 拖到桌宠身上。
   打开的角色会替换当前角色，本次选择不会持久保存。

高级用法（终端中）：
  .\shimeji2.exe --pack "D:\角色\my-pet.zip"
  .\shimeji2.exe --pack "D:\角色\multi-pack.zip" --character "角色名"
  .\shimeji2.exe --scale 1.5
  .\shimeji2.exe --seconds 30

预览版范围：
- 共享 Rust 行为引擎、透明置顶桌宠、桌面/前台窗口边缘、拖动和抛出。
- 支持现有加载器能识别的 ZIP/文件夹；多角色包默认使用第一个角色。
- 默认关闭繁殖，最多手动添加 20 只；声音、窗口搬运、Agent hooks、
  原生设置窗口、自动更新和开机启动尚未接入。
- 桌宠使用物理像素；高 DPI 电脑可通过 --scale 调整。
  混合 DPI、多显示器切换、休眠恢复和长时间运行仍待真机测试。
- 尚未进行 Windows 代码签名，下载后可能出现系统信誉提示。
  请核对来源；本包不要求管理员权限或修改系统安全设置。
- 出错会弹窗显示原因。反馈时请带上 Windows 版本、缩放比例、角色包名称
  以及复现步骤。角色消失时先尝试托盘菜单“找回桌宠”。

仅分发默认白色 shime；其他角色请自行核对作者和素材许可。
许可见 LICENSE、NOTICE.md、THIRD-PARTY-NOTICES.md、
WINDOWS-THIRD-PARTY-NOTICES.md 与 packs/shime/ATTRIBUTION.md。

番茄钟：右键托盘图标，选择“开始专注”或“番茄钟设置…”。
默认专注 25 分钟、短休息 5 分钟，每四轮提供 15 分钟长休息。
下一阶段手动开始。锁屏、休眠后暂停，回来后手动继续。
偏好保存在 %LOCALAPPDATA%\shimeji2；可用 --profile PATH 选择独立配置目录。
计时与桌宠暂停/隐藏独立；专注时桌宠安静待着，仍可拖动。
系统可能抑制通知；完成状态仍在托盘菜单和设置中显示，提示音默认关闭。
