# Swaw Kit Proj：架构与源码导读

## 一、项目是什么

1. Swaw Kit Proj（`Favorites/template.proj1.exe` + `_lib/proj`）是一个以项目为对象的本地控制面，通过同一套 Core 向 CLI 和 Web 提供项目配置、开发环境和功能命令。
2. 一个 Entry 对应一套相互隔离的项目资源：目标项目、功能命令、Git 身份、DataRoot、开发环境，以及随项目维护的 skills 和提示词。创建 Entry：`copy Favorites\template.proj1.exe MyProjEntry.exe`。
3. Entry 是约 10 KB 的原生 Launcher，只负责读取 `_lib/proj/_bin/current`、确定不可变 Release Set 中的共享 Core、传递自身路径与原始参数，并建立必要的进程边界。若 selector 尚未生成，它会调用 `_lib/proj/bootstrap.ps1`，由仓库内固定的 Rust/MSVC 工具链完成首次三件套构建和发布。
4. 运行主链可以记作：`Entry Launcher -> Rust Core（CLI / Host / Worker）-> Catalog -> Control / Kernel / Action`。Bootstrap 只是 Core 缺失时的恢复路径，不参与日常命令逻辑。
5. Rust Core 根据 Entry 名和 Windows File ID 识别入口，并在 `data/proj.{入口名}/` 管理专用 DataRoot；复制产生新 Entry，改名仍延续原身份。同一 Entry 只保留一个 Entry Host，普通 CLI 和 Entry Worker 则是按次创建的短生命周期进程。
6. Entry Profile 记录目标项目和开发环境等用户配置，CLI 与 Web 共用这份配置；项目自定义命令从目标项目的 `.swaw` 中发现。

理解后续设计，先记住六个核心名称：

| 名称 | 定义 |
| --- | --- |
| **Entry** | 一份稳定入口身份，以及与之绑定的 Profile、DataRoot 和单实例 Host 边界 |
| **目录命令模块** | 以目录为最小领域单元；目录层级定义地址，`run.*`、Guard、Help、View 和私有实现就近组织 |
| **Catalog** | Core 从目录树生成的统一命令读模型，供 CLI 与 Web 共同发现和定位能力 |
| **模块数据根** | Core 为每个目录命令模块派生的专属数据路径，与命令来源和相对目录同构 |
| **模块 Export / Provider State** | 模块对外发布的稳定资源，以及描述该发布当前是否可消费的状态协议 |
| **Entry Host / Entry Worker** | Host 提供单实例 Web 控制面；Worker 按次通过完整 Entry 链执行命令 |

架构记忆句是：**协议集中，领域自治**。Core 统一身份、地址、生命周期和进程协议；目录命令模块拥有行为、数据、状态与 Export。进一步可以记成：**目录定义能力，数据跟随模块，依赖通过 Export 连接，环境按次生成。**

## 二、代码地图与阅读顺序

中心代码只维护必须统一的事实和协议，例如 Entry 身份、DataRoot、Profile、Catalog 与进程执行边界；具体能力、状态和演进节奏归属各自的目录命令模块。

| 路径 | 主要职责 |
| --- | --- |
| `Favorites/template.proj1.exe` | 随仓库发布的 Launcher 模板，用于复制出新的 Entry |
| `_lib/proj/_launcher` | 原生 Launcher 源码；构建候选写入 `data/proj_cache/bootstrap/build/launcher/release/template.proj1.exe` |
| `_lib/proj/bootstrap.ps1` / `bootstrap.json` | Core 缺失时使用固定工具链构建并发布共享 Rust Core |
| `_lib/proj/build.ps1` | 使用固定工具链构建 App 与 Launcher 候选，不发布正式制品 |
| `_lib/proj/_toolchain` | 受管工具链及模块 Export、状态、锁、环境生成与激活等共享机制 |
| `_lib/proj/_app` | Rust 产品源码，按 Core、Host 与 Toolchain 领域承载身份、DataRoot、Profile、Catalog、命令执行、Web 生命周期及受管工具能力 |
| `_lib/proj/_bin` | Bootstrap 或显式发布产生的共享运行时制品 |
| `_lib/proj/.dev/{setup,status,bun,cargo,rustc,cl,cmd,ps}` | 管理受管开发环境，并提供开发工具与一次性 Shell 命令入口 |
| `_lib/proj/.<name>` / `.swaw/<name>` | Kernel 命令 / 目标项目的 Action 命令 |
| `_lib/proj/_test` | 原生 Entry、Rust Core、Entry Worker、Bootstrap 和工具模块的正式回归入口 |

