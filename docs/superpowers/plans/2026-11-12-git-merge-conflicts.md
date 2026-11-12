# Git Merge Conflicts Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在现有 Helix Git 工具中增加 PyCharm 式冲突文件列表和三栏编辑流程，让普通文本 merge 可以从发现冲突到人工完成合并。

**Architecture:** helix-vcs 读取真实 index stages，并负责候选合并结果、校验和精确路径写入/暂存。helix-view 管理专用结果 Document、决策状态和临时编辑布局；helix-term 在现有 GitPanel 上接入列表、两侧只读预览和异步发现，中央继续使用原生 EditorView。

**Tech Stack:** Rust 1.90、Tokio、Git CLI、现有 helix-core/helix-view/helix-tui、已有 imara-diff 和 workspace tempfile。

**Spec:** [调研与设计草案](../specs/2026-11-12-git-merge-conflicts-design.md)。

状态：2026-11-12 已从暂停检查点恢复并完成首版实现。用户已批准方案 A，并授权完整实现、测试和最终 git push。下方保留原始实施步骤；任务实际完成状态以[续做说明](../2026-11-12-git-merge-conflicts-handoff.md)为准，检查命令、独立审查与平台限制见[验证记录](../2026-11-12-git-merge-conflicts-validation.md)。

## Global Constraints

- 使用 Rust 1.90、现有 Tokio/Helix TUI/Git CLI；不新增第三方包，允许复用 workspace 中已有 tempfile。
- 保留 Space g 和既有 Space v 入口；新增 Space v r，合并动作位于 Space v m。
- pull 保持 --ff-only --no-autostash；merge 保持 --no-autostash；不增加隐式 stash、force、hard reset 或自动 push。
- 所有 Git 读写遵守现有 workspace trust、参数数组、路径验证、操作锁、20 秒查询/60 秒操作和 16 MiB 输出限制。
- 所有编辑、写盘和暂存必须绑定同一工作树及快照；不把清除标记视为解决，不暂存其它路径。
- 首版仅写入普通 merge 的受支持 UTF-8 文本；其它情况明确降级，保留原始数据。
- 保持 git feature 关闭时可编译；变更用临时仓库验证，不在用户仓库制造冲突。

## Review Focus

1. 已经在外部手改过的工作文件或 rerere 结果：重新打开不能被候选合并结果覆盖；Task 2、4、7 验证。
2. Stage 1/2/3 缺失、非文本和特殊文件：不能把“缺失”解释成空文件而写坏内容；Task 1、3 验证。
3. 读取后 HEAD/index/工作文件改变，或窗口关闭/切换仓库：旧结果不能写入新状态或重新弹窗；Task 3、6 验证。
4. 手改、undo/redo、跨块修改、EOF 和 CRLF：块区间和“已复核”状态不能过期；Task 2、4 验证。
5. 写盘成功但 git add/commit 失败，以及 hook 期间关闭窗口：保留真实结果，给出可恢复状态；Task 3、6、7 验证。

## 文件与职责

