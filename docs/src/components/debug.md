# `DebugComponent`

Emits an info-level span with a debug representation of the data received.

## Inputs

Primary input (any): the data to debug.

## Outputs

None.

## Configuration

Appears in configuration files with `type = "debug"`.

Additional fields:

- `noisy` (optional, boolean): if this is false, duplicate events from this component will be suppressed. Defaults to true.
