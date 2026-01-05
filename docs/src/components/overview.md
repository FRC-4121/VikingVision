# Components Overview

Components handle all of the actual processing in the pipelines. Each one handles a single step, and they all combine to form a pipeline that performs the computation and presents the results. These components lend themselves well to [dataflow programming](https://en.wikipedia.org/wiki/Dataflow_programming), which allows for parallelization with far more safety and clarity than traditional, imperative code.

The following components are available for use in pipelines:

## Vision

These all focus around manipulating an image or detecting features in it.

- [`apriltag`](apriltag.md) (requires the `apriltag` feature)
- [`detect-pose`](detect-pose.md) (requires the `apriltag` feature)
- [`color-space`](color-space.md)
- [`color-filter`](color-filter.md)
- [`blob`](blob.md)
- [`resize`](resize.md)
- [`percent-filter` / `erode` / `dilate` / `median-filter`](percent-filter.md)
- [`box-blur`](box-blur.md)
- [`gaussian-blur`](gaussian-blur.md)

## Aggregation

These combine repeated inputs into one, undoing some form of broadcasting.

- [`collect-vec`](collect-vec.md)
- [`select-last`](select-last.md)

## Presentation

These focus around presenting results.

- [`debug`](debug.md)
- [`draw`](draw.md)
- [`ffmpeg`](ffmpeg.md)
- [`ntable`](ntable.md) (requires the `ntable` feature)
- [`vision-debug`](vision-debug.md) (requires the `debug-gui` feature for some functionality)

## Utility

Miscellaneous components that don't have a better categorization.

- [`clone`](clone.md)
- [`fps`](fps.md)
- [`unpack`](unpack.md)
- [`wrap-mutex` / `canvas`](wrap-mutex.md)
