# mcp-server-grok-image

An MCP (Model Context Protocol) server for xAI's Grok image generation API. Built in Rust, exposes image generation and editing as MCP tools.

Communicates via stdio using JSON-RPC 2.0, like all MCP servers.

## Tools

| Tool | Description |
|------|-------------|
| `generate_image` | Generate an image from a text prompt |
| `edit_image` | Edit an existing image using natural language instructions |
| `headshot` | Corporate headshot from a source portrait (pad to 3:2 + fixed edit prompt) |
| `list_styles` | List available image styles for use with `generate_image` |

### generate_image

Generate an image from a text description.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `prompt` | string | yes | Text description of the desired image |
| `model` | string | no | Model to use (default: `grok-imagine-image-2.0`) |
| `n` | integer | no | Number of images to generate (1-10, default 1) |
| `aspect_ratio` | string | no | Aspect ratio: `1:1`, `16:9`, `9:16`, `4:3`, `3:4`, `3:2`, `2:3`, `2:1`, `1:2`, `19.5:9`, `9:19.5`, `20:9`, `9:20`, `21:9`, `5:2`, `auto` |
| `resolution` | string | no | Output resolution: `1k` (~1024px, default) or `2k` (~2048px) |
| `quality` | string | no | `low`, `medium`, or `auto` (2.0 only; omitted = `auto`. Auto currently serves `low` for generation) |
| `response_format` | string | no | Output format: `url` (default, temporary) or `b64_json` |
| `style` | string | no | Style name to apply (use `list_styles` to see options) |

When a style is set, the prompt is wrapped in the style's template. For example, with `style: "watercolor"` and `prompt: "a cat on a roof"`, the API receives `"a cat on a roof, as a watercolor painting"`. Avoid including style language in the prompt itself when using this parameter.

The response includes the resolved prompt so you can see exactly what was sent to the API.

### edit_image

Edit an existing image using natural language instructions.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `image_url` | string | no* | URL, base64 data URI, or local file path of the source image. Mutually exclusive with `images`. |
| `images` | string[] | no* | Up to 5 source images for multi-image editing. Reference them in the prompt as `<IMAGE_0>`, `<IMAGE_1>`, … |
| `prompt` | string | yes | Natural language edit instructions |
| `model` | string | no | Model to use (default: `grok-imagine-image-2.0`) |
| `n` | integer | no | Number of variations to generate (1-10, default 1) |
| `aspect_ratio` | string | no | Same set as `generate_image`, including `21:9` and `5:2` |
| `resolution` | string | no | Output resolution: `1k` (~1024px, default) or `2k` (~2048px) |
| `quality` | string | no | `low`, `medium`, or `auto` (2.0 only; omitted = `auto`. Auto currently serves `medium` for editing) |
| `response_format` | string | no | Output format: `url` (default, temporary) or `b64_json` |

\* Provide either `image_url` or `images`.

Note: The `style` parameter is intentionally not available on `edit_image` -- edit prompts are instructions (e.g. "remove the background"), not descriptions, so wrapping them in style templates would produce nonsense.

### headshot

**Expand-only** portrait fix (Gemini pipeline equivalent on Imagine). Does **not** reframe pose, cut out hair, or redesign the person.

1. Resize full source (default 550px wide) — **never crop**
2. Letterbox with **white** gutters to canvas width (default 780)
3. Call **`grok-imagine-image-2.0`** at **quality medium**: complete cut-off shoulders if needed; clean solid **white background**; keep face/hair/pose/clothing/logos

**No cutout / no rembg / no transparent alpha** — same job as the original Gemini headshot skill.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `image` | string | yes | Local path, http(s) URL, or `data:` URI |
| `clothing` | string | no | For missing-shoulder fill only |
| `notes` | string | no | Must-preserve details (glasses, exact logo text, …) |
| `pronoun` | string | no | `his` / `her` / `their` (default `their`) |
| `gravity` | string | no | Letterbox gravity (`North` default) |
| `content_width` | integer | no | Resize width before pad (default `550`) |
| `canvas_width` | integer | no | Padded width (default `780`) |
| `resolution` | string | no | `1k` or `2k` (default `2k`) |
| `output_path` | string | no | Optional final path (also under `save_dir`) |
| `n` | integer | no | Variations (1–10, default 1) |
| `quality` | string | no | `low` / `medium` / `auto` (default **`medium`**) |
| `model` | string | no | Default **`grok-imagine-image-2.0`** |

Padded intermediate: `save_dir/headshot-padded_*.jpg`.

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

| Model | Notes |
|-------|-------|
| `grok-imagine-image-2.0` (default) | Optional `quality` (`low` / `medium` / `auto`), up to 5 edit references, `21:9` and `5:2`. Auto currently serves `low` for generation and `medium` for editing. |
| `grok-imagine-image` | 1.0. Still available; no `quality` param. |
| `grok-imagine-image-quality` | Retires **2026-11-02**. After that the slug is served by `grok-imagine-image-2.0` at `quality: low` ($0.01 less per image than the quality model). |

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
  main.rs      process entry (stdio MCP)
  config.rs    TOML / env config
  styles.rs    built-in + custom styles
  grok.rs      xAI request/response types
  params.rs    MCP tool params + validation
  image_io.rs  data URIs, local files, mime, fetch
  headshot.rs  letterbox pad + expand prompt
  server.rs    MCP tools and Grok HTTP
```

## License

MIT