| 文件 | 操作 | 职责 |
| --- | --- | --- |
| `helix-vcs/src/log.rs` | 修改 | 接入冲突子模块，向子模块复用 runner/操作锁；保留 operate 原接口。 |
| `helix-vcs/src/log/conflicts.rs` | 新建 | 公共冲突类型和 feature-disabled API。 |
| `helix-vcs/src/log/conflicts_backend.rs` | 新建 | 工作树状态、stage/blob、候选结果、应用/完成/取消。 |
| `helix-vcs/src/merge.rs`、`helix-vcs/src/merge/test.rs` | 新建 | 文本/冲突块模型、受控 marker 解析、映射和编码。 |
| `helix-vcs/src/lib.rs`、`helix-vcs/Cargo.toml` | 修改 | 导出纯模型，将已有 tempfile 复用到运行依赖。 |
| `helix-vcs/src/log/test.rs`、`helix-vcs/src/log/conflicts_test.rs` | 修改/新建 | 临时真实 Git 仓库用例；可复用父测试模块 Repo。 |
| `helix-view/src/merge.rs`、`helix-view/src/merge/test.rs` | 新建 | 会话、草稿与布局所有权。 |
| `helix-view/src/{lib.rs,document.rs,editor.rs}` | 修改 | 结果文档角色、变更映射、保存限制、会话注册。 |
| `helix-term/src/ui/git/conflicts.rs` | 新建 | 仓库级冲突列表与 merge 操作确认。 |
| `helix-term/src/ui/git/merge/{mod.rs,render.rs,test.rs}` | 新建 | 合并工作区与只读源栏、响应式布局、输入。 |
| `helix-term/src/ui/git/{mod.rs,actions.rs,render.rs,test.rs}` | 修改 | 模式切换、操作后发现、已有 GitPanel 的暂停/恢复。 |
| `helix-term/src/handlers/git_conflicts.rs`、`helix-term/src/handlers.rs` | 新建/修改 | 去抖状态查询和生命周期控制。 |
| `helix-term/src/ui/editor.rs`、`helix-term/src/handlers/auto_save.rs` | 修改 | 焦点通知、结果编辑区域、跳过合并草稿自动保存。 |
| `helix-term/src/commands/{git.rs,typed.rs}`、`commands.rs`、`keymap/{default.rs}`、`keymap.rs` | 修改 | 可发现入口、上下文动作菜单和保存提示。 |
| `helix-term/tests/test/git_conflicts.rs`、`helix-term/tests/integration.rs` | 新建/修改 | 完整 TUI/真实 Git 联合验收。 |
| `book/src/keymap.md`、`docs/superpowers/2026-11-12-git-merge-conflicts-validation.md` | 修改/新建 | 快捷键、限制、实际执行证据。 |

## Task 1: 可靠发现冲突并读取三份源数据

**Files:** `helix-vcs/src/log.rs`、`log/conflicts.rs`、`log/conflicts_backend.rs`、`log/test.rs`、`log/conflicts_test.rs`。

**Interfaces:**

```rust
GitRepository::conflicts(&self) -> impl Future<Output = anyhow::Result<GitConflictSnapshot>>;
GitRepository::conflict_file(&self, path: &Path) -> impl Future<Output = anyhow::Result<GitConflictData>>;
```

类型公开在 `helix_vcs::git_tools`：

- `ConflictOperation::{None, Merge { head: String, parents: Vec<String> }, Rebase, CherryPick, Other}`。
- `ConflictStage { oid: String, mode: u32 }`。
- `GitConflictFile { path: PathBuf, status: String, stages: [Option<ConflictStage>; 3] }`，下标 0/1/2 对应 Git stage 1/2/3。
- `GitConflictSnapshot { operation: ConflictOperation, files: Vec<GitConflictFile> }`。
- `ConflictSupport::{Text, InspectOnly { reason: String }}`。
- `GitConflictData { file: GitConflictFile, operation: ConflictOperation, sources: [Option<Vec<u8>>; 3], working: Option<Vec<u8>>, format: Option<MergeTextFormat>, support: ConflictSupport, version: ConflictVersion }`。
- `ConflictVersion` 为 opaque、可 Clone 的文件快照凭据，内部覆盖设计中指定的工作树、Git 和磁盘状态；不允许调用方拼装任意路径凭据。
- `MergeTextFormat` 在 Task 1 定义 UTF-8 BOM/换行信息，Task 2 实现无损解码/编码。受大小限制的 InspectOnly 不加载 blob 全文，缺失与未加载用 support 区分。

- [ ] **先写失败测试。** `git_conflicts_reads_index_stages` 构造 UU，断言每侧 bytes/OID 与已知 base/main/topic 一致。`git_conflicts_missing_stages` 覆盖 AA、UD、DU，断言缺失为 None。`git_conflicts_detects_operation` 覆盖 merge/rebase/cherry-pick/无操作，包含没有英文错误文本的读取路径。
- [ ] **运行并确认失败。** `cargo test -p helix-vcs --features git git_conflicts`；预期新增 API 缺失或相应断言失败，不能把 fixture 建仓错误当 RED。
- [ ] **实现只读 API。** `conflicts_backend.rs` 作为现有 implementation 的子模块复用 runner；通过 `ls-files --unmerged -z` 和工作树特定的 Git 元数据读状态；通过 OID 读 blob，选中文件前检查大小、mode、attributes 和操作类型。仅支持设计矩阵中的 Text，其余返回 InspectOnly 原因。
- [ ] **补边界测试并跑绿。** 中文/空格/tab/换行文件名按平台允许情况测试；以 `-` 或 pathspec 元字符开头的路径、linked worktree、空仓库、嵌套仓库、无信任、超限、非 UTF-8、symlink/submodule 和特殊 attributes 均断言不能进入可写路径。兼容现有 40/64 位对象 ID 校验。
- [ ] **核验 feature gate 并提交本任务。** `cargo test -p helix-vcs --features git git_conflicts` 全部 PASS；`cargo check -p helix-vcs --no-default-features` 成功。只提交本任务文件，建议提交信息 `feat: read structured Git conflict snapshots`。

