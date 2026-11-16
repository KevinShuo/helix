# 本地模型切换与 UI 回归续验（2026-11-16）

已完成用户要求的实际安装配置模型切换验证，并修复三项剩余 UI 回归失败。

## 根因与修复

原失败项：

- `ai_commit_recommendation_requires_explicit_stage_before_generating_message`
- `ai_merge_native_preview_accepts_current_block_and_undoes`
- `ai_merge_preserves_bom_and_crlf_until_reviewed_apply`

三个失败发生在测试尚未等到真实 Git 事务完成时。Windows 上 PATH 首先找到 Scoop
Git launcher；一次失败前记录了 67 个已完成 Git 子命令，累计 12.585 秒，中位
186.86ms。Merge helper 的 100 次轮询预算先耗尽，而事务仍在做乐观状态检查。
同一个未修改的 Merge 测试，直接使用 Git 可执行文件时通过，耗时 10.54 秒。

修复只调整测试同步：Merge 等待实际 `busy` 状态清除，使用统一 30 秒截止时间；
Commit 通用工作流等待预算改为 30 秒。显式心跳、取消和请求超时的短时限断言
保持原样。磁盘、index、其他冲突、BOM/CRLF、预览接受和 undo 的原有断言均保留。
默认 Git PATH 下，Merge 单项由失败变为通过，耗时 21.50 秒。

诊断时临时添加的 Git 计时日志已移除，`helix-vcs/src/log.rs` 无最终差异。
没有绕过 Git launcher、降低生产校验、修改实际工作区的暂存区或自动提交。

独立只读审阅还发现新增真实模型测试的冷启动预算只读取旧连接字段，未考虑命名
provider。新增回归先复现了 17 秒与期望 62 秒的差异，再改为读取解析后的
Completion provider；同时覆盖外部命名 provider 不继承旧本地启动预算，以及旧
连接字段回退。新增真实模型 UI 测试要求配置选择 managed local 模型。

## 实际安装配置端到端验证

从 `C:/Users/wangs/AppData/Roaming/helix/config.toml` 读取实际 provider：

1. 启动 9B，使用生产补全客户端为合约明确的 `add(a,b)` 请求补全，得到 `b`。
2. 通过生产模型池临时卸载 9B，加载 Q5 27B、65536 上下文、99 GPU 层、Q8 KV、
   MTP5 和 GPU sampling，使用生产提交生成客户端处理合成 diff。
3. 返回合法中文 Conventional Commit，释放 27B，恢复 9B。
4. 再次执行相同补全，仍得到 `b`；最后停止模型池。

测试整体 **20.04 秒**；证据中的切换/生成/恢复后补全阶段为 **16.59 秒**，不含
最初 9B 启动和首个补全。证据：
`docs/benchmarks/2026-11-16-local-performance/live-installed-switch.json`。

另一次原生 Application/TUI 集成测试复制实际安装配置的 Completion provider，
只使用隔离端口，验证：

- 实际 9B 生成灰色预览 `sum(items)`，预览期间正文未改变。
- Shift-Right 接受后正文精确符合预期。
- 禁用时子进程退出、端口释放；重新启用并在启动时取消也释放端口。
- 再次启用可恢复预览；关闭应用后端口释放。

该实际模型 UI 测试 **1 项通过，9.20 秒**。测试仅使用合成代码，所有模型请求均
发送到本机。本次未更改已安装的模型参数；`load-mode` 仍为 `auto`，没有把此前
可选的 Direct I/O 实验写入配置。

## 最终验证

| 检查 | 结果 |
| --- | --- |
| AI UI 整组回归（含三项原失败） | 77 通过、0 失败、10 opt-in 忽略 |
| 冷启动预算修复后的补全回归子集 | 33 通过、0 失败、6 opt-in 忽略 |
| 常规 default-feature library，串行 | 227 通过、0 失败、13 opt-in 忽略 |
| 实际安装配置 9B → 27B → 9B，前后语义补全 | 1 通过 |
| 实际安装配置 9B 原生预览、接受、取消、重启和退出 | 1 通过 |
| 独立代码审阅 | 等待逻辑和语义探针无其他问题；启动预算 P2 已 RED→GREEN 修复 |

整组 AI UI 在新增冷启动预算辅助回归前运行；随后重跑了受该辅助函数影响的补全
子集及实际模型 UI 测试。没有把 opt-in 忽略项计作通过。

保留的非通过诊断：一次 `--features integration --lib` 运行有 6 个同步
function-hint 测试因缺少 Tokio runtime 失败；该 feature 用于原生 UI integration
测试，常规 library 使用 default features。一次并发 default-feature library
运行有一个回环 HTTP 连接失败；串行重跑完整 227 项通过。未修改这些无关生产代码，
未声称所有测试 feature 组合均通过。

## 命令与日志

测试统一设置 `HELIX_RUNTIME=C:/dev/helix/runtime`。

```text
cargo test -j 2 --profile integration --config profile.integration.package.helix-term.opt-level=0 -p helix-term --features integration --test integration ai_ -- --test-threads=1
cargo test -j 2 --profile integration --config profile.integration.package.helix-term.opt-level=0 -p helix-term --features integration --test integration ai_completion -- --test-threads=1
cargo test -j 2 --profile integration --config profile.integration.package.helix-term.opt-level=0 -p helix-term --lib -- --test-threads=1
```

实际切换测试使用 `HELIX_AI_LIVE_CONFIG` 和 `HELIX_AI_LIVE_SMOKE_OUTPUT`，显式选择
`local_generation_live_installed_runtime_restores_completion_after_commit --ignored`。
实际 TUI 测试使用 `HELIX_TEST_AI_CONFIG`，显式选择
`ai_completion_live_managed_local_model --ignored`。

日志在忽略目录 `target/local-ai-finish-*.log`；包括初次失败、Git 计时、直接 Git
对照、修复后整组检查、冷启动预算 RED/GREEN 和实际模型调用。此次最终改动是测试
同步及验证探针，已安装的生产推理程序不需要重新部署。
