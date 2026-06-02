# deepseek-cursor-proxy-rust

A local OpenAI-compatible proxy for DeepSeek models, rebuilt in Rust.

This project is designed for tools that can speak the OpenAI Chat Completions API but need compatibility fixes for DeepSeek reasoning and streaming behavior. It focuses on running locally, while still supporting an optional public HTTPS entrypoint through Cloudflare Quick Tunnel for clients like Cursor that cannot call `localhost` directly.

## Why this exists

DeepSeek-compatible clients often run into one or more of these problems:

- missing or inconsistent `reasoning_content` handling
- tool-call history that needs local repair
- streaming chunks that need normalization
- UI clients that need visible "thinking" content folded into normal assistant output
- local development setups that need a temporary public URL

This project provides a Rust implementation of those compatibility layers.

## Current status

This repository is already usable for local development and proxy experiments, but it is still an active rewrite. The core local proxy path is implemented and tested. Some advanced recovery and trace parity with the original Python project can still be improved over time.

What is implemented today:

- local `axum` HTTP server
- OpenAI-compatible `GET /models`, `GET /healthz`, `POST /v1/chat/completions`
- request normalization for DeepSeek-compatible chat payloads
- `functions -> tools` and `function_call -> tool_choice` compatibility
- `reasoning_effort` normalization
- stripping mirrored thinking blocks from assistant history
- non-streaming response rewriting
- streaming SSE chunk rewriting
- SQLite-based reasoning cache
- optional reasoning folding into visible assistant content
- config file auto-generation
- `--clear-reasoning-cache`
- optional Cloudflare Quick Tunnel integration

## Features

- Pure local runtime. No dedicated server required.
- OpenAI-style API surface for local tools and custom model integrations.
- SQLite reasoning cache stored on disk.
- Streaming-aware accumulation of `reasoning_content` and tool calls.
- Cursor-friendly thinking display via folded assistant content.
- Optional Quick Tunnel for clients that cannot access `localhost`.

## Project layout

Key modules:

- [src/http](/Users/amyas/github/deepseek-cursor-proxy-rust/src/http)  
  Local HTTP routes, handlers, SSE rewriting.
- [src/protocol](/Users/amyas/github/deepseek-cursor-proxy-rust/src/protocol)  
  Request normalization, response rewriting, reasoning folding.
- [src/reasoning](/Users/amyas/github/deepseek-cursor-proxy-rust/src/reasoning)  
  Cache keys, SQLite store, reasoning lookup primitives.
- [src/tunnel](/Users/amyas/github/deepseek-cursor-proxy-rust/src/tunnel)  
  Cloudflare Quick Tunnel integration.
- [src/trace](/Users/amyas/github/deepseek-cursor-proxy-rust/src/trace)  
  Basic trace file writing.

## Requirements

- Rust toolchain
- Cargo
- Network access to DeepSeek
- Optional: `cloudflared` if you want a public Quick Tunnel URL

## Installation

Clone the repo and run directly with Cargo:

```bash
git clone <your-repo-url> deepseek-cursor-proxy-rust
cd deepseek-cursor-proxy-rust
cargo run -- --port 9010
```

## Quick start

Start the local proxy:

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

List models:

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

## Cloudflare Quick Tunnel

Some clients, especially Cursor custom model integrations, cannot reliably call `localhost` or `127.0.0.1` directly. For those cases, this project can launch a Cloudflare Quick Tunnel and print a temporary public Base URL.

Start with tunnel enabled:

```bash
cargo run -- --port 9010 --tunnel --verbose
```

If `cloudflared` is not in your `PATH`, specify it explicitly:

```bash
cargo run -- --port 9010 --tunnel --cloudflared-bin /opt/homebrew/bin/cloudflared --verbose
```

When successful, the proxy prints a public URL like:

```text
https://example-name.trycloudflare.com/v1
```

Use that as your client Base URL.

Notes:

- Quick Tunnel URLs are temporary.
- Restarting the proxy usually creates a new public URL.
- Keep the proxy process running while your client is using the tunnel.

## Configuration

The proxy auto-creates a config file on first run:

```text
~/.deepseek-cursor-proxy-rust/config.yaml
```

Default reasoning cache path:

```text
~/.deepseek-cursor-proxy-rust/reasoning_content.sqlite3
```

Current config fields include:

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

## Command-line options

Common options:

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

Clear local reasoning cache:

```bash
cargo run -- --clear-reasoning-cache
```

## Debugging

Verbose mode:

```bash
cargo run -- --port 9010 --verbose
```

Verbose mode with basic request trace files:

```bash
cargo run -- --port 9010 --verbose --trace-dir /tmp/dcp-traces
```

## Testing

Run all tests:

```bash
cargo test
```

The current test suite covers:

- protocol normalization
- response rewriting
- SSE rewriting
- reasoning cache behavior
- route-level integration tests
- trace writer basics
- tunnel URL extraction

## Limitations

- This rewrite is not yet a byte-for-byte parity clone of the original Python implementation.
- Quick Tunnel integration depends on `cloudflared`.
- Quick Tunnel URLs are not stable across restarts.
- Some advanced recovery paths and richer trace details can still be expanded.

## Recommended usage modes

### Pure local development

Use:

```bash
cargo run -- --port 9010
```

and call:

```text
http://127.0.0.1:9010/v1
```

### Cursor or other clients that cannot reach localhost

Use:

```bash
cargo run -- --port 9010 --tunnel --verbose
```

and copy the printed:

```text
https://xxxx.trycloudflare.com/v1
```

## Roadmap

Areas that can still be improved:

- deeper parity with the original Python recovery behavior
- richer structured request/response tracing
- more complete upstream error passthrough
- stronger end-to-end tunnel lifecycle observability

## License

Add your preferred license here before public release.
