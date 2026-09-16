# MP14Tools

一个单进程、低占用的 Windows 小工具，只做两件事：

1. **触摸板重按 → 任意按键 / 鼠标键 / 组合键**
2. **OEM（厂商）热键 → 任意按键 / 鼠标键 / 组合键**

它是 `Meow-Box-main\src\MeowBox.*`（WinUI 3 双进程版本）的精简重构，只有一个进程、没有服务、没有安装包，
配置就是一个 JSON 文件。

---

## 功能

| 功能 | 说明 |
|---|---|
| 触摸板重按检测 | 读 Raw HID（UsagePage `0x0D` / Usage `0x05`），解码压力值；压力超过**轻按阈值**才开始跟踪，两次跨过**重按阈值**触发；两级阈值都可在界面上改，重载即生效 |
| 震动力度调整 | 通过触摸板私有 HID（`0x0D` 报告，33 字节定长帧）写入**轻触反馈**与**重按反馈**两级力度；取值范围与刻度取自固件实际使用的值（`0–128`，步进 `8`），默认就是出厂值，**默认不启用** |
| OEM 热键捕获 | 订阅 `ROOT\WMI` 的 `HID_EVENT20…23` 事件，按报告前缀匹配 |
| 显示调节 | 电池供电时把刷新率降到目标档（默认 60 Hz，取该值以下最高的可用档）；**可选**直接切换或先弹通知等待点击；切回交流电时自动恢复原来的档位 |
| HDR 检测 | 切换电源 / 刷新率时检查 HDR；若开启，通知的第二个按钮就是「关闭 HDR」。电池模式下每次唤醒也会再检查一次并弹出关闭按钮 |
| 内屏 / 外屏开关 | 两个独立开关，决定上面两项是否作用于笔记本自带屏 / 外接显示器 |
| 输出 | `SendInput` 发送键盘按键、鼠标左/右/中/侧键，可带 Ctrl/Shift/Alt/Win 修饰键 |
| 托盘图标 | 打开设置 / 暂停映射 / 命令提示符 / 开机自启 / 退出 |
| 应用图标 | 圆角方块 `#b4c6da` + 中心白点，**同一份设计**用于托盘图标、窗口/任务栏图标和 exe 文件图标（`assets/mp14tools.ico` 由 `tools/make-icon.ps1` 生成，`build.rs` 用 windres 嵌入） |
| OSD 提示 | 触发时在屏幕底部显示圆角提示条，淡出后销毁，不抢焦点、不挡点击 |
| 通知窗口 | 需要用户决定时（切刷新率、关 HDR、震动写入异常）弹出的置顶小窗，最多两个按钮，25 秒无操作自动收起 |
| 命令提示符 | 可选：随程序一起弹出控制台窗口，实时显示与日志文件相同的内容；默认关闭，可在「日志」页或托盘菜单随时开关 |
| 日志位置 | 日志文件所在目录可改（「日志」页，支持目录选择器），留空即 `%LOCALAPPDATA%\MP14Tools`；改动立即生效，无需重启 |
| 外部改动提示 | 配置被其他程序或手动修改时按文件重新加载并**不覆盖**，同时在界面标出；触摸板私有 HID 若被其他程序接管（设备不再响应）则暂停写入并在界面提示 |
| 配置热重载 | 直接编辑 `config.json` 保存即生效，无需重启 |
| 开机自启 | 单个 `HKCU\...\Run` 值，关掉开关即删除 |

除了“震动力度调整”——而且它默认关闭、写入的也只是两个强度字节——本工具**不做**其他触摸板私有
HID 写入，也**不做**除了按键/鼠标以外的动作类型（原版的音量、亮度、性能模式等不在精简范围内）。

> 震动力度写入走的是触摸板厂商私有协议，属于“尽力而为”：找不到私有 HID 集合时只会在日志里说明原因
> （并列出实际看到的 HID 设备路径），不会影响重按检测与热键映射。
>
> 显示调节与 HDR **默认全部关闭**，需要在「显示」页勾选「启用显示调节」才会动作。

---

## 运行要求

- Windows 10 1809+ / Windows 11（x64）
- 无需管理员权限
- 无需安装运行时（微软自家 CRT 之外无外部依赖）

## 构建

前置：Rust 工具链（MSVC 或 GNU 均可）。

### 1. 安装工具链（本机为 GNU 方案，无需管理员）

```powershell
# 安装 rustup（用户目录，不改 PATH）
$dir = "$env:TEMP\rustup-install"; New-Item -ItemType Directory -Force $dir | Out-Null
Invoke-WebRequest 'https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe' -OutFile "$dir\rustup-init.exe"
& "$dir\rustup-init.exe" -y --no-modify-path --profile minimal --default-host x86_64-pc-windows-gnu --default-toolchain none
& "$env:USERPROFILE\.cargo\bin\rustup.exe" toolchain install stable --profile minimal
& "$env:USERPROFILE\.cargo\bin\rustup.exe" default stable-x86_64-pc-windows-gnu
```

