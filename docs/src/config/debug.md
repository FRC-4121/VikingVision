# Vision Debugging

It's fairly common to just want to see the result of some vision processing, but normally, actually seeing the results of it live would be a lot of work. You'd need to set up an event loop, create windows, handle marshalling it to your main thread because everything needs to run on the main thread, and that's a lot of work just to show a simple window with an intermediate result, isn't it?

To solve this problem, there's a vision debugging tool, which has thread-safe, cheaply cloneable senders, and a receiver that sits on the main thread and blocks it for the rest of the program. From the dataflow side, it's even simpler: you just dump the frames as input to a component with the `vision-debug` type. Depending on configuration, this can display the windows or save them to a file.

## Configuration file

To configure this through the file, the `[debug]` table can be used. It has the following keys:

### `debug.mode`

Debug modes can be overridden through components, but they're optional. A global default can be set either here or through the environment variables, and if none is present, debugging is ignored.

### `debug.default_path`

The default path to save videos to. This supports all of the `strftime` escapes, along with `%i` for a unique ID (32 hex characters) and `%N` for a pretty, human-readable name. The video will be saved as an MP4 video.

### `debug.default_title`

The default window title for use when showing windows. This supports `%i` and `%N` like the `default_path` does.

## Environment variables

The configuration file takes precedent over the environment variables, but the variables can be more convenient.

### `VV_DEBUG_MODE`

Equivalent to the `debug.mode` configuration value, but also accepts uppercase values.

### `VV_DEBUG_SAVE_PATH`

Equivalent to the `debug.default_path` configuration value.

### `VV_DEBUG_WINDOW_TITLE`

Equivalent to the `debug.default_title` configuration value.
