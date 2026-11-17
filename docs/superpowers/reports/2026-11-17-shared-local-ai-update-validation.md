# 多 hx 共享 AI 更新与本机验证

日期：2026-11-17（Asia/Shanghai）。

## 更新与保留

- 拉取全部远端后，将 `fork/codex/shared-local-ai` 的四个新提交快进合入当前 `codex/git-merge-conflicts`，HEAD 为 `02cb3f776`。
- 拉取前的已修改和未跟踪文件保存在 `stash@{0}`：`codex-before-shared-ai-update-2026-11-17`；恢复后解决文档与 AI 文档客户端、格式化器的冲突，保留既有预算、进度和预览改动。备份仍保留，未创建本地提交或推送。
- 共享管理器负责跨进程模型生命周期和完整推理操作的排队租约。临时任务结束后恢复常驻补全模型，关闭首个编辑器不会停止其他编辑器使用的模型。

## 本次适配

- 将既有 provider 输入、输出、超时和文档大小预算接入新版结构化提示、源码证据校验与一次自动修订；生成和修订仍处于同一模型租约内。
- 更新旧测试的 JSON 解码和有效源码证据，使大输入、大输出及文本校验测试继续验证原行为。
- 独立审查发现 macOS/BSD 回退实现与共享实现同时定义 `LocalServers`。将共享类型、实现、注册任务及对应测试限制为 Windows/Linux 的普通构建。条件编译探针先复现 E0255，修复后 Windows、Linux、回退和 integration 四种条件分支均通过。仅安装了 Windows Rust 目标，未进行完整跨平台构建。

## 自动验证

- 旧安装 `helix 25.07.1 (d9d9cb33)` 不支持 `--internal-ai-manager`，共享进程回归按预期失败。
- `helix-term`、`helix-view` 包测试与文档测试合计：392 通过、0 失败、14 项明确忽略。
- AI 原生界面集成测试：83 通过、0 失败、10 项明确忽略，覆盖补全、提交、文档、测试生成和冲突合并。
- 真实 Release 辅助进程回归：11 项通过，覆盖共享、排队、模型恢复、先退出者、最后退出者、加载/推理取消、管理器崩溃、启动竞争、配置变更及外部进程保护。
- 修改过的 Rust 文件通过 rustfmt 检查，`git diff --check` 通过。

验证命令：

```powershell
$env:HELIX_DISABLE_AUTO_GRAMMAR_BUILD = '1'
$env:HELIX_RUNTIME = 'C:/dev/helix/runtime'
cargo test --locked --profile integration --jobs 2 --config 'profile.integration.package.helix-term.opt-level=0' -p helix-term -p helix-view -- --test-threads=1
cargo test --locked --profile integration --jobs 2 --config 'profile.integration.package.helix-term.opt-level=0' --features integration -p helix-term --test integration ai_ -- --test-threads=1
python contrib/test_shared_ai.py --hx target/release/hx.exe
cargo build --release --locked --jobs 2 -p helix-term --bin hx
```

完整日志保留在本地 `target/shared-ai-20261117-*`。构建包含三条既有未使用代码警告，不影响本次测试结果。

## 双编辑器与真实 GPU

使用用户现有本地模型、llama.cpp 与推理设置，仅在临时配置中更换端口。输入为临时合成 Python 函数，不使用线上 provider。

1. 两个实际 hx 编辑器 PID 52008、59540 通过同一管理器 PID 67168 共享一个 9B 模型 PID 14988；两边同时请求补全，均显示并接受 `b`。
2. 关闭先启动的 PID 52008 后，同一管理器和同一模型继续运行；第二个编辑器再次显示并接受 `b`。
3. 通过共享协议获得临时 27B 租约，27B 返回 `OK`。第二个编辑器的真实自动补全请求在此期间等待；释放租约并恢复 9B 后显示并接受 `b`。整个切换验证约 29.63 秒。
4. 关闭最后一个编辑器后，管理器删除发现文件并退出，所有测试模型端口关闭，无残留 hx 或 llama-server 测试进程。
5. 原始用户 `config.toml` 的 SHA-256 前后相同。已接受的两个临时 Python 文件均通过 Python 语法编译。

机器证据：`target/shared-ai-20261117-native/metadata.json`；最终临时源码验证包含 `return a + b`。

## 安装

- PATH 指向 `C:\dev\helix\target\release\hx.exe`。
- 原程序备份：`target/hx-backups/hx-before-shared-ai-20261117-02cb3f77.exe`。
- 新版本：`helix 25.07.1 (02cb3f77)`，包含当前工作区保留的本地 AI 改动。
- 最终 Release 构建退出码 0；`hx --health` 退出码 0，使用原始配置和工作区 runtime。
- 最终安装程序再次通过全部 11 项跨进程回归，0 失败，日志为 `target/shared-ai-20261117-process-final.log`。
- 最终程序 SHA-256：`3b3616dfc0ead94f9dadb3bd24a2efccf5c1262d06371e96cbd180e836d785cb`。安装元数据保存在 `target/shared-ai-20261117-installed.json`。
- 多个编辑器可以同时保持 AI 可用；本地 GPU 推理操作按租约排队，避免多个模型进程争用显存。
