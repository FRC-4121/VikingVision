# Running Pipelines

Pipelines are the most powerful feature of VikingVision, and they enable easily composable and parallel processing through something similar to the actor model. To run a pipeline, create a pipeline config file (more information about that can be found in the [configuration section](placeholder.md)) and run it with `vv-cli path/to/pipeline/config.toml`.

## Filtering cameras

Whether for testing, different setups, or even using the same configuration across multiple processes, it can be useful to define multiple cameras without intending to use them. They can all be defined in one file, and the `--filter` flag can match all cameras matching a given regular expression.

## Logging

VikingVision uses [`tracing`](https://docs.rs/tracing/latest/tracing/) to emit structured logs. Events happen within _spans_, which give additional context as to the state of the program as an event was happening. All of this information is provided in the log files, which can be opened as plain text.

### Logging to a file

By default, logs are sent to the standard error stream. In addition to this, they can be saved to an output file, passed as a second argument to the `vv-cli` command. This argument supports the percent-escape sequences that `strftime` uses, so you can pass `logs/%Y%m%d_%H%M%S.log` as the second parameter to have a log file created with the current time and date.

### Filtering logs

Logs can be filtered with the `VV_LOG` environment variable. The variable is parsed as a comma-separated sequence of directives, with a directive either being of `pattern=level` to match target locations against a regular expression, or just a level to set a default. When unset, the logs default to only allowing info-level and above logs through. When reporting bugs, please upload a log with debug level so we can see all of the information!
