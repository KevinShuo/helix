# Minimap 实现与验证 — 2026-11-12

## 实现范围

- 基于 `030843d3e`，复用 `codex/code-folding` 当前 checkout；未合并、推送或修改用户实际配置。
- `245d67dd9`：配置、参数校验、布局和源码行桶映射。
- `ebad47ab5`：有界源码摘要、分片作业、点阵/色块、视口和标记绘制。
- `85653f6e7`：独立鼠标捕获、绝对导航、折叠展开与首批集成测试。
- 收尾补充：取消控制器、会话预算、窗口增大补抽样、水平光标校正、兼容测试及文档。
- 其他功能原有的未提交修改保持独立，没有作为 Minimap 改动纳入提交。本地二进制基于当前工作区，因此包含这些已有修正。

配置默认关闭，最小启用方式：

```toml
[editor.minimap]
enabled = true
```

会话开关为 `:toggle-option minimap.enabled`，兼容绘制为
`:set-option minimap.render blocks`。详细选项见 `book/src/editor.md`。

## 验证环境与结果

Windows，Rust 1.90.0，使用本地已有 Python/C++ 等语法库。
首次自动拉取遇到既有的 sr.ht 证书吊销检查离线错误，因此后续构建进程设置
`HELIX_DISABLE_AUTO_GRAMMAR_BUILD=1`；没有关闭 TLS 校验，也不声称所有语法库被重新拉取/编译。
Git Bash 仅加入测试进程的 PATH。

| 检查 | 结果 |
| --- | --- |
| Minimap view 单元测试 | 6 passed，含配置、布局、正反行桶、Unicode/有界抽样和取消失效 |
| Minimap 绘制/标记单元测试 | 2 passed |
| 全局/项目配置合并测试 | passed；错误宽度被拒绝 |
| Minimap 集成测试 | 13 passed；1 手工测量默认 ignored，另行执行 |
| `cargo test` | 47 library + 2 doc tests passed；1 既有 doc test ignored |
| `cargo test -p helix-core -p helix-view` | 合计 306 passed，4 ignored |
| folding 集成过滤 | 10 passed（包含 Git 工具中的折叠共存用例） |
| function_hints 集成过滤 | 1 passed |
| ai_completion 集成过滤 | 21 passed，3 既有 ignored |
| git_tools 集成过滤 | 12 passed |
| `cargo check -p helix-term --no-default-features` | passed；有既有禁用 VCS 分支的 unused-parameter 警告 |
| 默认 `cargo build -p helix-term --locked` | passed |
| 独立优化版离线构建 | passed，输出 `target/release/hx-minimap.exe` |
| 修改文件的 `rustfmt --check`、`git diff --check` | passed |

Minimap 集成覆盖开关、block/Braille、窗口自动隐藏、独立分屏、鼠标滚轮焦点、
拖动出界/取消/释放不复制、选择模式与次选区、折叠目标展开、横向滚动后跳转、
AI 虚拟行和软换行、跨分片的多行字符串着色、语言/主题变化、编辑/撤销/重载、
现有诊断与 diff（含 EOF 删除）、大文件抽样、空 Windows CRLF 文档、待处理作业取消。

没有运行全部带 `integration` feature 的 230+ 项集成测试；这里列的是相关过滤集，
不把此前仓库已记录的全量平台失败或未运行部分描述为本次全部通过。

## 独立审阅和修正

独立只读审阅发现两项 Important，无 Critical 或 Minor：

1. 绝对跳转保留旧水平滚动时，短行上的主光标会不可见。
   新用例先复现水平 offset=209，然后补充仅校正水平可见性的处理；修正后 offset=0，所选垂直位置保持。
2. 大文件从 4 行高缩略图放大后，旧的低密度抽样会造成假空白条纹。
   新用例先复现，再按新几何需要补足抽样；均匀源码的每个投影行都有占用。

两项均在 `review-red.log` 观察到失败，随后 `review-green.log` 的 12 项用例通过。
后续加入跨批次语法着色验证，最终功能用例为 13 项。
审阅未判定的真实终端外观和性能由下方验收负责；其他功能的原有修改明确排除。

测试夹具另校正两点：Windows 新 scratch 含 CRLF（两个字符）；连续两次配置命令
必须等待前一条的异步配置事件生效，避免后一条按旧快照覆盖前者。未改变全局命令系统。

