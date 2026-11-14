use super::*;
use crate::job::{self, Callback, Job};

impl CommitWorkspace {
    pub(super) fn show_patch(&mut self, editor: &mut Editor) {
        if self.busy != Busy::Idle {
            return;
        }
        let (Some(repo), Some(snapshot), Some(row)) =
            (self.repo.clone(), self.snapshot.clone(), self.active())
        else {
            return;
        };
        if !super::super::trusted(editor, repo.root()) {
            editor.set_error("请先 :trust 信任当前工作区");
            return;
        }
        self.serial += 1;
        self.busy = Busy::Reading;
        let (instance, serial) = (self.instance, self.serial);
        self.message = "正在读取本地完整 diff；不会发送到模型…".into();
        self.task = Some(tokio::spawn(async move {
            let result = repo.commit_file_patch(&snapshot, row.id, row.staged).await;
            job::dispatch(move |editor, c| {
                let Some(p) = c
                    .find::<Self>()
                    .filter(|p| p.instance == instance && p.serial == serial)
                else {
                    return;
                };
                p.busy = Busy::Idle;
                p.task = None;
                match result {
                    Ok(text) => {
                        let text = format!("本地 diff · PgUp/PgDn 滚动 · Esc 关闭\n{text}");
                        let text = text
                            .lines()
                            .map(super::super::render::safe_text)
                            .collect::<Vec<_>>()
                            .join("\n");
                        c.push(Box::new(crate::ui::Popup::new(
                            "ai-commit-diff",
                            super::view::PatchView { text },
                        )));
                    }
                    Err(error) => {
                        p.message = error.to_string();
                        editor.set_error(p.message.clone());
                    }
                }
            })
            .await;
        }));
    }
    pub(super) fn refresh(&mut self, _editor: &mut Editor, analyze: bool) {
        if self.busy != Busy::Idle {
            return;
        }
        self.busy = Busy::Reading;
        self.serial += 1;
        let (instance, serial, root) = (self.instance, self.serial, self.root.clone());
        self.message = "正在读取 Git 暂存区和工作区…".into();
        self.task = Some(tokio::spawn(async move {
            let result = async {
                let repo = GitRepository::open(root, true).await?;
                let snapshot = repo.commit_snapshot().await?;
                Ok::<_, anyhow::Error>((repo, snapshot))
            }
            .await;
            job::dispatch(move |editor, c| {
                let Some(p) = c
                    .find::<Self>()
                    .filter(|p| p.instance == instance && p.serial == serial)
                else {
                    return;
                };
                p.busy = Busy::Idle;
                p.task = None;
                match result {
                    Ok((repo, snapshot)) => {
                        p.root = repo.root().to_owned();
                        p.repo = Some(repo);
                        p.install_snapshot(snapshot);
                        p.message =
                            "已有暂存将全部提交。空格选候选，s 暂存；g 生成信息，e 编辑，C 提交"
                                .into();
                        if analyze && editor.config().ai_commit.enabled {
                            p.start_ai(editor, true);
                        }
                    }
                    Err(error) => {
                        p.message = error.to_string();
                        editor.set_error(p.message.clone());
                    }
                }
            })
            .await;
        }));
    }
    fn install_snapshot(&mut self, snapshot: CommitSnapshot) {
        if self
            .snapshot
            .as_ref()
            .is_some_and(|old| !old.same_index(&snapshot))
        {
            self.draft.invalidate();
        }
        self.snapshot = Some(snapshot);
        self.selected.clear();
        self.reasons.clear();
        self.row = 0;
        self.diff_scroll = 0;
    }
    pub(super) fn start_ai(&mut self, editor: &mut Editor, recommend: bool) {
        let result = self.prepare_ai(editor, recommend);
        if let Err(error) = result {
            self.message = error.to_string();
            editor.set_error(self.message.clone());
        }
    }
    fn prepare_ai(&mut self, editor: &mut Editor, recommend: bool) -> Result<()> {
        ensure!(self.busy == Busy::Idle, "当前操作尚未结束");
        ensure!(
            editor.config().ai_commit.enabled,
            "请先在 [editor.ai-commit] 中启用 enabled = true；仍可手动暂存和提交"
        );
        ensure!(
            !editor.ai_commit_running && editor.ai_doc.is_none(),
            "已有 AI 显式生成任务，请先完成或取消"
        );
        let repo = self
            .repo
            .clone()
            .ok_or_else(|| anyhow::anyhow!("仓库尚未读取"))?;
        ensure!(
            super::super::trusted(editor, repo.root()),
            "请先 :trust 信任当前 Git 工作区"
        );
        let snapshot = self
            .snapshot
            .clone()
            .ok_or_else(|| anyhow::anyhow!("请先 r 刷新"))?;
        let mut config = editor.config().ai_commit.clone();
        config.language = self.language;
        ensure!(
            snapshot.files.len() <= config.max_files.clamp(1, 1024),
            "改动文件超过 max-files；请缩小仓库改动范围或调整配置后刷新，仍可手动暂存提交"
        );
        let ids = snapshot
            .files
            .iter()
            .filter(|f| recommend || f.staged)
            .map(|f| f.id)
            .collect::<Vec<_>>();
        ensure!(
            !self.dirty(editor, Some(&ids)),
            "分析涉及未保存缓冲区，请保存后 r 刷新"
        );
        if !recommend {
            ensure!(self.has_staged(), "暂存区为空，请先选择文件并 s 暂存");
        }
        let files = snapshot
            .files
            .iter()
            .filter(|f| recommend || f.staged)
            .map(|f| ai_commit::AiFileInput {
                id: f.id,
                path: super::super::render::safe_text(&f.path.to_string_lossy()),
                staged: f.staged,
                partial: f.partial,
                diff: if recommend {
                    format!(
                        "STAGED (already included):\n{}\nUNSTAGED (candidate):\n{}",
                        f.staged_diff, f.worktree_diff
                    )
                } else {
                    f.staged_diff.clone()
                },
                eligible: f.ai_eligible,
            })
            .collect::<Vec<_>>();
        let model = editor.config().ai_model_config();
        for doc in editor.documents_mut() {
            doc.ai_completion_controller.cancel();
            doc.ai_completion = None;
        }
        editor.ai_commit_running = true;
        self.busy = Busy::Ai;
        self.serial += 1;
        let (instance, serial, revision, intent) = (
            self.instance,
            self.serial,
            self.draft.revision,
            self.intent.clone(),
        );
        self.message = if recommend {
            "AI 正在逐批分析并推荐文件… Ctrl-c 取消"
        } else {
            "AI 正在根据真实暂存区生成信息… Ctrl-c 取消"
        }
        .into();
        self.task = Some(tokio::spawn(async move {
            let result = async {
                ensure!(
                    repo.commit_snapshot().await? == snapshot,
                    "仓库状态已变化，请 r 刷新后重试"
                );
                let result = if recommend {
                    AiResult::Recommendation(
                        ai_commit::recommend(&model, &config, &files, &intent).await?,
                    )
                } else {
                    AiResult::Message(
                        ai_commit::generate_message(&model, &config, &files, &intent).await?,
                    )
                };
                let now = repo.commit_snapshot().await?;
                ensure!(
                    if recommend {
                        now == snapshot
                    } else {
                        now.same_index(&snapshot)
                    },
                    "仓库状态已变化，已丢弃旧 AI 结果，请刷新"
                );
                Ok::<_, anyhow::Error>(result)
            }
            .await;
            job::dispatch(move |editor, c| {
                let Some(p) = c
                    .find::<Self>()
                    .filter(|p| p.instance == instance && p.serial == serial)
                else {
                    return;
                };
                editor.ai_commit_running = false;
                p.busy = Busy::Idle;
                p.task = None;
                let mut current = editor.config().ai_commit.clone();
                current.language = p.language;
                if current != config
                    || editor.config().ai_model_config() != model
                    || !super::super::trusted(editor, &p.root)
                {
                    p.message = "模型、语言或工作区配置已变化，旧结果已丢弃".into();
                    return;
                }
                match result {
                    Ok(AiResult::Recommendation(r)) => {
                        p.selected = r
                            .selected
                            .into_iter()
                            .filter(|id| {
                                snapshot.files.iter().any(|f| {
                                    f.id == *id && !f.partial && (f.unstaged || f.untracked)
                                })
                            })
                            .collect();
                        p.reasons = r.reasons;
                        p.message = if r.incomplete {
                            "推荐完成，部分文件需手动检查；空格调整，s 暂存所选"
                        } else {
                            "推荐完成，尚未 add；空格调整，s 暂存所选"
                        }
                        .into();
                    }
                    Ok(AiResult::Message(m)) => {
                        if p.draft.apply_generated(revision, m.text, m.incomplete) {
                            p.message = if m.incomplete {
                                "提交信息未覆盖全部内容，请 e 补充并 v 明确保留后提交"
                            } else {
                                "信息已生成：e 编辑 · l 切换语言 · g 重新生成 · C 提交"
                            }
                            .into();
                        } else {
                            p.message = "草稿已经手动修改，旧生成结果已丢弃".into();
                        }
                    }
                    Err(error) => {
                        p.message = error.to_string();
                        editor.set_error(p.message.clone());
                    }
                }
            })
            .await;
        }));
        Ok(())
    }
    pub(super) fn mutate(&mut self, cx: &mut Context, kind: u8) {
        if let Err(error) = self.prepare_mutation(cx, kind) {
            self.message = error.to_string();
            cx.editor.set_error(self.message.clone());
        }
    }
    fn prepare_mutation(&mut self, cx: &mut Context, kind: u8) -> Result<()> {
        ensure!(
            self.busy == Busy::Idle,
            "操作尚未结束，请等待或取消 AI 分析"
        );
        let repo = self
            .repo
            .clone()
            .ok_or_else(|| anyhow::anyhow!("仓库尚未读取"))?;
        ensure!(
            super::super::trusted(cx.editor, repo.root()),
            "请先 :trust 信任当前 Git 工作区"
        );
        let snapshot = self
            .snapshot
            .clone()
            .ok_or_else(|| anyhow::anyhow!("请先 r 刷新"))?;
        let ids = if kind == 1 {
            vec![
                self.active()
                    .filter(|r| r.staged)
                    .ok_or_else(|| {
                        anyhow::anyhow!("请选择已有暂存行；u 仅取消该文件暂存，不改变磁盘内容")
                    })?
                    .id,
            ]
        } else {
            self.selected.iter().copied().collect::<Vec<_>>()
        };
        if kind == 0 {
            ensure!(!ids.is_empty(), "请先勾选要暂存的候选文件");
        }
        ensure!(
            !self.dirty(cx.editor, if kind == 2 { None } else { Some(&ids) }),
            "相关文件尚未保存，请保存后 r 刷新"
        );
        if kind == 2 {
            ensure!(self.has_staged(), "暂存区为空");
            ensure!(
                self.draft.reviewed,
                "提交范围变化或 AI 分析不完整：请 g 重新生成，或检查 diff 后 v 保留手动文本"
            );
            ai_commit::validate_message(&self.draft.text)?;
        }
        self.serial += 1;
        self.busy = Busy::Writing;
        let (instance, serial, message) = (self.instance, self.serial, self.draft.text.clone());
        self.message = match kind {
            0 => "正在暂存所选完整文件…",
            1 => "正在取消该文件暂存…",
            _ => "正在提交，执行仓库 hooks 和签名配置…",
        }
        .into();
        cx.jobs.add(
            Job::with_callback(async move {
                let result = match kind {
                    0 => repo
                        .stage_files(&snapshot, &ids)
                        .await
                        .map(|s| (Some(s), None)),
                    1 => repo
                        .unstage_files(&snapshot, &ids)
                        .await
                        .map(|s| (Some(s), None)),
                    _ => repo
                        .commit_staged(&snapshot, &message)
                        .await
                        .map(|out| (None, Some(out))),
                };
                let refreshed = if kind == 2 || result.is_err() {
                    repo.commit_snapshot().await.ok()
                } else {
                    None
                };
                Ok(Callback::EditorCompositor(Box::new(move |editor, c| {
                    let status = match &result {
                        Ok((_, Some(out))) => format!(
                            "已提交 {}{}",
                            &out.oid[..out.oid.len().min(12)],
                            if out.changed_from_preview {
                                "；hooks 改变了预览内容，请检查实际提交"
                            } else {
                                ""
                            }
                        ),
                        Ok(_) => if kind == 0 {
                            "已暂存所选文件"
                        } else {
                            "已取消该文件暂存"
                        }
                        .into(),
                        Err(error) => format!(
                            "Git 操作未确认完成：{error}；实际状态已重新读取，请检查后再操作"
                        ),
                    };
                    if result.is_ok() {
                        editor.set_status(status.clone());
                    } else {
                        editor.set_error(status.clone());
                    }
                    if let Some(p) = c
                        .find::<Self>()
                        .filter(|p| p.instance == instance && p.serial == serial)
                    {
                        p.busy = Busy::Idle;
                        if let Some(snapshot) = refreshed {
                            p.install_snapshot(snapshot);
                        }
                        match result {
                            Ok((Some(snapshot), _)) => p.install_snapshot(snapshot),
                            Ok((_, Some(out))) => {
                                p.draft = Draft::default();
                                p.draft.text = out.message;
                            }
                            _ => {}
                        }
                        p.message = status;
                        if kind == 0
                            && editor.config().ai_commit.enabled
                            && !p.draft.manual
                            && p.draft.text.is_empty()
                        {
                            p.start_ai(editor, false);
                        } else if kind == 0 && !p.draft.text.is_empty() {
                            p.message
                                .push_str("；草稿已保留，请 g 重新生成或检查后 v 保留文本");
                        }
                    }
                    if let Some(panel) = c.find::<super::super::GitPanel>() {
                        panel.refresh();
                    }
                })))
            })
            .wait_before_exiting(),
        );
        Ok(())
    }
}

enum AiResult {
    Recommendation(ai_commit::Recommendation),
    Message(ai_commit::GeneratedMessage),
}
