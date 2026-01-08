# `BlobsComponent`

Detects the blobs in an image. A blob is an 8-connected component of non-black pixels (0 on _every_ channel, including in color spaces with multiple representations of black) in an image. Only the bounding rectangles and number of pixels in the blob are detected.

## Inputs

Primary (`Buffer`): the image to find blobs in. This should usually be a black/white image, like the results of a `filter`.

## Outputs

- Default channel (multiple, `Blob`): the blobs found
- `vec` (single, `Vec<Blob>`): the blobs found, collected into a vector

## Configuration

Appears in configuration files with `type = "blobs"`.

Additional fields:

- `min-w`: the minimum width of detected blobs
- `max-w`: the maximum width of detected blobs
- `min-h`: the minimum height of detected blobs
- `max-h`: the maximum height of detected blobs
- `min-px`: the minimum pixel count of detected blobs
- `max-px`: the maximum pixel count of detected blobs
- `min-fill`: the minimum fill ratio (pixels / (width × height)) of detected blobs
- `max-fill`: the maximum fill ratio of detected blobs
- `min-aspect`: the minimum aspect ratio (height / width) of detected blobs
- `max-aspect`: the maximum aspect ratio of detected blobs

All fields are optional, and if unset, default to the most permissive values.
