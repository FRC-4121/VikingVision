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
    let mut ycc = [0; 3];
    let mut hsv = [0; 3];
    let mut gray = [0];
    let mut luma = [0];
    rgb::ycc(&rgb, &mut ycc);
    rgb::hsv(&rgb, &mut hsv);
    rgb::gray(&rgb, &mut gray);
    rgb::luma(&rgb, &mut luma);
    assert_close(ycc, [46, 130, 122], "YCbCr mismatch");
    assert_close(hsv, [128, 133, 59], "HSV mismatch");
    assert_close(gray, [48], "gray mismatch");
    assert_close(luma, [46], "luma mismatch");
    dbg!(ycc, hsv, gray, luma);
}

#[test]
fn orange_ycc2rgb() {
    let ycc = [149, 79, 146];
    let mut rgb = [0; 3];
    let mut luma = [0];
    ycc::rgb(&ycc, &mut rgb);
    ycc::luma(&ycc, &mut luma);
    assert_close(rgb, [175, 153, 63], "RGB mismatch");
    assert_close(luma, [149], "luma mismatch")
}
