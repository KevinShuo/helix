# Git merge conflicts — paused checkpoint

2026-11-12：用户要求“把现在进度保存一下提交到 git，我回家后继续”。
本次只保存、提交并推送检查点，暂停功能开发。**首版尚未完成，不能作为最终验收通过。**

## 分支与续做入口

- 工作目录：`C:/dev/helix`，复用当前 checkout。
- 分支：`codex/git-merge-conflicts`。
- 推送远端：`fork`，`https://github.com/KevinShuo/helix.git`。`origin` 是 helix-editor 上游，不向它推送。
- 功能基线：`4bd21126f8a07447a6817027e568d174beef013e`。
- [设计](specs/2026-11-12-git-merge-conflicts-design.md)、[实施计划](plans/2026-11-12-git-merge-conflicts.md)、[验证记录](2026-11-12-git-merge-conflicts-validation.md)。
- 用户已批准内置三栏、完整实现和测试及最终 push。当前暂停来自用户的新指令，后续续做无需重新确认原方案。

## 已完成与实际状态

| 任务 | 状态 | 提交 |
| --- | --- | --- |
| 1. 真实 index stages / 操作状态 / 不支持类型降级 | 完成 | `d4fbc5876` |
| 2. 三方文本模型 / 选边 / 变更映射 / 格式保真 | 完成，独立审查通过 | `040049003` |
| 3. 安全写入与精确暂存 / retry / Finish / Abort | 完成，独立审查通过 | `1dceb9c4c` |
| 4. 原生 Document / 临时 Tree / 草稿生命周期 | 完成，审查修复后通过 | `8cbcc59bb`、`7eb4f12dd` |
| 5. 冲突列表 / 三栏 UI / 手动快捷键 | 已实现并提交；**任务独立审查尚未开始** | `2f9a85446214ec6d30e71adb96380ea9a822d16e` |
| 6. 操作后自动发现 / Focus 扫描 | 准备中，尚未接线 | 本检查点 |
| 7. 联合验收 / 普通编辑回归 / release / 真终端 | 未完成；测试与文档草稿已保存 | 本检查点 |

可手动 `Space v r` 打开列表，Enter 进入结果原生编辑器，`Space v m` 使用合并动作。
Space g 和原有 Git 入口保留。普通 merge 文本的 Apply、单独 Finish/Abort 已实现。
三栏模块的实现和测试细节见 [Task 5 报告](reports/2026-11-12-git-merge-conflicts-task-5.md)。

Task 5 测试记录：UI 单元 11/11，Git 快捷键 1/1。完整 14 个集成用例的一轮为
13 通过、1 失败；失败原因是暂存后列表顺序变化。稳定排序修复后，该 Finish 用例重跑
1/1 通过，扩展的鼠标预览用例也 1/1 通过。14 项均各自通过，但修复后尚未重跑整个批次。
新加 rebuild 单元、最终 no-git 与广泛回归仍待执行。

## 已保存但尚未接线的代码

1. `helix-view/src/git_conflicts.rs` 已注册到 view 的 lib/Editor：扫描 instance/revision/path、
   取消、重复通知抑制。两个纯状态测试曾通过；新增 `is_running()` 后尚未最终重跑。
2. `helix-term/src/handlers/git_conflicts.rs` **尚未在 handlers.rs 注册**。实现当前工作树的
   200 ms Focus 去抖、信任检查、文档/会话/请求版本校验、仅状态提示和取消 hook。
   它引用的 `ui::git::trusted` 仍为私有，接线时需改为 `pub(crate)`。
3. `helix-term/tests/test/git_conflicts_discovery.rs` **尚未注册为 git_conflicts 的子模块，也未运行**。
   包含 Focus、不抢焦点、路径为空的草稿、merge 菜单自动列表、普通错误、关闭面板、切仓库、
   已确认操作在应用退出时完成等场景。先注册并跑行为 RED，再实现。
4. `helix-term/tests/test/git_conflicts_acceptance.rs` **尚未注册，也未运行**。
   它要求 fixture 有 `clean.txt` 自动合并文件；现有 `repository()` 只有两个冲突文件，
   还需补 fixture：base/main 为 `clean base\n`，topic 为 `clean incoming\n`。
   测试验证 both + 手改 + review、Incoming、精确 stage-0、其它 staged OID 不变、
   Finish 展示全部 staged 路径和正确双父提交。
