# 本地推理优化记录（2026-11-16）

主要加速配置和 release 程序已经安装。用户随后要求续验，现已完成实际安装配置的
9B → 27B → 9B 切换、前后语义补全、原生灰字预览/接受/取消/清理验证，并修复
三项 UI 回归失败。详见
[续验记录](../../superpowers/reports/2026-11-16-local-ai-switch-ui-validation.md)。

## 已安装

- 使用官方 llama.cpp b12428 CUDA 12.4，独立安装于
  `C:/Users/wangs/.local/share/helix/llama.cpp-b12428-cuda12.4/`。
  两个下载包的 SHA256 与官方发布资产 digest 一致，见 `runtime-provenance.json`。
- 27B 保留原 Q5 GGUF、65536 上下文、原思考开关和预算；GPU 层数从 56 改为
  99，使用 Q8 KV、MTP（5 个预测 token）、GPU sampling、8 个线程和 256 微批次。
- 9B 保留原 Q6 GGUF 和 32768 上下文，使用 Q8 KV、n-gram speculative decoding、
  GPU sampling、8 个线程和 256 微批次。9B GGUF 没有 MTP heads，实测明确拒绝 MTP，
  因而没有为它启用 MTP。
- 新增的运行参数均有类型、范围校验和热重载识别；默认仍是 F16 KV，不发送新的
  speculative、GPU sampling 或 load-mode 参数，保留旧运行时兼容性。
- 程序为 `C:/dev/helix/target/release/hx.exe`，最终 release 构建成功。

配置位置：`C:/Users/wangs/AppData/Roaming/helix/config.toml`。
原始配置备份：
`C:/Users/wangs/AppData/Roaming/helix/config.before-local-performance-20261116-150155.toml`。
安装时检查了除这三个本地 provider 的 server-path、gpu-layers、runtime 之外，
全部解析值相等。记录见 `initial-config-install.json`。未修改其他任务的源码或设置。

## 实测

硬件：RTX 4090 24GB、i7-12700KF。一个模型进程独占 GPU，使用本地合成输入；
部分测试期间另有 CPU 编译任务，因此不把小幅差异解释为确定收益。

| 测试 | 原配置 | 优化配置 |
| --- | ---: | ---: |
| 27B，同一 414-token 提示词，输出 256 tokens | 中位 8.62 tokens/s（2 次） | 中位 135.95 tokens/s（3 次），约 15.8 倍 |
| 27B，输出 512 tokens | 未用此长度直接比较 | 约 110–141 tokens/s，随输入和运行条件变化 |
| 9B，重复源码生成 128 tokens | 中位 103.92 tokens/s | 中位 174.67 tokens/s |
| 9B，非重复结构化代码任务 | 约 104–106 tokens/s | 约 108–110 tokens/s |

256-token 对照原始数据：`original-production-control/baseline-27b/decode-*`
和 `final-matched-256/mtp5-q8-27b/decode-*`。9B 的对照在 `completion-b11081`
和 `completion-b12428`；Q8 普通解码与 n-gram 的固定源码/JSON 输出逐字节相同。
9B 的较大收益依赖重复模式，不能推广为所有行内补全均加速 68%。

已比较 MTP 预测长度 1/2/3/4/5/8、Q8/Q4/FP8 KV、n-gram、两个引擎版本、GPU
sampling、微批次 128/256/512 和线程设置。Q4 与 FP8 未显示一致优势；Q4 在
55K 输入上的 MTP 接受率明显下降。4 线程/128 微批次也较慢，因此安装配置保留
Q8、8 线程、256 微批次。没有更换模型权重精度或缩小上下文。

两种缓存候选均完成了 55026-token 提示词的生成，以及 55077-token 输入中
首/中/尾三个标记的检索。第二次检索复用了提示词缓存，不能计作第二次冷 prefill。
代码生成校验使用一个 `parse_port` 任务的 18 项可执行断言；重复通过不代表
18 个独立任务，也不证明一般代码质量或量化数值等价。

真实生产提示词评测中，Q8 与 Q4 各有 6/6 文档结果通过客户端校验；人工逐项对照
记录中的 core/edge，未见相悖行为或虚构副作用。两个成功的提交选择均为期望的
`[1]`，三个提交消息与固定 diff 一致。原配置和两个候选全部在同一
`commit_select_tests` 上失败：`AI did not explain every file`。
该失败保留在原始 JSONL，未重试替换成绩、改写输出或放宽生产校验。
因此不能宣称生产评测全部通过，亦没有证据把这个共同失败归因于加速设置。

这些吞吐量不包含模型切换。实测 27B Direct I/O 加载约 5.84 秒，普通读取约
6.25 秒，最后一次默认加载约 8.56 秒；有启动波动。`load-mode = "dio"` 的类型化
支持及回归已实现并编入最终程序，但用户清除目标时尚未写入实际使用配置。

## 初轮验证及当时未完成事项

- 最终 provider 测试：21 通过。
- 最终 terminal library 测试：227 通过、13 忽略；包括真实子进程 argv、默认兼容、
  runtime 热重载和已有 lease 生命周期测试。
- 初轮 workspace：885 通过、17 忽略、4 个既有路径/符号链接/hook 相关过滤项。
- benchmark 两个独立审阅发现的问题均先复现再修复：启动后 metadata 写入失败的
  子进程清理，以及把 schema 接受误称语义通过。Python 回归：2 通过。
- release 两次构建通过，实际安装配置通过程序解析/health 检查。
- 扩展 AI UI 集成：73 通过、4 失败、10 忽略。失败项如下，未计入通过：
  `ai_commit_recommendation_requires_explicit_stage_before_generating_message`、
  `ai_commit_shows_batch_progress_and_ticks_while_provider_is_silent`、
  `ai_merge_native_preview_accepts_current_block_and_undoes`、
  `ai_merge_preserves_bom_and_crlf_until_reviewed_apply`。
  单独复查后第二项通过，另外三项仍失败。表现为等待提交文本/预览应用状态；
  尚未解决或证明与本次改动无关。
- 实际安装配置的 GPU 模型切换与行内预览/接受验证尚未执行。为此新增的
  `local_generation_live_installed_runtime_restores_completion_after_commit`
  保持显式 opt-in，不把忽略视作通过。

上述 UI 失败和实际切换待验项已在续验中关闭：AI UI 整组 77 通过、0 失败、
10 忽略；补全子集 33 通过；常规 library 串行 227 通过。实际切换测试 20.04 秒，
9B 前后补全均为正确的 `b`；实际原生预览和生命周期测试 9.20 秒通过。
先前的失败记录与质量评测中共有的 `commit_select_tests` 失败均保留，后者不在
此次三项 UI 回归修复范围内。

测试日志位于忽略目录 `target/local-performance-*.log`。这些限制意味着本记录
证明了已测的推理吞吐提升，不宣称整个编辑器的端到端延迟提高 15.8 倍，也不
宣称已找到所有硬件/模型/任务上的绝对最优配置。

复现工具：`contrib/ai-local-performance.py`。每次运行创建新目录并保存 argv、
脚本快照、请求、回复、timings 和失败。旧快照中的 production `passed` 字段只
表示当时的客户端接受；当前脚本已改称 `client_validated`，并检查期望文件选择。
