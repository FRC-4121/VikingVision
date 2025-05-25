use super::*;

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
}
