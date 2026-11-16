# ai-doc quality checkpoint — 2026-11-17

用户要求兼顾准确性和速度，采用“源码证据增强＋严格校验＋失败时最多修订一次”，并明确使用现有 27B。用户随后要求保存并推送，明天继续。本次是可恢复的开发检查点，尚未构建或安装新的 release 编辑器。

## 已完成

- 从当前缓冲区按词法作用域补充相关常量、类型和辅助函数；限制依赖深度、块数和大小，不读取其他项目文件，也不截断目标函数。
- AST 提供 async、generator、实际返回值等事实，排除嵌套函数的返回和异常。
- 每项生成描述必须附源码原文证据，校验范围只包含实际发送给模型的源码；最终注释隐藏证据。保留参数、模板参数、异常、单行内容和 reST 等校验。
- 完整响应校验失败时最多修订一次，共用模型租约和总超时；传输错误、工具调用、截断响应不触发该修订。预览、取消和撤销行为保持现有流程。
- 扩充到 16 个合成实测样例，运行真实生产生成管线；审查发现的依赖作用域问题已用失败回归复现并修复，复查没有剩余 Critical/Important 项。

## 已运行的验证

环境变量：`HELIX_DISABLE_AUTO_GRAMMAR_BUILD=1`、`HELIX_RUNTIME=I:\DEV\helix\runtime`。

```powershell
cargo test --locked --profile integration --jobs 1 --config 'profile.integration.package.helix-term.opt-level=0' -p helix-term --lib ai_doc_ -- --test-threads=1
cargo test --locked --profile integration --jobs 1 --config 'profile.integration.package.helix-term.opt-level=0' --features integration -p helix-term --test integration ai_ -- --test-threads=1
cargo test --locked --profile integration --jobs 1 --config 'profile.integration.package.helix-term.opt-level=0' -p helix-term
```

结果：

- ai-doc 单元回归：37 passed、2 ignored。
- AI 原生集成测试：81 passed、10 ignored；其中 ai-doc 18 项全部通过，涵盖修订、修订上限、总超时、取消和上下文溢出。
- helix-term 包测试：212 passed、13 ignored；doc-tests：2 passed、1 ignored。
- 修改的 Rust 文件通过 `rustfmt --check --edition 2021`；`git diff --check` 通过。Git 仅提示现有 Windows 换行转换策略。
- 完整工作区测试未运行。日志见 `docs/benchmarks/2026-11-16-ai-doc-quality/`。

## 27B 实测

使用用户现有配置 `%APPDATA%\helix\config.toml` 的 `local-27b`：`Qwen3.8-27B-UD-Q5_K_M`，context 65536，thinking=true，thinking budget 4096，reasoning effort=medium。ai-doc 已配置 `local-provider="local-27b"`，未改用其他模型。

配置中的 `max-tokens=8192` 和 `timeout-ms=600000` 仍受原有实现上限约束：内容预算有效上限 4096 tokens，总生成/修订超时有效上限 120000 ms；模型启动另有超时。

```powershell
$env:HELIX_AI_DOC_EVAL_CONFIG = "$env:APPDATA\helix\config.toml"
$env:HELIX_AI_DOC_EVAL_PROVIDER = 'local-27b'
$env:HELIX_AI_EVAL_OUTPUT = 'I:\DEV\helix\target\ai-doc-quality\final-27b.jsonl'
cargo test --locked --profile integration --jobs 1 --config 'profile.integration.package.helix-term.opt-level=0' -p helix-term --lib -- --ignored --exact handlers::ai_doc::client::evaluation::ai_doc_quality_evaluation --nocapture --test-threads=1
```

注意：live harness 会记录单例错误，但整体测试成功不代表每个样例成功。此次已另行检查 JSONL：16/16 无错误且通过结构、证据和渲染校验。每例约 15–33 秒，包含切换模型及恢复原补全模型的开销；不是纯推理时间。原始输入和输出完整保存在 `27b.jsonl`。

16 个样例没有生成错误，耗时最短 15.338 秒、中位数 18.179 秒、最长 33.027 秒。格式检查和 `git diff --check` 均通过。

人工检查确认：普通/异步生成器、原地修改、空指针、返回码、闭包、相关常量、未知依赖和注释指令注入的关键行为基本符合源码。**16/16 校验通过不等于语义准确率 100%。** 源码引用只建立可追溯性，不能证明自然语言结论。

## 明天继续

1. 优先补回归并改进以下 27B 实测中的语义问题，再重复相同样例，避免只看校验通过率：
   - `doc_template`：返回描述仍声称结果“介于下界与上界之间”，源码未保证 lower <= upper。应描述分支行为，或明确有序前提。
   - `doc_async`：把 `len(await awaitable)` 的等待结果收窄为“序列”；实际只要求支持长度操作。
   - `doc_caught_error`：“转换失败时返回 None”范围过宽，实际仅捕获 ValueError；不应暗示 TypeError 等也被捕获。
   - `doc_misleading_name`：把无类型注解的可切片输入收窄为字符串。进一步约束根据操作推断协议，避免凭参数名虚构具体类型。
2. 自动证据校验尚不能识别这些自然语言过度断言。评估是否增加便宜的确定性事实检查，或调整短提示词；继续遵守单次生成、仅失败时一次修订的速度目标。
3. 改动后重跑定向回归、AI 集成和 27B 样例；记录语义检查结果，不能把格式成功率当准确率。
4. 再构建并安装 release。现有 `target/release/hx.exe` 可能被共享模型管理进程占用，先检查进程并保留备份，不要结束用户编辑器进程。当前运行的编辑器尚未包含本次修改。

开发分支：`codex/shared-local-ai`。未跟踪的 `.idea/` 和 `contrib/__pycache__/` 是原有本地文件，不纳入本次提交。临时提示词实验和旧 9B 结果仍在本地 `target/ai-doc-quality/`；本次提交只保存最终 27B 结果和通过的测试日志。
