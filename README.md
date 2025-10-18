# VikingVision

VikingVision is a Rust-based implementation of Viking Robotics's vision system, with a priority of ease of use and speed. It's based on our previous attempt using [Java](https://github.com/FRC-4121/4121-Vision-Java), but hopefully with enough performance to be competitive with other solutions.

## Installation

### Binary downloads

Binary artifacts will be available from the releases tab... once we have a major release.

### Building from source

Cargo makes it incredibly easy to build and our code. Installation instructions for it can be found on Rust's [getting started](https://www.rust-lang.org/learn/get-started) guide. Note that Cargo installs its binaries per-user, defaulting to `~/.cargo/bin` on Linux.

If you just want the binaries, you can run `cargo install --git https://github.com/FRC-4121/VikingVision vv-cli vv-gui vv-pg`, which will install three binaries: `vv-cli`, `vv-gui`, and `vv-pg`. These can be run from anywhere and are fully standalone.

Alternatively, you can clone the repository, which has other useful utilities in it. From there, you can run `cargo build --workspace --release` to build both the GUI and CLI, which can be found in the generated `target/release/` directory.

## The binaries

### `vv-pg`

This is the "playground", an environment for testing vision configurations. This is completely independent of the pipelines used for the other two binaries, and mostly made to show various vision processing options.

### `vv-cli`

This is a lightweight, headless runner for pipelines. It loads the configuration from a file and runs it without interaction, as would be intended for a daemon.

### `vv-gui`

Currently unimplemented! When implemented, it'll be an interactive interface for pipeline runs, allowing one to see the state of graphics buffers and debug values, along with a graphical editor for the pipeline graph (if I get to it).
