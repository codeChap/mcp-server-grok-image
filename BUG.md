# Bug: aspect_ratio parameter not working - FIXED

## Symptom
All generated images came back as 720x960 (portrait 3:4) regardless of the `aspect_ratio` parameter value.

## Root Cause
Two issues:
1. `aspect_ratio` was being appended to the prompt string instead of sent as a proper API field
2. The model was hardcoded to `grok-2-image` which does NOT support `aspect_ratio` (docs say "quality, size or style are not supported"). The `grok-imagine-image` model supports it.

## Fix Applied
1. Added `aspect_ratio` as a proper field on `GrokImageRequest` struct
2. When `aspect_ratio` is specified, the server now uses `grok-imagine-image` model; otherwise falls back to `grok-2-image`

## Status
Built and ready to test. Restart Claude Code to pick up the new binary.