## Task 2: 三方候选结果、逐块决策和保真模型

**Files:** `helix-vcs/src/merge.rs`、`merge/test.rs`、`log/conflicts_backend.rs`、`lib.rs`、`Cargo.toml`。

**Consumes:** Task 1 的 `GitConflictData`、`MergeTextFormat`。

**Produces:**

```rust
GitRepository::merge_candidate(&self, data: &GitConflictData)
    -> impl Future<Output = anyhow::Result<MergeCandidate>>;
MergeTextFormat::decode(&self, bytes: &[u8]) -> anyhow::Result<Rope>;
MergeTextFormat::encode(&self, text: RopeSlice<'_>) -> anyhow::Result<Vec<u8>>;
MergeModel::from_candidate(candidate: MergeCandidate) -> Self;
MergeModel::preserve_working(text: Rope, sources: MergeSources) -> Self;
MergeModel::choice(&self, block: usize, choice: ConflictChoice,
                   result: RopeSlice<'_>) -> anyhow::Result<Transaction>;
MergeModel::on_change(&mut self, changes: &ChangeSet);
MergeModel::mark_reviewed(&mut self, block: Option<usize>);
MergeModel::ready_to_apply(&self) -> bool;
```

`MergeSources` 保存 base/current/incoming 的文本和行映射；`MergeCandidate` 保存初始化 result、sources、blocks 和 `matches_working: bool`。`ConflictBlock` 保存三侧及 result 的字符区间和 `BlockDecision::{Unresolved, Reviewed, NeedsReview}`。`ConflictChoice::{Current, Incoming, Both}` 的 Both 顺序固定为 current 后 incoming。`MergeModel` 用 `Blockwise` 或 `WholeFileReview` 模式；后者不提供容易误删手改的块替换按钮。初始化内容由 model 提供只读访问，不复制成独立可变真源。

- [ ] **写模型失败测试。** 使用以下正文 fixture，增加无冲突变化作为独立场景：

  ```text
  base:     before\nvalue=1\nafter\n
  current:  before\nvalue=2\nafter\n
  incoming: before\nvalue=3\nafter\n
  ```

  断言初始 `ready_to_apply()==false`；选左结果等于 current，选右等于 incoming，Both 包含两行且顺序固定。两侧非冲突变更均被保留，不随冲突块选边丢失。
- [ ] **运行 RED。** `cargo test -p helix-vcs --features git merge::`；确认目标测试失败。
- [ ] **实现候选生成和解析。** 复用 tempfile 运行依赖，关闭临时文件句柄后交给 Git 读取以兼容 Windows；使用受控 marker-size/labels 和 `merge-file --stdout --diff3`。正确处理 0、1..127 和命令失败；用真实解析块数计数。比较工作文件正文时可规范化标准冲突结构，但不忽略正文编辑；无法确定对应关系返回 `matches_working=false`。
- [ ] **实现范围/编码规则并补测试。** `merge_literal_markers_not_conflicts`、`merge_more_than_127_blocks`、`merge_aa_empty_base`、`merge_eof_without_newline`、`merge_utf8_bom_crlf_roundtrip`、`merge_external_edits_preserved`、`merge_ranges_shift_after_insert`、`merge_cross_block_edit_needs_full_review`。合并结果上限为 2 MiB / 50,000 行；不做有损解码；whole-file 复核状态在任何文字变更后失效。
- [ ] **跑绿并提交。** 同时运行 `cargo test -p helix-vcs --features git merge::` 和 `cargo test -p helix-vcs --features git git_conflicts`，全部 PASS。建议提交信息 `feat: model editable three-way merge results`。

## Task 3: 校验后应用、精确暂存及 merge 生命周期

