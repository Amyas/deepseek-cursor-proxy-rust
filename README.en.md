# deepseek-cursor-proxy-rust

[中文](./README.md) | [English](./README.en.md)

A local OpenAI-compatible proxy for DeepSeek models, rebuilt in Rust.

This project is designed to run locally and provide a compatibility layer for DeepSeek-oriented clients and OpenAI-style tooling. It includes request normalization, reasoning cache support, streaming rewriting, and an optional Cloudflare Quick Tunnel for clients that cannot access `localhost` directly.

## Background

Many OpenAI-compatible clients run into one or more of the following issues when used with DeepSeek-style models:

- request fields do not fully match DeepSeek expectations
- tool-call history and reasoning round-tripping are inconsistent
- streaming chunks need normalization
- UI clients want visible "thinking" content folded into assistant output
- some desktop clients cannot reliably access local loopback addresses

This project provides a local proxy layer to address those cases.

## Current status

This repository is already usable as a local Rust proxy, but it is still an actively evolving rewrite.

Implemented today:

- local `axum` HTTP server
- `GET /healthz`
- `GET /models`
- `GET /v1/models`
- `POST /chat/completions`
- `POST /v1/chat/completions`
- OpenAI-style request normalization for DeepSeek-compatible payloads
- `functions -> tools`
- `function_call -> tool_choice`
- `reasoning_effort` normalization
- stripping mirrored thinking blocks from assistant history
- non-streaming response rewriting
- streaming SSE chunk rewriting
- SQLite reasoning cache
- optional visible reasoning folding into assistant content
- basic trace file writing
- `--clear-reasoning-cache`
- Cloudflare Quick Tunnel integration
- automatic sync of Cursor `Override OpenAI Base URL` after Quick Tunnel startup

Still open for future improvement:

- deeper parity with the original Python implementation
- richer missing-reasoning recovery flows
- more detailed trace output
- stronger upstream error passthrough

## Features

- Pure local runtime. No dedicated server required.
- OpenAI-compatible API surface.
- Non-streaming and streaming chat completions support.
- Local SQLite reasoning cache.
- Reasoning folding into visible assistant content.
- Optional public HTTPS exposure through Cloudflare Quick Tunnel.

## Project layout

Core modules:

- [src/http](/Users/amyas/github/deepseek-cursor-proxy-rust/src/http)
  Local HTTP routes, handlers, SSE rewriting.
- [src/protocol](/Users/amyas/github/deepseek-cursor-proxy-rust/src/protocol)
  Request normalization, response rewriting, reasoning folding.
- [src/reasoning](/Users/amyas/github/deepseek-cursor-proxy-rust/src/reasoning)
  Reasoning keys, cache logic, SQLite storage.
- [src/cursor](/Users/amyas/github/deepseek-cursor-proxy-rust/src/cursor)
  Cursor local state DB helpers and automatic `openAIBaseUrl` sync.
- [src/tunnel](/Users/amyas/github/deepseek-cursor-proxy-rust/src/tunnel)
  Cloudflare Quick Tunnel integration.
- [src/trace](/Users/amyas/github/deepseek-cursor-proxy-rust/src/trace)
  Basic trace writing.

## Requirements

- Rust toolchain
- Cargo
- Network access to DeepSeek upstream
- Optional: `cloudflared` if you want a public Quick Tunnel URL

## Installation

```bash
git clone git@github.com:Amyas/deepseek-cursor-proxy-rust.git
cd deepseek-cursor-proxy-rust
```

## Quick start

### Start locally

```bash
cargo run -- --port 9010
```

Health check:

```bash
curl --noproxy '*' http://127.0.0.1:9010/healthz
```

Expected response:

```json
{"ok":true}
```

### List models

```bash
curl --noproxy '*' http://127.0.0.1:9010/v1/models
```

## Usage

### Non-streaming request

```bash
curl --noproxy '*' http://127.0.0.1:9010/v1/chat/completions \
  -H 'Authorization: Bearer <DEEPSEEK_API_KEY>' \
  -H 'Content-Type: application/json' \
  -d '{
    "model": "deepseek-v4-pro",
    "messages": [
      {
        "role": "user",
        "content": "Introduce yourself briefly."
      }
    ]
  }'
```

### Streaming request

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
        "content": "Think step by step and answer: what is 1 + 1?"
      }
    ]
  }'
