# VikingVision

VikingVision is a high-performance vision processing library for FRC teams, built in Rust for speed and reliability.

## Why VikingVision?

FRC vision processing needs to be fast, reliable, and run on resource-constrained hardware. VikingVision provides:

- **Performance**: Parallel pipeline processing with minimal overhead
- **Ease of use**: Configure pipelines in TOML (and soon a GUI!) without writing code
- **AprilTag support**: Built-in detection for FRC game pieces
- **NetworkTables integration**: Easy communication with robot code

Also, this is done in Rust, so it's 🔥blazingly fast🔥 or whatever.

### Why not OpenCV?

Using it from Python leads to performance bottlenecks, and unless you're working with the latest Python version, has almost no capability for parallel processing. In any language, writing and maintaining complex programs for requirements that change every year is also a significant overhead, especially for teams without many programmers. By using VikingVision, that work is gone, and only a small configuration file needs to be maintained.

OpenCV's spotty documentation and lack of safety (it caused a memory error in a _Java_ program once) were enough for me (the person writing this documentation, hi!!!) to want to avoid it in favor of lower-level system bindings and reimplementations of the needed algorithms.

### Why not Limelight?

It's proprietary and expensive.

### Why not PhotonVision?

PhotonVision has way more features, but VikingVision's smaller feature set is suitable for a lot of use cases, and it should run faster for those. The total binary size of all of the artifacts (x64 linux, stripped, release build) comes in at about 42 MB, compared to PhotonVision's 102MB JAR. Also having our own vision system makes us look cool to the judges.

## What can you do with it?

- Detect AprilTags for autonomous alignment
- Build vision pipelines that can track game pieces by color and estimate their positions
- Run multiple camera feeds simultaneously

## Example

Want to detect all of the AprilTags that a camera can see and publish it to NetworkTables? A basic config looks like this:

```toml
[ntable]
team = 4121
identity = "vv-client"

[camera.tag-cam]
type = "v4l"
path = "/dev/video0"
width = 640
height = 480
fourcc = "YUYV"
fps = 30
outputs = ["fps", "tags"]

[component.fps]
type = "fps"

[component.tags]
type = "apriltag"
family = "tag36h11"

[component.tags-unpack]
type = "unpack"
input = "tags"

[component.nt-fps]
type = "ntable"
prefix = "%N/fps"
input.pretty = "fps.pretty"
input.fps = "fps.fps"
input.min = "fps.min"
input.max = "fps.max"

[component.nt-tags]
type = "ntable"
prefix = "%N/tags"
input.found = "tags.found"
input.ids = "tags-unpack.id"
```

Then running it is as simple as `vv-cli config.toml`.
