# Windows Git 收尾与取消测试修复

## 已确认的问题

此前原生套件停在合并预览测试返回后的 Tokio runtime 析构。线程栈显示 blocking pool 等待 Windows Git stdout/stderr 的同步读取；后代进程仍持有管道写端。

Git 查询的超时分支会显式清理进程树，但取消调用 future 只依赖 `kill_on_drop`。PATH 中的 Git 可以是启动器；结束启动器并不保证其后代退出。新的真实 pre-commit hook 回归在旧实现上复现：取消后释放 hook，后代仍写入 `late-file`，测试因“canceling Git left a hook descendant able to modify the repository”失败。

两项 AI 提交取消测试还有夹具竞态：服务端先通知请求已收到，再按固定 300/1000 ms 计时发送结果。UI 稍晚消费通知时，分析可能已完成。调查中延迟消费旧夹具，配置切换测试确实在 `ai_commit_running` 前置断言失败。原始发布运行的具体失败异常没有恢复，这个独立复现不能冒充原始诊断。另一个已检查的时序问题是：初次 UI 快照后 AI 仍要校验 Git 快照，而原请求助手只等待三秒且不主动驱动 UI；仓库工作流原本使用三十秒预算。

本次完整原生回归还暴露了冲突测试的等待漏洞。共享 `open_ui` 等待循环耗尽后仍无条件发 Enter；列表尚无数据时该键会被消费而不打开工作区。迟到回复夹具固定等待 700 ms；确定性实验在接到请求后延迟 900 ms 并驱动 UI，模型预览已先打开，原来的 `iuser` 按键未改动草稿。新增“必须实际编辑”断言失败，证实旧测试可能没有执行其声称覆盖的编辑操作。本次首轮的无上下文 deadline 仍不能精确还原，不把这个独立复现冒充其完整调用栈诊断。

## 修复

- Windows Git 工具调用和函数作者查询使用每条命令独立的非继承 Job 句柄，设置 kill-on-close。进程先挂起创建，在用户代码运行前加入 Job，然后恢复初始线程，避免后代在归属建立前逃逸。
- future 取消和 runtime 退出时，Job 生命周期终止相应 Git 启动器及后代，释放其管道写端。不会按进程名结束其他 Git。
- 明确超时/正常清理通过 Job 终止并确认 ActiveProcesses 归零。取消析构使用最多 100 ms 的同步退出屏障；无法确认时，在释放仓库操作锁前禁止后续写入，保留原有安全策略。
- Unix 的既有进程组逻辑保持原路径；本次平台实测为 Windows。
- 两项取消测试使用可控首条回复门闩及发送完成通知；等待真实 HTTP 请求时驱动 UI，沿用仓库工作流预算。测试在确认分析仍进行时触发 Ctrl-C 或配置切换，再释放迟到回复，并验证无后续请求、无 HEAD/index 改动。
- 冲突迟到回复回归同样使用回复门闩，确认真实编辑或显式取消后再发送，验证请求已取消、正文不变且预览不复活。冲突列表和工作区等待助手按就绪/活动会话条件驱动 UI，使用仓库工作流的 30 秒预算；未就绪直接失败，不再误发 Enter。

## 已取得的验证

- Git 取消回归先失败；修复后取消后代写入、继承管道不阻塞 runtime 退出、超时停止 hook 三项实际 Git 测试均通过。
- 两项取消测试在故意延迟 UI 消费 2.1 秒后通过，超过原来的两种固定回复延时；相同构建的 AI commit 组 15 通过、0 失败、1 项明确忽略。临时延迟探针已移除，最终源码已重新构建并通过下述完整套件。
- 独立审查指出取消也必须确认后代退出后才能放行后续写入；已补上退出屏障/不确定时禁止写入，以及取消后立即尝试下一次暂存的回归。
- 最终生产源码的 `helix-term`、`helix-view`、`helix-vcs` 包级和文档测试共 506 通过、0 失败、17 项明确忽略，命令退出码 0。包含全部 114 项 VCS 常规回归。
- 两项提交取消测试按最终夹具重复运行三轮，三个独立进程均退出 0，共 6/6 通过。
- 本次首次完整 AI 原生运行正常退出，耗时 544.63 秒，82 通过、2 失败、10 忽略，退出码 101；此前挂起的合并预览用例正常通过，后续用例也完成。两项新失败分别是冲突迟到回复等待超时、冲突工作区未打开。完整错误保留在 `target/git-shutdown-ai-native-initial-failures.log`，退出记录在 `target/git-shutdown-ai-native-initial-exit.json`；不将本轮称为整套通过。
- 修复额外夹具后的最终源码重新编译，移除了 900 ms 实验探针。AI Merge 组 14 通过、0 失败，77.56 秒，退出 0；使用同一等待助手的其余 Git 冲突原生测试 34 通过、0 失败，206.86 秒，退出 0。通用 HTTP 请求 3 秒和按键处理 8 秒限制未提高。
- 最终完整 AI 原生套件在同一个进程内运行 94 项：84 通过、0 失败、10 项明确忽略，547.05 秒，进程退出码 0。包括两项提交取消、两项首轮失败的冲突回归，以及此前 runtime 收尾挂起的合并预览。没有分组拼接本次完整结果，也未使用调试器终止进程。
- 生产进程生命周期和后续夹具改动分别经过独立审查，修订后无重要问题；最终 Rust 格式检查和 `git diff --check` 通过。

## 复核命令与证据

包级验证使用 `cargo test --locked --profile integration --jobs 2 --config 'profile.integration.package.helix-term.opt-level=0' -p helix-term -p helix-view -p helix-vcs -- --test-threads=1`。最终原生程序按项目默认 integration profile、开启 integration feature 重新构建，执行 `integration-1a2fcfa5d57374bb.exe ai_ --test-threads=1 --nocapture`，并单独记录进程退出码。

机器日志：`target/git-shutdown-cancellation-red.log`、`target/git-shutdown-cancellation-green.log`、`target/git-shutdown-full-package-tests.log`、`target/git-shutdown-full-ai-native-tests.log`、`target/git-shutdown-full-ai-native-exit.json`、`target/git-shutdown-cancel-repeats.json`、`target/git-shutdown-ai-merge-delayed-ui-red.log`、`target/git-shutdown-ai-merge-final-green.log`、`target/git-shutdown-git-conflicts-helper-final-green.log`。发布附件 `BUILDINFO.json` 保存最终套件计数和日志校验值。原始失败及实验日志继续保留，不以重跑通过抹去原记录。