```

## Cursor / clients that cannot access localhost

Some clients cannot reliably access `127.0.0.1` or `localhost`. In those cases, you can enable Cloudflare Quick Tunnel to temporarily expose the local proxy through a public HTTPS URL.

### Start Quick Tunnel

```bash
cargo run -- --port 9010 --tunnel --verbose
```

If `cloudflared` is not in your `PATH`:

```bash
cargo run -- --port 9010 --tunnel --cloudflared-bin /opt/homebrew/bin/cloudflared --verbose
```

On success, the proxy prints:

```text
https://xxxx.trycloudflare.com/v1
```

Use that as the client Base URL.

If Cursor is installed on the same machine, the proxy also updates Cursor's local `Override OpenAI Base URL` automatically by default. The stored value is the raw public URL without `/v1`, for example:

```text
https://xxxx.trycloudflare.com
```

Behavior summary:

- automatic sync is enabled by default
- the proxy creates a backup of `~/Library/Application Support/Cursor/User/globalStorage/state.vscdb` before writing
- the default Cursor state DB path is `~/Library/Application Support/Cursor/User/globalStorage/state.vscdb`
- if Cursor is already running, it may later write the old in-memory state back to disk
- the safest flow is to quit Cursor before starting the proxy and Quick Tunnel
- if Cursor was already open, restart Cursor after the tunnel URL has been updated

Disable automatic sync:

```bash
cargo run -- --port 9010 --tunnel --no-sync-cursor-base-url
```

Override the Cursor state DB path:

```bash
cargo run -- --port 9010 --tunnel --cursor-state-db "/path/to/state.vscdb"
```

Notes:

- Quick Tunnel URLs are usually temporary
- the public URL may change after restart
- keep the proxy process running while the client is using the tunnel

## Configuration

The proxy auto-generates a config file on first run:

```text
~/.deepseek-cursor-proxy-rust/config.yaml
```

Default reasoning cache path:

```text
~/.deepseek-cursor-proxy-rust/reasoning_content.sqlite3
```

Current main fields:

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
- `sync_cursor_openai_base_url`
- `cursor_state_db_path`

## Common CLI options

```bash
--host
--port
--verbose
--trace-dir
--clear-reasoning-cache
--tunnel
--tunnel-provider
--cloudflared-bin
--no-sync-cursor-base-url
--cursor-state-db
```

Clear local reasoning cache:

```bash
cargo run -- --clear-reasoning-cache
```

## Debugging

### Verbose logging

```bash
cargo run -- --port 9010 --verbose
```

### Trace files

```bash
cargo run -- --port 9010 --verbose --trace-dir /tmp/dcp-traces
```

## Testing

Run all tests:

```bash
cargo test
```

Current coverage includes:

- protocol normalization
- non-streaming response rewriting
- SSE rewriting
- reasoning cache behavior
- route-level integration tests
- basic trace writer coverage
- Quick Tunnel URL extraction
- Cursor `openAIBaseUrl` sync logic

## Release binaries

The repository includes a GitHub Actions release workflow:

- file: [`.github/workflows/release.yml`](/Users/amyas/github/deepseek-cursor-proxy-rust/.github/workflows/release.yml:1)
- trigger: push a tag starting with `v`, for example `v0.1.0`
- build targets:
  - Linux `x86_64-unknown-linux-gnu`
  - macOS `aarch64-apple-darwin`
  - Windows `x86_64-pc-windows-msvc`

Release commands:

```bash
git tag v0.1.0
git push origin v0.1.0
```

After pushing the tag, GitHub Actions will:

- run tests
- build release binaries
- package archives
- create a GitHub Release
- upload downloadable assets to the release page

## Recommended usage modes

### Pure local development

```bash
cargo run -- --port 9010
```

Then use:

```text
http://127.0.0.1:9010/v1
```

### Cursor and similar clients

```bash
cargo run -- --port 9010 --tunnel --verbose
```

Then use the printed:

```text
https://xxxx.trycloudflare.com/v1
```

## Known limitations

- The current version is not yet a byte-for-byte parity clone of the original Python project
- Quick Tunnel depends on `cloudflared`
- Quick Tunnel URLs are not stable
- some advanced recovery branches can still be improved

## Roadmap

- stronger parity with the original Python behavior
- more complete recovery logic
- richer trace and debug output
- more complete client integration guides

## License

Add a formal license file before public release, such as `MIT` or `Apache-2.0`.
