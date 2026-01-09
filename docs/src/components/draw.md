# `DrawComponent<T>`

Draws data on a canvas.

## Inputs

- `canvas` (`Mutex<Buffer>`): a mutable canvas to draw on
- `elem` (`T`): the element to draw on the canvas

## Ouptputs

None.

## Configuration

Appears in configuration files with `type = "draw"`.

Additional fields:

- `draw` ([Type](../config/types.md#types)): the types of elements to draw. Only `blob`, `apriltag`, `line`, and their bracketed variants are recognized.
- `space` (`luma` | `rgb` | `hsv` | `yuyv` | `ycc`): the color space to filter in.
- Channels of the color. These depend on the `space` parameter. For `yuyv`, the channels are `y`, `u`, and `v`, for example.
