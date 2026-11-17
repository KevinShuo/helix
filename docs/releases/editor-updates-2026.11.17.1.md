# Helix Windows x64 · Git 收尾与取消测试修复 · 2026.11.17.1

本次补丁修复 Windows 上取消 Git 操作后，启动器的后代仍持有管道、导致运行时退出等待的问题。

- Git 工具调用与函数作者查询使用每条命令独立的 Windows Job。Git 先挂起创建，加入 Job 后再运行；取消、超时和退出会清理对应后代。
- 取消 Git 写操作时，在放开仓库操作锁前确认后代已停止。若短暂等待后仍无法确认，当前会话会禁止该仓库继续写入。
- 两项 AI 提交取消回归改用可控服务端回复，先确认分析正在进行，再触发取消或配置切换，并验证迟到回复不能继续请求或修改 HEAD/index。
- 完整保留上一版的多 hx 共享 AI、处理进度与 token 计数、provider 预算、完整大预览和源码证据文档等功能。

验证记录：[Windows Git 收尾与取消测试](../superpowers/reports/2026-11-17-git-shutdown-cancellation-validation.md)。附件 `BUILDINFO.json` 记录源码提交和测试结果，`SHA256SUMS.txt` 校验发布附件，包内 `MANIFEST.sha256` 校验解压文件。

完整解压 ZIP，保持 `hx.exe` 与 `runtime` 同目录。保留个人配置；已打开的 hx 需保存后重启才能使用新代码。发布包不含模型权重、个人配置或凭据。Windows 10/11 x64 需要 Microsoft Visual C++ v14 x64 运行库。本次验证平台为 Windows，外部模型等按需测试明确忽略。
