# Helix Windows x64 · CMake 补全与可迁移语言配置 · 2026.11.18

本次基于 master 的全部已有功能，新增可随当前电脑用户目录解析的语言配置。

- LSP 的 `command`、`args`、`environment` 和嵌套 `config`，以及 formatter 的命令和参数，支持 `${env:NAME}`。
- 提供完整 Windows 示例 `examples/languages.windows-portable.toml`，使用 `LOCALAPPDATA`、`APPDATA`、`USERPROFILE`，不再写死用户名或盘符。
- 示例为 neocmakelsp 设置 CMake 模块搜索前缀，修复 Windows 上 `include(FetchContent)` 后缺少 `FetchContent_Declare`、`FetchContent_MakeAvailable` 等候选的问题。
- 替换在 TOML 解析及可信工作区合并之后执行，缺失变量会报告配置错误；保留原有 formatter 占位符，并保护路径中来自环境变量的 `%`。

## 安装与公司电脑迁移

1. 解压 Windows x64 ZIP，将 `hx.exe` 和同目录的 `runtime` 一起放到安装目录；已有安装可备份后替换。
2. 将 `examples/languages.windows-portable.toml` 复制为 `%APPDATA%\helix\languages.toml`。这是可选示例，不会自动覆盖个人配置。
3. 同时携带或重新安装示例依赖的语言工具；工具目录结构见 `examples/portable-languages-windows.md`。系统变量只解析位置，不会安装这些工具。
4. 重启 hx。旧版 hx 不支持这里新增的环境变量语法，不能只替换 TOML。

CMake 示例使用 Scoop 的 `%USERPROFILE%\scoop\apps\cmake\current`。其他安装方式可按随包说明改用 `${env:HELIX_CMAKE_PREFIX}`；该环境变量应指向当前电脑的 CMake 安装目录。

## 验证与追溯

验证覆盖配置加载、不同用户目录、formatter 实际展开，以及 FetchContent、ExternalProject、GoogleTest、CMakePackageConfigHelpers 的真实 LSP 补全请求。具体测试命令、结果和构建提交记录于随包的 `BUILDINFO.json`。

发布包包含 Windows x64 程序、已跟踪 runtime、Windows 语法 DLL、可迁移配置示例及校验清单；不包含个人配置、凭据或模型文件。解压逐文件校验后，再验证包内 parser/queries，并以同一程序替换本地 hx。
