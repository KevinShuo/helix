# 2026-11-17 Release 功能核对

## 核对范围与来源

用户要求将共享 AI 修复提交到 master、推送到远端、替换本地 hx 并发布 Release，随后明确要求检查当前全部功能，特别是聊天“显示处理进度”（`01a1e282-3d47-7241-8ee2-8c2eca00bcf0`）中已完成的进度与实际 token 计数。

远端为 `KevinShuo/helix`，对应本地 remote `fork`；`origin` 是上游 `helix-editor/helix`。本次推送用户仓库。核对时 fork/master 为 `bc354cca1`，是当前功能分支的祖先，已提交功能领先 master 11 个提交。最新已发布包为 `local-ai-performance-2026.11.16`，其 `BUILDINFO.json` 记录准确源码 `d9d9cb338`，不是当前全部工作区源码。

## 功能覆盖表

| 功能 | 上次发布 | 本次源码与验证入口 |
| --- | --- | --- |
| AI 提交阶段、批次、文件数、总耗时与本步耗时 | 未包含；只在本地未提交文件 | `handlers/ai_commit.rs`、`ui/git/commit/tasks.rs`；原生 heartbeat/batch 回归 |
| 实际生成 token 数与不可用提示 | 未包含；新增 progress.rs 未跟踪 | `ui/git/commit/progress.rs`；真实 HTTP 计数、换批次和取消回归 |
| 可配置提交文件数及较大本地预算 | 未包含 | `handlers/ai_commit.rs` 及各生成客户端；文件范围和上下文预算回归 |
| provider auto/configured 预算及有限重试 | 未包含 | `helix-view/src/ai/budget.rs`、共享 HTTP transport；provider/transport 回归 |
| 大文档、测试、冲突预览和大提交文本 | 未包含 | `ui/ai_preview_text.rs`、三个预览、提交视图；完整尾部、宽度变化与文本校验回归 |
| 多 hx 模型共享、排队及临时模型恢复 | 晚于上次发布 | `ai_manager/*`、生产客户端接入；跨进程回归和双实际编辑器验证 |
| 源码证据文档生成与一次自动修订 | 晚于上次发布 | `handlers/ai_doc/*`；证据、相关源码、修订和原生预览回归 |
| `.gitignore` 生成器、本地推理 runtime 参数等 | 已包含；但相应提交尚未推进 fork/master | 当前分支已有提交；本次一并快进到 master |

聊天最终回复称进度/token 已安装；其本地安装清单仍以旧 HEAD `bc354cca` 标记版本，同时记录工作区未提交源码。因此“本地可用”和“远端 Release 已发布”在此前并不等价，用户反馈成立。

## 发布保障

- 将当前相关 Rust 源码、回归测试、用户文档和验证记录全部纳入提交；Python 缓存不进入源码或发布包。
- 从完整已提交的 master 构建，发布标签、远端 master、BUILDINFO 中的 commit 和 `hx --version` 必须一致。
- 用已提交 runtime 和 Windows grammar DLL 打包；逐文件比较 ZIP 解压结果，核对 manifest，再在隔离空配置下检测 Python、C/C++、Rust、Go 和 git-ignore 的解析器/查询。
- 替换本地 hx 前备份旧程序；个人 config.toml 前后 SHA-256 必须相同。
- 发布前验证所有附件大小和 SHA-256，并确认 GitHub 上的 latest 指向新 Release。

独立审查发现 AI Test 的大输出接受上限已提高，而旧预览仍使用 u16 行数；本次补充实际 UI 尾部滚动回归并修复。具体最终测试、提交和附件哈希记录在 Release 的 BUILDINFO.json，机器验证日志保留在 `target/all-ai-release-20261117/` 与 `target/publish-all-ai-20261117-*`。

新增原生回归先复现 `ui/text.rs` 的 u16 行数加法溢出；采用共享 PreviewText 后通过，确认 70000 行后的尾部、变窄后的 Ctrl-End、Home 回到开头，以及源缓冲区和磁盘文件保持原样。独立复审没有遗留问题。

已有本次工作区验证为 392 项包/文档测试、83 项 AI 界面测试、11 项跨进程测试通过；它们不是新发布二进制的最终验证替代。最终提交在发布前另行核验。
