# mcp-server-grok-image

An MCP (Model Context Protocol) server for xAI's Grok image generation API. Built in Rust, exposes image generation and editing as MCP tools.

Communicates via stdio using JSON-RPC 2.0, like all MCP servers.

## Tools

| Tool | Description |
|------|-------------|
| `generate_image` | Generate an image from a text prompt |
| `edit_image` | Edit an existing image using natural language instructions |
| `list_styles` | List available image styles for use with `generate_image` |

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
| `style` | string | no | Style name to apply (use `list_styles` to see options) |

When a style is set, the prompt is wrapped in the style's template. For example, with `style: "watercolor"` and `prompt: "a cat on a roof"`, the API receives `"a cat on a roof, as a watercolor painting"`. Avoid including style language in the prompt itself when using this parameter.

The response includes the resolved prompt so you can see exactly what was sent to the API.

### edit_image

Edit an existing image using natural language instructions.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `image_url` | string | yes | URL, base64 data URI, or local file path of the source image |
| `prompt` | string | yes | Natural language edit instructions |
| `model` | string | no | Model to use (default: `grok-imagine-image-pro`) |
| `n` | integer | no | Number of variations to generate (1-10, default 1) |
| `resolution` | string | no | Output resolution: `1k` (~1024px, default) or `2k` (~2048px) |
| `response_format` | string | no | Output format: `url` (default, temporary) or `b64_json` |

Note: The `style` parameter is intentionally not available on `edit_image` -- edit prompts are instructions (e.g. "remove the background"), not descriptions, so wrapping them in style templates would produce nonsense.

### list_styles

Returns all available image styles with their name, description, and prompt template. No parameters.

### Built-in Styles

| Style | Description |
|-------|-------------|
| `watercolor` | Watercolor painting style |
| `oil-painting` | Oil painting with visible brushstrokes |
| `pencil-sketch` | Detailed pencil sketch |
| `pixel-art` | Retro pixel art |
| `anime` | Anime style illustration |
| `pop-art` | Bold pop art style |
| `art-nouveau` | Art nouveau with flowing organic lines |
| `cinematic` | Cinematic photography with dramatic lighting |
| `portrait` | Professional portrait photography |
| `macro` | Extreme macro photography |
| `aerial` | Aerial drone photography |
| `studio` | Studio photography on clean background |
| `noir` | Dark film noir style |
| `vintage` | Faded vintage photograph |

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

### Custom Styles

Add custom styles to your config file. Custom styles with the same name as a built-in will override it.

```toml
api_key = "xai-..."

[[styles]]
name = "my-style"
description = "My custom look"
template = "{prompt}, in my custom style"

[[styles]]
name = "watercolor"
description = "My watercolor variant"
template = "{prompt}, as a loose expressive watercolor with ink outlines"
```

Templates must contain the `{prompt}` placeholder. Any custom style missing it will be skipped with a warning at startup.

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
  main.rs  - everything: config, styles, API types, MCP server, tool definitions
```

## License

MIT
