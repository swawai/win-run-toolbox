# Swaw Kit Proj：架构与源码导读

## 一、项目是什么

1. Swaw Kit Proj（根目录 Entry + `_lib/proj`）是一个以项目为对象的本地控制面，通过同一套 Core 向 CLI 和 Web 提供项目配置、开发环境和功能命令；`Favorites/template.proj1.exe` 只作为待复制模板发布。
2. 一个 Entry 对应一套相互隔离的项目资源：目标项目、功能命令、Git 身份、DataRoot、开发环境，以及随项目维护的 skills 和提示词。Entry 必须直接位于 `SWAWKIT_HOME`；创建 Entry：在根目录执行 `copy Favorites\template.proj1.exe MyProjEntry.exe`。
3. Entry 是约 10 KB 的原生 Launcher，只负责读取 `_lib/proj/_bin/current`、确定不可变 Release Set 中的共享 Core、传递自身路径与原始参数，并建立必要的启动语义。Web Worker 的 Job Object 由 Rust 父进程在恢复 Launcher 前建立。若 selector 尚未生成，Launcher 会调用 `_lib/proj/bootstrap.ps1`，由仓库内固定的 Rust/MSVC 工具链完成首次三件套构建和发布。
4. 运行主链可以记作：`Entry Launcher -> Rust Core（CLI / Host / Worker）-> Catalog -> Control / Kernel / Action`。Bootstrap 只是 Core 缺失时的恢复路径，不参与日常命令逻辑。
5. Rust Core 根据 Entry 名和 Windows File ID 识别入口，并在 `data/proj.{入口名}/` 管理专用 DataRoot；复制产生新 Entry，改名仍延续原身份。同一 Entry 只保留一个 Entry Host，普通 CLI 和 Entry Worker 则是按次创建的短生命周期进程。
6. Entry Profile 记录目标项目、界面语言和开发环境等用户配置，CLI 与 Web 共用这份配置；项目自定义命令从目标项目的 `.swaw` 中发现。

理解后续设计，先记住六个核心名称：

| 名称 | 定义 |
| --- | --- |
| **Entry** | 一份稳定入口身份，以及与之绑定的 Profile、DataRoot 和单实例 Host 边界 |
| **目录命令模块** | 以目录为最小领域单元；目录层级定义地址，`run.*`、`_module.json`、Guard、Help、View 和私有实现就近组织 |
| **Catalog** | Core 从目录树生成的统一命令读模型，供 CLI 与 Web 共同发现和定位能力 |
| **模块数据根** | Core 为每个目录命令模块派生的专属数据路径，与命令来源和相对目录同构 |
| **模块 Export / Provider State** | 模块对外发布的稳定资源，以及描述该发布当前是否可消费的状态协议 |
| **Subject / Facet** | Subject 是静态命令或动态对象的统一身份；Facet 是该 Subject 可浏览、投影或执行的能力面，精确映射到 Catalog 关系或既有 CLI 命令 |
| **Entry Host / Entry Worker** | Host 提供单实例 Web 控制面；Worker 按次通过完整 Entry 链执行命令 |

架构记忆句是：**协议集中，领域自治**。Core 统一身份、地址、生命周期和进程协议；目录命令模块拥有行为、数据、状态与 Export。进一步可以记成：**目录定义能力，数据跟随模块，依赖通过 Export 连接，环境按次生成。**

## 二、代码地图与阅读顺序

中心代码只维护必须统一的事实和协议，例如 Entry 身份、DataRoot、Profile、Catalog 与进程执行边界；具体能力、状态和演进节奏归属各自的目录命令模块。

