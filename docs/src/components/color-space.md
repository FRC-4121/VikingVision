# `ColorSpaceComponent`

Converts an image to a given color space.

## Inputs

Primary input (`Buffer`): the image to transform.

## Outputs

- Primary channel (single, `Buffer`): the image, in the new color space.

## Configuration

Appears in configuration files with `type = "color-space"`.

Additional fields:

- `format` ([`PixelFormat`](../config/types.html#pixelformat)): the format to convert into