### 2. 安装 dlltool/as 工具对（**必需**）

```powershell
.\tools\install-mingw.ps1
```

<details>
<summary>为什么需要这一步</summary>

`eframe`/`egui` 依赖 `windows` 系 crate，其中 `windows-link` 在 Windows 上**无条件**使用
`#[link(kind = "raw-dylib")]`。GNU 目标下 rustc 要生成这些导入库，就必须调用 `dlltool`，
而 `dlltool` 需要一个 GNU 汇编器 `as.exe`——rustup 的 GNU 工具链**不提供**
（它自带的 gcc 只能当链接器用，见 `GCC-WARNING.txt`）。

更麻烦的是：`dlltool` 解析汇编器时**不走 PATH**，而是对裸名字 `as` 调 `CreateProcess`，
只会在 dlltool 自己所在目录里找。所以 `as.exe` 必须和 `dlltool.exe` 放在同一个目录。

该脚本会下载 WinLibs MinGW-w64，只抽出这两个工具 + 它们需要的 DLL（约 6MB）到
`build\toolchain\assembler`，并验证它确实能生成导入库。
所有产物都落在项目目录内，不向系统注册任何东西，删掉 `build` 目录即完全卸载。

> 如果使用 MSVC 工具链（`stable-x86_64-pc-windows-msvc` + VS Build Tools），
> 这一步完全不需要，`cargo build` 直接可用。

</details>

### 3. 构建

```powershell
.\build.ps1              # debug 构建
.\build.ps1 -Release     # release 构建（体积更小）
.\build.ps1 -Run         # 构建并运行
.\build.ps1 -Check       # 仅类型检查
```

如果本机执行策略禁止运行脚本（`无法加载文件 build.ps1，因为在此系统上禁止运行脚本`），
用同参数的批处理入口即可，它只在这一次进程里放宽策略，不改任何全局设置：

```cmd
build.cmd
build.cmd -Release
```

两个入口在构建前都会先结束**正在运行且路径等于本次产物**的 mp14tools 实例：
Windows 不允许覆盖运行中的映像，否则 cargo 会在链接阶段报 `Access denied`。
若不想让它结束进程（例如你刻意留着旧实例），加 `-KeepRunning`。

产物：`build\obj\debug\mp14tools.exe` 或 `build\obj\release\mp14tools.exe`。

### 4. 图标（可选）

`assets\mp14tools.ico` 是**已生成好**的，正常构建不需要这一步。改了 `src\icon.rs` 里的颜色或比例后
需要重新生成并让 `build.rs` 重新嵌入：

```powershell
.\tools\make-icon.ps1      # 按 src\icon.rs 里同一组数字写出 7 种尺寸的 .ico
.\build.cmd -Release       # build.rs 用 windres 编译 assets\mp14tools.rc
```

`windres` 取自 `build\toolchain\mingw64\bin`（与 dlltool/as 同一套），找不到时构建只 warn，
产物只是没有文件和版本图标，窗口与托盘图标不受影响（那两个是运行时画出来的）。

### 目录布局

