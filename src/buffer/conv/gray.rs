pub fn rgb(from: &[u8; 1], to: &mut [u8; 3]) {
    let [v] = *from;
    to.fill(v);
}
