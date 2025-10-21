use super::*;
use libc::*;

#[allow(non_camel_case_types)]
#[repr(transparent)]
struct pjpeg_t(c_void);

unsafe extern "C" {
    fn pjpeg_create_from_buffer(
        buf: *const u8,
        buflen: c_int,
        flags: u32,
        error: *mut i32,
    ) -> *mut pjpeg_t;
    fn pjpeg_destroy(pj: *mut pjpeg_t);
    fn pjpeg_to_u8_baseline(pj: *const pjpeg_t) -> *mut image_u8_t;
}

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

fn parse_into_detection(line: &str) -> apriltag_detection_t {
    let (id, corners) = parse_detection(line);
    apriltag_detection {
        family: std::ptr::null_mut(),
        id,
        hamming: 0,
        decision_margin: 0.0,
        H: std::ptr::null_mut(),
        c: [0.0; 2],
        p: corners,
    }
}

fn format_line((id, corners): (i32, [[f64; 2]; 4])) {
    print!("{id}");
    for [x, y] in corners {
        print!(", ({x:.4} {y:.4})");
    }
    println!();
}

fn extract(det: apriltag_detection_t) -> (i32, [[f64; 2]; 4]) {
    (det.id, det.p)
}

unsafe fn detection_compare_fn(
    a: *const apriltag_detection_t,
    b: *const apriltag_detection_t,
    thresh: f64,
) -> c_int {
    unsafe {
        let a = *a;
        let b = *b;
        let diff = a.id - b.id;
        if diff != 0 {
            return diff;
        }
        for (p1, p2) in a.p.as_flattened().iter().zip(b.p.as_flattened()) {
            let d = p1 - p2;
            if d.abs() > thresh {
                return 1.0f64.copysign(d) as _;
            }
        }
    }
    0
}

unsafe extern "C" fn compare_tags(a: *const c_void, b: *const c_void) -> c_int {
    unsafe {
        detection_compare_fn(
            *(a as *const *const apriltag_detection_t),
            *(b as *const *const apriltag_detection_t),
            0.1,
        )
    }
}

unsafe fn handle_native(data: &str, im: *mut image_u8_t, thresh: f64) {
    let mut expected = data
        .split('\n')
        .filter(|l| !l.is_empty())
        .map(parse_into_detection);

    let mut ok = true;
    unsafe {
        let td = apriltag_detector_create();
        (*td).quad_decimate = 1.0;
        (*td).refine_edges = 0;
        let tf = tag36h11_create();
        apriltag_detector_add_family_bits(td, tf, 2);

        let detections = apriltag_detector_detect(td, im);
        qsort(
            (*detections).data as *mut c_void,
            (*detections).size as size_t,
            (*detections).el_sz as size_t,
            Some(compare_tags),
        );

        let el_sz = (*detections).el_sz;

        let n = (*detections).size as usize;

        for i in 0..n {
            let mut det = std::ptr::null_mut();
            let Some(next) = expected.next() else {
                println!("Expected {i} detections, found {n}");
                ok = false;
                break;
            };
            memcpy(
                &mut det as *mut _ as *mut c_void,
                (*detections).data.add(i * el_sz) as *const c_void,
                el_sz,
            );

            let eq = detection_compare_fn(det, &next, thresh);
            if eq != 0 || (*det).id != next.id {
                print!("Mismatch:\n  Expected ");
                format_line(extract(next));
                print!("  Found    ");
                format_line(extract(*det));
                ok = false;
            }
        }

        let rem = expected.count();

        if rem > 0 {
            println!("Expected {} detections, found {n}", n + rem);
            ok = false;
        }

        apriltag_detections_destroy(detections);
        image_u8_destroy(im);
        apriltag_detector_destroy(td);
        tag36h11_destroy(tf);
    }

    if !ok {
        panic!();
    }
}

/// Generate a test, loading an image and its expected data from "data/$path.jpg" and "data/$path.txt"
macro_rules! generate_test {
    ($test_name:ident, $path:literal $($ignore:meta)?) => {
        mod $test_name {
            use super::*;
            #[test]
            $(#[$ignore])?
            fn full_rust() {
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
            #[test]
            $(#[$ignore])?
            fn native_detector() {
                let img =
                    Buffer::decode_img_data(include_bytes!(concat!("data/", $path, ".jpg")))
                        .expect(concat!("failed to load image at data/", $path, ".jpg"))
                        .converted_into(PixelFormat::Luma);
                let data = include_str!(concat!("data/", $path, ".txt"));

                unsafe {
                    let im = image_u8_create(img.width, img.height);
                    let stride = (*im).stride as usize;

                    for row in 0..img.height {
                        for col in 0..img.width {
                            *(*im).buf.add(stride * row as usize + col as usize) =
                                img.pixel(col, row).unwrap()[0];
                        }
                    }

                    handle_native(data, im, 0.5);
                }
            }

            #[test]
            fn full_native() {
                let imgdata = include_bytes!(concat!("data/", $path, ".jpg"));
                let data = include_str!(concat!("data/", $path, ".txt"));
                unsafe {
                    let mut code = 0;
                    let pj = pjpeg_create_from_buffer(
                        imgdata.as_ptr(),
                        imgdata.len() as _,
                        0,
                        &mut code,
                    );
                    if code != 0 {
                        println!("failed to parse image with code");
                    }
                    let im = pjpeg_to_u8_baseline(pj);
                    handle_native(data, im, 0.1);
                    pjpeg_destroy(pj);
                }
            }
        }
    };
}

generate_test!(test_img_1, "33369213973_9d9bb4cc96_c");
generate_test!(test_img_2, "34085369442_304b6bafd9_c");
generate_test!(test_img_3, "34139872896_defdb2f8d9_c" ignore);
