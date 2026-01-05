# Camera Overview

Cameras are the entry point for a program. When a configuration is run with the CLI, each camera gets its own thread to read from (which is crucial for performance since reading from a camera is a blocking operation). Each camera that creates a pipeline run with its frame sent to each of the outputs _independently_ (note that this is not ideal behavior and subject to change). This is prone to issues like more expensive pipelines being starved and components unexpectedly not running, so it will be fixed soon.

## In the config file

Cameras are created as tables, as their name under the `[camera]` table. Each camera has a `type` field that specifies which kind of camera it is, and an `outputs` field that should be an array of strings, to specify which components they should send their frames to.

A camera config might look like this:

```toml
[camera.front] # we call this our front camera
type = "v4l" # we want to use the V4L2 backend
outputs = ["detect-tags"] # send out output to a component called detect-tags
width = 640 # V4L2-specific options
height = 320
fourcc = "YUYV"
path = "/dev/video0"
```

## Additional camera features

In addition to the basic raw frame we can get from the backend, the cameras support some additional quality-of-life features.

### Reloading and Retrying

If reading from a frame fails, the camera tries reloading the camera and then tries again. It does so with exponential backoff, so if the camera's been genuinely lost, it doesn't waste time retrying to connect. By reloading the camera, we can recover from a loose USB connection or dropped packets instead of losing the camera altogether.

### FPS throttling

Especially with the static cameras, it's very easy to have the camera send input way faster than it can be processed. FPS throttling sleeps if the real framerate exceeds to configured one, which frees up CPU time for other, more important things.

A maximum framerate can be set with the `max_fps` field for a camera.

### Resizing

If the frame size isn't desirable, it can be resized through the camera config itself. The `resize.width` and `resize.height` keys allow a new size to be set for the camera. Resizing uses nearest-neighbor scaling for simplicity.
