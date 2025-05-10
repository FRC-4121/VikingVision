#[inline(always)]
pub fn ilumaa(buf: &mut [u8; 4]) {
    buf[1] = 255;
    buf[3] = 255;
}
#[inline(always)]
pub fn ycc(from: &[u8; 4], to: &mut [u8; 6]) {
    let [y1, u, y2, v] = *from;
    *to = [y1, u, v, y2, u, v];
}
#[inline(always)]
pub fn luma(from: &[u8; 4], to: &mut [u8; 2]) {
    to[0] = from[0];
    to[1] = from[2];
}
#[inline(always)]
pub fn lumaa(from: &[u8; 4], to: &mut [u8; 4]) {
    to[0] = from[0];
    to[1] = 255;
    to[2] = from[2];
    to[3] = 255;
}
