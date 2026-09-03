# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

MCP server that wraps xAI's Grok image API. Hits two endpoints: `POST /v1/images/generations` for text-to-image and `POST /v1/images/edits` for image-to-image. Exposes four tools over stdio JSON-RPC: `generate_image` (text-to-image with optional style), `edit_image` (single- or multi-image editing via natural language; up to 5 source images), `headshot` (Gemini-equivalent expand-only: white letterbox + clean white background via `grok-imagine-image-2.0` at quality medium — no cutout/rembg, never cover-crop or reframe pose), and `list_styles` (discover available styles). Built with the `rmcp` crate.

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

Binary crate with domain modules under `src/`:

| Module | Responsibility |
|--------|----------------|
| `main.rs` | Tracing + stdio serve |
| `config.rs` | TOML / `XAI_API_KEY` config |
| `styles.rs` | 14 built-ins + custom merge (`{prompt}` required; same-name overrides) |
| `grok.rs` | xAI request/response types, model defaults, `quality_supported` |
| `params.rs` | MCP tool params (`schemars`) + input validation |
| `image_io.rs` | Data URIs, local files, mime/magic bytes, multi-image source resolve, HTTP fetch |
| `headshot.rs` | Letterbox pad, expand-only prompt, pronoun, quality default for 2.0 |
| `server.rs` | `GrokImageServer` (`rmcp` tools), Grok HTTP, save/format MCP content |

- **Tools** — `list_styles` (parameterless, returns all styles), `generate_image` (POSTs to `/v1/images/generations`; resolves style before API call, shows resolved prompt in response), `edit_image` (POSTs to `/v1/images/edits`; no style support; for multi-image edits, prompt references sources as `<IMAGE_0>`, `<IMAGE_1>`, ...; up to 5 sources), `headshot` (letterbox pad 550→780 default, expand-only prompt, default model `grok-imagine-image-2.0` at quality medium, aspect 3:2 / 2k / b64_json; optional `output_path`)

## Model Selection

Default model is `grok-imagine-image-2.0` (optional `quality`: `low` / `medium` / `auto`; omitted = `auto`, which currently serves `low` for generation and `medium` for editing). `grok-imagine-image` (1.0) remains available. `grok-imagine-image-quality` retires on 2026-11-02 and then redirects to 2.0 at `quality: low`. (xAI retired `grok-imagine-image-pro` and other prior-generation models on 2026-05-15.)