## 原生 Windows 终端验收

使用默认 feature 构建的二进制副本 `target/minimap-work/hx-native.exe`，在 ConPTY 中打开
包含中文/emoji 的 Python 文件。观察到了 Braille 点阵和 blocks 色块，SGR 鼠标点击
将编辑位置从首行导航到第 209 行；临时配置文件中关闭/开启后执行 `:config-reload`，
缩略图消失/恢复，进程正常退出（0）。

源文件操作前后的 SHA-256 都为：

`dc9c7d00386960556954e694403db829ff562846cceb2cb7be67587d9a5ccfed`

部分终端转录及检查标志保存于忽略的 `target/minimap-work/native-terminal-smoke.json`。
优化版 `target/release/hx-minimap.exe` 也经过原生 ConPTY 验收：两种绘制、鼠标跳到 209 行、正常退出及源码哈希不变；转录为 `native-release-smoke.json`。
没有修改真实用户配置或安装路径。所有字体、终端和平台的外观不在本次单机验收保证内。

注意：`cargo test --features integration` 也会生成带 TestBackend 的 `hx.exe`，该版本启动时不会绘制真实终端。
验收和交付的执行文件通过最后一次**默认 feature build** 恢复；ConPTY 使用独立副本以避免 Windows 文件锁干扰链接。

## 资源与性能

- 常规文档每个源码行使用固定占用位图和八个近似语法色带。10,001 行的摘要行存储为 480,048 字节。
- 只为实际显示缩略图的文档安排作业，按可见文档数分配 32MiB 会话源码摘要预算（单文档不超过 8MiB）。
- 超过 50,000 行/4MiB，或超出该文档预算时改用单色有界抽样；最多 4,096 个代表行。
- 分片最多 128 行/32KiB，约 3ms 后主动让出；单次底层高亮调用不可抢占，3ms 是调度目标而非实时保证。
- 高亮查询按受限批次复用，跨批次的多行字符串测试通过。滚动/光标变化复用源码摘要和投影。

手工测量用例：

```text
cargo test -p helix-term --features integration --test integration minimap_measure_cache_and_rendering -- --ignored --nocapture --test-threads=1
```

本机 debug/TestBackend 的两次测量：1 万行从打开到摘要可用约 7.5–8.8 秒；100 次热重绘
开启平均 6.151/8.126ms，关闭平均 5.864/8.399ms。机器同时有其他工作，不能把差值或
批次重构前后的端到端差异视为可靠的速度提升。初次摘要在后台分片生成，期间可继续编辑。
测量包含解析/打开/防抖/事件循环等待，不是单独的源码扫描耗时，也不是物理终端输入延迟。

优化构建在相同 10,001 行、显式 `HELIX_RUNTIME` 和已编译 release 集成测试可执行文件中测得：
打开到摘要可用 3,285.73ms，100 次热重绘开启平均 0.538ms、关闭平均 0.463ms，摘要行存储仍为 480,048 字节。
这是本机 TestBackend 测量，不是物理终端输入延迟，也没有证明分批查询本身的端到端加速。
没有给普通 CI 设置依赖机器速度的硬时间阈值。

标准 release 目标的 `hx.exe` 正由已有进程使用，Cargo 无法替换它。没有终止该进程；
release 集成测试可执行文件已成功链接，手工直接执行测量通过。
独立优化版 `target/release/hx-minimap.exe` 使用忽略目录中的临时构建入口，引用原来的
`helix-term/src/main.rs` 和本仓库 path dependencies，复用锁定版本，离线构建并避开已占用的文件名。
源码没有复制或修改启动入口。旁边的 `hx-minimap.cmd` 仅为该 workspace 设置运行时目录并传递参数。

## 实现取舍

- 复用已有功能分支和 checkout，避免把既有兼容修正拆到另一份环境；原有本地改动不纳入本次提交。
- 八个色带和单元前景色近似语法颜色，终端无法逐像素缩小字体；颜色精度低于 GUI Minimap。
- 已折叠目标即使处于视口包络高亮内，也优先展开导航；可见源码上的视口抓取保持不跳动。
- 第一版仅右侧、全文件 fit，不含悬停预览、搜索结果标记或 VS Code 的全部外观选项。
