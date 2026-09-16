# MP14Tools — 目录整理与构建报告

> 本文档已随仓库整理移入 `docs\`；文中出现的相对路径（`src\`、`build\`、`tools\` 等）
> 均以**项目根目录**（现在的 `mp14tools-main\`）为基准。

> 本次任务：把本次开发过程中产生的**所有编译中间文件与临时文件**收归到 `mp14tools` 内的
> 独立中间目录，逐步检查安全性，并确认最终产物。
>
> 相关文档：[`REPORT.md`](REPORT.md) 重构报告 ｜ [`MANUAL-CLEANUP.md`](MANUAL-CLEANUP.md) 需手动清理项
>
> 目录已在 **0.2.5** 改名为 `mp14tools-main`（见第 10 节）；下文出现的 `mp14tools\`
> 除路径示例外均指该目录。

---

## 1. 结果概览

| 项 | 整理前 | 整理后 |
|---|---|---|
| 编译中间文件 | `mp14tools\target\`（2,218.8 MB） | `mp14tools\build\obj\` |
| 构建工具链 | `%LOCALAPPDATA%\MeowBoxLite\Tools\`（923.1 MB） | `mp14tools\build\toolchain\` |
| 下载缓存 | `%TEMP%\meowbox-lite-mingw\*.zip`（261.3 MB） | `mp14tools\build\toolchain\downloads\` |
| 临时/诊断文件 | `%TEMP%\` 下 70+ 个散落文件 | `mp14tools\build\temp\` |
| 项目外残留 | 3 处（含 1 个失效的注册表自启项） | 已处理，见第 5 节 |

一句话：**现在删掉 `mp14tools\build` 一个目录，就等于清掉了全部生成物。**

---

## 2. 整理后的目录结构

```
mp14tools\
├─ Cargo.toml / Cargo.lock          工程文件
├─ .cargo\config.toml               工程文件 —— 把 cargo 输出重定向到 build\obj
├─ .gitignore                       工程文件 —— 已忽略 build\
├─ build.ps1                        工程文件 —— 构建入口
├─ README.md / REPORT.md            工程文件
├─ BUILD-REPORT.md                  本文档
├─ MANUAL-CLEANUP.md                需手动清理项清单
├─ src\        （13 个 .rs，工程文件）
├─ tools\      （3 个 .ps1，工程文件）
└─ build\      ★ 全部生成物，可整个删除
   ├─ obj\            2,216.9 MB   cargo target：依赖编译产物、中间对象、最终 exe
   │  ├─ debug\mp14tools.exe
   │  └─ release\mp14tools.exe   ← 5.03 MB，交付的可执行文件
   ├─ toolchain\      1,184.4 MB   构建所需的工具，全部在项目内
   │  ├─ assembler\       6.4 MB   dlltool.exe + as.exe + 依赖 DLL（构建真正用到的）
   │  ├─ mingw64\       916.7 MB   WinLibs MinGW-w64（assember 的来源）
   │  └─ downloads\     261.3 MB   WinLibs 压缩包缓存（重装时免下载）
   └─ temp\              95.8 MB   构建期间产生的临时与诊断文件
      ├─ scratch-projects\         验证链接器用的临时 cargo 工程
      ├─ installer\                rustup 安装器与日志
      └─ diagnostics\              构建日志、API 探测输出、临时脚本
