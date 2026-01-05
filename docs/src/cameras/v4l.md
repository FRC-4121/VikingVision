# V4L2 Cameras

_Requires the `v4l` feature to be enabled_

V4L2 is the primary way that we read from physical cameras. It allows for cameras to be read by a path or index, where an index `N` corresponds to `/dev/videoN`. Note that due to how V4L2 works, typically, only even-numbered indices are actually readable as cameras, and odd-numbered ones should not be used.

## Configuration

A V4L camera must have its source, frame shape, and FourCC set. All other configuration is optional.

### Source

The source specifies where to find the capture device. This can be a path, passed as the `path` field, or an ordinal index, under the `index` field. As a placeholder, `unknown = {}` can be used to make the config file parse, but it will fail to load a camera.

### Width and Height

The camera's dimensions are specified in the `width` and `height` fields. If these don't correspond to an actual resolution that the camera is capable of, there may be unexpected results.

### FourCC

FourCC is the format in which data is sent over the camera. It should be a four-character string, typically uppercase letters and numbers. For most cameras, either `YUVY` or `MJPG` should be used. The following codes are recognized by VikingVision:

- `YUYV`
- `RGB8`
- `RGBA`
- `MJPG`

### Overriding Pixel Formats

The FourCC codes we recognize cover the most common uses, but to support additional formats, VikingVision supports overriding the pixel format with the `pixel_format` field. This doesn't need to be set if the code is already correctly recognized.

In addition, the `decode_jpeg` value can be used to specify that the input is JPEG data, like with the `MJPG` FourCC. If this is set, the output frames will always have the RGB format.

### Exposure and Intervals

The camera's exposure can be set with the `exposure` field. The values for this don't seem to have any predefined meaning, but setting it to around 300 was good for Logitech cameras.

The camera's framerate can also be set. This can either be set as an integer framerate with the `fps` field, or an interval fraction, which can be set with the `interval.top` and `interval.bottom` fields.
