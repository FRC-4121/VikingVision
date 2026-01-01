# Installation

Right now, the only option for installation is to build from source. This requires Rust to be installed (which is typically done using [rustup](https://rustup.rs/)), along with a C compiler (GCC or Clang on Linux, one of which should already be installed by default) to build with apriltag support.

## Quick installation

With everything needed installed, you can run `cargo install --git https://github.com/FRC-4121/VikingVision <bins>`, where `<bins>` is the binary targets to install, passed as separate arguments. For example, to install just the CLI and playground, you'd run `cargo install --git https://github.com/FRC-4121/VikingVision vv-cli vv-pg`. The available targets are:

- `vv-cli` - The command-line interface for the library, which handles loading and running pipelines. It doesn't have any fancy features, but can run without a desktop environment and only using the minimal resources.
- `vv-gui` - Currently a stub that just prints, "Hello, World!". Development is ongoing, and hopefully it'll be usable by March 2026.
- `vv-pg` - A "playground" environment, made for testing some basic image processing tools. It's not super fancy, but it aims to at least be an alternative to messing around with OpenCV in Python that uses _our_ libraries instead.

## Building from source

If you want more control over the build, or want to more easily maintain an up-to-date nightly version, you can clone the repository with `git clone --recursive https://github.com/FRC-4121/VikingVision`. The source for the apriltag code is in a Git submodule, so you have to do a recursive clone! From there, you can use Cargo to build and run. **It's strongly recommended that you use release builds for production, they run about ten times faster!**

## Features

Various parts of this project can be conditionally enabled or disabled. The default features can be disabled by passing the `--no-default-features` flag to the Cargo commands, and then re-enabled using `--features` and then a comma-separated list of features. For example, to build on Windows without V4L, you could append `--no-default-features --features apriltag,ntable` to your commands.

- `apriltag` (enabled by default) - Allow detecting AprilTags. This requires a C compiler. See [`apriltag-sys`'s README](https://github.com/jerry73204/apriltag-rust/blob/master/apriltag-sys/README.md) for more information on how to build.
- `v4l` (enabled by default) - Allow video capture through V4L2 APIs. This is the only kind of camera that's supported, and only works on Linux (V4L stands for "video for _Linux_", after all). To build on other systems, this feature must be disabled.
- `ntable` (enabled by default) - Enable a NT 4.1 client. This is implemented in Rust, but pulls in a lot of dependencies through its use of `async` that it makes sense to have it be conditionally enabled.
- `debug-gui` - Enable window creation for debugging images. This pulls in `winit`, which is pretty large dependency for window creation.
- `debug-tools` - Right now, only deadlock detection. This **severely** impacts performance, and is only meant to be enabled for debugging deadlocks in the code.
