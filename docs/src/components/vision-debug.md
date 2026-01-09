# `VisionDebugComponent`

Shows the frame sent to it. This is primarily for debugging purposes, see the [vision debugging](../config/debug.md) docs for more general configuration.

## Inputs

Primary input (`Buffer`): the image to show.

## Outputs

None.

## Configuration

Appears in configuration files with `type = "vision-debug"`.

Additional fields:

- `mode` (`auto` | `none` | `save` | `show`): what to do with images we receive (defaults to `auto`):
  - `auto`: use the configured default, or `none`
  - `none`: ignore this image
  - `save`: save this image to a given path
- `path` (string, requires `mode = "save"`): see the [`debug.default_path`](../config/debug.md#debugdefault_path) documentation
- `show` (string, requires `mode = "show"`): see the [`debug.default_title`](../config/debug.md#debugdefault_title) documentation
