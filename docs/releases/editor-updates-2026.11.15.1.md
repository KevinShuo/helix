# Helix Windows x64 · 原生测试面板与文件删除 · 2026.11.15.1

## 更新

- 原生测试工作台：Python pytest/unittest 与 C/C++ GoogleTest/CTest 的行旁运行按钮、结果面板、失败跳转和失败用例重跑，不依赖 AI 模型。
- 新增 `:test`、`:test-file`、`:test-project`、`:test-show`、`:test-rerun`、`:test-rerun-failed`、`:test-stop`；自定义 gutter 配置可添加 `test`。
- 新增 `:delete-file`：显示文件路径，输入 `yes` 回车后永久删除当前文件并关闭缓冲区；Esc 或其他回答取消。
- `:delete-file!` 允许丢弃未保存修改，仍需确认。删除失败时保留内容，待完成的保存不会在删除后重新创建文件。
- 修复诊断更新：接受适用的空诊断清除，重启或退出语言服务器时清理旧诊断，丢弃退休服务器或旧文档身份的迟到结果。
- 确认提示在窄终端中按实际宽度排版，保留长路径的目标文件名。

## 使用

下载 Windows x64 ZIP 并完整解压，保持 `hx.exe` 与 `runtime` 同目录。
包内包含示例配置、许可证、逐文件校验清单及 `BUILDINFO.json`；附件
`SHA256SUMS.txt` 用于校验下载。个人配置、密钥和模型不包含在发布包中。

测试执行前须保存项目缓冲区。Python 解释器需安装 pytest；C/C++ 需现有
CMake/CTest 测试项目。测试面板中 Enter 跳转，`r` 重跑选中项，`f` 重跑失败项，
`R` 重复上次运行，`s` 停止，`q` 关闭面板。

已在运行的旧编辑器会话需保存并重启以加载新程序。Windows 10/11 x64
需 Microsoft Visual C++ v14 x64 运行库；Git、语言服务器及测试框架按需安装。

## 验证与构建

- 删除文件专项集成测试 12 项通过；终端单元测试 163 项和文档测试 2 项通过。
- 发布前补充验证：视图库单元测试 91 项、真实 pytest 适配器回归 4 项通过；Rust 格式、Python Ruff 检查与命令文档生成通过。
- 完整集成回归：346 通过、17 跳过，编译输出压力与测试声明缓存两个用例超时；两项在相同测试程序中单独重跑均通过。未将这次完整运行记为全绿。
- 测试工作台的真实 pytest/CTest 验证、选择器和取消流程见相应验证记录。
- 发布构建使用 release 优化，并沿用已验证的本机非 LTO 设置，避开本机链接器的堆损坏；仓库默认构建配置不变。

详细记录：[文件删除验证](../superpowers/reports/2026-11-15-delete-current-file-validation.md)、
[测试工作台验证](../superpowers/2026-11-15-test-workbench-validation.md)。
