# 需要手动清理的项

> 本文档已随仓库整理移入 `docs\`；文中出现的相对路径均以**项目根目录**为基准。

> 这份清单只列**既不是工程文件、也不是临时文件**的东西——也就是需要你自己决定去留的项。
> 判定口径：
>
> - **工程文件**：`src\`、`tools\`、`.cargo\config.toml`、`Cargo.toml`、`Cargo.lock`、`build.ps1`、
>   `README.md`、`REPORT.md`、`BUILD-REPORT.md`、`MANUAL-CLEANUP.md`、`.gitignore` → **不要删**
> - **临时 / 中间文件**：已全部集中到 `mp14tools\build\` → 见第 3 节，可整个删
> - **本清单**：以下第 1、2 节

---

## 1. 需要你决定去留（非工程、非临时）

| # | 位置 | 体积 | 是什么 | 建议 |
|---|---|---|---|---|
| 1 | `G:\Work\Code\Meow-Box-main\meowbox-architecture.md` | 2,483 B | 我（AI 助手）写的架构笔记，记录的是**原 `src\MeowBox.*`（WinUI 3）版**的结构 | **删除**：不是你的工程文件，内容已在 `REPORT.md` 覆盖 |
| 2 | `%LOCALAPPDATA%\MP14Tools\config.json` | 2.9 KB | 程序的运行时配置（你配置的按键映射） | **保留**：删掉会重置为默认，已配置的映射会丢失 |
| 3 | `%LOCALAPPDATA%\MP14Tools\mp14tools.log` | 数 KB | 运行日志 | 可随时删 |
| 4 | `%LOCALAPPDATA%\MeowBoxLite\`（旧目录） | 2.9 KB | 改名前的数据目录；配置已自动搬到 `MP14Tools\`，仅剩一份副本 | 确认程序正常后可删 |
| 5 | 注册表 `HKCU\...\Run\MP14Tools` | — | 开机自启项（改名时旧的 `MeowBoxLite` 项已删） | 按需，见下 |
| 6 | `%USERPROFILE%\.cargo` | 605.7 MB | 为构建本项目安装的 Rust 工具链（包缓存） | 按需，见下 |
| 7 | `%USERPROFILE%\.rustup` | 1,493.5 MB | 同上（工具链本体） | 按需，见下 |
| 8 | `mp14tools-main\build\temp\` | 95.8 MB | 构建期间的临时与诊断文件，在项目内但**不是工程文件** | **可删**，构建不依赖 |

### 1.1 删除我留下的 AI 笔记

```powershell
Remove-Item 'G:\Work\Code\Meow-Box-main\meowbox-architecture.md'
```

### 1.2 开机自启项

程序已改名为 **MP14Tools**：改名时自动删掉了旧的 `Run\MeowBoxLite` 值，当前**没有**自启项。
如果之后在「通用」页开启过开机自启，注册表值为：

```
HKCU\Software\Microsoft\Windows\CurrentVersion\Run\MP14Tools
  = "…\mp14tools-main\build\obj\release\mp14tools.exe"
```

不要它的两种方式：

```powershell
# 方式一（推荐）：在程序“通用”页取消勾选“开机时自动启动”，会同时同步配置文件
# 方式二：直接删注册表值
Remove-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' -Name 'MP14Tools'
```

> ⚠️ **注意**：自启项当前指向的是**构建输出目录**。只要你删除了 `build\`，它就指向不存在的文件。
> 见第 4 节的建议。

### 1.3 Rust 工具链（2.05 GB）

这是为**构建**本项目而装的系统级开发环境，与程序**运行时无关**——`mp14tools.exe` 是无需
安装运行时的单一可执行文件。如果你还打算继续改这个项目就保留；不再用 Rust 时再删：

```powershell
Remove-Item -Recurse -Force "$env:USERPROFILE\.cargo", "$env:USERPROFILE\.rustup"
```

**已核实：安装时使用了 `--no-modify-path`，系统 PATH 未被修改。**

| PATH 作用域 | 是否含 cargo / rustup / mingw 条目 |
|---|---|
| 用户 PATH | 否 |
| 系统 PATH | 否 |

所以删除这两个目录不会留下悬空的 PATH 条目（但 `build.ps1` 会因此在构建时提示找不到工具链）。

### 1.4 项目内的临时文件（95.8 MB）

```
mp14tools\build\temp\
├─ scratch-projects\rust-link-check\   83.45 MB  验证工具链能否链接的临时 cargo 工程
├─ installer\rustup-install\           12.13 MB  rustup 安装器副本与安装日志
└─ diagnostics\                         0.18 MB  构建日志、Win32 API 探测输出、整理期间的脚本
```

```powershell
Remove-Item -Recurse -Force .\build\temp
```

删掉不影响构建，也不影响 `install-mingw.ps1`（它只用 `build\toolchain`）。

---

## 2. 一键清理（只动生成物与我留下的文件，不碰你的配置）

```powershell
cd g:\Work\Code\MP14Tools\mp14tools-main