| 路径 | 主要职责 |
| --- | --- |
| `Favorites/template.proj1.exe` | 随仓库发布的 Launcher 模板；复制出的 Entry 必须放在 `SWAWKIT_HOME` 根目录 |
| `_lib/proj/_launcher` | 原生 Launcher 源码；构建候选写入 `data/proj_cache/bootstrap/build/launcher/release/template.proj1.exe` |
| `_lib/proj/bootstrap.ps1` / `bootstrap.json` | Core 缺失时使用固定工具链构建并发布共享 Rust Core |
| `_lib/proj/build.ps1` | 使用固定工具链构建 App 与 Launcher 候选，不发布正式制品 |
| `_lib/proj/_toolchain` | 受管工具链及模块 Export、状态、锁、环境生成与激活等共享机制 |
| `_lib/proj/_app` | Rust 产品源码，按 Core、Host 与 Toolchain 领域承载身份、DataRoot、Profile、Catalog、命令执行、Web 生命周期及受管工具能力 |
| `_lib/proj/_bin` | Bootstrap 或显式发布产生的共享运行时制品 |
| `_lib/proj/..runtime` | Runtime 与 Host 的有状态 Control：聚合状态、退出、重启和显式 Release 清理 |
| `_lib/proj/.dev/{setup,status,bun,rust,msvc,exec,cmd,pwsh,...}` | 管理受管开发环境、类型化工具设置，并提供专用工具与一次性进程入口 |
| `_lib/proj/.help` / `.check` | 分别读取命令说明，以及只读检查模块声明、Guard 结构、依赖 Provider 与 Export 当前状态；目标地址都使用前缀参数形式 |
| `_lib/proj/.runs` | 查询所有命令或指定命令的持久 Run Journal，并为 Web 提供全局与命令范围的 Run 集合 |
| `_lib/proj/.context` | Agent Context 领域模块；静态子命令负责行为，根模块以 Facet 暴露 Context 集合，动态 `::context/<id>` Subject 携带自身 Facet；`show` 输出持久结构，`render` 生成确定性 Agent Markdown |
| `_lib/proj/.<name>` / `.swaw/<name>` | Kernel 命令 / 目标项目的 Action 命令 |
| `_lib/proj/_test` | 原生 Entry、Rust Core、Entry Worker、Bootstrap 和工具模块的正式回归入口 |

建议沿主链阅读源码：先看 `_launcher/launcher.c` 和 `bootstrap.ps1`，再进入 `_app/src/launch.rs`、`main.rs`、`cli.rs`，随后按问题深入 `catalog`、`data_root`、`profile`、`command`、`entry_runner` 或 `server`。

命令分为三类：Control 以 `..` 开头并管理 Entry/Host；Kernel 在 `_lib/proj` 中定义且以 `.` 开头；Action 在目标项目的 `.swaw` 中定义且没有前缀。Core 扫描这些目录并忽略下划线开头的私有实现。Kernel/Action 可执行模块只提供一个 `run.*` 入口，Control 使用受限的 `run.core.json`；`_module.json`、`_guard`、`_help`、`_view/web.json` 和私有库与所属命令放在一起，由 Catalog 汇总为统一模型。

## 三、开发取向

1. 框架保持一条清晰主路径和一个事实源；新路径成立后，旧实现应有明确的退出点。
2. Core 只承载跨模块协议和必要生命周期；目录命令模块拥有领域行为、数据和 Export。共享库负责路径校验、原子发布、锁和状态检查等通用机制。
3. 模块依赖由消费方在 `_module.json` 就近声明，并指向明确的提供命令和 producer contract；提供方在同一协议声明自己提供的 contract。稳定概念出现第二个真实消费者后再抽象，AHA 优先于 DRY，局部清晰优先于通用包装。

## 四、关键不变量与协议

