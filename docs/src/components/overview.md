# Components Overview

Components handle all of the actual processing in the pipelines. Each one handles a single step, and they all combine to form a pipeline that performs the computation and presents the results. These components lend themselves well to [dataflow programming](https://en.wikipedia.org/wiki/Dataflow_programming), which allows for parallelization with far more safety and clarity than traditional, imperative code.

The following components are available for use in pipelines:

## Vision

These all focus around manipulating an image or detecting features in it.

- [`apriltag`](components/apriltag.md) (requires the `apriltag` feature)
- [`detect-pose`](components/detect-pose.md) (requires the `apriltag` feature)
- [`color-space`](components/color-space.md)
- [`color-filter`](components/color-filter.md)
- [`blob`](components/blob.md)
- [`percent-filter` / `erode` / `dilate` / `median-filter`](components/percent-filter.md)
- [`box-blur`](components/box-blur.md)
- [`gaussian-blur`](components/gaussian-blur.md)

## Aggregation

These combine repeated inputs into one, undoing some form of broadcasting.

- [`collect-vec`](components/collect-vec.md)
- [`select-last`](components/select-last.md)

## Presentation

These focus around presenting results.

- [`debug`](components/debug.md)
- [`draw`](components/draw.md)
- [`ffmpeg`](components/ffmpeg.md)
- [`ntable`](components/ntable.md) (requires the `ntable` feature)
- [`vision-debug`](components/vision-debug.md) (requires the `debug-gui` feature for some functionality)

## Utility

Miscellaneous components that don't have a better categorization.

- [`clone`](components/clone.md)
- [`fps`](components/fps.md)
- [`unpack`](components/unpack.md)
- [`wrap-mutex` / `canvas`](components/wrap-mutex.md)
