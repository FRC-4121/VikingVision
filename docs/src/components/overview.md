# Components Overview

Components handle all of the actual processing in the pipelines. Each one handles a single step, and they all combine to form a pipeline that performs the computation and presents the results. These components lend themselves well to [dataflow programming](https://en.wikipedia.org/wiki/Dataflow_programming), which allows for parallelization with far more safety and clarity than traditional, imperative code.

## Pipeline Rules

Not every representable graph is a valid pipeline. In order for a graph to successfully be compiled into a runner, all components in the graph need to have their inputs satisfied, and their inputs must be unambiguously broadcast. If these requirements aren't met, the pipeline will fail to start.

### Inputs and Outputs

Components communicate through channels, somewhat similarly to how functions have parameters and returns. A component can define what inputs it requires and which channels it's capable of outputting on. It's an error to try to connect a component's input to an output that another component doesn't output on.

A component can either take a single, primary input, or multiple (including zero or one) named inputs. If a component takes named inputs, it can also take additional inputs, which is also dependent on the component type. The inputs each component takes, along with their uses, is documented on each of the components' pages. A component is guaranteed to only be run if all of its inputs are available, including optional ones (the distinction between required and optional is only present in the graph, not the compiled runner).

### Broadcasting

Part of the added flexibility of channels is that multiple outputs can be sent on a single channel. Any components that depend on this channel will be run multiple times, once with every value sent. For components with multiple inputs, inputs that haven't branched will be copied across multiple runs. This functionality is called broadcasting, and it allows for components to operate on individual components in a collection. Broadcasting must be unambiguous; if two components can send multiple outputs, the pipeline will be rejected.

### Aggregation

Broadcasting is a powerful feature, and its dual is aggregation. Aggregating components get access to all of the inputs (either relative to their least split input or through the whole pipeline run). Because they take all of the inputs and only run once for the group, aggregating components are considered to at their least split input when checking the pipeline graph for multi-output components.

## Available Components

The following components are available for use in pipelines:

### Vision

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

### Aggregation

These combine repeated inputs into one, undoing some form of broadcasting.

- [`collect-vec`](collect-vec.md)
- [`select-last`](select-last.md)

### Presentation

These focus around presenting results.

- [`debug`](debug.md)
- [`draw`](draw.md)
- [`ffmpeg`](ffmpeg.md)
- [`ntable`](ntable.md) (requires the `ntable` feature)
- [`vision-debug`](vision-debug.md) (requires the `debug-gui` feature for some functionality)

### Utility

Miscellaneous components that don't have a better categorization.

- [`clone`](clone.md)
- [`fps`](fps.md)
- [`unpack`](unpack.md)
- [`wrap-mutex` / `canvas`](wrap-mutex.md)