**Files:** `helix-vcs/src/log/{conflicts.rs,conflicts_backend.rs,conflicts_test.rs}`、`log.rs`。

**Consumes:** `ConflictVersion`、编码后的结果和仓库操作锁。后端不依赖 UI 的“已解决”布尔值；UI 仅在 Task 2 ready 后提交请求。

**Produces:**

```rust
GitRepository::apply_resolution(&self, request: &ResolutionRequest)
    -> impl Future<Output = anyhow::Result<ApplyOutcome>>;
GitRepository::retry_stage(&self, retry: &StageRetry)
    -> impl Future<Output = anyhow::Result<ApplyOutcome>>;
GitRepository::prepare_merge_action(&self, action: MergeAction)
    -> impl Future<Output = anyhow::Result<MergeActionPlan>>;
GitRepository::execute_merge_action(&self, plan: &MergeActionPlan)
    -> impl Future<Output = anyhow::Result<GitConflictSnapshot>>;
```

`ResolutionRequest { version: ConflictVersion, contents: Vec<u8> }`；`ApplyOutcome::{Staged, WrittenNotStaged { retry: StageRetry, message: String }, NeedsRefresh { message: String }}`。写盘前的失败用 Err；写后失败保留专门结果，禁止误报未写。`StageRetry` 保存已写 bytes 的校验与原 stage 版本，只允许重新暂存同一个结果。

`MergeAction::{Finish, Abort}`；`MergeActionPlan` 为 immutable，包含 action、opaque 状态 token、供 UI 预览的双方提交/全部 staged 路径/默认合并说明。Finish 的 token 覆盖 index tree；Abort 的 token 覆盖该次 merge 身份及将受影响的状态。非 Merge 操作拒绝生成这两种计划。

- [ ] **写失败测试。** `git_conflicts_apply_one_path_only` 断言目标 unmerged 消失、另一冲突仍在、无关 staged OID 未改变。`git_conflicts_stale_snapshot_no_write` 分别变更 HEAD、stage、工作文件，断言原文件/新 index 没被覆盖。`git_conflicts_finish_requires_no_unmerged` 断言任何未合并条目都会拒绝 Finish。
- [ ] **运行 RED。** `cargo test -p helix-vcs --features git git_conflicts`；确认新增写入/生命周期断言失败。
- [ ] **实现 apply/retry。** 锁内验证 worktree/path/file type 和完整 token；同目录临时写入、保留 metadata、原子替换；再校验实际 bytes，仅执行 `git add -- <path>`，核验 stage-0 与预期规范化内容和其余 index 条目。上限/编码/attributes 在写前重查；外部竞态检测失败保留草稿，不能用 reset 回滚。retry 不重复覆盖磁盘。
- [ ] **实现 Finish/Abort。** prepare 给出具体预览，execute 重验 token。Finish 仅在 MERGE_HEAD 有效且无 unmerged 时执行 `git commit --no-edit`；Abort 仅执行 `git merge --abort`。不绕过 hooks，不清空其它变更，不在失败时推断 merge 已结束。所有入口保持禁用 git feature stub。
- [ ] **补失败恢复测试。** 通过受控 index.lock/写权限/失败 hook 构造保存失败、写成功但 stage 失败、commit 失败；验证 retry 后正确状态。再测确认后 index 改变、当前文件在 stage 前外部被改、符号链接或目录 junction 指向工作树外、取消失败、工作树隔离、操作锁和 hooks 关闭窗口仍继续的后端前提。Windows 无 symlink 权限的用例单独标记原因，不能把缺失覆盖写成通过。
- [ ] **跑绿并提交。** `cargo test -p helix-vcs --features git git_conflicts`；`cargo check -p helix-vcs --no-default-features`。建议提交信息 `feat: apply and finish merge resolutions safely`。

## Task 4: 原生结果编辑与草稿生命周期

**Files:** `helix-view/src/merge.rs`、`merge/test.rs`、`lib.rs`、`document.rs`、`editor.rs`；`helix-term/src/commands.rs`、`commands/typed.rs`、`handlers/auto_save.rs`；`helix-term/tests/test/git_conflicts.rs`、`tests/integration.rs`。

**Consumes:** `GitConflictData`、`MergeCandidate`、`MergeModel`；后端 apply API 由 Task 5 的 UI 调用。