建议沿主链阅读源码：先看 `_launcher/launcher.c` 和 `bootstrap.ps1`，再进入 `_app/src/launch.rs`、`main.rs`、`cli.rs`，随后按问题深入 `catalog`、`data_root`、`profile`、`command`、`entry_runner` 或 `server`。

命令分为三类：Control 以 `..` 开头并管理 Entry/Host；Kernel 在 `_lib/proj` 中定义且以 `.` 开头；Action 在目标项目的 `.swaw` 中定义且没有前缀。Core 扫描这些目录并忽略下划线开头的私有实现。Kernel/Action 可执行模块只提供一个 `run.*` 入口，Control 使用受限的 `run.core.json`；`_guard`、`_help`、`_view/web.json` 和私有库与所属命令放在一起，由 Catalog 汇总为统一模型。

## 三、开发取向

1. 框架保持一条清晰主路径和一个事实源；新路径成立后，旧实现应有明确的退出点。
2. Core 只承载跨模块协议和必要生命周期；目录命令模块拥有领域行为、数据和 Export。共享库负责路径校验、原子发布、锁和状态检查等通用机制。
3. 模块依赖由消费方就近声明，并指向明确的提供命令。稳定概念出现第二个真实消费者后再抽象，AHA 优先于 DRY，局部清晰优先于通用包装。

## 四、关键不变量与协议

1. **身份与配置。** Entry 以 Windows File ID 保持身份：复制产生新身份，改名延续原身份，文件被替换时才要求显式认领（claim）。`_entry.json` 只记录 Entry 身份，`_profile.json` 只记录用户配置；运行时环境变量不能反过来成为配置来源。
2. **目录即命令协议。** 命令地址由目录层级唯一推导，一个目录最多有一个规范执行入口；Help、Web 展示提示和 Guard 都是模块的伴随声明。Catalog 以 `swawkit.command-catalog/v3` 生成结构性的 `runnable` 与 `diagnostic`；Control 的 `run.core.json` 使用 `swawkit.core-command/v1`，Kernel 产品命令可用 `run.toolchain.json` 与 `swawkit.toolchain-command/v1` 静态选择同 Release Set 中的受限 Rust handler，`_view/web.json` 使用 `swawkit.command-view/web/v1`。全局与模块 Guard 在执行前判断运行前提。
3. **模块数据根。** Core 按命令来源和相对目录，将每个目录命令模块同构映射到 `DataRoot/modules/{control|kernel|action}/...`。例如 `.dev.setup` 对应 `modules/kernel/.dev/setup/`。`SWAWKIT_PROJ_DATA_ROOT` 表示 Entry 根，`SWAWKIT_PROJ_CORE_COMMAND_DATA_ROOT` 表示当前模块数据根；普通命令的工作数据、状态、锁和 Export 都归入后者。只有 Core 缺失时的 Bootstrap 构建和确实需要跨 Entry 复用的下载缓存进入 `data/proj_cache/`。
4. **模块 Export 与显式依赖。** 模块把稳定候选或供其他模块消费的产物发布到自身的 `export/`，中间文件留在 `work/`。`export/` 只是稳定发布边界，不自动等于一套资产系统：需要自动消费的构建产物通过 `manifest.json` 声明文件名、长度与 SHA-256，并由提供命令地址、`producerContract` 和 `_state.json` 建立可验证发布边界。依赖尚未 Ready 或制品与 Manifest 不一致时，错误信息直接给出应执行的提供命令。
5. **Provider State。** `_state.json` 使用 `swawkit.command-provider-state/v1`，状态为 `unavailable` 或 `ready`。影响开发环境的 Profile 输入变化时，`.dev.setup` 状态同步变为 `unavailable`；重新执行 `.dev.setup` 后，完成的 Export 与 `ready` 状态一起成为新的有效发布。`inputRevision`、`token` 与 `producerContract` 共同标识这次发布并保证消费一致性。
6. **环境变量是边界，不是状态。** 除系列根目录使用 `SWAWKIT_HOME` 外，Proj 拥有的变量统一使用 `SWAWKIT_PROJ_` 前缀：`SWAWKIT_PROJ_CORE_LAUNCH_*` 传递一次性启动声明，`SWAWKIT_PROJ_CORE_COMMAND_*` 描述单次命令上下文，`SWAWKIT_PROJ_MODULE_*` 用于模块私有适配。Core 在读取 Launcher 声明后、创建线程前清除进程中的 `SWAWKIT_HOME` 与全部 `SWAWKIT_PROJ_*`；Host 的长期事实保存在类型化对象和 DataRoot。每次执行命令时，Core 再从当前 Entry/Profile 生成命令环境，适配器私有变量在消费后清除。
7. **执行一致性。** 普通 Kernel 和 Action 依次执行全局 Guard、模块 Guard 和 `run.*`；Guard 只检查前提，不安装工具或修复状态。CLI 直接进入这条执行链，Web 则通过同一个 Entry Launcher 创建 Entry Worker，再进入同一条链，因此两者共享解释器、cwd、Profile、DataRoot 与环境语义。
8. **持久运行日志。** CLI 和 Web 运行都在所属模块数据根的 `_runs/{run-id}/` 写入同一套事实：`events.jsonl` 使用 `swawkit.command-run-event/v1` 追加带时间和阶段的 `kind=output|progress` 事件；`output` 保存 stdout/stderr UTF-8 文本，`progress` 保存稳定 ID、状态、数值、单位和消息。`_state.json` 使用 `swawkit.command-run-journal/v1` 原子发布来源、起止时间和终态。Journal 统一生成 sequence 与时间戳；CLI 控制台、Web 实时窗口和历史查询消费同一事件，而不是各自重新编号或从日志文件轮询实时输出。原始参数不落盘，只记录数量；每次运行最多保留 8 MiB 事件文件，达到上限后明确标记 `truncated`。日志写入是执行契约的一部分，初始化或完成日志失败会使本次运行失败，而不是静默丢失。
9. **构建与发布分离。** build 只产出候选文件，显式 publish/update 命令才可替换正式制品。`proj.publish.app` 只消费 `proj.build.app` 的 Ready Provider 与匹配 Manifest，在共享锁内原子切换 Release Set selector；旧进程继续运行已映射版本，新调用使用新版本。工具下载、安装、更新和主动清理由显式命令触发，受管工具始终使用已验证的受管版本，而不是系统 PATH 中的同名程序。

