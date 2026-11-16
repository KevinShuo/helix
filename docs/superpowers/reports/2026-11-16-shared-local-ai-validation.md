# 多 hx 实例共享本地模型验证

## 行为

Windows/Linux 的普通构建通过当前用户的后台 hx 管理器管理模型。编辑器连接负责登记生命周期，请求连接持有覆盖完整推理操作的 FIFO 租约。相同配置复用一份模型，临时生成释放后恢复补全；关闭首个编辑器不会影响其他实例。最后一个连接退出后 10 秒释放模型。

未知外部模型进程仍然受到保护；其退出后，管理器可在确认端口空闲的情况下重新取得模型管理权。配置重载会取消使用旧配置的等待和推理。

## 自动验证

- 默认包测试：196 通过、0 失败、13 个明确 opt-in 测试忽略；文档测试 2 通过、1 忽略。
- 实际 hx 辅助进程回归：11 通过，覆盖同一模型复用、先启动者退出、最后客户端退出、慢恢复取消、启动竞争、排队/加载/推理取消、崩溃及旧发现文件恢复、运行参数变更、外部进程保护及外部所有者退出。
- 新增 Rust 回归覆盖生产 registrar/discovery/with_model 路径、配置重载释放租约，以及模拟超过 180 秒的多请求操作不会失去模型。
- AI 原生界面回归：76 通过、0 失败、10 个明确 opt-in 测试忽略。
- 修改过的 Rust 文件 rustfmt 检查、git diff --check 通过。

运行命令：

```powershell
$env:HELIX_DISABLE_AUTO_GRAMMAR_BUILD = '1'
$env:HELIX_RUNTIME = 'I:\DEV\helix\runtime'
cargo test --locked --profile integration --jobs 1 --config 'profile.integration.package.helix-term.opt-level=0' -p helix-term -- --test-threads=1
python contrib/test_shared_ai.py --hx target/shared-ai-probe/hx.exe
cargo test --locked --profile integration --jobs 1 --config 'profile.integration.package.helix-term.opt-level=0' --features integration -p helix-term --test integration --no-run
# 然后直接运行构建出的 integration 测试程序：ai_ --test-threads=1
```

基线按仓库默认 integration 优化等级编译时 rustc 发生 STATUS_ACCESS_VIOLATION；将 helix-term 测试优化降为 0、串行构建后通过。Release 使用仓库原有优化设置。integration feature 保留隔离的应用内生命周期后端；跨进程共享另由实际 hx 程序与生产客户端路径回归覆盖。

## 本机双编辑器与 GPU 验证

RTX 4090 24GB，使用已安装的官方 llama.cpp b12428 CUDA 12.4 和用户现有 GGUF。复制现有配置到临时文件，仅将本地端口改为 19360/19361/19362；模型、上下文和运行参数沿用实际配置。全部输入为临时合成 Python 代码。

1. 两个独立 hx 编辑器 PID 35164、3416 连接同一个管理器 PID 1544；常驻模型只有一个进程 PID 6740。
2. 两个编辑器均显示灰字 `b`，正文在接受前未改变。
3. 真实 9B 补全返回 `b`；通过共享租约切换 27B，生成返回 `OK`；期间第二个编辑器的补全请求等待，恢复 9B 后继续显示 `b`。
4. 关闭先启动的编辑器后，管理器与恢复后的模型 PID 24428 保持运行；第二个编辑器继续获得 `b`，Shift-Right 成功接受为正文。
5. 关闭最后一个编辑器后，管理器在约 10.04 秒删除发现文件并退出，模型进程被释放。

机器证据在 `target/shared-ai-gpu-result.json`、`target/shared-ai-native-result.json`，完整测试输出在 `target/shared-ai-*-final.log` 和 `target/shared-ai-native-tests.log`。这些本机临时证据未加入版本控制，不包含管理器认证凭据。

## 独立审阅

审阅提出 5 个 Important 问题，已覆盖回归并修复：移除虚构的 180 秒总操作时限；以 OS 锁验证发现文件；在恢复前释放断开连接的计数；端口空闲时允许接管已退出的外部所有者；仅在已实现父进程崩溃保护的平台启用共享。

没有未处理的 Minor 问题。Windows 已实机验证；Linux 使用 parent-death signal，本次未实机运行。macOS/BSD 保留原后端，仍有原先的多实例切换限制。

## 安装

旧版备份：`I:\DEV\helix\target\release\hx.before-shared-ai-20261116-220446.exe`。正在运行的旧程序另保留为 `hx.running-before-shared-ai-20261116-221537.exe`。

`cargo build --release --locked --jobs 2 -p helix-term --bin hx` 成功，耗时 3 分 43 秒，沿用仓库原有 thin LTO 优化。已替换 `I:\DEV\helix\target\release\hx.exe`，版本为 `helix 25.07.1 (78ace1e0)`。SHA-256：`9bbb3e7525859915981eb9adc75b731d5a752b2fa149471db7dec39fdf6df424`。

安装后 `hx --health` 确认现有配置有效，最终 Release 可执行文件再次通过全部 11 项进程回归（0 失败）。测试模型子进程已清理。保留用户正在运行的旧 hx 进程，用户需重新打开所有旧实例以使用共享管理器。代码保留在本地 `codex/shared-local-ai` 分支。
