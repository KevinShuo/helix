# Git merge conflicts — completed follow-up

2026-11-12：已在家中机器 `I:/DEV/helix` 从检查点恢复，完成原生冲突工作区首版及本机验收。

## 分支与交付

- 分支：`codex/git-merge-conflicts`，保留在当前 checkout。
- 本机远端 `origin` 是 `https://github.com/KevinShuo/helix.git`。
  原工作机的个人远端名为 `fork`；本机不使用该旧名字。
- 功能基线：`4bd21126`。续做实现：`b46e6172`、`e7c9c357`、`96c94bfe`。
- 原方案与最终 push 已获授权；没有合并到 master，也没有创建发布标签。
- [设计](specs/2026-11-12-git-merge-conflicts-design.md)、[实施计划](plans/2026-11-12-git-merge-conflicts.md)、[完整验证记录](2026-11-12-git-merge-conflicts-validation.md)。

## 完成状态

| 任务 | 实际状态 |
| --- | --- |
| 1. Index stages / 操作状态 / 支持边界 | 完成 |
| 2. 三方文本模型 / 变更映射 / 格式保真 | 完成 |
| 3. 安全写入 / 精确暂存 / retry / Finish / Abort | 完成 |
| 4. 原生 Document / 临时 Tree / 草稿生命周期 | 完成 |
| 5. 列表 / 三栏 / 决策动作 | 完成，独立审查发现的问题已用回归验证修复 |
| 6. 自动发现 / Focus 扫描 / 命令接线 | 完成 |
| 7. 联合验收 / 回归 / release / 真实终端 | 完成 Windows 本机范围；基线失败和平台限制单列 |

## 使用入口

`Space v r` 打开冲突列表，Enter 进入结果编辑器，`Space v m` 使用合并动作。
正常模式下 l/r/b 选当前/传入/两侧，m 复核，a 应用并精确暂存，f 返回列表。
已暂存结果只读；Finish Merge 和 Abort Merge 均须独立确认。原 Space g 保留。

release 输出为 `target/release/hx.exe`；未覆盖全局安装的 hx。
宽屏 160×40 已实测鼠标只读预览、两文件选边/手改、Apply、Finish、无关 staged OID、
双父提交和原折叠/分屏恢复。80×24 已实测外部 merge 焦点提示、窄屏标签、草稿保留/恢复、
显式放弃以及 Abort 的取消/确认与原文件恢复。测试未在用户仓库制造冲突。

## 验证与限制

最新合并用例 33/33、Git 工具 12/12、保存 32/32（排除已知的三个 symlink 权限用例）、
折叠 10/10、view 层 112/112；no-git 与格式检查通过，release 已构建。
全仓与完整集成的实际输出保留在验证记录中，不宣称全套全绿：Windows OS 1314 的
符号链接用例仍失败，另有一个 C 风格注释/CRLF 缩进用例在开发前的测试程序中同样复现。
完整回归抓到的面板重开不刷新已修复；真实终端抓到的信任路径和未变文件折叠问题也已修复。

已知 Minor：原生 buffer picker 对无路径草稿仍使用未命名标签；冲突列表和工作区标题显示目标路径。
Linux/macOS 运行与真实符号链接权限未在这台 Windows 主机实测。
后续扩展仍按设计另行规划：路径决策、二进制选边、rebase/cherry-pick 生命周期和外部 mergetool。
