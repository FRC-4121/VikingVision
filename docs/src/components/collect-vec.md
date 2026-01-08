# `CollectVecComponent<T>`

_this component aggregates its inputs_

Collects the results of its inputs (of a known type) into a vector.

## Inputs

- `elem` (`T`): the element to collect.
- `ref` (any): a reference point. The value sent on this channel is ignored, but if it's tied to a single-output channel of a component (like `$finish`), this will collect all of the values that came from the result of that component's execution.

## Outputs

- Default channel (single, `Vec<T>`): the values submitted to `elem`, in an unspecified order.
- `sorted` (single, `Vec<T>`): the values submitted to `elem`, in the order that they would've come in if we used single-threaded, depth-first execution.

## Configuration

Appears in configuration with `type = "collect-vec"`.

Additional fields:

- `inner`: ([Type](../config/types.md)): the inner element type.
