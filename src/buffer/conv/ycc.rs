pub fn rgb(from: &[u8; 3], to: &mut [u8; 3]) {
    let [y, cb, cr] = from.map(|c| c as i32);
    let [r, g, b] = to;
    *r = (y + 359 * cr / 256).clamp(0, 255) as u8;
    *g = (y - (88 * cb + 183 * cr) / 256).clamp(0, 255) as u8;
    *b = (y + 454 * cb / 256).clamp(0, 256) as u8;
}
#[inline(always)]
pub fn luma(from: &[u8; 3], to: &mut [u8; 1]) {
    let [y, _, _] = *from;
    to[0] = y;
}
pub fn yuyv(from: &[u8; 6], to: &mut [u8; 4]) {
    let [y1, b1, r1, y2, b2, r2] = *from;
    let [ya, u, yb, v] = to;
    *ya = y1;
    *yb = y2;
    *u = b1.wrapping_add(b2) >> 1;
    *v = r1.wrapping_add(r2) >> 1;
}
