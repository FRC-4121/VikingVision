# `BoxBlurComponent`

Applies a box blur to an image.

## Inputs

Primary input (`Buffer`): the image to blur.

## Outputs

- Primary channel (single, `Buffer`): the image, blurred.

## Configuration

Appears in configuration files with `type = "box-blur"`.

Additional fields:

- `width` (odd, positive integer): the width of the blur window
- `height` (odd, positive integer): the index of the blur window