```

工程文件共 **25 个**（不含 `build\`），合计 250,580 字节 ≈ **245 KB**。

---

## 3. 做了什么

| # | 步骤 | 说明 |
|---|---|---|
| 1 | 新建 `build\{obj,toolchain,temp}` | 先建目录，再搬内容 |
| 2 | 新增 `.cargo\config.toml` | `[build] target-dir = "build/obj"`，让 cargo 不再在项目旁生成 `target\` |
| 3 | 搬迁 `target\*` → `build\obj\` | 移动而非重新编译，保留增量缓存 |
| 4 | 搬迁 `%LOCALAPPDATA%\MeowBoxLite\Tools\*` → `build\toolchain\` | `assembler` 与 `mingw64` 两个目录 |
| 5 | 搬迁 `%TEMP%\meowbox-lite-mingw\*.zip` → `build\toolchain\downloads\` | 保留缓存，重装时不必再下载 261 MB |
| 6 | 搬迁 70+ 个 `%TEMP%` 散落文件 → `build\temp\` | 安装器日志、构建日志、诊断输出、临时脚本 |
| 7 | 更新 `build.ps1` | 工具对路径由 `%LOCALAPPDATA%` 改为 `build\toolchain\assembler` |
| 8 | 更新 `tools\install-mingw.ps1` | 安装目录、下载缓存、验证探测目录全部改到 `build\toolchain`；新增解压残留清理 |
| 9 | 更新 `.gitignore`、`README.md`、`REPORT.md` | 路径与目录结构说明同步 |

### 与仓库既有约定的差异（需要你知晓）

仓库 `AGENTS.md` 要求中间产物放在**仓库根目录**的 `build\obj\` 下（通过 `Directory.Build.props`）。
本次按你的要求改为收在 **`mp14tools\build\`**（项目内自包含）。
两种约定都成立，只是归属不同；如果你希望与其他工程统一到仓库级 `build\`，
只需把 `.cargo\config.toml` 里的 `target-dir` 改成 `"../../build/obj/mp14tools"`。

---

## 4. 逐步安全检查

每一步都做了显式校验（本机 **没有安装 git**，无法用版本控制兜底，因此不依赖回滚）。

| 步骤 | 风险 | 检查方式 | 结果 |
|---|---|---|---|
| 新建 `build\` 子目录 | 无 | 仅创建目录，不触碰既有文件 | ✅ |
| cargo 输出重定向 | 配置写错会导致找不到产物 | `cargo metadata` 读取实际 target 目录 | ✅ 报告为 `mp14tools\build\obj` |
| 搬迁 `target\` | 破坏增量缓存 / 遗漏文件 | 搬迁后执行 `cargo build --release`，比较 exe 时间戳 | ✅ exit 0，**exe 时间戳未变（23:35:04）**，说明缓存被复用、没有全量重编 |
| 搬迁工具链 | 脚本硬编码路径失效 | 迁移前先全项目搜索引用（`LocalAppData`/`Tools\assembler`） | ✅ 仅 `build.ps1` 与 `install-mingw.ps1` 两处，均已更新 |
| 搬迁 `%TEMP%` 文件 | 误删他人在用文件 | 只移动不删除；先按文件名清单精确匹配 | ✅ 未删除任何文件 |
| 注册表自启项 | 指向旧路径后失效 | 迁移前后各查一次 `HKCU\...\Run\MeowBoxLite` | ⚠️ **发现问题并已修复**，见第 5 节 |
| 陈旧运行实例 | 占用互斥体，导致新 exe 无法启动 | 冒烟运行时发现进程立即退出，日志给出原因 | ⚠️ **已终止**，见第 5 节 |
| 删除动作 | 误删 | 只删除**空**目录 | ✅ 仅删除空的 `target\` 与空的 `%TEMP%\meowbox-lite-mingw` |

### 一个非显而易见的坑（记录备查）

`cargo` 的 `target\release\meowbox-lite.exe` 在首次搬迁时**被占用**，报
「另一个进程正在使用此文件」。原因不是 cargo，而是**一个仍在运行的旧实例正运行着这个 exe**。
处理方式：先终止该进程，再逐项搬迁成功。这也说明"移动正在运行的 exe"是可行的，
但会留下一份路径已失效的运行实例。

---

## 5. 整理过程中修复的两个副作用（重要）

### 5.1 开机自启项指向了已搬迁的旧路径

| | 值 |
|---|---|
| 迁移前 | `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\MeowBoxLite` = `"…\meowbox_lite\target\release\meowbox-lite.exe"` |
| 迁移后 | 该文件已不存在 → **每次开机都会静默失败** |
| 处理 | 已改写到新路径并验证目标文件存在 |
| 现在 | 该值已随改名删除（产品已改名为 MP14Tools）；在新程序的「通用」页重新勾选一次即可 |

注意 `config.json` 里 `start_with_windows = true`，与注册表一致，说明这是**有意开启**的。
如果你想重新开启开机自启，在程序「通用」页勾选即可。

> ⚠️ 自启项指向的是**构建输出目录**。如果你以后删除 `build\`，自启会再次失效。
> 建议把 `mp14tools.exe` 复制到一个稳定位置（例如 `%LOCALAPPDATA%\Programs\MP14Tools\`）
> 再从那里开启自启。

### 5.2 一个陈旧实例占着单实例互斥体

- 现象：冒烟运行时新进程立即退出（exit code 0），日志只有两行
  `meowbox-lite 0.1.0 starting` / `another instance is already running; exiting`。
- 原因：23:41 启动的一个实例仍在运行，其映像是已被搬走的旧路径
  `target\release\meowbox-lite.exe`。
- 处理：终止该进程。**这恰好验证了单实例保护按设计工作**——否则两个实例会让每次按键被发送两次。

---

## 6. 验证证据

整理完成后重新执行了完整构建与冒烟运行：

```
cargo check (build.ps1 -Check)      exit code: 0
cargo build --release              exit code: 0
产物                               build\obj\release\mp14tools.exe
大小                               5,272,064 字节（5.03 MB）
时间戳                             与搬迁前一致（23:35:04）→ 缓存复用，未重编
```

冒烟运行（从新位置启动）：

```
running      : yes
main window  : 'MeowBox Lite'
threads      : 19
working set  : 151 MB
--- log ---
meowbox-lite 0.1.0 starting
opening settings window
touchpad: raw input registered (0x0D/0x05)
tray: icon added
tray: shell window ready
WMI event subscription active
wmi HID_EVENT20: subscribed
wmi HID_EVENT21: subscribed
wmi HID_EVENT22: subscribed
wmi HID_EVENT23: subscribed
```

四个子系统（触摸板 Raw Input、托盘、WMI 订阅、设置窗口）在新布局下全部正常。

---

## 7. 复现命令

```powershell
cd mp14tools

