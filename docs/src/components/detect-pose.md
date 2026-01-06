# `DetectPoseComponent`

_requires the `apriltag` feature_

Converts a `Detection` into a `PoseEstimation`.

## Inputs

Primary input: a `Detection` to process.

## Outputs

- Default channel (single, `PoseEstimation`): the estimated pose, along with its error
- `pose` (single, `Pose`): the estimated pose
- `error` (single, `f64`): the estimation's error

## Configuration

Appears in configuration files with `type = "detect-pose"`.

Additional fields:

- `spec` (`"fixed"` | `"infer"`): the specification of detection parameters
- `center` (2-element float array): the coordinates of the center of the image
- `fov` (2-element float array): the FOV of the camera, in pixels
- `tag_size` (float | `"FRC_INCHES"` | `"FRC_CM"` | `"FRC_METERS"`): the size of the tag, in whatever units the measurements should be in

If `spec = "fixed"`, all fields are required. If `spec = "infer"`, the `center` and `fov` are determined by camera parameters, and only the `tag_size` is accepted.
