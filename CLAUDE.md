# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

MCP server that wraps xAI's Grok image generation API (`https://api.x.ai/v1/images/generations`). Exposes three tools over stdio JSON-RPC: `generate_image` (text-to-image with optional style), `edit_image` (image editing via natural language), and `list_styles` (discover available styles). Built with the `rmcp` crate.

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
- **API types** — `GrokImageRequest`/`GrokImageResponse` for the xAI REST API
- **MCP param types** — `GenerateImageParams` (includes optional `style`), `EditImageParams` (no style — edit prompts are instructions, not descriptions), both with `schemars` for JSON Schema generation
- **Server** — `GrokImageServer` with `#[tool_router]` and `#[tool_handler]` macros from `rmcp`; stores merged styles as `Arc<Vec<Style>>`; `call_grok_api()` handles HTTP, `format_response()` builds text output
- **Tools** — `list_styles` (parameterless, returns all styles), `generate_image` (resolves style before API call, shows resolved prompt in response), `edit_image` (no style support)

Both `generate_image` and `edit_image` share the same API endpoint and request struct; `edit_image` sets the `image_url` field while `generate_image` leaves it `None`.

## Model Selection

Default model is `grok-imagine-image-pro`. The legacy `grok-2-image-1212` model does **not** support `aspect_ratio` — see `BUG.md` for history on this issue.
