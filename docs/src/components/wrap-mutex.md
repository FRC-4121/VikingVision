# `WrapMutexComponent<T>`

Wraps an input in a `Mutex`, making it mutable.

This is necessary for drawing on a buffer with the [`draw`](draw.md) component, and its the contents can be later accessed by [unpacking](unpack.md) the `inner` field.

## Inputs

Primary channel (`T`): the value to wrap.

## Outputs

Default channel (single, `Mutex<T>`): the value, wrapped in a mutex.

## Configuration

Appears in configuration files with `type = "wrap-mutex"`.

Additional fields:

- `inner` ([Type](../config/types.md#types)): the type of the value to wrap. `apriltag` and `blob` are not recognized.

### Additional Consrtuctors

Components with a `type` of `canvas` also construct this component, with an inner type of `Buffer`.