## 五、当前做到哪里

1. Native Launcher 和 Rust Core 已接管正常启动、Entry 身份、DataRoot、Profile、命令发现、Guard、CLI、单实例 Entry Host 与 Entry Worker 主链。
2. Entry Host 采用 Axum、Tray 和系统浏览器；Web 已能浏览命令、编辑 Profile，并启动、增量读取和取消 Kernel/Action 命令，还能按命令查看跨 Host 保留的 CLI/Web 历史日志。命令页使用可刷新的 `/commands/{source}/...` 深链；Finder 将“命令层级”和“当前命令视图”分成两个维度：只有路径末端命令会在原行下展开其实际能力对应的本地视图，普通命令为子命令、概览、帮助、可选执行和日志，Profile 变量命令为设置、概览和帮助；祖先命令保持折叠，Finder 后续列只表示真实子命令。Host 会原子发布带 Entry 身份、Boot ID、PID 和回环 URL 的瞬时运行描述；二次启动验证健康端点后由新进程打开控制台，Web 同时显示 Host 状态并提供显式退出入口，因此托盘不是唯一恢复路径。Control 仍由受限 Core handler 或专用 API 承担，不作为任意 Entry Worker 命令开放。
3. `_profile.json` 是用户配置的唯一来源；相关环境输入变化会同步使 `.dev.setup` Provider State 失效。`.dev.setup` 的 `env.cmd`、`env.ps1` 位于自身 `export/`，`.dev.bun`、`.dev.cargo`、`.dev.rustc`、`.dev.cl` 等消费者通过统一检查后加载。
4. 当前执行器支持 `run.exe`、由当前 Entry 开发环境执行的 Action `run.ts`、Kernel 专用 `run.toolchain.json`、`run.ps1` 和受限的 `run.cmd`。Action `run.ts` 在 Guard 完成后严格解析 Entry Profile 与 `.dev.setup` Provider Export，复验受管 Bun、PowerShell、MSVC 与 Rust 的声明、安装元数据和完整文件哈希，并只把当次启用且验证通过的环境映射给 Action；它不依赖调用者碰巧继承的系统 PATH，也不会加载生成的 `env.ps1` 形成第二套解释器。`run.toolchain.json` 不执行目录脚本，而是固定调用同 Release Set 的 `swawkit-proj-toolchain.exe command-v1 <handler>`，Catalog 同时限制 handler 白名单与 Kernel 所有权。`.dev.status` 与 `.dev.setup` 已完成垂直迁移，原 PowerShell 入口均已删除。Bun/PowerShell 的 selection、安装元数据、文件清单、重解析点、完整 SHA-256 与信任分类收敛为一套 Archive Tool SSOT，其中 Core `run.ts` 消费 Bun，Toolchain 消费 Bun 与 PowerShell。MSVC 与 Rust 也分别形成原生领域闭环：受限来源、内容寻址缓存、严格解包或隔离安装、规范元数据、精确文件清单、完整 SHA-256、中断恢复、原子发布和环境映射共用各自读写契约；已就绪或可恢复的安装保持完全离线。四个默认领域由同一个原生总编排管理：共用 setup 锁和 Provider CAS，先完整预检所有启用声明，再按固定领域顺序离线优先解析或恢复安装，发布字节稳定的 `env.cmd` / `env.ps1` 后才完成 ready；CLI 与 Web Worker 的下载进度通过统一事件协议记录和渲染。只有 Toolchain 尚不存在时的冷 Bootstrap 继续保留系统原生 Shell 实现。
5. `proj.build.app` 已迁移为 `run.ts`，直接消费上述受信开发环境，不再依赖 `_toolchain` 私有 PowerShell；它构建高频 CLI/Worker Core、常驻 Web/Tray Host 与低频原生 Toolchain，分别发布为 `swawkit-proj.exe`、`swawkit-proj-host.exe`、`swawkit-proj-toolchain.exe`，并组成不可变 Release Set。`proj.publish.app` 校验整组 Manifest 后写入 `_bin/releases/<release-id>/`，最后只原子切换 `_bin/current`；`_bin` 根目录不再保留可执行 bridge。当前维护的 Entry Launcher 已原地迁移到 selector 协议并保持文件身份，`proj.publish.launcher` 继续只更新新建 Entry 模板。失败构建不会授予 Ready 状态。正式测试同时覆盖 Launcher/Core/Host/Toolchain 协议、模块 Export 与 Provider State、Entry 环境 Bun 与 Action `run.ts`、Entry Host 单实例、Entry Worker 输出和整棵进程树取消。