1. **身份与配置。** Entry 以 Windows File ID 保持身份：复制产生新身份，改名延续原身份，文件被替换时才要求显式认领（claim）。`_entry.json` 只记录 Entry 身份，`_profile.json` 只记录用户配置；运行时环境变量不能反过来成为配置来源。
2. **目录即命令协议。** 命令地址由目录层级唯一推导，一个目录最多有一个规范执行入口；Module、Help、Web 展示提示、Facet 和 Guard 都是模块的伴随声明。Catalog 以 `swawkit.command-catalog/v13` 生成结构性的 `runnable`、`diagnostic`、语言、`facets`、`subjectKinds` 与可选 `module`；静态子命令关系和模块声明的动态集合都收敛为 `kind=collection` 的 Facet，Finder 不再自行猜测或合成另一套能力。Help 从模块自己的 `_help/{zh-CN|en}.txt` 按 Entry Profile 选择，英文译文缺失时回退简体中文。`_module.json` 使用 `swawkit.command-module/v4` 声明 `requires`、`provides`、Command Facet 与 Instance Facet 模板；collection 的 `subjectKind` 是包含 `kind + provider Command SubjectRef` 的显式类型引用，允许不同命令通过同一个可信 Provider 复用一种 Instance 模板，而不是复制 resolver 或按字符串暗中全局查找。同 ID 的模块 Facet 可明确替换 Core 默认 Facet，声明无效时直接形成诊断，不静默回退。`_view/web.json` 使用 `swawkit.command-view/web/v4`，只保留列宽和运行表单等展示提示，不再声明能力。Control 的 `run.core.json` 使用 `swawkit.core-command/v1`，Kernel 中由 CLI 在普通执行器之前消费的精确内置命令可声明受限 Core handler，其他 Kernel 产品命令可用 `run.toolchain.json` 与 `swawkit.toolchain-command/v1` 静态选择同 Release Set 中的受限 Rust handler。全局与模块 Guard 在执行前判断运行前提。
3. **模块数据根与动态 Subject。** Core 按命令来源和相对目录，将每个目录命令模块同构映射到 `DataRoot/modules/{control|kernel|action}/...`。例如 `.dev.setup` 对应 `modules/kernel/.dev/setup/`。`SWAWKIT_PROJ_DATA_ROOT` 表示 Entry 根，`SWAWKIT_PROJ_CORE_COMMAND_DATA_ROOT` 表示当前模块数据根；普通命令的工作数据、状态、锁和 Export 都归入后者。统一对象模型只有三个角色：`SubjectRef` 表达身份，`Facet` 表达一个 Subject 可浏览、投影或执行的能力，`SubjectCollection` 表达某个 collection Facet 的解析结果。静态命令引用为 `{type=command, source, address}`；动态对象引用为 `{type=instance, kind, id}`，规范显示投影是 `::kind/id`，不携带发现它的 owner，也不伪装成 CLI 命令。集合保留 `owner + facet` 来表达“从谁的哪个关系发现”。`swawkit.subject-collection/v2` 刻意只支持静态 Command owner 到一层 Instance；成员只返回 `facetIds`，不能携带 resolver，Core 与 Web 必须从 collection Facet 显式引用的 Provider Catalog `subjectKinds` 模板重建 Facet，并通过类型化 `{bind="subject.id"}` 绑定当前实例。`kind` 在一个 Catalog 中全局唯一；集合可以按资源状态返回模板能力的子集。Instance 暂不暴露 collection Facet；真实嵌套对象需求出现后，必须连同 provenance 与 URL 深链一起设计下一版，不能制造不可恢复的半递归。`.context/_module.json` 直接声明 `contexts` collection Facet 和 `context` Instance 模板，resolver 分别映射 `.context.list --json` 以及 `.context.show/.context.add/...` 等真实 CLI 能力；资源目录中的 `_resource.json` 只保存 Context 状态。其持久目录仍是 `modules/kernel/.context/mycontext01/`，目录内部格式和操作由 Context 领域自治。Core 不扫描 DataRoot 生成动态命令，不认识 context/run/artifact 类型，也不再维护 `resource.core.json` 或按资源类型硬编码注册表。只有 Core 缺失时的 Bootstrap 构建和确实需要跨 Entry 复用的下载缓存进入 `data/proj_cache/`。
4. **模块 Export 与显式依赖。** 模块把稳定候选或供其他模块消费的产物发布到自身的 `export/`，中间文件留在 `work/`。消费方 `_module.json` 的 `requires` 指向 Provider 命令地址和精确 contract，提供方的 `provides` 声明自身 contract；Catalog 与 Web 因而无需读取实现源码即可展示静态关系。`export/` 只是稳定发布边界，不自动等于一套资产系统：需要自动消费的构建产物可由模块自己的 `manifest.json` 声明文件名、长度与 SHA-256，并由提供命令地址、`producerContract` 和 `_state.json` 建立可验证发布边界。`.check <command-address> [--json]` V1 只读遍历依赖闭包，复验公共的声明、Provider State、producer contract 与安全的 Export 根目录，并有界列出顶层产物；它既不猜测模块私有 Manifest，也不单独执行 `_guard`，避免“检查”产生业务副作用。依赖尚未 Ready 时，错误信息直接给出应执行的提供命令；需要内容级完整性校验的模块仍由自身 Guard 或消费路径负责。
5. **Provider State。** `_state.json` 使用 `swawkit.command-provider-state/v1`，状态为 `unavailable` 或 `ready`。影响开发环境的 Profile 输入变化时，`.dev.setup` 状态同步变为 `unavailable`；重新执行 `.dev.setup` 后，完成的 Export 与 `ready` 状态一起成为新的有效发布。`inputRevision`、`token` 与 `producerContract` 共同标识这次发布并保证消费一致性。
6. **环境变量是边界，不是状态。** 除系列根目录使用 `SWAWKIT_HOME` 外，Proj 拥有的变量统一使用 `SWAWKIT_PROJ_` 前缀：`SWAWKIT_PROJ_CORE_LAUNCH_*` 传递一次性启动声明，`SWAWKIT_PROJ_CORE_COMMAND_*` 描述单次命令上下文，`SWAWKIT_PROJ_MODULE_*` 用于模块私有适配。Core 在读取 Launcher 声明后、创建线程前清除进程中的 `SWAWKIT_HOME` 与全部 `SWAWKIT_PROJ_*`；Host 的长期事实保存在类型化对象和 DataRoot。每次执行命令时，Core 再从当前 Entry/Profile 生成命令环境，适配器私有变量在消费后清除。
7. **执行一致性。** 普通 Kernel 和 Action 依次执行全局 Guard、模块 Guard 和 `run.*`；Guard 只检查前提，不安装工具或修复状态。CLI 直接进入这条执行链，Web 则通过同一个 Entry Launcher 创建 Entry Worker，再进入同一条链，因此两者共享解释器、cwd、Profile、DataRoot 与环境语义。
8. **持久运行日志。** CLI 和 Web 运行都在所属模块数据根的 `_runs/{run-id}/` 写入同一套事实：`events.jsonl` 使用 `swawkit.command-run-event/v1` 追加带时间和阶段的 `kind=output|progress` 事件；`output` 保存 stdout/stderr UTF-8 文本，`progress` 保存稳定 ID、状态、数值、单位和消息。`_state.json` 使用 `swawkit.command-run-journal/v1` 原子发布来源、起止时间和终态。Journal 统一生成 sequence 与时间戳；CLI 控制台、Web 实时窗口和历史查询消费同一事件，而不是各自重新编号或从日志文件轮询实时输出。新运行先在隐藏 work 目录准备完整 events / state，由写入者在 `_runs/` 根独占持有同 Run ID 的 owner 租约，最后一次目录重命名才发布 `{run-id}`。进程异常结束后操作系统释放租约，下一次历史读取会只在取得该精确租约后，去掉未完整的 JSONL 尾记录、同步完整事件并原子收敛为带明确原因的 `failed`；缺少租约的旧版记录不猜测改写。原始参数不落盘，只记录数量；每次运行最多保留 8 MiB 事件文件，达到上限后明确标记 `truncated`。日志写入是执行契约的一部分，初始化或完成日志失败会使本次运行失败，而不是静默丢失。
9. **构建与发布分离。** build 只产出候选文件，显式 publish/update 命令才可替换正式制品。`proj.publish.app` 只消费 `proj.build.app` 的 Ready Provider 与匹配 Manifest，在共享锁内原子切换 Release Set selector；旧进程继续运行已映射版本，新调用使用新版本。工具下载、安装、更新和主动清理由显式命令触发，受管工具始终使用已验证的受管版本，而不是系统 PATH 中的同名程序。
10. **有状态操作只发布完整可用结果。** 产品拥有的 Control / Kernel 状态遵循 `prepare -> validate -> commit -> recover`：候选与中间文件留在 `work/`、staging 或显式 `unavailable` 状态，锁或 CAS 固定并发输入，最后一个原子 selector / State 才授予可用性；提交前失败应恢复旧状态，无法证明所有权时宁可保留不可用现场而不猜测删除。提交后的旧备份或垃圾清理失败只报告告警，不撤销已经完整可用的新结果。多项独立清理不是伪装成全局事务，而应保证每项先隔离、可重试且最终收敛。Action 仍是受信任项目代码；Core 能统一生命周期和数据边界，但其业务状态原子性由模块实现、模板与故障测试约束。

