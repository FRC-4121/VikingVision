# `UnpackComponent`

Unpacks fields from the input.

## Inputs

Primary input (any): the data to get fields of.

## Outputs

Any output (single, any): the value of a field in the input with the channel's name.

## Configuration

Appears in configuration files with `type = "unpack"`.

Additional fields:

- `allow_missing`: if this is false (the default), emit a warning if the requested field isn't present
