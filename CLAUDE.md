# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

MCP server that wraps xAI's Grok image generation API (`https://api.x.ai/v1/images/generations`). Exposes two tools over stdio JSON-RPC: `generate_image` (text-to-image) and `edit_image` (image editing via natural language). Built with the `rmcp` crate.

## Build & Run

```bash
cargo build --release    # Binary: target/release/mcp-server-grok-image
cargo build              # Debug build
cargo run                # Run (stdio transport, needs config)
RUST_LOG=debug cargo run # Debug logging
```

Rust edition 2024. No test suite.

## Configuration

Reads `~/.config/mcp-server-grok-image/config.toml`:

```toml
api_key = "xai-..."
```

## Architecture

Single-file server (`src/main.rs`, ~270 lines). Everything lives in one file:

- **Config** — `Config` struct + `load_config()` reads TOML from `~/.config/`
- **API types** — `GrokImageRequest`/`GrokImageResponse` for the xAI REST API
- **MCP param types** — `GenerateImageParams`/`EditImageParams` with `schemars` for JSON Schema generation
- **Server** — `GrokImageServer` with `#[tool_router]` and `#[tool_handler]` macros from `rmcp`; `call_grok_api()` handles HTTP, `format_response()` builds text output

Both tools share the same API endpoint and request struct; `edit_image` sets the `image_url` field while `generate_image` leaves it `None`.

## Model Selection

Default model is `grok-imagine-image-pro`. The legacy `grok-2-image-1212` model does **not** support `aspect_ratio` — see `BUG.md` for history on this issue.
