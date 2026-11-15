# AI Merge 当前冲突块设计

日期：2026-11-15。用户已选择“当前冲突块”，批准上一轮建议并要求开始开发。本文将该方案具体化；默认值和键位由实现者确定。本轮完成开发与验证，使用当前 checkout 的 `codex/ai-merge-conflict-block` 分支。

## 目标与方案

在现有三栏 Merge 窗口中，根据 Base / Current / Incoming 和当前 Result 生成当前冲突块的候选代码，预览差异后接收到 Result。接受后的块仍需 Review，再通过既有 Apply 暂存；Finish Merge 沿用现有流程。

比较过逐块生成与当前文件批量生成。用户选择前者：上下文较小、修改范围明确，便于使用现有本地模型和人工检查。第一版不包含批量生成或跨文件修改。

## 入口和交互

- Merge 工具栏增加 `AI Merge`，Normal 模式的 `Space v m i` 进入同一操作。
- `:ai-merge [自然语言要求]` 使用提交命令时选中的冲突块；参数作为数据发送。
- `:ai-merge-cancel` 取消生成或候选；不改变草稿。
- 生成期间显示状态，编辑器继续响应。结果可用时在原 Merge 会话打开只读差异预览，展示当前块、候选及中文理由/需注意事项。
- 预览内 Enter 接受（有可用代码时）、Esc/Ctrl-c 取消、r 重新生成、j/k 和翻页滚动。窄终端使用居中可滚动预览，操作说明单独占一行，避免长标题隐藏按键。
- 返回 `needs-manual` 时展示原因，但不能接受。重试保留同一要求与目标快照。
- 接受用一个原生 Transaction 仅替换目标 result 字符范围，单次 undo/redo，按现有变更规则标为 NeedsReview。允许空字符串表示删除块。

## 上下文与协议

复用统一 `[editor.ai]` provider、本地服务生命周期和有界 HTTP transport，不新增依赖；旧版 `[editor.ai-completion]` 连接仍可作为回退。根据用户后续选择，Merge 使用 27B，补全使用 9B，通过 `local-provider` 接入现有按需模型租约：生成前卸载 9B，结束或取消后恢复 9B。命名 provider 的设置与其他 AI 功能保持一致，可用 `provider` 显式覆盖。独立配置：

```toml
[editor.ai-merge]
enabled = false
timeout-ms = 60000
max-tokens = 1536
max-input-bytes = 16384
```

请求必须完整包含文件相对路径、语言、冲突块 ID、Base / Current / Incoming / 当前 Result 的目标块文本及可选要求。源码为数据，不是指令。可选补充每个版本附近最多 12 行、文件开头最多 40 行（含声明/imports）；仅在输入和模型上下文预算允许时加入。完整必需输入超过预算直接报错，不截断冲突或要求。采用现有保守字节上界并预留输出和模板空间。

使用非流式 JSON 请求、低温、完整结束检查。模型只返回 `block_id`、`status`（`resolved` / `needs-manual`）、`code`（字符串或 null）、`explanation`（字符串）。响应禁止未知字段、目标 ID 不符、工具调用、截断、过量代码、控制字符和未清除的冲突标记。`resolved` 需要字符串 code，`needs-manual` 必须 code=null。code 的 CRLF 统一为草稿 LF；原文件 BOM/换行仍由现有 Apply 恢复。解释只作为建议，不能代表编译/测试通过。

实测修正：模型输入使用 `draft` 命名当前 Result，并发送事实标记 `draft_is_base`。编辑器的初始 Base 占位不是用户已选定的最终答案；提示词明确三方差异、独立修改的组合及真正互斥的取舍。模型不能把未修改的一侧当作反对另一侧修改的意图。

完整预览（包含差异前缀和解释）限制为 60,000 UTF-8 字节，保证窄列折行后的行数和滚动位置在 TUI 的 u16 范围内；超过上限报错并保留草稿。极小窗口取消外边距并独立保留状态及操作行，12×6 也可识别候选/需人工处理和取消按键。

## 状态与有效性

Operation 绑定请求序号、文档和视图 ID/版本、MergeSessionId、已加载 Git 冲突快照、块序号及完整目标文本、模型和功能配置。预览和接受都重新检查绑定。切换块/文件/会话、编辑（含 undo/redo）、Refresh、重新构建、关闭/放弃草稿、配置变化会使请求失效；迟到回调不能修改内容或重开预览。实际 Git 快照的最终一致性仍由既有 Apply 在写入前校验。

仅支持 Blockwise 且可写的 Draft。Staged、NeedsRefresh、Retry、busy 和 WholeFileReview 给出明确原因。必须仍在受信任的工作树中。AI 生成不持有 Git 写锁；HTTP 失败保留草稿。

单次只有一个 AI Merge 请求/候选。显式 AI Merge 与 AI doc / commit 互斥，自动补全在期间暂停；取消或接受后恢复正常调度。不会执行模型给出的命令。

实测边界：一侧整个目标块为空、Base 非空且另一侧同时修改时，无明确要求先显示本地 needs-manual 说明，不调用模型。用户可手动选一侧，或用非空意图再次生成。此保守规则防止已复现的“删除被模型误当作可忽略状态”；不是对所有语义冲突的自动判定，仍须人工 Review。

## 实现边界

- `helix-view/src/ai_merge.rs`：独立配置、目标和 Operation 的有效性；Editor 保存 Operation，Document 保存取消控制器。
- `helix-term/src/handlers/ai_merge/`：上下文、请求/严格解析；handler 管理生命周期和 Transaction。
- `helix-term/src/ui/ai_merge.rs`：只读差异预览。
- 现有 Merge 模块、命令和渲染仅增加入口及切块失效通知。
- 不改现有 stage/Finish/Abort 协议，不替换用户安装的 release 程序。

## 验收

真实临时 Git 冲突仓库 + 本地 HTTP 测试端点覆盖生成/预览/接受/Review/Apply；接受前后普通文件及 index 保持原样，接受只改一个块，可撤销，Apply 才写入并暂存。覆盖空删除、Unicode、CRLF/BOM、多个块、已有手改、拒绝/取消/超时/截断/无效响应、切块/切会话/配置变化/延迟回调、只读和整文件模式、80×24 及宽终端预览。自动化测试只证明协议和编辑行为；真实模型质量通过可选显式配置的本地模型测试记录，不能假称所有语义正确。