## 五、当前做到哪里

1. Native Launcher 和 Rust Core 已接管正常启动、Entry 身份、DataRoot、Profile、命令发现、Guard、CLI、单实例 Entry Host 与 Entry Worker 主链。DataRoot Create / Claim 在发布绑定记录前固定目录身份；首建或重命名后的记录发布失败会按同一目录身份尝试精确回滚，无法安全恢复时保留不可用现场并报错，不猜测删除。成功路径则把同一目录租约连续传递到最终绑定，避免先发布再重新打开目录的空窗。
2. Entry Host 采用 Axum、Tray 和系统浏览器；Web 已能浏览命令、编辑 Profile，并启动、增量读取和取消 Kernel/Action 命令。Catalog v13 将当前语言、真实 Facet、Instance Facet 模板以及 `_module.json` 的依赖、产物和能力契约交给同一界面。命令本身的地址、调用方式和静态 Module 契约直接作为基础详情呈现，不再伪造一个不可执行的 `overview` Facet；真正需要实时解析的检查由 Core 为适用的 Kernel / Action Command 生成 `check` projection Facet，精确执行 `.check <command-address> --json`，并按 `swawkit.module-check/v1` 结构化渲染可运行状态、Guard、依赖和发布物。命令页使用可刷新的 `/commands/{source}/...` 深链；Finder 只渲染 Subject 已声明的 Facet：`collection` Facet 在点击后调用自身 resolver 组织下一列，`projection` Facet 返回声明协议的文档，`operation` Facet 映射到真实 CLI 执行边界。静态 children 由 Catalog relation 解析；动态文档统一通过 `POST /api/v2/facet-resolutions` 懒加载，客户端只提交结构化 Subject、Facet ID 与发现路径，Host 必须从 Catalog 模板和上游 SubjectCollection 的 `facetIds` 重建受信 resolver，不能接受客户端或资源数据自报命令与参数。`.context` 与 `.runs` 分别是持久业务资源和持久运行事实的完整垂直样例；Web/协议层都不内建 context 或 run 类型，只按声明的返回协议选择投影渲染器。Subject 操作中的固定 ID 参数不可编辑，只有 `acceptsTail` 明确允许的尾参数交给用户输入。顶部面包屑与 Host 状态栏已移除，避免出现脱离命令模型的第二套导航和控制面；`..runtime` 根命令直接呈现 Host / selector 聚合状态，`..runtime.host.exit`、`..runtime.host.restart` 与 `..runtime.cleanup` 承接相应控制。Host 会原子发布带 Entry 身份、Boot ID、PID 和回环 URL 的瞬时运行描述；二次启动验证健康端点后由新进程打开控制台。新 Release 已发布时，安全重启由短命协调进程先固定旧 Host 进程句柄并确认就绪，旧 Host 才释放单实例租约，协调进程随后通过原 Entry Launcher 启动新 Release，因此不依赖睡眠、PID 猜测或任务管理器。Control 仍由受限 Core handler 或专用 API 承担，不作为任意 Entry Worker 命令开放。
3. `_profile.json` 使用 `swawkit.entry-profile/v2`，是用户配置的唯一来源；公开设置使用 `.dev.bun.mode`、`.dev.rust.toolchain`、`..entry.git.name`、`..entry.language` 等类型化命令地址，`SWAWKIT_PROJ_*` 只作为执行边界变量，不再构成配置 API。语言只接受 `zh-CN` 与 `en`，同时驱动 CLI Help、Catalog 和 Web；`repository.remote` 交还 Git 配置这个事实源，没有实际消费者的默认 IDE/Shell 设置不再保留。相关环境输入变化会同步使 `.dev.setup` Provider State 失效；语言和 Git 等非 Provider 输入不会使开发环境失效。`.dev.setup` 仍以一个锁和一次 Provider CAS 原子协调完整项目环境，其 `env.cmd`、`env.ps1` 位于自身 `export/`。`.dev.bun`、`.dev.rust.cargo`、`.dev.rust.rustc`、`.dev.msvc.cl` 等专用消费者复验各自依赖闭包后运行，`.dev.exec` 明确提供完整开发环境中的直接进程入口。
4. 当前执行器支持 `run.exe`、由当前 Entry 开发环境执行的 Action `run.ts`、Kernel 专用 `run.toolchain.json`、由 PowerShell 7 执行的 `run.ps1` 和受限的 `run.cmd`。`run.ts` 与 `run.ps1` 都在 Guard 完成后严格解析 Entry Profile 与 `.dev.setup` Provider Export，复验当次启用的受管 Bun、PowerShell、MSVC 与 Rust 声明、安装元数据和完整文件哈希，再映射开发环境；它们不加载生成的 `env.ps1` 形成第二套解释器。`SWAWKIT_PROJ_PWSH_MODE` 只有 `managed`、`system`、`disabled`：managed 使用 `.dev.setup` 发布的受管 PowerShell，system 只接受 PATH 中探测确认为 PowerShell Core 7+ 的 `pwsh.exe`，disabled 使全部 `run.ps1` 在 Catalog 中不可运行；任何模式都不回退 Windows PowerShell 5.1。`run.toolchain.json` 不执行目录脚本，而是固定调用同 Release Set 的 `swawkit-proj-toolchain.exe command-v1 <handler>`，Catalog 同时限制 handler 白名单与 Kernel 所有权。`.dev.status` 与 `.dev.setup` 已完成垂直迁移，原 PowerShell 入口均已删除。Bun/PowerShell 的 selection、安装元数据、文件清单、重解析点、完整 SHA-256 与信任分类收敛为一套 Archive Tool SSOT，其中 Core 消费 Bun 与 PowerShell，Toolchain 消费 Bun 与 PowerShell。MSVC 与 Rust 也分别形成原生领域闭环：受限来源、内容寻址缓存、严格解包或隔离安装、规范元数据、精确文件清单、完整 SHA-256、中断恢复、原子发布和环境映射共用各自读写契约；已就绪或可恢复的安装保持完全离线。四个默认领域由同一个原生总编排管理：共用 setup 锁和 Provider CAS，先完整预检所有启用声明，再按固定领域顺序离线优先解析或恢复安装，发布字节稳定的 `env.cmd` / `env.ps1` 后才完成 ready；CLI 与 Web Worker 的下载进度通过统一事件协议记录和渲染。只有 Toolchain 尚不存在时的冷 Bootstrap 继续保留系统原生 Shell 实现。
5. `proj.build.app` 与 `proj.publish.app` 均已迁移为 `run.ts`，直接消费上述受信开发环境，不再依赖 `_toolchain` 私有 PowerShell。前者构建高频 CLI/Worker Core、常驻 Web/Tray Host 与低频原生 Toolchain，分别发布为 `swawkit-proj.exe`、`swawkit-proj-host.exe`、`swawkit-proj-toolchain.exe`，并组成不可变 Build Release Set；后者在构建 Provider 锁内复验 Ready State、selector、Manifest 与完整文件哈希，再写入 `_bin/releases/<release-id>/`，最后只原子切换 `_bin/current`。`_bin` 根目录不保留可执行 bridge，旧进程继续运行已映射版本。当前维护的 Entry Launcher 已原地迁移到 selector 协议并保持文件身份，`proj.publish.launcher` 继续只更新新建 Entry 模板。失败构建不会授予 Ready 状态。正式测试同时覆盖 Launcher/Core/Host/Toolchain 协议、模块 Export 与 Provider State、Entry 环境 Bun 与 Action `run.ts`、Entry Host 单实例、Entry Worker 输出和整棵进程树取消；CLI 在恢复命令根进程前先将其加入本次运行专属的 Windows Job Object，控制台取消由 Core 回收整棵命令进程树并把 Journal 收敛为 `canceled`，既有薄 Launcher 无需升级，`.dev.setup` 还覆盖 setup 锁等待时取消、确定性网络拒绝、显式公网冷下载和断网缓存重装。

