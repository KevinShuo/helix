# Helix Windows x64 · 本地 AI 推理优化 · 2026.11.16

本次发布增加托管 llama.cpp 的类型化性能参数，并修复三项 UI 回归测试的等待逻辑。已有功能和前次发布的 `.gitignore` 生成器继续包含在包内。

- 支持按 provider 配置 Q8/Q4 KV cache、MTP 或 n-gram 推测解码、GPU sampling、线程、批次和模型加载方式；参数有范围校验，运行参数变化会重启托管进程。
- 默认保持 F16 KV，不发送新的可选参数，兼容既有运行时。MTP 需要模型具有预测 heads，并使用支持 `draft-mtp` 的 llama.cpp。
- 合成输入实测：RTX 4090 上，27B 同一提示词、256-token 输出从中位 8.62 提升到 135.95 tokens/s；9B 重复源码从 103.92 提升到 174.67 tokens/s，普通结构化任务约 108–110 tokens/s。收益依赖模型、输入和运行时，不能推广为所有任务的相同加速。
- 实际安装配置的 9B → 27B → 9B 切换以及前后语义补全验证通过；原生灰字预览、接受、取消、重启及退出清理通过。
- 修正 Windows Git launcher 较慢时测试过早结束等待的问题，保留文件、暂存区、BOM/CRLF 和撤销断言；补全测试等待预算覆盖命名 provider 冷启动。

完整解压 Windows x64 ZIP，保持 `hx.exe` 与 `runtime` 同目录。包内含中文说明、示例配置、许可证、构建元数据和逐文件校验清单，附件提供 SHA-256。

性能设置需要自行按硬件和模型配置。发布包不包含个人配置、凭据、模型权重或 llama.cpp；不会自动下载或改写它们。Windows 10/11 x64 需要 Microsoft Visual C++ v14 x64 运行库。其他任务尚未提交的预算策略和进度 UI 不进入本次发布。

此前工作区 AI UI 回归 77 项、常规 library 227 项通过；这些记录包含当时其他任务的未提交改动。本次独立源码的复验结果另记入发布附件 `BUILDINFO.json`。不宣称所有测试配置或全部模型质量评测通过：基线与优化配置共同存在一个提交选择评测失败，部分 integration-feature library 测试缺少 Tokio runtime；详见原始记录。

验证记录：[模型切换及 UI 回归](../superpowers/reports/2026-11-16-local-ai-switch-ui-validation.md)、[性能对照与限制](../benchmarks/2026-11-16-local-performance/report.md)。
