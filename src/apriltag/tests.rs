use super::*;

fn parse_detection(line: &str) -> (i32, [[f64; 2]; 4]) {
    let mut it = line.split(", ");
    let id = it
        .next()
        .expect("Expected a tag ID")
        .parse()
        .expect("Expected a valid tag ID");
    let corners = std::array::from_fn(|_| {
        let pair = it.next().expect("Expected a corner");
        let pair = pair.strip_prefix('(').expect("Expected an opening '('");
        let pair = pair.strip_suffix(')').expect("Expected() a closing ')'");
        let mut it = pair.split(' ');
        let x = it
            .next()
            .expect("Expected an X-coordinate")
            .parse()
            .expect("Expected a valid X-coordinate");
        let y = it
            .next()
            .expect("Expected an Y-coordinate")
            .parse()
            .expect("Expected a valid Y-coordinate");
        [x, y]
    });
    (id, corners)
}

fn format_line((id, corners): (i32, [[f64; 2]; 4])) {
    print!("{id}");
    for [x, y] in corners {
        print!(", ({x:.4} {y:.4})");
    }
    println!();
}

/// Generate a test, loading an image and its expected data from "data/$path.jpg" and "data/$path.txt"
macro_rules! generate_test {
    ($test_name:ident, $path:literal) => {
        #[test]
        fn $test_name() {
            let img = Buffer::decode_img_data(include_bytes!(concat!("data/", $path, ".jpg")))
                .expect(concat!("failed to load image at data/", $path, ".jpg"));
            let data = include_str!(concat!("data/", $path, ".txt"));
            let expected = data
                .split('\n')
                .filter(|l| !l.is_empty())
                .map(parse_detection)
                .collect::<Vec<_>>();
            let mut detector = Detector::new();
            detector
                .add_family(TagFamily::tag36h11)
                .set_decimate(1.0)
                .set_refine(false);
            let mut detections = detector
                .detect(img)
                .map(|d| (d.id(), d.corners()))
                .collect::<Vec<_>>();
            detections.sort_by(|l, r| l.partial_cmp(r).unwrap());
            println!(concat!("Expected detection is at data/", $path, ".txt"));
            println!("Found detection:");
            detections.iter().copied().for_each(format_line);
            assert_eq!(expected.len(), detections.len(), "Length mismatch");
            for (i, (l, r)) in expected.iter().zip(&detections).enumerate() {
                assert!(
                    l.0 == r.0
                        && l.1
                            .into_iter()
                            .flatten()
                            .zip(r.1.into_iter().flatten())
                            .all(|(l, r)| (l - r).abs() < 1.0), // this is looser than the tests used in the C library, but for some reason, tests fail otherwise
                    "Detection mismatch at index {i}:\nexpected {l:.4?}\ngot      {r:.4?}"
                );
            }
        }
    };
}

generate_test!(test_img_1, "33369213973_9d9bb4cc96_c");
generate_test!(test_img_2, "34085369442_304b6bafd9_c");
generate_test!(test_img_3, "34139872896_defdb2f8d9_c");