## 六、Web 与执行边界

1. Web 使用固定 Finder 式界面，目录命令模块通过 `_view/web.json` 提供展示提示。命令行只在自身成为路径末端时原位展开 UI 拥有的视图菜单；有真实子命令时默认选择“子命令”，普通叶子命令默认选择“概览”，Profile 变量叶子命令默认选择“设置”，“执行”永不自动触发且只对 Kernel/Action 开放。继续选择子命令后，父命令菜单收起并追加下一列。URL 路径编码可分享、可刷新和可前进后退的命令身份，非默认视图以 `?view=edit|overview|help|run|logs` 表达；默认视图、临时表单与运行状态不进入 URL。视图标签不属于 Catalog 地址，因此不会与英文命令名冲突。Profile 变量的 Finder 摘要、Web 帮助与 CLI 帮助共同读取叶子命令目录的 `_help/zh-CN.txt`，Web 不维护第二份领域文案。Web 提交 Catalog 中的命令地址和参数，解释器、磁盘路径、cwd 和环境变量由 Core 解析。Host 状态与退出使用受 Host 头约束、要求显式控制请求头的专用本地 API；单实例租约只负责互斥，不再兼任无法确认结果的激活通道。
2. Web run 使用 `swawkit.command-run/v1` 表达当前 Host 内的运行标识、状态、增量事件和退出结果。每次运行都从当前用户环境基线启动新的 Entry Worker，并过滤已有的 Swaw Kit 环境命名空间；Host 负责请求与生命周期，具体命令仍经过完整 Entry 边界。
3. Entry Worker 使用 Windows Job Object 管理整棵子进程树；取消和 Entry Host 退出都会回收后代进程。stdin 关闭，stdout/stderr 由执行边界截获并转换为 UTF-8 `output` 事件；启用 `swawkit.command-event-frame/v1` 的第一方模块还可在同一管道发送严格、可回退的 `progress` 帧，非法或不完整帧按普通文本保留。CLI 将进度渲染为行式状态，Web 实时窗口按进度 ID 原位更新，历史日志保留每次状态变化；三者共享同一 Journal 事件身份和容量上限。当前 `.dev.setup` 下载已接入该协议，直接绕过 Core 调用脚本时仍保留原有控制台文本；当前执行模型面向非交互任务。
4. 当前 Host 的 run registry 只负责活跃运行、取消和短期内存窗口；持久事实由模块 `_runs/` 中的 Run Journal 承担。CLI 在执行器边界边回显边记录，Web 则由 Host 在 Worker 外层记录，因此 Worker 启动失败和取消也能形成一致终态。Web 的日志视图通过 `/api/v2/command-run-journals` 读取最近 32 次摘要和游标增量事件；命令身份由单值 `command=kernel/.dev.status`（或 `action/proj.build`）同时指定来源与地址，Catalog 一次扫描即可定位模块，读取历史不要求命令当前仍可运行。界面将运行记录索引与单次运行内容分为相邻两列，以时间为运行记录的首要识别信息；“打开目录”只属于右侧已选运行详情，不改变左侧 Finder 记录的统一选择行为。其中“事件”表示持久化的 output/progress 事件记录数，不等同于文本换行数。Agent 与脚本通过前缀元命令 `swawkit .logs <command-address>` 查询同一事实源，`--latest <n|n..m>` 提供单次调用内的相对查询，稳定自动化使用 `--run <run-id> [--after <cursor>]`，`--open <run-id>` 在校验身份后打开对应 `_runs/<run-id>/`；磁盘日志不依赖当前 Host 生命周期。Proj 帮助同样采用不会误执行目标的前缀形式 `swawkit .help <command-address>`，而 `<command-address> --help` 保留给模块自身解释。
5. `.swaw` Action 是以当前用户权限运行的受信任项目代码；Web 约束调用入口和生命周期，但 Action 本身不是安全沙箱。Run Journal 是执行可观测性，不替代模块自身的业务状态、Provider State 或 Export。