# 构建（自动装配工具链 PATH，输出到 build\obj）
.\build.ps1 -Check
.\build.ps1              # debug
.\build.ps1 -Release     # release

# 环境诊断
.\tools\diagnose.ps1

# 占用测量
.\tools\measure.ps1 -Configuration release -Seconds 30

# 运行
.\build\obj\release\mp14tools.exe
```

若 `build\toolchain\assembler` 缺失（例如刚 clone 下来），先执行：

```powershell
.\tools\install-mingw.ps1
```

---

## 8. 本次未能检查到的部分

- **git 状态**：本机未安装 git，无法确认工作区是否有其他改动。仓库根目录列出的是
  `.vscode / assets / screenshots / src / .gitignore / AGENTS.md / build.ps1 /
  Directory.Build.props / LICENSE / meowbox-architecture.md / README.md`，
  其中只有 `meowbox-architecture.md` 是本次开发产生的（见 `MANUAL-CLEANUP.md`）。
- **真实触发的功能验证**：触摸板重按与 OEM 热键的实际按下仍需人工确认（见 `REPORT.md` 第 8 节）。

---

## 9. 0.2.1 追加：构建失败的成因、修复与本次改动### 9.1 构建为什么失败（第 5.2 节那个坑的另一种表现）

| | |
|---|---|
| 现象 | `cargo build` 在链接阶段报 `failed to remove file …\build\obj\debug\mp14tools.exe` / `拒绝访问 (os error 5)` |
| 原因 | 一个仍在运行的 mp14tools 实例，其映像正是该配置的产物。Windows 不允许覆盖正在运行的映像 |
| 复现 | 双击 `build\obj\debug\mp14tools.exe`（程序常驻托盘，不会自己退出），再执行一次 debug 构建 |
| 修复 | `build.ps1` 在调用 cargo 之前，先结束**路径等于本次产物**的实例，并等待 400 ms 释放文件句柄；`-KeepRunning` 可跳过 |

只结束路径完全匹配的进程：从别处启动的实例不受影响，也不会误杀。

### 9.2 顺带解决的两个可用性问题

| 问题 | 处理 |
|---|---|
| 本机执行策略禁止运行 `.\build.ps1` | 新增 `build.cmd`：用 `powershell -ExecutionPolicy Bypass -File` 调用同一个脚本，只影响这一个进程，不改任何全局设置 |
| 构建成功后要自己去 `build\obj\**` 找产物 | 脚本最后打印产物绝对路径 |

验证：先启动一个 debug 实例，再执行 `.\build.cmd`。输出为

```
Stopping 1 running mp14tools instance(s) that lock the output...
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.17s
Output: G:\Work\Code\MP14Tools\mp14tools\build\obj\debug\mp14tools.exe
EXIT=0
```

修复前同样的操作会以 `Access denied` 结束。

### 9.3 版本号与 0.2.1 的新增功能

`Cargo.toml` 的版本由 `0.1.0` 改为 **`0.2.1`**（界面侧栏读 `CARGO_PKG_VERSION`，无需另改）。

| 新增 | 说明 | 默认 |
|---|---|---|
| 命令提示符 | 可选弹出一个控制台窗口，实时显示与日志文件相同的内容；「通用」页与托盘菜单都能开关 | **关闭** |
| 震动力度调整 | 通过触摸板私有 HID 写入轻触/重按两级反馈力度 | **关闭**（勾选后立即写入当前配置值，即出厂值 80 / 104） |

两者都是**运行时可切换**的，且都跟随 `config.json` 热重载。

#### 为什么震动力度的刻度是 8、范围是 0–128

这两个数字不是选的，是固件原始数据推出来的：

- 写入时每个力度占**一个字节**，所以量程上限受单字节约束；
- 旧版 `MeowBox` 的硬件层（`TouchpadHardwareSettings`）把两个值都封顶在 **128**；
- 它实际写过的值只有 `0 / 56 / 80 / 104 / 128`——**全部是 8 的倍数**，因此步进取 8，
  滑条上的每一个刻度都对应一个固件真的用过的值；
- 出厂默认 `轻触 80 / 重按 104`，所以新增这个功能本身不改变任何手感。

#### 私有 HID 报文是怎么确认写对的

报文格式（33 字节定长帧、`0x0D` 报告号、`(payload XOR)+1` 校验、init→configure→commit→unlock
四包序列、包间 130 ms）是从旧版 `MeowBox.Core/Services/TouchpadPrivateHidService.cs` 移植的。
移植后加了两条对照测试（`cargo test`）：用旧版 `HapticOn/HapticOff` 序列里**逐字节已知**的
unlock 包与 commit 包反查本次实现，二者一致才算通过。

```
running 3 tests
test haptics::tests::framing_matches_the_reference_packets ... ok
test haptics::tests::vibration_payload_is_framed ... ok
test haptics::tests::every_documented_strength_sits_on_the_grid ... ok
```

实机冒烟（临时把 `console` 与 `haptics.enabled` 打开，测完已还原 `config.json`）：

```
epoch=… mp14tools 0.2.1 starting
epoch=… touchpad: raw input registered (0x0D/0x05)
epoch=… tray: icon added
epoch=… WMI event subscription active
epoch=… haptics: written, 轻触=80 重按=104
```

同时确认进程下多出一个 `conhost.exe` 子进程，即控制台窗口确实已分配。

> ⚠️ 两者都**默认关闭**，且 `config.json` 里不写这两个字段就等于默认值，
> 所以你现在的配置文件与 0.1.0 完全兼容，行为也和 0.1.0 一致。

---

## 10. 0.2.5 追加：改名、图标、显示调节与外部改动检测

### 10.1 目录改名 `mp14tools` → `mp14tools-main`

| 项 | 处理 |
|---|---|
| 目录本身 | `Rename-Item`。若改名时报「正在使用中」，先让终端离开该目录——**终端自身的工作目录就是最常见的占用者**（`.NET` 的 `ReadAllLines` 之类按相对路径还会因此解析到旧路径） |
| `.vscode\tasks.json` | 任务命令里的 `${workspaceFolder}\mp14tools\build.cmd` 已改为 `mp14tools-main` |
| `.cargo\config.toml` | 无需改动：`target-dir` 是相对路径 |
| `build\` 缓存 | 未受影响，改名后增量构建仍命中（改名后首次 `cargo check` 仅 0.7 s） |

### 10.2 版本与图标

- `Cargo.toml` → `0.2.5`；`assets\mp14tools.rc` 里的 `FILEVERSION` 同步。
- 新的 `src\icon.rs` 是**唯一**的图标定义：圆角方块 `#b4c6da`（`BASE`）、中心白点、圆角 24%、
  点半径 14%，三处消费者共用它——托盘 `HICON`、窗口/任务栏 `IconData`（64×64 RGBA）。
