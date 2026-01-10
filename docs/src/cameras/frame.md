# Frame Cameras

For testing purposes, rather than dealing with physical cameras, it can be more useful to show a single, static frame. These can easily run at hundreds or even thousands of frames per second because no actual work is done reading the frame.

## Configuration

Frame cameras can be loaded either from a file or generated from a single, static color.

### Loading from a Path

A frame camera can load an image from a path by using the `path` field. This is incompatible with the `color` field.

### Single-color Images

A single-color camera can be specified with the `color` field. This can either be done explicitly by passing `color.format`, a string containing the desired pixel format, and `color.bytes`, an array of integers to use as the bytes, or by parsing a string under the `color` field (not currently supported, so loading a camera with a color specified in this way will fail).

The shape of the frame must be set with `width` and `height` fields.