`run.py` 当前只作为未来入口被 Catalog 识别并显示“受管 Python 尚未就绪”的诊断，不会向 CLI/Web 暴露虚假的可执行能力；其启用门槛是 `.dev.setup` 完整拥有 Python 的版本、来源、安装元数据和文件哈希。

## 六、Web 与执行边界

1. Web 使用固定 Finder 式界面，目录命令模块通过 `_module.json` 声明公共 Facet，通过 `_view/web.json` 提供纯展示提示和少量运行表单配置。命令只在自身成为路径末端时原位展开 Catalog 已解析的 Facet；有真实子命令时默认选择 `children` 集合，Profile 设置叶子命令默认选择“设置”，普通叶子命令不自动选择 Facet，而是直接显示命令基础详情，“执行”永不自动触发且只对 Kernel/Action 开放；`..runtime` 是明确例外，它直接显示聚合状态基础详情。动态 Subject 集合与静态子命令集合使用同一 Finder 投影：选择 collection Facet 时懒加载下一列，选择 projection 或 operation Facet 时打开对应详情；选择动态 Subject 后保持发现它的 owner collection Facet 选中，并在 Subject 行下展开它自己的 Facet。URL 路径只编码稳定静态 owner 命令，查询参数各负其责：`facet=<owner-facet>`、`subject=::kind/id`、`subject-facet=<subject-facet>`；高频增删对象不进入目录命令路由，默认 Facet、临时表单与运行状态也不进入 URL。Finder 摘要、Web 帮助与 CLI 帮助共同读取叶子命令目录的 `_help/{language}.txt`，Web 不维护第二份领域文案；Web 自身固定控件由同一 Entry language 切换。通用 `web/v4` run 配置仍只服务 Kernel/Action 的固定 argv；Runtime 生命周期是 Control 领域，由专用本地 API 和受限 Core handler 承接，不伪装成普通命令执行。`swawkit.runtime-status/v1` 聚合 selector、Release 数量和可选 Host 状态；`swawkit.host-status/v1` 同时公开运行 Release 与 selector Release，避免把“已发布”误报成“已运行”。cleanup 的 preview/apply 通过显式控制头区分，最终路径校验、占用复验和删除边界仍由低频 Toolchain 单点实现。单实例租约只负责互斥，不再兼任无法确认结果的激活或更新通道。
2. Web run 使用 `swawkit.command-run/v1` 表达当前 Host 内的运行标识、状态、增量事件和退出结果。每次运行都从当前用户环境基线启动新的 Entry Worker，并过滤已有的 Swaw Kit 环境命名空间；Host 负责请求与生命周期，具体命令仍经过完整 Entry 边界。
3. Entry Worker 使用 Windows Job Object 管理整棵子进程树；取消和 Entry Host 退出都会回收后代进程。stdin 关闭，stdout/stderr 由执行边界截获并转换为 UTF-8 `output` 事件；启用 `swawkit.command-event-frame/v1` 的第一方模块还可在同一管道发送严格、可回退的 `progress` 帧，非法或不完整帧按普通文本保留。CLI 将进度渲染为行式状态，Web 实时窗口按进度 ID 原位更新，历史日志保留每次状态变化；三者共享同一 Journal 事件身份和容量上限。当前 `.dev.setup` 下载已接入该协议，直接绕过 Core 调用脚本时仍保留原有控制台文本；当前执行模型面向非交互任务。
4. 当前 Host 的 run registry 只负责活跃运行、取消和短期内存窗口；持久事实由模块 `_runs/` 中的 Run Journal 承担。CLI 在执行器边界边回显边记录，Web 则由 Host 在 Worker 外层记录，因此 Worker 启动失败和取消也能形成一致终态。`.runs/_module.json` 是 Run 类型和浏览能力的唯一声明：它自己的 `all` collection Facet 通过 `.runs --json` 汇总最近 32 次持久运行；Core 还为每个可运行 Kernel / Action Command 生成 `runs` 上下文 collection Facet，通过 `.runs --json <source/address>` 只返回该命令的运行。因此 `.runs` 在 Web 中同时拥有“全部运行记录”和自身的“运行记录”，两者范围明确。两种关系都显式引用 `.runs` 提供的 `run` SubjectKind，并返回 owner 无关的同一 `::run/<run-id>` 身份；模块声明同 ID Facet 时可替换默认行为。每个成员只携带模板内的 `overview/open` Facet ID；`overview` 通过 `.runs --run <run-id>` 返回 `swawkit.command-run-journal/v1`，Web 只按返回协议选择 Run 投影渲染器，`open` 则通过普通 operation Facet 调用 `.runs --open <run-id>`。专用 Journal Web API 已移除，全局与上下文入口都使用同一个 Facet resolution、SubjectKind 模板和 Run resolver，不构成第二套寻址或权限路径。Run ID 使用纳秒时间、进程 ID 与进程内序列组合，新写入格式在一个 DataRoot 中具备足够强的全局身份；读取时仍兼容旧 ID，并对跨命令重复 ID 明确报歧义。`.runs` 无参数直接输出全局最近运行；命令范围查询使用 `swawkit .runs <command-address>`；`.runs <command-address> --latest <n|n..m>` 提供单次调用内的相对查询，稳定自动化使用 owner 无关的 `.runs --run <run-id> [--after <cursor>]`。通过 `swawkit .check <command-address> --json` 查询依赖和发布事实；磁盘日志不依赖当前 Host 生命周期。Proj 帮助同样采用不会误执行目标的前缀形式 `swawkit .help <command-address>`，而 `<command-address> --help` 保留给模块自身解释。
5. `.swaw` Action 是以当前用户权限运行的受信任项目代码；Web 约束调用入口和生命周期，但 Action 本身不是安全沙箱。Run Journal 是执行可观测性，不替代模块自身的业务状态、Provider State 或 Export。