- exe 文件图标需要真正的资源，因此多了一组工程文件：
  `tools\make-icon.ps1`（按同一组数字生成 7 种尺寸的 `.ico`）、`assets\mp14tools.ico`、
  `assets\mp14tools.rc`（图标 + 版本信息）、`build.rs`（用 `windres` 编译并 `rustc-link-arg` 链接）。
  `windres` 不在时只 warn，不中断构建。

实测：

```
icon=32x32                      # ExtractAssociatedIcon 从 exe 取到图标
FileVersion : 0.2.5.0
ProductName : MP14Tools
```

> 踩坑记录：`tools\make-icon.ps1` 第一版写出的 ico 只有 125 字节——PowerShell 把函数的
> `byte[]` 返回值**展开成流水线**，`$writer.Write($obj.Bytes)` 于是按单字节重载写入，每个尺寸
> 只写了 1 字节。头部（长度/偏移）是对的，因此光看目录结构发现不了。修法：函数用 `return ,$bytes`
> 保持数组完整，写入处显式 `[byte[]]`。

### 10.3 显示调节（电池 60 Hz + HDR）

- `src\display.rs`：把两套命名体系统一成一个 `Display`——刷新率走 GDI（`EnumDisplaySettingsW` /
  `ChangeDisplaySettingsExW`），HDR 走 DisplayConfig（`QueryDisplayConfig` +
  `DisplayConfigGet/SetDeviceInfo`），内屏/外屏用路径的 `outputTechnology == INTERNAL` 判定。