Remove-Item -Recurse -Force .\build\temp                              # 第 1.4 节：临时文件 95.8 MB
Remove-Item -Force '..\Meow-Box-main\meowbox-architecture.md'         # 第 1.1 节：AI 笔记
Remove-Item -Force "$env:LOCALAPPDATA\MP14Tools\mp14tools.log"        # 第 1.3 节：日志
```

上面三条删完**不影响任何功能**。若还想清掉全部生成物（下次构建需重新编译全部依赖，
约 6–10 分钟，并且需要重新运行 `tools\install-mingw.ps1`）：

```powershell
Remove-Item -Recurse -Force .\build
```

---

## 3. 可以保留的生成物（不是必须清理）

| 位置 | 体积 | 删除后果 |
|---|---|---|
| `build\obj\` | 2,216.9 MB | 下次构建重新编译全部依赖（6–10 分钟） |
| `build\toolchain\assembler\` | 6.4 MB | **必须**重新运行 `tools\install-mingw.ps1`，否则构建失败 |
| `build\toolchain\mingw64\` | 916.7 MB | 只有 `install-mingw.ps1` 需要；可删（但 assembler 要留） |
| `build\toolchain\downloads\` | 261.3 MB | 只是压缩包缓存；删了下次安装需重新下载 261 MB |
| `build\` 整体 | **3,497.0 MB** | 见上；`.\build.ps1` + `.\tools\install-mingw.ps1` 可完全恢复 |

`build\` 已写入 `mp14tools-main\.gitignore`，不会被提交。

---

## 4. 建议（不是清理项，但会影响以后的省心程度）

自启项现在指向 `build\obj\release\`，那是**构建输出目录**。建议把可执行文件放到稳定位置：

```powershell
$target = "$env:LOCALAPPDATA\Programs\MP14Tools"
New-Item -ItemType Directory -Force $target | Out-Null
Copy-Item .\build\obj\release\mp14tools.exe $target
```

然后从该位置启动一次，在"通用"页重新勾选开机自启（会写入正确路径）。
这样即使以后清空 `build\`，自启仍然有效。

---

## 5. 已确认无需清理

| 项 | 说明 |
|---|---|
| `mp14tools` 下的 25 个工程文件（约 245 KB） | 全部是工程文件 |
| `mp14tools-main\build\` | 生成物，已 gitignore；留即是缓存，删即是清理 |
| `%TEMP%` | 本次开发产生的文件已**全部**收归到 `build\temp\`，已核实残留为 0 |
| VS Code 会话资源（`%APPDATA%\Code\User\workspaceStorage\...`） | VS Code 自行管理的会话缓存，不属于本项目 |
| 调试期间遗留的 33 个 PowerShell 终端 | 已全部关闭 |
| `%LOCALAPPDATA%\MeowBoxLite\Tools\`（旧位置） | 已搬空，只剩上面第 2、3 项两个文件 |

---

## 6. 另外两处与清理无关、但你应该知道的事

1. **仓库根目录没有版本控制兜底**：本机未安装 `git`，所以本次整理是「移动 + 显式校验」
   而不是「改完可回滚」。所有移动都保留原文件，唯一被删除的只有空目录。
2. **本次整理修好了两个副作用**（详见 `BUILD-REPORT.md` 第 5 节）：
   失效的开机自启路径、以及一个仍占用单实例互斥体的陈旧进程。如果没有处理，
   你下次双击新位置的 exe 会看到它"闪一下就没反应"。