## 七、下一步

1. `..runtime` 已验证“Catalog 中的有状态 Control + 专用类型化 API + 同一 CLI handler”这条垂直路径。后续只有出现第二个真实的同类控制面后，才提取共享状态卡片或通用控制协议；不要把 Runtime 专有状态提前塞入 `_view/web.json`，也不要制造 mode 驱动的万能 Control 表单。临时输入和输出不进入 URL。
2. Run Journal V1 先通过 8 MiB 单次上限约束失控输出，并已用运行所有者租约收敛新版进程异常退出留下的未完成记录；正式黑盒已能启动真实 Action、确认首条持久事件、强制结束本次测试专属的完整进程树，再通过公共 `.runs --run` 验证原事件保留、`failed` 终态、owner 清理和后代退出。积累真实使用量后，再定义跨进程安全的按模块保留数量与清理入口，不提前加入常驻日志服务或 OpenTelemetry SDK。确有跨进程 Trace 或外部采集需求时，再从当前事件协议映射到 OTLP。
3. `.dev.setup` / `.dev.status` 已验证 `run.toolchain.json` 的完整垂直路径；App 与 Launcher 的 build → publish 均已验证完整开发环境驱动 Action `run.ts`、模块 Provider 消费和原子发布的日常垂直路径。`run.ps1` 的 managed/system/disabled 三模式已有 CLI 黑盒与 Web/Catalog 协议验收；Host Release 黑盒同时覆盖二次启动激活、更新状态、安全重启和无残留退出。默认四领域的长期执行边界现已补齐 CLI 控制台取消、Provider/Journal 收尾、网络失败不发布、Bun latest 公网冷下载及离线缓存重装；公网验收保持显式慢速入口，不加入每次快速回归。冷 Bootstrap 的 Rust/MSVC 准备逻辑继续保留在系统原生 Shell。为受管 Python 建立明确所有权后再启用目前只由 Catalog 诊断的 `run.py`，避免系统 Python fallback。
4. Launcher 的日常 Action 与冷 Bootstrap 构建器共用声明式 `build.json` 编译契约；前者消费 Entry 受管 MSVC，后者只在 Core 不存在时准备固定工具链。不要把 Bootstrap 恢复逻辑重新引入普通 Action，也不要为单一产品制品制造通用资产框架。
5. 不可变 Release Set 已由显式 `..runtime.cleanup` 管理：默认 preview，只有 `--apply` 才删除；Core/Host 只调用版本化的窄协议，低频扫描和删除仍由 Toolchain 单点实现。它与发布共用锁，严格复验 selector、Manifest、成员、长度和 SHA-256，并保留当前 Release、被 Core/Host/Toolchain 进程映射的 Release、破损目录与重解析点。生命周期判断不进入每次发布。
6. 长期目标是保持一个正式 Core、一套配置与命令协议，让 CLI、Web 和后续入口共享同一套行为语义，同时让每个目录化模块独立拥有自己的领域数据与演进节奏。