5. `book/src/keymap.md` 的冲突章节与 validation 是开发草稿；尤其 Focus 自动提示描述需要
   Task 6 真正接通后才算实现。

## 下一步顺序

1. 先对 Task 5 做独立任务审查：范围 `7eb4f12dd..2f9a85446`，读 Task 5 brief/报告/设计。
   本轮还没有该任务的 reviewer，不要把 implementer 自查当审查通过。
2. 注册 discovery 子模块，跑 Focus 和 merge-menu 用例，确认缺失行为的 RED。
3. 接 `handlers::git_conflicts`、hooks 和 EditorView FocusGained；两个新 overlay 已让
   FocusGained/FocusLost 继续传递，以保留原生焦点与 autosave 行为。
4. 接现有 `GitPanel::start_operation`：成功/失败后都查询真实 conflicts；实际冲突才打开列表。
   `conflicts::present_snapshot` 接收已有快照，不需要重复查询。
   用 panel instance、操作编号、工作树和原 load_version/当前文档上下文防止旧结果重开窗口或抢焦点。
   关闭面板后仍完成已确认操作，只提示；切换工作树后不把结果显示到新仓库。
5. 将现有 GitPanel mutation 接到 `Job::with_callback(...).wait_before_exiting()`。
   可将 `operation_task` 改为 `AbortHandle`，实际 JoinHandle 由 Job 等待；增加独立
   `operation_version`，避免请求刷新使 UI 永久 busy。Editor callback 收尾，另派受身份保护的
   compositor 更新。复用 `merge::reload_clean` 刷新所有干净普通文档，不只可见视图。
6. GitPanel 有效刷新触发 Explicit 通知扫描；Focus 扫描保持通知，不自动打开。
   无活跃 merge 时 `git_merge_menu` 应立即提示不可用。
7. Task 6 跑绿、提交、独立审查；随后补 acceptance fixture/注册并完成 Task 7。
8. 按计划跑格式、VCS/view/term、git_conflicts/git_tools、write/splits/folding、no-git、release，
   真终端 smoke，独立整分支审查并修复；更新验证文档后提交和再次 push。

## 环境与验证限制

- 暂停检查点运行 `cargo check -p helix-term` 通过（24.44s），仅 `present_snapshot` 尚未使用的警告。
  `git diff --check` 通过。全仓 `cargo fmt --all -- --check` 报既有
  `helix-core/src/indent.rs:906` 排版问题，已核对基线 `4bd21126f` 相同，未改动无关代码。
- Windows PowerShell，Rust 1.90。Git Bash：`C:/Users/wangs/scoop/apps/git/current/bin/bash.exe`。
  需要 bash 的测试应在当前进程 PATH 前加入该 bin；默认 bash 会落到故障的 WSL 启动器。
- **同一时间只跑一个 term 构建/集成测试**：Windows 正在运行的测试 exe 会让重新链接报 LNK1104。
- 已知实施前基线：VCS 两个 symlink 测试因 Windows 权限 1314 失败；普通 write 的三个
  symlink 测试同样受限。另有既有 AI missing-runtime 测试偶发端口冲突，最终应串行复验。
  不要把旧中断批次或 0 tests 当通过。
- Task 4 普通 write 的早期批次被中断，不能宣称通过。改用 Git Bash 后
  `test_write_new_path --exact --test-threads=1` 曾通过。
- 已有七个 no-git VCS unused-variable 警告和 Windows helper 的 unused-field 警告。
  Task 5 的 `present_snapshot` 暂时 unused，Task 6 接线后应消失。
- 未安装新依赖，未在用户仓库制造测试冲突，未运行真实终端功能 smoke，未做本轮 release build。
- 已验证工具 `exec_command(tty=true)` 可通过 `[Console]::SetWindowSize(160,40)` 提供真实终端尺寸；
  后续可用新构建 hx 做 160×40 和 80×24 smoke，无需额外 Python/ConPTY 包。

本机 `.superpowers/sdd/2026-11-12-git-merge-conflicts/` 保存完整 ledger、日志和临时 smoke 脚本，
该目录被 Git 忽略。跨机器续做以本文件、已提交代码、计划和验证记录为准，不能依赖本机临时仓库。
