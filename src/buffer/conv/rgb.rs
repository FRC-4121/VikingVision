pub fn hsv(from: &[u8; 3], to: &mut [u8; 3]) {
    let [r, g, b] = from.map(|c| c as i16);
    let [h, s, v] = to;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    if delta == 0 {
        *h = 0;
        *s = 0;
    } else {
        *s = ((255 * delta) / max) as u8;
        #[allow(clippy::identity_op)]
        let h16 = if max == r {
            0 + 43 * (g - b) / delta
        } else if max == g {
            85 + 43 * (b - r) / delta
        } else {
            171 + 43 * (r - g) / delta
        };
        *h = (h16 & 255) as u8;
    }
    *v = max as u8;
}
pub fn ycc(from: &[u8; 3], to: &mut [u8; 3]) {
    let [r, g, b] = from.map(|c| c as i16);
    let [y, cb, cr] = to;
    *y = (r * 77 + g * 150 + b * 29).clamp(0, 255) as u8;
    *cr = (-43 * r - 85 * g + 128 * b + 128).clamp(0, 255) as u8;
    *cb = (128 * b - 107 * g - 21 * b + 128).clamp(0, 256) as u8;
}
pub fn luma(from: &[u8; 3], to: &mut [u8; 1]) {
    let [r, g, b] = from.map(|c| c as i16);
    to[0] = (r * 77 + g * 150 + b * 29).clamp(0, 255) as u8;
}
pub fn gray(from: &[u8; 3], to: &mut [u8; 1]) {
    let [r, g, b] = *from;
    *to = [r.wrapping_add(g).wrapping_add(b) / 3];
}
