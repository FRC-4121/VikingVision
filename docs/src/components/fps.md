# `FpsComponent`

Tracks the framerate of its invocations.

## Inputs

Primary channel (any): the input, ignored.

## Outputs

- `min` (single, `f64`): the minimum framerate in the period.
- `max` (single, `f64`): the maximum framerate in the period.
- `avg` (single, `f64`): the average framerate in the period.
- `pretty` (single, `String`): a pretty, formatted string, formatted as `min/max/avg FPS`.

## Configuration

Appears in configuration files with `type = "fps"`.

Additional fields:

- `duration` (string): a duration string, like `1min`, defaulting to `10s`
