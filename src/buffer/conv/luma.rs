pub fn ycc(from: &[u8; 1], to: &mut [u8; 3]) {
    let y = from[0];
    *to = [y, 128, 128];
}
pub fn rgb(from: &[u8; 1], to: &mut [u8; 3]) {
    let y = from[0] as i32;
    let cr = 128;
    let cb = 128;
    let [r, g, b] = to;
    *r = (y + 359 * cr / 256).clamp(0, 255) as u8;
    *g = (y - (88 * cb + 183 * cr) / 256).clamp(0, 255) as u8;
    *b = (y + 454 * cb / 256).clamp(0, 256) as u8;
}
pub fn yuyv(from: &[u8; 2], to: &mut [u8; 4]) {
    let [y1, y2] = *from;
    *to = [y1, 128, y2, 128];
}
