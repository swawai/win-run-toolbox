# Swaw Kit Proj：架构与源码导读

## 一、项目是什么

1. Swaw Kit Proj（`Favorites/template.proj1.exe` + `_lib/proj`）是一个以项目为对象的本地控制面，通过同一套 Core 向 CLI 和 Web 提供项目配置、开发环境和功能命令。
2. 一个 Entry 对应一套相互隔离的项目资源：目标项目、功能命令、Git 身份、DataRoot、开发环境，以及随项目维护的 skills 和提示词。
3. 创建 Entry：`copy Favorites\template.proj1.exe MyProjEntry.exe`。
4. 运行主链可以记作：`Entry -> Bootstrap（按需）-> Rust Core -> Catalog -> Control / Kernel / Action`。
5. Entry 是几 KB 的原生 Launcher，只传递自身路径和参数。若共享 Core `_lib/proj/_bin/swawkit-proj.exe` 尚未生成，它会调用 `_lib/proj/bootstrap.ps1`，由仓库内固定的 Rust/MSVC 工具链完成构建和发布。
6. Rust Core 根据 Entry 名和 Windows File ID 识别入口，并在 `data/proj.{入口名}/` 管理专用 DataRoot；复制产生新 Entry，改名仍延续原身份。
7. Entry Profile 记录目标项目和开发环境等用户配置，CLI 与 Web 共用这份配置；项目自定义命令从目标项目的 `.swaw` 中发现。

## 二、代码地图与阅读顺序

中心代码只维护需要统一的事实和协议，例如 Entry 身份、DataRoot、Profile、Catalog 与进程执行规则；具体能力则靠近各自的目录化命令模块。

| 路径 | 主要职责 |
| --- | --- |
| `Favorites/template.proj1.exe` | 随仓库发布的 Launcher 模板，用于复制出新的 Entry |
| `_lib/proj/_launcher` | 原生 Launcher 源码；构建候选写入 `data/proj_cache/bootstrap/build/launcher/release/template.proj1.exe` |
| `_lib/proj/bootstrap.ps1` / `bootstrap.json` | Core 缺失时使用固定工具链构建并发布共享 Rust Core |
| `_lib/proj/build.ps1` | 使用固定工具链构建 App 与 Launcher 候选，不发布正式制品 |
| `_lib/proj/_toolchain` | 受管工具的声明、安装、环境生成与激活库 |
| `_lib/proj/_app` | Rust Core 源码，承载身份、DataRoot、Profile、Catalog、命令执行、CLI 与 Host |
| `_lib/proj/_bin` | Bootstrap 发布的共享运行时产物 |
| `_lib/proj/.dev/{setup,status,bun,cargo,rustc,cl,cmd,ps}` | 管理受管开发环境，并提供开发工具与一次性 Shell 命令入口 |
| `_lib/proj/.<name>` / `.swaw/<name>` | Kernel 命令 / 目标项目的 Action 命令 |
| `_lib/proj/_test` | 原生 Entry、Rust Core、Bootstrap 和工具模块的正式回归入口 |

建议沿主链阅读源码：先看 `_launcher/launcher.c` 和 `bootstrap.ps1`，再进入 `_app/src/launch.rs`、`cli.rs`，随后按问题深入 `catalog`、`data_root`、`profile`、`command` 或 `server`。

命令分为三类：Control 以 `..` 开头并管理 Entry/Host；Kernel 在 `_lib/proj` 中定义且以 `.` 开头；Action 在目标项目的 `.swaw` 中定义且没有前缀。Core 扫描这些目录并忽略下划线开头的私有实现。Kernel/Action 可执行模块只提供一个 `run.*` 入口，Control 使用受限的 `run.core.json`；Help、Guard、展示信息和相关实现尽量与命令放在一起。

## 三、开发取向

1. 框架保持一条清晰主路径和一个事实源；新路径成立后，旧实现应有明确的退出点。
2. 稳定概念出现第二个真实消费者后再抽象，AHA 优先于 DRY，局部清晰优先于通用包装。

## 四、关键不变量

1. 除系列根目录使用 `SWAWKIT_HOME` 外，Proj 定义的环境变量统一使用 `SWAWKIT_PROJ_` 前缀。
2. Entry 以 Windows File ID 保持身份，复制产生新身份，改名延续原身份，文件被替换时才要求显式认领（claim）。
3. `_entry.json` 只记录 Entry 身份，`_profile.json` 只记录用户配置，运行时环境变量不能反过来成为配置来源。
4. Catalog 负责描述“有什么命令”；解释器、Profile 和前置条件共同决定命令当前是否 Ready。
5. 普通 Kernel 和 Action 依次执行全局检查、模块 Guard 和 `run.*`，Guard 只检查前提，不安装工具或修复状态。
6. 工具下载、安装、更新和主动清理由显式命令触发；受管工具始终使用已验证的受管版本，而不是系统 PATH 中的同名程序。
7. 同一次开发环境发布生成的 `env.cmd`、`env.ps1` 和 `_state.json` 共同构成一个完整 generation，三者一致时环境才是 Ready。
8. build 产出候选文件，publish 和 update 独立完成发布；普通 Action 不直接替换正在运行的 Core。

## 五、当前做到哪里

1. Native Launcher 和 Rust 主程序已经接管正常启动、Entry 身份、DataRoot、Profile、命令发现、Guard、CLI 和 Host 主链。
2. Host 已采用 Axum、Tray 和系统浏览器，Web 目前可以浏览命令和编辑 Profile，但还不能运行或取消命令。
3. `_profile.json` 已是用户配置的唯一来源，项目自定义命令固定从目标项目的 `.swaw` 中发现。
4. Rust 当前可执行 `run.exe`、`run.ps1` 和受限的 `run.cmd`；旧 PowerShell Core 已移除，PowerShell 只保留开发工具安装、环境激活和命令适配职责，`run.ts/run.py` 暂不支持执行。
5. Bun、PowerShell、MSVC 和 Rust 的受管环境已经实现；正式测试入口同时覆盖原生 Entry、Rust Core 和各工具模块。

## 六、Web 与执行边界

1. Web 使用固定 Finder 式界面，目录化命令模块不各自携带一套网页。
2. Web 只提交已发现的命令地址和受限输入，解释器、磁盘路径、cwd 和环境变量都由 Core 决定。
3. `.swaw` Action 是以当前用户权限运行的受信任项目代码；Web 负责控制执行入口，但 Action 本身不是安全沙箱。

## 七、下一步

1. 继续收窄 Action 对开发工具私有实现的依赖，并统一 Control 命令登记方式和命令可用性判断。
2. 让 CLI 与 Web 共用同一套命令执行、输出和取消逻辑，使两种界面自然保持一致。
3. 根据真实消费者清理 Profile 字段，再按实际需求推进更新、RPC、远程访问或跨平台。
4. 长期目标是保持一个正式 Core、一套配置与命令协议，让 CLI、Web 和后续入口共享同一套行为语义。
