# mcp-server-grok-image

An MCP (Model Context Protocol) server for xAI's Grok image generation API. Built in Rust, exposes image generation and editing as MCP tools.

Communicates via stdio using JSON-RPC 2.0, like all MCP servers.

## Tools

| Tool | Description |
|------|-------------|
| `generate_image` | Generate an image from a text prompt |
| `edit_image` | Edit an existing image using natural language instructions |

### generate_image

Generate an image from a text description.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `prompt` | string | yes | Text description of the desired image |
| `model` | string | no | Model to use (default: `grok-imagine-image-pro`) |
| `n` | integer | no | Number of images to generate (1-10, default 1) |
| `aspect_ratio` | string | no | Aspect ratio: `1:1`, `16:9`, `9:16`, `4:3`, `3:4`, `3:2`, `2:3`, `2:1`, `1:2`, `auto`, etc. |
| `resolution` | string | no | Output resolution: `1k` (~1024px, default) or `2k` (~2048px) |
| `response_format` | string | no | Output format: `url` (default, temporary) or `b64_json` |

### edit_image

Edit an existing image using natural language instructions.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `image_url` | string | yes | URL or base64 data URI of the source image |
| `prompt` | string | yes | Natural language edit instructions |
| `model` | string | no | Model to use (default: `grok-imagine-image-pro`) |
| `n` | integer | no | Number of variations to generate (1-10, default 1) |
| `resolution` | string | no | Output resolution: `1k` (~1024px, default) or `2k` (~2048px) |
| `response_format` | string | no | Output format: `url` (default, temporary) or `b64_json` |

### Available Models

| Model | Quality | Resolution | Cost | Rate Limit |
|-------|---------|------------|------|------------|
| `grok-imagine-image-pro` | Premium (default) | 1k/2k | $0.07/image | 30 RPM |
| `grok-imagine-image` | Faster | 1k/2k | $0.02/image | 300 RPM |
| `grok-2-image-1212` | Legacy | Fixed | $0.07/image | 300 RPM |

Note: The legacy `grok-2-image-1212` model does not support `aspect_ratio` or `resolution` parameters.

## Prerequisites

- Rust (edition 2024)
- An xAI API key from [console.x.ai](https://console.x.ai)

## Setup

Create the config file:

```bash
mkdir -p ~/.config/mcp-server-grok-image
```

Create `~/.config/mcp-server-grok-image/config.toml`:

```toml
api_key = "xai-..."
```

## Build

```bash
cargo build --release
```

This produces `target/release/mcp-server-grok-image`.

For development:

```bash
cargo build              # debug build
cargo run                # run in dev mode
RUST_LOG=debug cargo run # run with debug logging
```

## MCP Configuration

Add to your Claude Desktop config (`~/.config/Claude/claude_desktop_config.json`):

```json
{
  "mcpServers": {
    "grok-image": {
      "command": "/path/to/mcp-server-grok-image"
    }
  }
}
```

## Project Structure

```
src/
  main.rs  - everything: config, API types, MCP server, tool definitions (~280 lines)
```

## License

MIT
