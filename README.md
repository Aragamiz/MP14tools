# MP14Tools

[中文说明](#中文说明) | [English](#english)

一个单进程、低占用的 Windows 小工具：把笔记本触摸板的重按与厂商（OEM）热键变成自定义动作，
并在电池供电时自动降低刷新率、检查 HDR。

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
| 显示调节 | 电池供电时把刷新率降到目标档（默认 60 Hz，取不高于该值的最高可用档），插回电源恢复原档位；行为可选**不处理 / 通知确认 / 直接切换** |
| HDR 检测 | 切电源与切刷新率时检查 HDR，若处于开启状态，通知的第二个按钮提供「关闭 HDR」；电池模式下**每次唤醒**也会检查一次 |
| 内屏 / 外屏开关 | 两个独立开关，决定显示与 HDR 相关行为作用于哪些显示器 |
| 托盘图标 | 打开设置 / 暂停映射 / 命令提示符 / 开机自启 / 退出 |
| OSD 提示 | 触发时在屏幕底部显示圆角提示条，不抢焦点、不挡点击 |
| 通知窗口 | 需要用户决定时弹出的置顶小窗，最多两个按钮，25 秒无操作自动收起 |
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
4. **显示**页：勾选「启用显示调节」，选择电池时的行为、目标刷新率、是否检查 HDR，以及内屏/外屏开关。
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
  "osd": { "enabled": true, "duration_ms": 900 },
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
    "battery_refresh_rate": 60,    // 取该值以下最高的可用档
    "hdr_check": true,
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
- 显示调节只做刷新率与 HDR，不改分辨率；HDR 需要驱动支持。
- OSD 文本使用 Microsoft YaHei UI；缺少该字体的系统上中文会退化为方框。
- 程序未签名，首次运行可能触发 SmartScreen 提示。

### 📄 许可

以 **GNU GPL-3.0** 发布，`LICENSE` 为许可证原文。本项目是
[`Meow-Box`](https://github.com/leehyukshuai/Meow-Box) 的衍生作品；
分发本程序或其修改版时请一并保留 `LICENSE` 与署名。

---

## English

MP14Tools is a small single-process Windows utility that turns the touchpad deep press and the
vendor (OEM) hotkeys of a laptop into configurable actions, and that lowers the refresh rate and
checks HDR while running on battery.

> 📥 **Download**: [latest release](https://github.com/Aragamiz/MP14tools/releases/latest)
> - a single `mp14tools.exe`, no installer and no admin rights.

### Features

- **Touchpad deep press** mapped to a key, mouse button or chord (integer thresholds, factory-value switch).
- **Haptic feedback strength** for light and deep press (0–128, step 8), off by default.
- **OEM hotkeys** matched by report prefix, mapped to the same kinds of actions.
- **Display policy**: drop to 60 Hz on battery (off / notify / force) and offer to turn HDR off,
  including a re-check after every resume while on battery, with separate internal/external switches.
- Tray icon, on-screen display, an always-on-top notice window with buttons, an optional console
  window, a relocatable log file, live configuration reload and run-at-logon.

### Requirements

Windows 10 1809+ / Windows 11 (x64). No admin rights and no runtime installation required.

### Usage

Run `mp14tools.exe` and configure everything in the settings window; closing it keeps the tool in the
tray. The configuration lives in `%LOCALAPPDATA%\MP14Tools\config.json` and is applied as soon as the
file changes.

### Build

Requires Rust (MSVC or GNU). With the GNU target, run `tools\install-mingw.ps1` once first, then:

```powershell
.\build.ps1 -Release     # or build.cmd -Release if script execution is blocked
```

Output: `build\obj\release\mp14tools.exe`.

### License

Released under the **GNU GPL-3.0**; `LICENSE` is the verbatim license text. This project is a
derivative work of [`Meow-Box`](https://github.com/leehyukshuai/Meow-Box).
