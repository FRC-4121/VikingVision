# `GaussianBlurComponent`

Applies a Gaussian blur to an image.

## Inputs

Primary input (`Buffer`): the image to blur.

## Outputs

- Primary channel (single, `Buffer`): the image, blurred.

## Configuration

Appears in configuration files with `type = "gaussian-blur"`.

Additional fields:

- `sigma` (positive float): the standard deviation of the blur
- `width` (odd, positive integer): the width of the blur window
- `height` (odd, positive integer): the index of the blur window

`width` and `height` can typically be roughly `sigma * 3`.
