pub fn rgb(from: &[u8; 3], to: &mut [u8; 3]) {
    let [h, s, v] = from.map(|c| c as u16);
    if s == 0 {
        to.fill(v as _);
        return;
    }
    let region = h / 43;
    let c = (v * s) >> 8;
    let x = (c * (43 - (h % 85).abs_diff(43))) / 43;
    let m = v - c;
    let c = (c + m).clamp(0, 255) as u8;
    let x = (x + m).clamp(0, 255) as u8;
    let m = m as u8;
    *to = match region {
        0 => [c, x, m],
        1 => [x, c, m],
        2 => [m, c, x],
        3 => [m, x, c],
        4 => [x, m, c],
        _ => [c, m, x],
    }
}
