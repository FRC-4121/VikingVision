# Additional Configuration Types

In addition to the standard data types, some configuration parameters take only certain allowed strings.

## `PixelFormat`

A pixel format is a string, with the following known recognized values:

- `?n` where `n` is a number from 1 to 200 (inclusive): an anonymous format, with `n` channels. For example, `?3` is a format with three channels.
- `luma`, `Luma`, `LUMA`: a single, luma channel.
- `rgb`, `RGB`: three channels: red, green, and blue.
- `hsv`, `HSV`: three channels: hue, saturation, and value. Note that all three are in the full 0-255 range.
- `ycc`, `YCC`, `ycbcr`, `YCbCr`: three channels: luma, blue chrominance, red chrominance. All channels are in the 0-255 range.
- `rgba`, `RGBA`: four channels: red, green, blue, alpha. Because VikingVision doesn't typically care about the alpha, the alpha is unmultiplied.
- `yuyv`, `YUYV`: four channels every two pixels, YUYV 4:2:2.

## Types

A type of a generic argument can be specified as a generic string. The following values are recognized:

- `i8`, `i16`, `i32`, `i64`, `isize`, `u8`, `u16`, `u32`, `u64`, `usize`, `f32`, f64`: all the same as their Rust equivalent
- `buffer`: a Rust `Buffer`
- `string`: a Rust `String`
- `blob`: a Rust `Blob`
- `apriltag`: a Rust `Detection` (requires the `apriltag` feature)
- any of the previous, wrapped in brackets, like `[usize]`: a `Vec` of the contained type