**Produces:** `MergeSessionId`、`MergeLeave::{KeepDraft, Discard}`、`MergeSessions`，以及：

```rust
Editor::enter_merge(&mut self, data: GitConflictData, candidate: MergeCandidate)
    -> anyhow::Result<MergeSessionId>;
Editor::resume_merge(&mut self, id: MergeSessionId) -> anyhow::Result<()>;
Editor::leave_merge(&mut self, disposition: MergeLeave) -> anyhow::Result<()>;
Editor::active_merge(&self) -> Option<MergeSessionId>;
Document::merge(&self) -> Option<&MergeDocumentState>;
Document::merge_mut(&mut self) -> Option<&mut MergeDocumentState>;
```

`MergeDocumentState` 单独持有 model、来源 data 和 session id，是结果决策的唯一可变真源；registry 仅维护会话索引/DocumentId/原 Tree 和焦点，不能再复制一份 model。原始 result 内容通过 Document 构建初始化，不把“刚打开”记成用户修改。

- [ ] **写失败测试。** `merge_document_edits_and_undo` 验证选择一侧经原生 Transaction/历史生效，undo/redo 触发重新复核；`merge_document_save_guard` 断言普通、force、指定新路径保存均不能旁路 Apply。`merge_workspace_restores_tree` 断言原 ViewId、布局、选择、折叠仍在。
- [ ] **运行 RED。** `cargo test -p helix-view merge::`；记录具体失败。
- [ ] **实现会话。** 移动保存原 Tree，临时 Tree 只放结果 Document；左右源内容由上层只读组件拥有。结果无文件路径，依据目标扩展名设置语法，不接入原文件 LSP 写入。重新打开按 canonical worktree/path 查找已有草稿；已有普通脏缓冲区不被修改或复制覆盖。
- [ ] **接入变更和生命周期。** `Document::apply_impl()` 成功修改后通知 model 映射 ChangeSet；跨块不可恢复则进入 whole-file 复核。选择侧操作先执行同一编辑路径，再标记该块 reviewed；undo 不恢复旧 reviewed 标志。`save_impl()` 对 merge result 返回清晰提示，auto-save 跳过；`:wa` 保存其它文档并报告未应用草稿。KeepDraft 保留 Document 并恢复 Tree；Discard 经 UI 明确动作后才销毁。`:q`/关闭结果缓冲区先离开临时 Tree 并保留草稿，强制关闭仅放弃当前草稿；退出应用仍检查全部文档。文件/缓冲区切换和分屏命令先 KeepDraft、恢复 Tree，再执行原命令，不能让 Result 绑定到其它文档。
- [ ] **补边界并跑绿。** 测试关闭/恢复多个文件草稿、终端尺寸改变、干净但未应用的初始化草稿、应用中的冻结、已有手改工作文件进入 whole-file 模式、`:q`/`:q!`/`:wa`/应用退出，以及文件切换和分屏恢复。运行 `cargo test -p helix-view merge::`；相关普通保存回归留给 Task 7 一次联合执行。此任务就在 `helix-term/tests/integration.rs` 注册 `git_conflicts` 模块，命令层测试通过直接构建会话独立验证，并运行 `cargo integration-test git_conflicts`；Task 6 继续扩展该模块。
- [ ] **提交任务。** 建议提交信息 `feat: host merge drafts in native editor documents`。

## Task 5: 冲突列表、三栏呈现与决策动作

**Files:** `helix-term/src/ui/git/conflicts.rs`、`ui/git/merge/{mod.rs,render.rs,test.rs}`、`ui/git/{mod.rs,render.rs,actions.rs,test.rs}`、`ui/editor.rs`。

**Consumes:** Tasks 1–4 全部接口。

**Produces:** `OpenAction::Conflicts`、`MergeEditor` 组件、`MergeLayout::for_area(Rect) -> MergeLayout`，以及 `open_merge(cx: &mut commands::Context, path: PathBuf)` 和 `merge_action(cx: &mut commands::Context, action: MergeUiAction)`。`MergeUiAction` 包括 Next、Previous、UseCurrent、UseIncoming、KeepBoth、ShowBase、MarkReviewed、Apply、Files、AcceptCurrentFile、AcceptIncomingFile；整文件动作必须经独立确认。

