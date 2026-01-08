# `SelectLastComponent`

_this component aggregates its inputs_

Selects the last value submitted.

In addition to the typical usage, selecting the last result of a component's execution, this can be combined with mutable data to continue after all operations have finished. In that case, the `elem` channel should be connected to the earlier, mutable output, and the `ref` channel should be connected to the `$finish` channels of later components.

## Inputs

- `elem` (any): the elements to select from.
- `ref` (any): a reference point. The value sent on this channel is ignored, but if it's tied to a single-output channel of a component (like `$finish`), this will collect all of the values that came from the result of that component's execution.

## Outputs

- Default channel (single, any): the last value submitted to `elem`.

## Configuration

Appears in configuration with `type = "select-last"`.
