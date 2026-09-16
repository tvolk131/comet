//! Compare at the original 2× capture size, without rescaling or aligning images.
//! Flat colors have a 3/255 allowance. Edge pixels may vary within a one-pixel
//! neighborhood (half a logical pixel), with at most 64/255 extra coverage error.
//! Both images must contain an edge there. A separate 32×32 local color budget
//! prevents that allowance from hiding lost strokes, carets, or selections.
use std::{
    fs::File,
    io::{BufReader, BufWriter},
    path::Path,
};

const COLOR_TOLERANCE: u8 = 3;
// The largest allowance is needed at the one-pixel FAB shadow boundary, where
// MSAA and tiny-skia blend the surface, shadow, and background differently.
const EDGE_TOLERANCE: u8 = 64;
const TILE: usize = 32;
const TILE_MEAN_TOLERANCE: i64 = 4;

#[derive(Clone, Debug)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

impl Image {
    pub fn read(path: &Path) -> Self {
        let mut decoder = png::Decoder::new(BufReader::new(File::open(path).unwrap()))
            .read_info()
            .unwrap();
        let mut rgba = vec![0; decoder.output_buffer_size().unwrap()];
        let info = decoder.next_frame(&mut rgba).unwrap();
        assert_eq!(info.color_type, png::ColorType::Rgba);
        assert_eq!(info.bit_depth, png::BitDepth::Eight);
        rgba.truncate(info.buffer_size());
        Self {
            width: info.width as usize,
            height: info.height as usize,
            rgba,
        }
    }

    pub fn write(&self, path: &Path) {
        let mut encoder = png::Encoder::new(
            BufWriter::new(File::create(path).unwrap()),
            u32::try_from(self.width).unwrap(),
            u32::try_from(self.height).unwrap(),
        );
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&self.rgba)
            .unwrap();
    }

    fn pixel(&self, x: usize, y: usize) -> &[u8] {
        let offset = (y * self.width + x) * 4;
        &self.rgba[offset..offset + 4]
    }

    fn range(&self, x: usize, y: usize) -> ([u8; 3], [u8; 3]) {
        let mut min = [255; 3];
        let mut max = [0; 3];
        for row in y.saturating_sub(1)..=(y + 1).min(self.height - 1) {
            for col in x.saturating_sub(1)..=(x + 1).min(self.width - 1) {
                for c in 0..3 {
                    min[c] = min[c].min(self.pixel(col, row)[c]);
                    max[c] = max[c].max(self.pixel(col, row)[c]);
                }
            }
        }
        (min, max)
    }
}

#[derive(Debug)]
pub struct Difference {
    pub reason: String,
    pub heatmap: Image,
}

pub fn compare(expected: &Image, actual: &Image) -> Result<(), Difference> {
    if (expected.width, expected.height) != (actual.width, actual.height) {
        return Err(Difference {
            reason: format!(
                "dimensions differ: {}×{} versus {}×{}",
                expected.width, expected.height, actual.width, actual.height
            ),
            heatmap: actual.clone(),
        });
    }
    let mut heatmap = Image {
        width: actual.width,
        height: actual.height,
        rgba: vec![0; actual.rgba.len()],
    };
    let mut mismatches = 0;
    let mut first = None;
    let mut worst_tile = 0.0_f64;
    let mut bad_tiles = 0;
    for top in (0..actual.height).step_by(TILE) {
        for left in (0..actual.width).step_by(TILE) {
            let bottom = (top + TILE).min(actual.height);
            let right = (left + TILE).min(actual.width);
            let mut sums = [0_i64; 3];
            for y in top..bottom {
                for x in left..right {
                    let a = expected.pixel(x, y);
                    let b = actual.pixel(x, y);
                    let delta = (0..3).map(|c| a[c].abs_diff(b[c])).max().unwrap();
                    let offset = (y * actual.width + x) * 4;
                    heatmap.rgba[offset..offset + 4].copy_from_slice(&[delta.saturating_mul(4); 4]);
                    heatmap.rgba[offset + 3] = 255;
                    for c in 0..3 {
                        sums[c] += i64::from(a[c]) - i64::from(b[c]);
                    }
                    let matches = a[3] == b[3]
                        && (delta <= COLOR_TOLERANCE || {
                            let (amin, amax) = expected.range(x, y);
                            let (bmin, bmax) = actual.range(x, y);
                            // Do not grant edge tolerance to a newly missing feature
                            // or to a color change on an otherwise flat surface.
                            let a_edge = (0..3).any(|c| amax[c] - amin[c] > COLOR_TOLERANCE);
                            let b_edge = (0..3).any(|c| bmax[c] - bmin[c] > COLOR_TOLERANCE);
                            a_edge
                                && b_edge
                                && (0..3).all(|c| {
                                    a[c] >= bmin[c].saturating_sub(EDGE_TOLERANCE)
                                        && a[c] <= bmax[c].saturating_add(EDGE_TOLERANCE)
                                        && b[c] >= amin[c].saturating_sub(EDGE_TOLERANCE)
                                        && b[c] <= amax[c].saturating_add(EDGE_TOLERANCE)
                                })
                        });
                    if !matches {
                        mismatches += 1;
                        first.get_or_insert((x, y));
                        heatmap.rgba[offset..offset + 4].copy_from_slice(&[255, 0, 80, 255]);
                    }
                }
            }
            let count = i64::try_from((bottom - top) * (right - left)).unwrap();
            let error = sums.into_iter().map(i64::abs).max().unwrap();
            #[allow(clippy::cast_precision_loss)]
            {
                worst_tile = worst_tile.max(error as f64 / count as f64);
            }
            if error > TILE_MEAN_TOLERANCE * count {
                bad_tiles += 1;
                first.get_or_insert((left, top));
            }
        }
    }
    if mismatches > 0 || bad_tiles > 0 {
        Err(Difference { reason: format!("{mismatches} pixels outside edge tolerance; {bad_tiles} tiles exceed local color budget (worst {worst_tile:.2}/255); first difference {first:?}"), heatmap })
    } else {
        Ok(())
    }
}
