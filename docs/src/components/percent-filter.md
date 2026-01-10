# `PercentileFilterComponent`

## Inputs

Primary input (`Buffer`): the image to transform.

## Outputs

- Primary channel (single, `Buffer`): the resulting image.

## Configuration

Appears in configuration files with `type = "percent-filter"`.

Additional fields:

- `width` (odd, positive integer): the width of the filter window
- `height` (odd, positive integer): the height of the filter window
- `index` (nonnegative integer less than `width * height`): the index of the pixel within the window. `0` is a erosion, `width * height - 1` is a dilation, and `width * height / 2` is a median filter.

### Additional Constructors

Components with a `type` of `erode`, `dilate`, and `median-filter` perform erosions, dilations, and median filters, respectively. For these, the `index` field is not accepted.
