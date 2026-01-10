# `FfmpegComponent`

Saves a video by piping it into `ffmpeg`.

An example for the arguments to be passed here is `["-c:v", "libx264", "-crf", "23", "vv_%N_%Y%m%d_%H%M%S.mp4"]`, which saves a MP4 video with the date and time in its name.

## Inputs

Primary input (`Buffer`): the frames to save

## Outputs

None.

## Configuration

Appears in configuration files with `type = "ffmpeg"`.

Additional fields:

- `fps` (number): the framerate that the video should be saved with. This should match the camera framerate.
- `args` (array of strings): arguments for the output format of `ffmpeg` (everything after the `-`). `strftime` escapes, along with `%i` and `%N` for pipeline ID and pipeline name, are supported and can be replaced.
- `ffmpeg` (string, optional): an override for the `ffmpeg` command
