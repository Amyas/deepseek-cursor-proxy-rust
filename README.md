# deepseek-cursor-proxy-rust

一个使用 Rust 重写的本地 OpenAI 兼容代理，面向 DeepSeek 模型与相关客户端兼容场景。

这个项目的目标是：

- 以本地运行方式提供 DeepSeek 兼容代理
- 修复和归一化部分请求/响应行为
- 支持 `reasoning_content` 缓存与流式处理
- 为不能直接访问 `localhost` 的客户端提供可选的 Cloudflare Quick Tunnel 出口

当前文档默认使用中文。

## 项目背景

很多使用 OpenAI Chat Completions 协议的客户端，在接入 DeepSeek 或带推理内容的模型时，会遇到这些问题：

- 请求字段与 DeepSeek 的兼容性不完全一致
- 工具调用历史与推理内容回传逻辑不一致
- 流式输出需要额外规范化
- 客户端界面希望看到可展开的 thinking 内容
- 一些客户端无法直接访问本地 `localhost`

这个项目就是围绕这些场景，提供一个本地代理层。

## 当前状态

当前仓库已经可以作为一个可运行、可测试的 Rust 本地代理使用，但它仍然是一个持续演进中的重构版本。

当前已实现：

- 本地 `axum` HTTP 服务
- `GET /healthz`
- `GET /models`
- `GET /v1/models`
- `POST /chat/completions`
- `POST /v1/chat/completions`
- OpenAI 风格请求字段兼容转换
- `functions -> tools`
- `function_call -> tool_choice`
- `reasoning_effort` 归一化
- assistant 历史中的 thinking block 剥离
- 非流式响应重写
- 流式 SSE chunk 改写
- SQLite reasoning cache
- reasoning 可见内容折叠展示
- 基础 trace 文件输出
- `--clear-reasoning-cache`
- Cloudflare Quick Tunnel 集成

当前仍可继续增强：

- 与原 Python 版本更深度的行为完全对齐
- 更完整的 missing reasoning recovery 分支
- 更细的 trace 结构
- 更强的上游错误透传

## 功能特性

- 纯本地运行，不需要单独部署服务器
- 提供 OpenAI 兼容 API 接口
- 支持非流式和流式 chat completions
- 支持本地 SQLite reasoning 缓存
- 支持将 reasoning 内容折叠进 assistant content
- 支持通过 Cloudflare Quick Tunnel 暴露公网 HTTPS 地址

## 目录结构

核心模块：

- [src/http](/Users/amyas/github/deepseek-cursor-proxy-rust/src/http)
  本地 HTTP 路由、handler、SSE 改写
- [src/protocol](/Users/amyas/github/deepseek-cursor-proxy-rust/src/protocol)
  请求规范化、响应重写、thinking 展示折叠
- [src/reasoning](/Users/amyas/github/deepseek-cursor-proxy-rust/src/reasoning)
  reasoning key、缓存、SQLite 存储
- [src/tunnel](/Users/amyas/github/deepseek-cursor-proxy-rust/src/tunnel)
  Cloudflare Quick Tunnel 集成
- [src/trace](/Users/amyas/github/deepseek-cursor-proxy-rust/src/trace)
  基础 trace 写入

## 环境要求

- Rust toolchain
- Cargo
- 可访问 DeepSeek 上游网络
- 如果要启用 Quick Tunnel，需要安装 `cloudflared`

## 安装

```bash
git clone git@github.com:Amyas/deepseek-cursor-proxy-rust.git
cd deepseek-cursor-proxy-rust
```

## 快速开始

### 本地启动

```bash
cargo run -- --port 9010
```

本地健康检查：

```bash
curl --noproxy '*' http://127.0.0.1:9010/healthz
```

期望返回：

```json
{"ok":true}
```

### 查看模型列表

```bash
curl --noproxy '*' http://127.0.0.1:9010/v1/models
```

## 使用方式

### 非流式请求

```bash
curl --noproxy '*' http://127.0.0.1:9010/v1/chat/completions \
  -H 'Authorization: Bearer <DEEPSEEK_API_KEY>' \
  -H 'Content-Type: application/json' \
  -d '{
    "model": "deepseek-v4-pro",
    "messages": [
      {
        "role": "user",
        "content": "你好，请简单介绍一下你自己。"
      }
    ]
  }'
```

### 流式请求