## 七、下一步

1. 以真实命令为驱动扩展 `_view/web.json` 的最小交互协议，逐步覆盖参数类型、确认提示和结果展示；不要把临时输入和输出塞进 URL。
2. Run Journal V1 先通过 8 MiB 单次上限约束失控输出；积累真实使用量后，再定义跨进程安全的按模块保留数量、清理入口和“进程异常退出后未完成”的对账规则，不提前加入常驻日志服务或 OpenTelemetry SDK。确有跨进程 Trace 或外部采集需求时，再从当前事件协议映射到 OTLP。
3. `.dev.setup` / `.dev.status` 已验证 `run.toolchain.json` 的完整垂直路径，`proj.build.app` 已验证完整开发环境驱动 Action `run.ts` 的垂直路径；下一批按真实收益迁移仍依赖 `_toolchain` 私有 PowerShell 的 Action，并为默认四领域补长期运行、取消和真实网络故障验收。冷 Bootstrap 的 Rust/MSVC 准备逻辑继续保留在系统原生 Shell。删除通用 `run.ps1` / `run.cmd` 适配器前，先决定 `run.py` 的运行时所有权并形成剩余入口迁移清单，避免长期兼容层。
4. 继续收窄现有 Action 对 `_toolchain` 私有实现的直接依赖；等 Launcher 出现真实发布消费者时，再按 App 已验证的模式增加对应 publish 入口，不预先制造通用资产框架。
5. 为不可变 Release Set 增加显式保留与清理命令：只删除不再由 `current` 指向、且未被旧 Host 映射的目录；不要把生命周期判断塞进每次发布。
6. 长期目标是保持一个正式 Core、一套配置与命令协议，让 CLI、Web 和后续入口共享同一套行为语义，同时让每个目录化模块独立拥有自己的领域数据与演进节奏。