- [ ] **写失败 UI 测试。** `merge_layout_wide_narrow_tiny` 在 160×40、120×18、80×24、40×7 和非零原点 Rect 验证界限；`merge_source_panes_are_read_only` 断言键盘/鼠标只影响预览滚动；`merge_apply_disabled_until_reviewed` 断言未决定/待复核禁用 Apply。
- [ ] **运行 RED。** `cargo test -p helix-term --lib ui::git`；确认新增 UI 断言失败。
- [ ] **实现列表和工作区。** 文件列表显示操作/路径/状态/降级原因；全屏三栏与窄屏标签共享同一结果 Document；只读侧清理终端控制字符并按 Unicode display width 截断，真实路径不被改写。GitPanel 暂停占区与事件，MergeEditor 的 editor_area 返回中央空间。保留命令行、状态栏和提示层，不让两次裁剪或 bufferline 改变使点击坐标错位。
- [ ] **实现动作与保存反馈。** 一切选择调用 model + 原生 Transaction；导航/滚动用行映射；插入模式原样交给 EditorView。Apply 冻结草稿、检查同路径脏缓冲区与 trust，再调用 Task 3；Staged 后刷新列表并可进入下一文件；其它结果保留草稿并提供 refresh 或 retry stage。所有同路径干净文档都更新，不能遗漏隐藏缓冲区。
- [ ] **接入整文件与 merge 确认。** 整文件选边说明另一侧所有改动会丢弃，确认只生成新的草稿，不直接写盘。Finish/Abort 使用 Task 3 的 immutable plan，显示 staged 全集及结果/风险；关闭工作区不等于 abort。退出时按 KeepDraft/Discard/Cancel 执行，恢复原 GitPanel 的停靠/选择/焦点。
- [ ] **跑绿并提交。** `cargo test -p helix-term --lib ui::git` 全部 PASS；覆盖边界插入块、空侧、中文宽字符、多行长度差、切换文件有草稿。建议提交信息 `feat: add native three-pane conflict workspace`。

## Task 6: 接入现有 merge、命令和外部冲突发现

**Files:** `helix-term/src/handlers/git_conflicts.rs`、`handlers.rs`、`ui/editor.rs`、`ui/git/{mod.rs,actions.rs}`、`commands/git.rs`、`commands.rs`、`keymap/default.rs`、`keymap.rs`；`tests/test/git_conflicts.rs`、`tests/integration.rs`。

**Consumes:** `GitRepository::conflicts()`、`OpenAction::Conflicts`、Task 5 的 MergeUiAction。

**Produces:** `git_resolve_conflicts`、`git_merge_menu` 两个 mappable commands；`handlers::git_conflicts::request(editor: &mut Editor, reason: ConflictScanReason)`，reason 为 `Explicit` 或 `FocusGained`。操作完成扫描由 GitPanel 的 operation job 直接调用，保留其实例/编号信息。

- [ ] **写失败集成测试。** `git_conflicts_merge_opens_list` 从现有 action menu 发起真实冲突 merge，断言出现列表；`git_conflicts_non_conflict_error_stays_error` 断言普通 Git 错误不打开 resolver；`git_conflicts_manual_reentry` 先用 fixture Git 产生冲突再按 Space v r，断言可进入。
- [ ] **运行 RED。** `cargo integration-test git_conflicts`，必须使用临时仓库和隔离 Git 配置。
- [ ] **实现结构化发现。** 保留 `operate() -> Result<String>`，operation job 无论返回成功/失败都重新查询实际冲突/操作状态，消息与结构状态分别处理；只在匹配的发起面板仍有效时自动切换。非普通 merge 只显示支持边界，不误显示 Finish Merge。已关闭时给状态消息，不重建窗口。
- [ ] **实现快捷键与扫描调度。** Space v r 打开列表；Space v m 在活跃合并会话打开动作菜单，菜单项映射设计中固定键位，非合并场景显示明确不可用提示。活跃合并草稿无路径，Git 命令和焦点扫描从 session 取工作树，不能用 cwd 代替。FocusGained 扫描去抖 200 ms，每个工作树最多一个过期可取消查询；终端焦点事件原有 autosave/编辑行为不受影响。查询实例、工作树、请求版本三项都匹配才接收结果。
- [ ] **补竞态/兼容并跑绿。** `git_conflicts_closed_panel_stays_closed`、`git_conflicts_switch_repository_ignores_old_result`、`git_conflicts_focus_scan_no_focus_steal`、`git_conflicts_operation_finishes_after_close`；同时扩充 `git_group_preserves_changed_file_picker` 检查 normal/select 的全部入口。运行 `cargo integration-test git_conflicts` 和 `cargo test -p helix-term --lib git_group`。
- [ ] **提交任务。** 建议提交信息 `feat: connect merge conflicts to Git workflow and commands`。