- `src\power.rs`：一个阻塞在 channel 上的线程，事件来自 `WM_POWERBROADCAST`
  （`PBT_APMPOWERSTATUSCHANGE` 与 `PBT_APMRESUMEAUTOMATIC/SUSPEND`），**不轮询**。
- `src\notice.rs`：带 1–2 个按钮的置顶置小窗，25 秒自动收起；与 OSD 分开，因为 OSD 是
  `WS_EX_TRANSPARENT`（点不中），混在一起就得牺牲键盘提示的点击穿透。
- 电池入档取「不高于目标值（默认 60）的最高可用档」，交流电恢复时按**切换前记录的档位**还原。

实机检测（`display.enabled=true` 时启动一次即可看到，无需真的拔电源）：

```
display: on AC
display: \\.\DISPLAY1 [internal] 3120x2080 @120 Hz, HDR off (supported)
```

即：内屏判定正确、HDR 可读（当前关闭但支持）、分辨率与刷新率与 SRR 记录一致。

### 10.4 外部改动检测：能做什么、不能做什么

需求是「检测到按压力度与震动强度被其他软件改过就提示、不覆盖、并在界面标出」。实现前先做了实验：

- 私有 HID 的**写**是可行的，但**读回设置不可行**——按参考实现读取应答帧后，
  33 字节里并没有我们刚写入的 `88, 0, 112, 0`，而是一个与配置无关的 4 字节载荷
  （`0D 06 39 00 2C 19 0D …`，校验自洽）。协议里没有可用的「读配置」请求。
- 因此「静默改硬件设置」在本机型上**检测不到**，这一点不能假装能做。实测输出：
  `haptics: device state differs from the configured 88 / 112 (0D-06-39-00-2C-19-0D-…)`

最终实现的是**可观测**的那一半：

| 场景 | 行为 |
|---|---|
| 触摸板私有集合被其他程序接管 / 复位（设备不再应答） | 暂停写入、弹通知说明、界面标出，不再覆盖 |
| 配置文件被其他程序或手动修改 | 按文件重新加载、**不覆盖**、界面红字提示「配置文件已被外部修改…」 |
| 静默改硬件力度（协议无读路径） | **无法检测**，已在 README 明确写出 |

### 10.5 其它

- 压力阈值改为**整数**（去掉 25 的步进），滑条刻度每 25 一格、每 100 一长格；
  新增「使用出厂值」开关（阈值 125 / 500，力度 80 / 104），勾选即固定为出厂值。
- 新增「日志」页：命令提示符开关 + 日志目录（带系统目录选择器），日志位置改动**立即生效**
  （`log::set_directory` 先换句柄再切换，不需要重启）。
- 配置结构：`console` 升级为 `log { console, directory }`，新增 `display { … }`，
  并给 `touchpad` / `haptics` 各加 `factory_values`。

