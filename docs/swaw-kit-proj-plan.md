# Swaw Kit Proj 开发设计纲要

## 一、项目是什么

1. Swaw Kit Proj（`Favorites/template.proj1.exe` + `_lib/proj`）是一个以项目为对象的本地控制面，用于控制和运维其他项目的代码仓库，同时提供 Web GUI 和 CLI 两套操作界面。
2. 一个入口命令即一套资源：它绑定一个受控目标项目，并拥有相互隔离的功能命令（包括 skills、提示词）、Git 身份、数据目录和开发环境。
3. 创建入口命令：`copy Favorites\template.proj1.exe MyProjEntry.exe`。
4. 首次运行 `MyProjEntry.exe` 时会检查 `_lib/proj/_bin/swawkit-proj.exe`，若不存在则调用 `_lib/proj/_bootstrap`，准备便携的 Rust/MSVC 工具链并立即编译。
5. 入口命令（Entry）本身只有几 KB，共享 Rust Core（`swawkit-proj.exe`）存在后，它只传递自身路径和参数，File ID 识别及所有业务逻辑都由 Core 处理。
6. Core（`swawkit-proj.exe`） 根据入口名和 File ID，在 `data/proj.{入口名}/` 中管理该 Entry（入口命令） 的专用数据，其中入口名不含 `.exe`。
7. Entry 首次运行时要求设置目标项目和开发环境等 Profile 信息，Web GUI 与 CLI 共用这份配置，项目自定义命令固定从目标项目的 `.swaw` 中发现。


## 二、怎么组织代码

1. 项目框架和代码结构尽量采用去中心化、功能领域驱动设计（DDD）的组织方式。
2. 中心代码只维护必须统一的协议、和必须提前准备的基础资源，例如 Entry 身份、专用数据目录和进程执行规则。
3. 每个具体功能都有对应目录，例如 `MyProjEntry .dev.setup` 对应 `_lib/proj/.dev/setup`，这种结构称为`目录化命令模块`。
4. 可执行的 Kernel/Action 模块必须有且只有一个 `run.exe/run.ts/run.py/run.ps1/run.cmd`，Control 则使用受限的 `run.core.json`。
5. `swawkit-proj.exe` 自动扫描 `_lib/proj` 和目标项目的 `.swaw`，发现目录化命令模块，并忽略下划线开头的私有目录。
6. 目录化命令模块可以就近拥有前置条件检查、配置、Help、在 Web/CLI 中的展示信息和测试用例。
7. 新增普通功能命令时，原则上只需要增加对应的目录化命令模块。
8. 命令分为 Control、Kernel 和 Action 三类。
9. Control 以 `..` 开头并管理 Entry/Host，Kernel 在 `_lib/proj` 中定义且以 `.` 开头，Action 在目标项目的 `.swaw` 中定义且没有前缀。


## 三、开发风格

1. 框架只保留一条主路径和一个事实源，不用 fallback 长期维护新旧两套实现。
2. 稳定概念出现第二个真实消费者后再抽象，AHA 优先于 DRY。



## 四、必须守住的规则

1. 除系列根目录使用 `SWAWKIT_HOME` 外，Proj 定义的环境变量统一使用 `SWAWKIT_PROJ_` 前缀。
2. Entry 以 Windows File ID 保持身份，复制产生新身份，改名延续原身份，文件被替换时才要求显式认领（claim）。
3. `_entry.json` 只记录 Entry 身份，`_profile.json` 只记录用户配置，运行时环境变量不能反过来成为配置来源。
4. 命令目录（Catalog）能发现一个命令不代表它现在可执行，解释器、Profile 和前置条件都满足后才能显示 Ready。
5. 普通 Kernel 和 Action 依次执行全局检查、模块 Guard 和 `run.*`，Guard 只检查前提，不安装工具或修复状态。
6. 工具下载、安装、更新和主动清理只能由显式命令触发，受管工具不得退回系统 PATH 中碰巧存在的版本。
7. 同一次开发环境发布生成的 `env.cmd`、`env.ps1` 和 `_state.json` 必须保持一致，半发布状态不能算 Ready。
8. build 只产生候选文件，publish 和 update 是独立操作，普通 Action 不得覆盖正在运行的 Core。

## 五、当前做到哪里

1. Native Launcher 和 Rust 主程序已经接管正常启动、Entry 身份、DataRoot、Profile、命令发现、Guard、CLI 和 Host 主链。
2. Host 已采用 Axum、Tray 和系统浏览器，Web 目前可以浏览命令和编辑 Profile，但还不能运行或取消命令。
3. `_profile.json` 已是用户配置的唯一来源，项目自定义命令固定从目标项目的 `.swaw` 中发现。
4. Rust 当前可执行 `run.exe`、`run.ps1` 和受限的 `run.cmd`；旧 PowerShell Core 已移除，PowerShell 只保留开发工具安装、环境激活和命令适配职责，`run.ts/run.py` 暂不支持执行。
5. Bun、PowerShell、MSVC 和 Rust 的受管环境已经实现；正式测试入口同时覆盖原生 Entry、Rust Core 和各工具模块。

## 六、Web 应守的边界

1. Web 使用固定 Finder 式界面，目录化命令模块不各自携带一套网页。
2. Web 只提交已发现的命令地址和受限输入，解释器、磁盘路径、cwd 和环境变量都由 Core 决定。
3. `.swaw` Action 是以当前用户权限运行的受信任项目代码，Web 授权只能限制谁能发起执行，不能把 Action 变成安全沙箱。

## 七、下一步

1. 先继续收窄 Action 对开发工具私有实现的依赖，统一 Control 命令登记方式和命令可用性判断。
2. 然后让 CLI 与 Web 共用同一套命令执行、输出和取消逻辑，避免 Web 再造第二套 Core。
3. 再后面才清理 Profile 中没有真实消费者的字段，并按实际需求设计更新、RPC、远程访问或跨平台。
4. 项目完成时只能有一个正式 Core、一套配置和命令协议，CLI 与 Web 行为一致，且不存在隐式 fallback 或新旧主路径并存。
