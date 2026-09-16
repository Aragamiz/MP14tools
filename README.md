# MP14Tools

[中文说明](#中文说明) | [English](#english)

为小米book pro 14 2026定制

一个单进程、低占用的 Windows 小工具：重新映射触摸板的重按与厂商（OEM）热键动作，并可调节按压触发力度阈值。
支持自动识别电池供电时，弹窗提示或自动降低刷新率、检查 HDR 状态并弹窗提示。

> 📥 **下载**：[最新版本 Releases](https://github.com/Aragamiz/MP14tools/releases/latest)
> — 单文件 `mp14tools.exe`，免安装，无需管理员权限。

---

## 中文说明

### ✨ 功能

| 功能 | 说明 |
|---|---|
| 触摸板重按映射 | 触摸板重按 → 任意键盘键、鼠标键或组合键。两级压力阈值都是整数，可在界面上拖动或输入精确值，也可一键切到「使用出厂值」（125 / 500） |
| 震动力度调整 | 调整触摸板的**轻触反馈**与**重按反馈**两级力度：范围 `0–128`、步进 `8`，默认即出厂值（80 / 104）。**默认关闭**，不启用时本工具不写触摸板 |
| OEM 热键映射 | 捕获厂商热键并映射成任意键盘键、鼠标键或组合键；每个热键用「报告前缀」决定匹配哪个键 |
| 输出动作 | 键盘按键（字母、数字、功能键、媒体键等）、鼠标左/右/中/侧键，均可带 Ctrl / Shift / Alt / Win 修饰键 |
| 显示调节 | 电池供电时把刷新率切到设定档位：**内屏**可选 60 / 120 Hz，**外屏**可选最高档 / 60 Hz；插回电源恢复原档位。行为可选**不处理 / 通知确认 / 直接切换** |
| HDR 检测 | **只针对内屏**。切电源与切刷新率时检查，若处于开启状态，通知的第二个按钮提供「关闭 HDR」；电池模式下**每次唤醒**也会检查一次 |
| 内屏 / 外屏开关 | 两个独立开关，决定显示调节作用于哪些显示器 |
| 托盘图标 | 打开设置 / 暂停映射 / 命令提示符 / 开机自启 / 退出 |
| OSD 提示 | 触发时在屏幕底部显示圆角提示条，不抢焦点、不挡点击；停留 5 秒后在 2 秒内淡出消失（停留时长可调） |
| 通知窗口 | 需要用户决定时弹出的置顶小窗，文字与按钮居中；最多两个按钮，25 秒无操作自动收起 |
| 命令提示符 | 可选：随程序弹出控制台窗口实时显示日志；默认关闭 |
| 日志位置 | 日志目录可改（带目录选择器），改动立即生效 |
| 配置热重载 | 直接编辑 `config.json` 保存即生效；配置文件被外部修改时按文件加载且**不覆盖**，并在界面标出 |
| 开机自启 | 单个 `HKCU\...\Run` 值，关掉开关即删除 |

### 🧱 运行要求

- Windows 10 1809+ / Windows 11（x64）
- 无需管理员权限
- 无需安装运行时（除系统自带 CRT 外无外部依赖）

### 🖱 使用

1. 运行 `mp14tools.exe`，设置窗口打开（关闭窗口后仍在托盘运行）。
2. **触摸板**页：勾选启用，按住触摸板观察「当前压力」读数与状态，据此设置阈值；
   滑条与数值框都支持精确的整数，按「确定」后生效。勾选「使用出厂值」则固定用 125 / 500。
   再选目标按键。震动力度在同一页，启用后立即写入触摸板（默认关闭）。
3. **OEM 按键**页：为每个厂商热键选目标按键；`报告前缀` 决定匹配哪个键。
4. **显示**页：勾选「启用显示调节」，选择电池时的行为、内屏档位（60 / 120 Hz）、
   外屏档位（最高档 / 60 Hz）、是否检查内屏 HDR，以及内屏/外屏开关。
5. **日志**页：需要时打开命令提示符，或把日志文件换到别的目录。
6. 托盘菜单可暂停映射、打开设置、开命令提示符、开关自启或退出。

### ⚙️ 配置文件

`%LOCALAPPDATA%\MP14Tools\config.json`

```jsonc
{
  "version": 1,
  "log": {
    "console": false,              // 是否随程序弹出命令提示符窗口
    "directory": ""                // 日志目录，留空 = %LOCALAPPDATA%\MP14Tools
  },
  "start_with_windows": false,
  "show_tray_icon": true,
  "osd": {
    "enabled": true,
    "duration_ms": 5000            // OSD 停留时长（毫秒），之后 2 秒淡出
  },
  "touchpad": {
    "enabled": true,
    "factory_values": false,       // true = 固定用出厂阈值，忽略下面两个值
    "light_press_threshold": 125,  // 整数，1–500
    "deep_press_threshold": 500,   // 整数，须大于轻按，最大 1000
    "action": { "modifiers": [], "target": "F13" }
  },
  "haptics": {
    "enabled": false,              // 默认关闭：不启用时本工具不写触摸板
    "factory_values": true,        // true = 固定用出厂力度
    "normal_strength": 80,         // 0–128，步进 8
    "deep_press_strength": 104,    // 不低于轻触
    "device_marker": "hid#bltp7853&col05"
  },
  "display": {
    "enabled": false,              // 默认关闭
    "battery_action": "notify",    // off | notify | force
    "internal_refresh_rate": 60,   // 内屏档位：60 | 120
    "external_refresh_rate": "60hz", // 外屏档位：highest（最高档）| 60hz
    "hdr_check": true,             // 只检测内屏 HDR
    "internal": true,              // 影响内屏
    "external": true,              // 影响外屏
    "srr_folder": ""               // 留空 = 按面板实际支持的档位自动决定
  },
  "oem_keys": [
    {
      "name": "Performance mode (Fn+K)",
      "enabled": true,
      "report_hex": "01-28-01",    // 前缀匹配，大小写与连字符随意
      "press_only": true,          // 只在按下时触发
      "action": { "modifiers": [], "target": "KeyP" }
    }
  ]
}
```

- `target` 取值见设置界面下拉列表：鼠标键 `MouseLeft` / `MouseRight` / `MouseMiddle` /
  `MouseX1` / `MouseX2`，键盘键 `KeyA`…`KeyZ`、`Digit0`…、`F1`…`F24`、媒体键等；
  `modifiers` 为 `ctrl` / `shift` / `alt` / `win`。
- `internal_refresh_rate` 只接受 `60` / `120`；`external_refresh_rate` 为 `"highest"`
  （外屏跑它的最高可用档）或 `"60hz"`。切换时取不高于该档位的最高可用档，
  插回电源时恢复切换前的档位。
- 手工改完保存即生效（程序每 1.5 秒比对一次文件内容），越界数值会被自动夹到合法范围。

### 🔨 构建

前置：Rust 工具链（MSVC 或 GNU 均可）。

#### 1. 安装工具链（GNU 方案，无需管理员）

```powershell
$dir = "$env:TEMP\rustup-install"; New-Item -ItemType Directory -Force $dir | Out-Null
Invoke-WebRequest 'https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe' -OutFile "$dir\rustup-init.exe"
& "$dir\rustup-init.exe" -y --no-modify-path --profile minimal --default-host x86_64-pc-windows-gnu --default-toolchain none
& "$env:USERPROFILE\.cargo\bin\rustup.exe" toolchain install stable --profile minimal
& "$env:USERPROFILE\.cargo\bin\rustup.exe" default stable-x86_64-pc-windows-gnu
```

#### 2. 安装 dlltool/as 工具对（**仅 GNU 工具链需要**）

```powershell
.\tools\install-mingw.ps1
```

> 使用 MSVC 工具链（`stable-x86_64-pc-windows-msvc` + VS Build Tools）时这一步不需要。

#### 3. 构建

```powershell
.\build.ps1              # debug 构建
.\build.ps1 -Release     # release 构建（体积更小）
.\build.ps1 -Run         # 构建并运行
.\build.ps1 -Check       # 仅类型检查
```

若本机执行策略禁止运行脚本，用同参数的批处理入口 `build.cmd`（只在这一次进程里放宽策略）：

```cmd
build.cmd
build.cmd -Release
```

构建前会先结束**正在运行且路径等于本次产物**的实例（Windows 不允许覆盖运行中的映像），
加 `-KeepRunning` 可跳过。

产物：`build\obj\debug\mp14tools.exe` 或 `build\obj\release\mp14tools.exe`。

### ⚠️ 已知限制

- OEM 热键前缀默认按参考机型提供，其它机型需要在 `config.json` 里改成自己的前缀；
  只有走系统事件上报的厂商键能作为触发源（不安装全局键盘钩子）。
- 震动力度**无法读回**：因此「其他软件静默改了硬件力度」检测不到；能检测的是设备不再应答与配置文件被外部修改。
- 显示调节只做刷新率与 HDR，不改分辨率；HDR 只针对内屏，且需要驱动支持。
- 外接显示器的可用档位由它自己上报，超出范围的档位不会出现在选择里（外屏只有「最高档 / 60 Hz」两种目标）。
- OSD 文本使用 Microsoft YaHei UI；缺少该字体的系统上中文会退化为方框。
- 程序未签名，首次运行可能触发 SmartScreen 提示。

### 📄 许可

以 **GNU GPL-3.0** 发布，`LICENSE` 为许可证原文。本项目是
[`Meow-Box`](https://github.com/leehyukshuai/Meow-Box) 的衍生作品；
分发本程序或其修改版时请一并保留 `LICENSE` 与署名。

---

## English

Customized for the Xiaomi Book Pro 14 2026.

A small single-process, low-footprint Windows utility: it remaps the touchpad deep press and the
vendor (OEM) hotkeys to custom actions, and it can adjust the pressure trigger thresholds.
On battery power it detects the change by itself, then either prompts or automatically lowers the
refresh rate, and it checks the HDR state and prompts about it.

> 📥 **Download**: [latest release](https://github.com/Aragamiz/MP14tools/releases/latest)
> — a single `mp14tools.exe`, no installer, no admin rights.

### ✨ Features

| Feature | Description |
|---|---|
| Touchpad deep press mapping | Touchpad deep press → any keyboard key, mouse button or chord. Both pressure thresholds are integers and can be dragged or typed exactly in the UI, and one switch falls back to the "use factory values" setting (125 / 500) |
| Haptic strength | Adjusts the touchpad's **light press feedback** and **deep press feedback** levels: range `0–128`, step `8`, defaults equal the factory values (80 / 104). **Off by default**; while it is off this tool does not write to the touchpad |
| OEM hotkey mapping | Captures vendor hotkeys and maps them to any keyboard key, mouse button or chord; the "report prefix" of each entry decides which key is matched |
| Output actions | Keyboard keys (letters, digits, function keys, media keys, …), mouse left/right/middle/side buttons, each optionally with Ctrl / Shift / Alt / Win modifiers |
| Display policy | On battery the refresh rate is switched to the configured mode: the **internal** panel offers 60 / 120 Hz, an **external** display offers its highest mode / 60 Hz; the previous mode is restored when power is plugged back in. The behaviour can be **off / notify / force** |
| HDR check | **Internal panel only.** It is checked when the power source or the refresh rate changes; if HDR is on, the second button of the notification offers "turn HDR off". On battery it is checked once more **on every resume** |
| Internal / external switches | Two independent switches decide which displays the display policy applies to |
| Tray icon | Open settings / pause mapping / command prompt / run at logon / exit |
| OSD | On a trigger it shows a rounded hint bar at the bottom of the screen, without stealing focus or blocking clicks; it stays for 5 seconds and then fades out within 2 seconds (the hold time is configurable) |
| Notice window | An always-on-top small window shown when the user has to decide something, with the text and buttons centred; at most two buttons, and it collapses itself after 25 seconds without input |
| Command prompt | Optional: a console window showing the log in real time along with the program; off by default |
| Log location | The log directory can be changed (with a folder picker) and takes effect immediately |
| Live config reload | Editing `config.json` and saving it applies immediately; when the file is modified externally it is loaded from the file and **not overwritten**, and the UI marks it |
| Run at logon | A single `HKCU\...\Run` value, deleted as soon as the switch is turned off |

### 🧱 Requirements

- Windows 10 1809+ / Windows 11 (x64)
- No admin rights
- No runtime installation (no external dependency besides the CRT shipped with the system)

### 🖱 Usage

1. Run `mp14tools.exe`; the settings window opens (closing the window keeps the tool running in the tray).
2. **Touchpad** page: tick the switch, hold the touchpad to watch the "current pressure" reading and
   state, and set the thresholds from that; both the slider and the number box accept exact integers
   and take effect after the matching "OK". "Use factory values" pins them to 125 / 500.
   Then choose the target key. Haptic strength is on the same page and is written to the touchpad
   immediately once enabled (off by default).
3. **OEM keys** page: choose a target key for each vendor hotkey; the `report prefix` decides which key matches.
4. **Display** page: tick "enable display policy", then choose the behaviour on battery, the internal
   mode (60 / 120 Hz), the external mode (highest / 60 Hz), whether to check the internal HDR, and the
   internal/external switches.
5. **Log** page: open the command prompt if needed, or move the log file to another directory.
6. The tray menu can pause the mapping, open the settings, open the command prompt, toggle autostart or exit.

### ⚙️ Configuration

`%LOCALAPPDATA%\MP14Tools\config.json`

```jsonc
{
  "version": 1,
  "log": {
    "console": false,              // show a console window along with the program
    "directory": ""                // log directory, empty = %LOCALAPPDATA%\MP14Tools
  },
  "start_with_windows": false,
  "show_tray_icon": true,
  "osd": {
    "enabled": true,
    "duration_ms": 5000            // how long the OSD stays, in milliseconds; then a 2 s fade
  },
  "touchpad": {
    "enabled": true,
    "factory_values": false,       // true = always use the factory thresholds, ignoring the two below
    "light_press_threshold": 125,  // integer, 1–500
    "deep_press_threshold": 500,   // integer, must be above the light press, max 1000
    "action": { "modifiers": [], "target": "F13" }
  },
  "haptics": {
    "enabled": false,              // off by default: while it is off this tool does not write to the touchpad
    "factory_values": true,        // true = always use the factory strengths
    "normal_strength": 80,         // 0–128, step 8
    "deep_press_strength": 104,    // not below the light press
    "device_marker": "hid#bltp7853&col05"
  },
  "display": {
    "enabled": false,              // off by default
    "battery_action": "notify",    // off | notify | force
    "internal_refresh_rate": 60,   // internal mode: 60 | 120
    "external_refresh_rate": "60hz", // external mode: highest | 60hz
    "hdr_check": true,             // checks the internal panel's HDR only
    "internal": true,              // affects the internal display
    "external": true,              // affects the external displays
    "srr_folder": ""               // empty = decided automatically from the modes the panel actually supports
  },
  "oem_keys": [
    {
      "name": "Performance mode (Fn+K)",
      "enabled": true,
      "report_hex": "01-28-01",    // prefix match, case and hyphens are free
      "press_only": true,          // trigger on press only
      "action": { "modifiers": [], "target": "KeyP" }
    }
  ]
}
```

- The `target` values are the ones in the settings drop-down: mouse buttons
  `MouseLeft` / `MouseRight` / `MouseMiddle` / `MouseX1` / `MouseX2`, keyboard keys
  `KeyA`…`KeyZ`, `Digit0`…, `F1`…`F24`, media keys, and so on;
  `modifiers` are `ctrl` / `shift` / `alt` / `win`.
- `internal_refresh_rate` only accepts `60` / `120`; `external_refresh_rate` is either `"highest"`
  (the external display runs at its highest available mode) or `"60hz"`. The switch lands on the
  highest available mode not above the chosen one, and the mode from before the switch is restored
  when power is plugged back in.
- Saving a manual edit applies immediately (the program compares the file content every 1.5 seconds),
  and out-of-range numbers are clamped to the valid range.

### 🔨 Build

Prerequisite: a Rust toolchain (MSVC or GNU are both fine).

#### 1. Install the toolchain (GNU route, no admin)

```powershell
$dir = "$env:TEMP\rustup-install"; New-Item -ItemType Directory -Force $dir | Out-Null
Invoke-WebRequest 'https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe' -OutFile "$dir\rustup-init.exe"
& "$dir\rustup-init.exe" -y --no-modify-path --profile minimal --default-host x86_64-pc-windows-gnu --default-toolchain none
& "$env:USERPROFILE\.cargo\bin\rustup.exe" toolchain install stable --profile minimal
& "$env:USERPROFILE\.cargo\bin\rustup.exe" default stable-x86_64-pc-windows-gnu
```

#### 2. Install the dlltool/as pair (**GNU toolchain only**)

```powershell
.\tools\install-mingw.ps1
```

> This step is not needed with the MSVC toolchain (`stable-x86_64-pc-windows-msvc` + VS Build Tools).

#### 3. Build

```powershell
.\build.ps1              # debug build
.\build.ps1 -Release     # release build (smaller)
.\build.ps1 -Run         # build and run
.\build.ps1 -Check       # type check only
```

If the local execution policy blocks scripts, use the batch entry point with the same arguments
(the policy is relaxed for that one process only):

```cmd
build.cmd
build.cmd -Release
```

Before building, any instance **running from exactly this output path** is ended (Windows does not
allow overwriting a running image); add `-KeepRunning` to skip that.

Output: `build\obj\debug\mp14tools.exe` or `build\obj\release\mp14tools.exe`.

### ⚠️ Known limitations

- The OEM hotkey prefixes are shipped for the reference model; other models need their own prefixes in
  `config.json`; only vendor keys reported through system events can act as a trigger (no global
  keyboard hook is installed).
- The haptic strength **cannot be read back**: a strength silently changed by another tool is therefore
  not detected; what can be detected is a device that no longer answers and an externally modified
  configuration file.
- The display policy only handles the refresh rate and HDR, not the resolution; HDR is handled for the
  internal panel only, and needs driver support.
- The modes available on an external display are the ones it reports itself, so modes outside that
  range never appear in the choice (an external display only has the two targets "highest / 60 Hz").
- OSD text uses Microsoft YaHei UI; on systems without that font Chinese degrades to boxes.
- The program is unsigned, so the first run may trigger a SmartScreen prompt.

### 📄 License

Released under the **GNU GPL-3.0**; `LICENSE` is the verbatim license text. This project is a
derivative work of [`Meow-Box`](https://github.com/leehyukshuai/Meow-Box); distributing this program
or a modified version requires keeping `LICENSE` and this attribution.
