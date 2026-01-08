# `ColorFilterComponent`

Filters an image based on pixel colors.

## Inputs

Primary input (`Buffer`): the image to filter.

## Outputs

- Primary channel (single, `Buffer`): a new image, in the `LUMA` color space, with white where pixels were in range and black where they weren't.

## Configuration

Appears in configuration files with `type = "filter"`.

The color space to filter in is determined with the `space` field. Recognized values are `luma`, `rgb`, `hsv`, `yuyv`, and `ycc`. Based on this, `min-` and `max-` fields should be present for every channel. For example, with `rgb`, the fields `min-r`, `max-r`, `min-g`, `max-g`, `min-b`, and `max-b` should be present. Note that the YUYV filter uses channels `y`, `u`, and `v`, while the YCbCr filter uses channels `y`, `b`, and `r`.
