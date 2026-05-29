# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

MCP server that wraps xAI's Grok image API. Hits two endpoints: `POST /v1/images/generations` for text-to-image and `POST /v1/images/edits` for image-to-image. Exposes three tools over stdio JSON-RPC: `generate_image` (text-to-image with optional style), `edit_image` (single- or multi-image editing via natural language; up to 3 source images), and `list_styles` (discover available styles). Built with the `rmcp` crate.

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

# Optional custom styles (override built-ins by using the same name)
[[styles]]
name = "my-style"
description = "My custom look"
template = "{prompt}, in my custom style"
```

## Architecture

Single-file server (`src/main.rs`). Everything lives in one file:

- **Styles** — `Style` struct, `BUILTIN_STYLES` constant (14 built-in styles), `build_styles()` merges custom config styles with built-ins (same-name overrides). Templates use `{prompt}` placeholder; invalid custom templates are skipped with a warning.
- **Config** — `Config` struct (with optional `styles: Vec<StyleConfig>`) + `load_config()` reads TOML from `~/.config/`
- **API types** — `GrokGenerateRequest` (text-to-image), `GrokEditRequest` (image-to-image) with `image: GrokImageRef` for single source or `images: Vec<GrokImageRef>` for multi-source (mutually exclusive), and shared `GrokImageResponse`
- **MCP param types** — `GenerateImageParams` (includes optional `style`), `EditImageParams` (accepts `image_url` or `images` — local paths, http(s) URLs, or `data:` URIs; no style — edit prompts are instructions, not descriptions), both with `schemars` for JSON Schema generation
- **Server** — `GrokImageServer` with `#[tool_router]` and `#[tool_handler]` macros from `rmcp`; stores merged styles as `Arc<Vec<Style>>`; `call_grok_api()` is generic over the request body and takes the endpoint URL, `format_response()` builds text output, `resolve_image_source()` normalizes user input (URL / data URI / local path → API-ready string)
- **Tools** — `list_styles` (parameterless, returns all styles), `generate_image` (POSTs to `/v1/images/generations`; resolves style before API call, shows resolved prompt in response), `edit_image` (POSTs to `/v1/images/edits`; no style support; for multi-image edits, prompt references sources as `<IMAGE_0>`, `<IMAGE_1>`, ...)

## Model Selection

Default model is `grok-imagine-image`. (xAI retired `grok-imagine-image-pro` and other prior-generation models on 2026-05-15.)
