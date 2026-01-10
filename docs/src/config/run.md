# Run Configuration

Run configuration is optional, and goes under the `[run]` table in the config. It controls thread counts and run-related parameters.

## `run.max_running`

This is the maximum number of concurrently running pipelines. If a new frame comes in while this many frames are already being processed, the new frame will be dropped.

## `run.num_threads`

This controls the number of threads to be used in the thread pool. It can be overridden by passing `--threads N` to the CLI. If neither of these is set, `rayon` searches for the `RAYON_NUM_THREADS` environment variable, and then the number of logical CPUs.
