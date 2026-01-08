# `ResizeComponent`

Resize an image to a given size.

## Inputs

Primary input (`Buffer`): the image to resize.

## Outputs

- Primary channel (single, `Buffer`): the image, resized.

## Configuration

Appears in configuration files with `type = "resize"`.

Additional fields:

- `width` (nonnegative integer): the width of the resulting image
- `height` (nonnegative integer): the height of the resulting image
