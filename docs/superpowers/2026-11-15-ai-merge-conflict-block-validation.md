# AI Merge 当前冲突块验证

日期：2026-11-15。开发分支：`codex/ai-merge-conflict-block`，基线 `33027403d`。

## 功能与入口

- Merge 工具栏 `AI Merge`、`Space v m i`、`:ai-merge [要求]` 使用同一个选中块。
- 候选和差异只读预览；Enter 接受到 Result，r 重试，Esc/Ctrl-c 取消；`:ai-merge-cancel` 取消生成。
- 单块替换是原生 Transaction，可 undo/redo；接受后仍需 Review，既有 Apply 才写入并仅暂存该文件。
- 完整 Base / Current / Incoming / draft 和意图必须在预算内；可选附近代码与文件头按剩余预算加入。输入使用 `draft` 和 `draft_is_base`，避免把初始 Base 占位当作用户最终选择。
- 请求绑定文档、版本、视图、会话、源快照、选中块和配置；编辑、切块、离开或刷新会话、配置改变会取消旧结果。
- 一侧整个块删除、另一侧修改且没有意图时，本地展示需人工处理的说明，不发送模型请求；给出意图后可重新生成。
- 60,000 UTF-8 字节的完整预览上限保证窄列折行后的大小与滚动安全；超限保留草稿并报错。

## 自动化验证

环境：Windows、Rust 1.90.0、仓库现有依赖；`HELIX_RUNTIME=I:/DEV/helix/runtime`，禁用自动 grammar build，并使用 Git 安装目录中的原生 git.exe。

- `cargo test --workspace --profile integration --locked --no-fail-fast -- --test-threads=1`：**762 passed，0 failed，12 ignored**。包含 7 个 AI Merge 协议/策略测试及独立配置测试。
- `cargo integration-test --no-run` 后执行同一测试程序 `--test-threads=2`：**325 passed，0 failed，11 ignored**，435.52 秒；包含 13 项 AI Merge 集成用例。
- `cargo check --profile integration --locked -p helix-term --no-default-features`：通过。
- `cargo fmt --all -- --check`、`git diff --check`：通过。

集成覆盖：请求携带完整当前块及意图；只读预览；接受前磁盘/index 不变；接受仅修改一个块且保留邻块；Unicode 字符偏移；单次 undo/redo；相同文本也要求 Review；Review 前拒绝 Apply；CRLF/BOM 恢复；空删除；needs-manual、重试、取消、超时、截断；切块后返回、离开后返回、编辑及配置变更后迟到回调失效；已有 prompt 不被抢焦点；staged/stale/busy/readonly/trust/WholeFileReview 限制；整块删改先问取舍；160×40、80×24、40×12、12×6。

## 独立审查与回归

一次新上下文审查未发现范围替换、Git 写入边界或过期回调方面的问题，指出两项 Important：

1. 12×6 弹窗边框吃掉全部内容和操作提示。回归先复现空弹窗，再用紧凑布局通过：状态、接受和取消均可见，needs-manual 不提供接受。
2. 合法字节上限内的 65,536 个换行导致旧 u16 行数计算溢出。回归先复现非展开 panic，再通过预览总字节上限及准确 Paragraph 折行计数修复；长单行也拒绝超限预览，草稿不变。

无待处理的 Critical/Important 代码审查项；没有列出的延期 Minor。

## 真实终端与模型

独立临时 Git 仓库、开发版 exe、单独测试配置；使用本机已运行的 9B 服务，测试内容均为合成代码，没有改动全局 Helix 配置。

- **160×40**：真实模型返回 JSON 候选，保留 Current 的 timeout=20 与 Incoming 的 retries=3。预览差异、Enter 接受、u/U 正常；接受后仍有三个 unmerged index stage，磁盘仍有原冲突标记。Review 前 Apply 被拒绝，Review 后才成功写入并暂存，MERGE_HEAD 仍存在。关闭测试编辑器，归还干净测试仓库。
- **12×6**：真实 ConPTY 显示“候选待复核”、`Enter:OK`、`Esc:No r:R`；单独发送 Escape 关闭预览，随后退出码 0；磁盘及三组 index stage 不变。测试仓库随后 abort 到干净状态。

## 9B / 27B 小样本对照

同一版三方合并提示词与 8 组合成输入，temperature=0、max_tokens=1536、上下文 4096。以下为增加最终删改说明和本地删改拦截之前的匹配对照，不代表通用准确率。

| 本机模型 | 达到该样例预期 | 单次耗时中位数 |
| --- | --- | --- |
| Qwen3.5-9B-Q6_K | 5 / 8 | 0.805 秒 |
| Qwen3.8-27B-UD-Q5_K_M | 7 / 8 | 18.595 秒 |

基础校验组合、独立 JSON 字段、嵌套 JSON、显式取舍均能生成合理候选；互斥策略均会要求人工处理。27B 另外正确组合了“参数重命名 + 新增校验”和“await + timeout”，9B 对这两例较保守。两者都曾在包含删除和修改的语义场景中单方面采用 Incoming；追加提示词后 9B 转为人工处理，但 27B 仍有猜测。因此保留强制 Review，并为明显的整块删改模式加入本地检查，不能把更大模型当作正确性保证。

RTX 4090 24GB 上原有 9B 仍驻留；27B 比较服务只将 32 层放入 GPU，其余使用 CPU，所以耗时不代表 27B 独占 GPU 的表现。临时 27B 服务按 PID、启动时间和路径核实后已关闭，原有 9B 服务保留。建议 Merge 优先使用更能处理组合改动的 27B，自动补全保留低延迟模型；本功能本身沿用当前仓库的公共模型接口，不硬编码某个模型。

## 验证限制与执行异常

第一次完整基线运行通过：workspace 760 项、integration 323 项（此后增加了协议/删改/显示边界回归）。后续并行压力下出现过 rustc / integration 进程原生 `0xc0000005` 退出；释放临时模型资源后，默认并行单元测试又在 `ai_doc_distinguishes_void_pointer_trailing_return_and_none` 命中 alloc BTreeMap 的 unreachable UB 检查。没有证据把这些异常归因于特定单一原因。

同一个终端单元测试程序串行执行 **125 passed / 4 ignored**，独立的 ai_doc target 12 项在 4 线程也通过；最终 workspace 串行完整运行通过。**不声称默认并行测试稳定。** 保留诊断日志以便后续定位；此功能没有新增语法树遍历或模型代码执行。

语义正确性仍依赖用户检查，支持边界仍为现有 Merge 的可写、可定位普通文本块；不支持整文件批量、跨文件修改或自动提交。Linux/macOS 尚未独立运行验证。
