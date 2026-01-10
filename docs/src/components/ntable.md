# `NtPrimitiveComponent`

_requires the `ntable` feature_

_this component aggregates its inputs_

Publishes data to NetworkTables. This requires NetworkTables to be configured in the file. Topic names recognize the `%N` and `%i` escapes, which are replaced with the camera name and a unique pipeline ID, respectively.

## Inputs

Any named input: data to send over NetworkTables. By default, the topic is the channel name.

## Outputs

None.

## Configuration

Appears in configuration with `type = "ntable"`.

Additional fields:

- `prefix` (optional, string): a prefix for all topics to be published. A `/` will be automatically inserted between it and the topic name.
- `remap` (optional, table of strings): a map from input channels to topics. This may be more convenient than writing quoted names for input channels.