```bash
curl --noproxy '*' -N http://127.0.0.1:9010/v1/chat/completions \
  -H 'Authorization: Bearer <DEEPSEEK_API_KEY>' \
  -H 'Content-Type: application/json' \
  -d '{
    "model": "deepseek-v4-pro",
    "stream": true,
    "messages": [
      {
        "role": "user",
        "content": "请一步一步思考后回答：1 + 1 等于几？"
      }
    ]
  }'
```

## Cursor / 无法访问 localhost 的客户端

一些客户端不能直接访问 `127.0.0.1` 或 `localhost`。这时可以启用 Cloudflare Quick Tunnel，让本地代理临时暴露为公网 HTTPS 地址。

### 启动 Quick Tunnel

```bash
cargo run -- --port 9010 --tunnel --verbose
```

如果 `cloudflared` 不在 `PATH` 中：

```bash
cargo run -- --port 9010 --tunnel --cloudflared-bin /opt/homebrew/bin/cloudflared --verbose
```

启动成功后，程序会打印：

```text
https://xxxx.trycloudflare.com/v1
```

把这个地址填入客户端的 Base URL 即可。

注意：

- Quick Tunnel 地址通常是临时的
- 每次重启代理后地址可能变化
- 使用期间需要保持代理进程持续运行

## 配置文件

程序首次启动时会自动生成配置文件：

```text
~/.deepseek-cursor-proxy-rust/config.yaml
```

默认 reasoning cache 文件：

```text
~/.deepseek-cursor-proxy-rust/reasoning_content.sqlite3
```

当前主要配置项包括：

- `host`
- `port`
- `upstream_base_url`
- `upstream_model`
- `thinking`
- `reasoning_effort`
- `request_timeout_secs`
- `max_request_body_bytes`
- `reasoning_content_path`
- `display_reasoning`
- `collapsible_reasoning`
- `verbose`
- `trace_dir`
- `tunnel_enabled`
- `tunnel_provider`
- `cloudflared_bin`

## 常用命令行参数

```bash
--host
--port
--verbose
--trace-dir
--clear-reasoning-cache
--tunnel
--tunnel-provider
--cloudflared-bin
```

清空本地 reasoning cache：

```bash
cargo run -- --clear-reasoning-cache
```

## 调试

### verbose 日志

```bash
cargo run -- --port 9010 --verbose
```

### trace 文件

```bash
cargo run -- --port 9010 --verbose --trace-dir /tmp/dcp-traces
```

## 测试

运行全部测试：

```bash
cargo test
```

当前测试覆盖包括：

- 协议字段规范化
- 非流式响应重写
- SSE 改写
- reasoning cache 行为
- 路由级集成测试
- trace writer 基础能力
- Quick Tunnel URL 提取逻辑

## 发布可执行文件

当前仓库已经包含 GitHub Actions 的 release 工作流：

- 文件位置：[.github/workflows/release.yml](/Users/amyas/github/deepseek-cursor-proxy-rust/.github/workflows/release.yml:1)
- 触发方式：推送以 `v` 开头的 tag，例如 `v0.1.0`
- 产物平台：
  - Linux `x86_64-unknown-linux-gnu`
  - macOS `aarch64-apple-darwin`
  - Windows `x86_64-pc-windows-msvc`

发布步骤：

```bash
git tag v0.1.0
git push origin v0.1.0
```

推送后，GitHub Actions 会自动：

- 运行测试
- 构建 release 二进制
- 打包归档文件
- 创建 GitHub Release
- 把可执行文件作为 Release Assets 上传

用户后续就可以直接在 GitHub Releases 页面下载对应平台的可执行文件。

## 推荐使用模式

### 纯本地开发

```bash
cargo run -- --port 9010
```

然后访问：

```text
http://127.0.0.1:9010/v1
```

### 给 Cursor 等客户端使用

```bash
cargo run -- --port 9010 --tunnel --verbose
```

然后使用输出的：

```text
https://xxxx.trycloudflare.com/v1
```

## 已知限制

- 当前版本还不是原 Python 项目的逐字节完全等价实现
- Quick Tunnel 依赖 `cloudflared`
- Quick Tunnel 地址不是固定地址
- 部分高级恢复分支仍可继续增强

## 后续方向

- 更强的 Python 行为对齐
- 更完整的 recovery 逻辑
- 更细的 trace 和调试输出
- 更丰富的客户端接入说明

## 许可证

发布前请补充正式许可证文件，例如 `MIT`、`Apache-2.0` 或其他你希望公开使用的许可证。
