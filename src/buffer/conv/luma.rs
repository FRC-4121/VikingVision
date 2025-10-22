pub fn ycc(from: &[u8; 1], to: &mut [u8; 3]) {
    let y = from[0];
    *to = [y, 128, 128];
}
pub fn rgb(from: &[u8; 1], to: &mut [u8; 3]) {
    let [y] = *from;
    to.fill(y);
}
pub fn yuyv(from: &[u8; 2], to: &mut [u8; 4]) {
    let [y1, y2] = *from;
    *to = [y1, 128, y2, 128];
}
