use super::*;

fn assert_close<const N: usize>(left: [u8; N], right: [u8; N], msg: &'static str) {
    for (l, r) in left.into_iter().zip(right) {
        assert!(
            l.abs_diff(r) <= 10,
            "colors don't match: {left:?} vs {right:?}: {msg}"
        );
    }
}

#[test]
fn midteal_rgb2all() {
    let rgb = [28, 58, 58];
    let ycc = rgb2ycc(rgb);
    let hsv = rgb2hsv(rgb);
    let luma = rgb2luma(rgb);
    assert_close(ycc, [46, 130, 122], "YCbCr mismatch");
    assert_close(hsv, [128, 133, 59], "HSV mismatch");
    assert_close([luma], [46], "luma mismatch");
    assert_close(rgb, ycc2rgb(ycc), "YCbCr roundtrip mismatch");
    assert_close(rgb, hsv2rgb(hsv), "HSV roundtrip mismatch");
}

#[test]
fn orange_ycc2rgb() {
    let ycc = [149, 79, 146];
    let rgb = ycc2rgb(ycc);
    assert_close(rgb, [175, 153, 63], "RGB mismatch");
}