所有生成物都在 `build\` 下（由 `.cargo\config.toml` 把 cargo 的 target 目录重定向到 `build\obj`）：

```
mp14tools\
├─ src\            源码（工程文件）
├─ tools\          构建/诊断/测量脚本（工程文件）
├─ .cargo\         cargo 配置（工程文件）
├─ build\          全部生成物，可整个删除
│  ├─ obj\         cargo target：编译中间文件 + exe
│  ├─ toolchain\   dlltool/as 工具对、MinGW-w64、下载缓存
│  └─ temp\        构建过程中产生的临时与诊断文件
├─ README.md       （工程文件）
├─ REPORT.md       （工程文件）
└─ BUILD-REPORT.md （本文档的补充：本次整理做了什么）
```

---

## 使用

1. 运行 `mp14tools.exe`，设置窗口打开。
2. **触摸板**页：勾选启用，按住触摸板观察“当前压力”读数（带“轻按已触发 / 重按已触发”状态），
   据此设定轻按与重按阈值：滑条每格 25，也可以在数值框里直接输入精确值，按对应「确定」后生效，
   右侧会显示“已生效：xxx”。轻按上限 500，重加上限 1000，且重按必须高出轻按至少一格。
   然后选目标按键（例如 `Ctrl` + `鼠标中键`）。
3. **OEM 按键**页：为每个厂商热键选目标按键。`报告前缀` 决定匹配哪个键。
4. 关闭窗口程序仍在托盘运行；托盘菜单可暂停映射或退出。

### 配置文件

`%LOCALAPPDATA%\MP14Tools\config.json`

```jsonc
{
  "version": 1,
  "start_with_windows": false,
  "show_tray_icon": true,
  "osd": { "enabled": true, "duration_ms": 900 },
  "touchpad": {
    "enabled": true,
    "light_press_threshold": 125,   // 达到它才算“有意按压”，25 的倍数，最大 500
    "deep_press_threshold": 500,    // 达到它并保持两帧才算重按，25 的倍数，最大 1000
    "action": { "modifiers": ["ctrl"], "target": "MouseMiddle" }
  },
  "oem_keys": [
    {
      "name": "Performance mode (Fn+K)",
      "enabled": true,
      "report_hex": "01-28-01",     // 前缀匹配，大小写和连字符都无所谓
      "press_only": true,           // 只在按下时触发，忽略抬起
      "action": { "modifiers": [], "target": "KeyP" }
    }
  ]
}
```

- `target` 取值见设置界面的下拉列表，鼠标键是 `MouseLeft` / `MouseRight` / `MouseMiddle` /
  `MouseX1` / `MouseX2`，键盘键是 `KeyA`…`KeyZ`、`Digit0`…、`F1`…`F24`、媒体键等。
- `modifiers` 取值为 `ctrl` / `shift` / `alt` / `win`。
- 手工改完保存即生效（程序每 1.5 秒比对一次文件内容）。
- 手工写进去的阈值会被自动归整：轻按 `25–500`、重按 `轻按+25–1000`，并对齐到 25 的倍数。
- 旧版本（MeowBox Lite）留下的 `%LOCALAPPDATA%\MeowBoxLite\config.json` 会在首次启动时自动搬到新目录，
  并删掉旧的开机自启项。

### 日志

`%LOCALAPPDATA%\MP14Tools\mp14tools.log`，记录订阅状态、热重载和触发结果。

### 辅助脚本

| 脚本 | 用途 |
|---|---|
| `tools\install-mingw.ps1` | 安装构建所需的 `dlltool`/`as` 工具对（仅 GNU 工具链需要） |
| `tools\diagnose.ps1` | 检查工具链是否齐备、应用控制策略状态 |
| `tools\measure.ps1` | 启动程序并测量内存/线程/句柄/空闲 CPU |

---

## 已知限制

- OEM 热键的可用性取决于机型：报告前缀是**参考机型（Xiaomi Book Pro 14 2026）**的值，
  其它机型需要在 `config.json` 里改成自己的前缀。
- 只能重映射走 WMI HID 事件上报的厂商键，普通键盘按键不能作为触发源（没有全局键盘钩子）。
- OSD 文本使用 Microsoft YaHei UI；非中文 Windows 上如果该字体缺失，中文会退化为方框。
- 程序未签名，首次运行可能触发 SmartScreen 提示。
- 显示调节只做**刷新率**与 **HDR**：分辨率不变；电池档取「不高于目标值（默认 60 Hz）的最高可用档」。
  切回交流电时恢复的是「本工具切换前记录的档位」，因此刚启动就插着电时不会有任何动作。
- HDR 的开关走 DisplayConfig 的高级颜色接口，需要驱动支持；驱动不认这个查询时按「不支持 HDR」处理，
  不会报错。
- **震动强度无法读回**：私有 HID 协议只有写路径（应答帧里不含当前设置，已实测确认），
  所以「别的软件静默改了硬件力度」这件事本工具**检测不到**。能检测的是**设备被接管/不再应答**，
  以及**配置文件被外部修改**——这两种都会暂停写入、弹通知并在界面标出。
- 压力阈值与震动力度都是本地配置值：本工具只把它们写进触摸板（力度）或用于自身判定（阈值），
  不读回硬件状态。
- 本机（16 核）实测空闲占用约 **8% 单核**（相当于整机 0.5%），主要来自设置窗口打开时的重绘与
  触摸板原始输入流；托盘最小化、窗口关闭后应更低。用 `tools\measure.ps1` 可复测。
- 震动力度写入依赖触摸板厂商的私有 HID 集合。默认按 `hid#bltp7853&col05` 匹配接口路径
  （参考机型 Xiaomi Book Pro 14 2026）；其它机型如果匹配不上，日志会列出实际枚举到的
  HID 设备路径，把 `config.json` 里 `haptics.device_marker` 改成自己的即可。
- 震动力度的上下限 `0–128` 与步进 `8` 来自固件实际使用的值：写入时每个力度占一个字节，
  厂商自带的设置程序把两个值都封顶在 `128`，它写过的值（`0 / 56 / 80 / 104 / 128`）均为 `8` 的倍数。
  如果固件接受不到边界值，把 `config.json` 里的两个数值改小即可。
- 命令提示符是**本进程自己**的控制台窗口：关闭它会一并关闭该窗口（日志文件不受影响）。
  从终端里启动的 debug 版本本来就有控制台，此时这个开关不会重复开窗。
