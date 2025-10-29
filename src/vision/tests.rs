use super::*;
use std::time::{Duration, Instant};

#[track_caller]
fn assert_timeout<R: Send + 'static, F: FnOnce() -> R + Send + 'static>(
    timeout: Duration,
    f: F,
) -> R {
    let end = Instant::now() + timeout;
    let handle = std::thread::spawn(f);
    loop {
        if handle.is_finished() {
            return handle
                .join()
                .unwrap_or_else(|payload| std::panic::resume_unwind(payload));
        }
        if Instant::now() > end {
            panic!("Test took more than {timeout:?} to finish");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

mod blob {
    use super::*;

    #[test]
    fn _1x1() {
        let img = [[true]];
        assert_eq!(
            BlobsIterator::new(img).collect::<Vec<_>>(),
            &[Blob {
                min_x: 0,
                max_x: 1,
                min_y: 0,
                max_y: 1,
                pixels: 1
            }]
        );
    }
    #[test]
    fn _2x2() {
        let img = [[true, true], [true, true]];
        assert_eq!(
            BlobsIterator::new(img).collect::<Vec<_>>(),
            &[Blob {
                min_x: 0,
                max_x: 2,
                min_y: 0,
                max_y: 2,
                pixels: 4
            }]
        );
    }
    #[test]
    fn cross_2x2() {
        let img = [[true, false], [false, true]];
        assert_eq!(
            BlobsIterator::new(img).collect::<Vec<_>>(),
            &[Blob {
                min_x: 0,
                max_x: 2,
                min_y: 0,
                max_y: 2,
                pixels: 2
            }]
        );
    }
    #[test]
    fn two_regions() {
        let img = [[true, false, false], [false, false, true]];
        assert_eq!(
            BlobsIterator::new(img).collect::<Vec<_>>(),
            &[
                Blob {
                    min_x: 0,
                    max_x: 1,
                    min_y: 0,
                    max_y: 1,
                    pixels: 1
                },
                Blob {
                    min_x: 2,
                    max_x: 3,
                    min_y: 1,
                    max_y: 2,
                    pixels: 1
                }
            ]
        );
    }
    #[test]
    fn one_row() {
        let img = [[true, false, true]];
        assert_eq!(
            BlobsIterator::new(img).collect::<Vec<_>>(),
            &[
                Blob {
                    min_x: 0,
                    max_x: 1,
                    min_y: 0,
                    max_y: 1,
                    pixels: 1
                },
                Blob {
                    min_x: 2,
                    max_x: 3,
                    min_y: 0,
                    max_y: 1,
                    pixels: 1
                }
            ]
        );
    }
    #[test]
    fn big_u() {
        let img = [
            [true, false, false, true],
            [true, false, false, true],
            [false, true, true, false],
        ];
        assert_eq!(
            BlobsIterator::new(img).collect::<Vec<_>>(),
            &[Blob {
                min_x: 0,
                max_x: 4,
                min_y: 0,
                max_y: 3,
                pixels: 6
            }]
        );
    }
    #[test]
    fn big_arch() {
        let img = [
            [false, true, true, false],
            [true, false, false, true],
            [true, false, false, true],
        ];
        assert_eq!(
            BlobsIterator::new(img).collect::<Vec<_>>(),
            &[Blob {
                min_x: 0,
                max_x: 4,
                min_y: 0,
                max_y: 3,
                pixels: 6
            }]
        );
    }
    #[test]
    fn zigzag() {
        let row1 = [
            false, true, false, true, false, true, false, true, false, true, false, true,
        ];
        let row2 = row1.map(|x| !x);
        let img = [row1, row2];
        assert_eq!(
            BlobsIterator::new(img).collect::<Vec<_>>(),
            &[Blob {
                min_x: 0,
                max_x: 12,
                min_y: 0,
                max_y: 2,
                pixels: 12
            }]
        );
    }
    #[test]
    fn fermata() {
        let img = [
            [false, true, true, true, false],
            [true, false, false, false, true],
            [false, false, true, false, false],
        ];
        assert_eq!(
            BlobsIterator::new(img).collect::<Vec<_>>(),
            &[
                Blob {
                    min_x: 0,
                    max_x: 5,
                    min_y: 0,
                    max_y: 2,
                    pixels: 5,
                },
                Blob {
                    min_x: 2,
                    max_x: 3,
                    min_y: 2,
                    max_y: 3,
                    pixels: 1,
                },
            ]
        )
    }

    static FERRIS: &[u8] = include_bytes!("data/ferris.png");

    #[test]
    fn ferris_1() {
        let img = Buffer::decode_png_data(FERRIS).unwrap();
        let mut buf = Buffer::empty_rgb();
        swizzle(img.borrow(), &mut buf, &[2]); // blue channel, just the eyes
        let blobs = assert_timeout(Duration::from_millis(100), move || {
            BlobsIterator::from_buffer(&buf)
                .filter(|b| b.width() >= 10 && b.height() >= 10)
                .collect::<Vec<_>>()
        });
        assert_eq!(
            blobs,
            [
                Blob {
                    min_x: 256,
                    max_x: 283,
                    min_y: 150,
                    max_y: 183,
                    pixels: 542
                },
                Blob {
                    min_x: 177,
                    max_x: 202,
                    min_y: 150,
                    max_y: 185,
                    pixels: 568
                }
            ]
        );
    }

    #[test]
    fn ferris_2() {
        let img = Buffer::decode_png_data(FERRIS).unwrap();
        let mut buf = Buffer::empty_rgb();
        swizzle(img.borrow(), &mut buf, &[3]); // alpha channel, all of ferris
        let blobs = assert_timeout(Duration::from_millis(100), move || {
            BlobsIterator::from_buffer(&buf)
                .filter(|b| b.width() >= 100 && b.height() >= 100)
                .collect::<Vec<_>>()
        });
        assert_eq!(
            blobs,
            [Blob {
                min_x: 14,
                max_x: 446,
                min_y: 13,
                max_y: 295,
                pixels: 64969
            }]
        );
    }
}
mod window {
    use super::*;
    use crate::buffer::*;

    // Box filters are done in the same way, so if this works for one, it'll work for the other.
    #[test]
    fn filter() {
        let img = Buffer::monochrome(10, 10, PixelFormat::RGB, &[255, 0, 128]);
        let mut dst = Buffer::empty_rgb();
        percentile_filter(img, &mut dst, 3, 3, 0);
    }
    #[test]
    fn filter_single() {
        let img = Buffer::monochrome(1, 1, PixelFormat::RGB, &[255, 0, 128]);
        let mut dst = Buffer::empty_rgb();
        percentile_filter(img, &mut dst, 15, 15, 0);
    }
}
