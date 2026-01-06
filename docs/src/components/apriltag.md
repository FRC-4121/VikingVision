# `AprilTagComponent`

_requires the `apriltag` feature_

Detects AprilTags on its input channel.

## Inputs

Primary input: a frame to process.

## Outputs

- Default channel (broadcast, `Detection`): the tags detected
- `vec` (single, `Vec<Detection>`): the tags detected, collected into a vector
- `found` (single, `usize`): the number of tags detected

## Configuration

Appears in configuration files with `type = "apriltag"`.

Additional fields:

- `family` (string): the tag family to use, conflicts with `families`
- `families` (string array): an array of tag families to use, conflicts with `family`
- `max_threads` (integer, optional): the maximum number of threads to use for detection
- `sigma` (float, optional): the `quad_sigma` parameter for the detector
- `decimate` (float, optional): the `quad_decimate` parameter for the detector

For FRC, the `tag36h11` family should be used.