## Task 7: 联合验收、文档和实际验证记录

**Files:** `helix-term/tests/test/git_conflicts.rs`、`book/src/keymap.md`、`docs/superpowers/2026-11-12-git-merge-conflicts-validation.md`，以及验收发现问题的所属文件。

- [ ] **补端到端验收。** 创建至少两个文本冲突文件和一个干净自动合并文件的受控场景；先产生 merge 冲突，再暂存一个无关文件，避免 merge 因预先脏 index 拒绝启动。混合使用选左、选右、手改和复核；断言 Apply 后每个 stage-0 内容正确、无关 OID 不变；Finish 的确认必须展示 staged 全集，完成后实际 commit 有正确双父且 MERGE_HEAD 消失。另测工作区外人工解决后 refresh、marker-free 未暂存文件、恢复草稿、stage/commit 失败及 Abort。
- [ ] **完成普通编辑回归。** 运行下列命令；所有针对新增功能的测试必须 PASS。既有失败须与实施前基线区分，不能用旧验证文档里的数字替代当前结果。

  ```powershell
  cargo fmt --all -- --check
  cargo test -p helix-vcs --features git
  cargo test -p helix-view
  cargo test -p helix-term --lib
  cargo integration-test git_conflicts
  cargo integration-test git_tools
  cargo integration-test test::commands::write
  cargo integration-test test::splits
  cargo integration-test test::folding
  cargo check -p helix-term --no-default-features
  cargo build -p helix-term --release
  ```

- [ ] **做真实终端 smoke test。** Windows 下仅在临时仓库用新构建 hx 验证 merge→列表→三栏→手动输入→Apply→Finish；覆盖 160×40 与 80×24、鼠标、Esc/返回、恢复原折叠与分屏。其它平台通过 CI 或可用环境验证；不能声称本机 Windows 检查覆盖了 Linux/macOS。
- [ ] **补用户文档。** 记录 Space v r、Space v m、正常编辑模式、Apply 与 Finish 的区别、FocusGained 的限制、UTF-8/大小/操作支持矩阵以及不支持时的实际操作提示。文档中不把未来 rename/rebase 能力写成现有能力。
- [ ] **审查实际 diff 并记录证据。** 检查每个设计要求到任务/测试的覆盖；独立代码评审可在后续实现阶段采用，但本计划的自查不替代实现检查。validation 文件记录命令、结果和未覆盖项；不要自动下载安装、修改用户 Git 配置、执行用户仓库 merge 或远程发布。
- [ ] **提交验收收尾。** 仅在相应验证成功后创建本地提交，建议信息 `test: verify native Git merge conflict workflow`。

## 实施顺序与首版交付边界

顺序为 1 → 2 → 3 → 4 → 5 → 6 → 7。Task 1/2 可先展示只读数据与模型结果；用户可用的首版须完成 Task 7，不能只把冲突列表上线便声称具备 PyCharm 式冲突解决能力。

首版重点是普通文本 merge 的完整闭环；二进制、路径决策、rebase/cherry-pick 生命周期和外部工具集成按设计中的后续阶段单独规划。整个功能需要 Git、编辑器模型和 TUI 三层配合，属于中型功能，不适合压缩成“错误时弹一个对话框”的单点改动。

## 本次计划检查记录

- 已按官方资料区分 JetBrains 行为与本项目提案；源码路径以当前 checkout 为准。
- 设计草案经过独立 builder check，未发现需要用户补充才能规划的重要问题；这不代表用户批准设计或代码验证通过。
- 计划自查覆盖入口、状态模型、编辑/撤销、格式、暂存、完成/取消、恢复、特殊类型和 feature-disabled 构建；所有实现/测试 checkbox 保持未完成。
