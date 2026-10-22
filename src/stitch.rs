use image::{RgbaImage, imageops};

const MAX_PIXELS: u64 = 80_000_000;

#[derive(Debug, PartialEq)]
pub enum StitchResult {
    Added(u32),
    Duplicate,
    NoOverlap,
    LimitReached,
}

pub struct Stitcher {
    // The fixed header rows, then the scrolled content, then the fixed footer rows.
    pub image: RgbaImage,
    last: RgbaImage,
    last_offset: i64,
    min_offset: i64,
    // Rows at the top and bottom that stay still while the content scrolls, such as sticky
    // headers and toolbars. Fixed when the first movement is seen.
    margins: Option<(u32, u32)>,
}

impl Stitcher {
    pub fn new(frame: RgbaImage) -> Self {
        Self {
            image: frame.clone(),
            last: frame,
            last_offset: 0,
            min_offset: 0,
            margins: None,
        }
    }
    pub fn frame_size(&self) -> (u32, u32) {
        self.last.dimensions()
    }
    // Height of the rows that scroll, excluding sticky bars once they are known.
    pub fn moving_height(&self) -> u32 {
        let (top, bottom) = self.margins.unwrap_or((0, 0));
        self.last.height() - top - bottom
    }
    pub fn push(&mut self, frame: RgbaImage) -> StitchResult {
        if frame.dimensions() != self.last.dimensions() {
            return StitchResult::NoOverlap;
        }
        let (w, h) = frame.dimensions();
        let current = Signature::new(&frame, 0, h);
        if score(&Signature::new(&self.last, 0, h), &current, 0, 0, h) < 0.7 {
            return StitchResult::Duplicate;
        }
        let (pair_top, pair_bottom) = static_margins(&self.last, &frame);
        let (top, bottom) = self.margins.unwrap_or((pair_top, pair_bottom));
        let band_h = h - top - bottom;
        // Match only the rows that move. A sticky bar seen in this pair is ignored as well.
        let (match_top, match_bottom) = (top.max(pair_top), bottom.max(pair_bottom));
        let match_h = h - match_top - match_bottom;
        let shift = find_shift(
            &Signature::new(&self.last, match_top, match_h),
            &current.rows(match_top, match_h),
        );
        let body_h = self.image.height() - top - bottom;
        let offset = match (shift, self.margins) {
            (Some(delta), _) => self.last_offset + delta,
            // The view jumped past the previous frame. Find it again in the stitched content,
            // so scrolling back a little resumes the capture instead of failing forever.
            (None, Some(_)) => {
                let body = Signature::new(&self.image, top, body_h);
                match locate(&body, &current.rows(top, band_h)) {
                    Some(position) => self.min_offset + position,
                    None => return StitchResult::NoOverlap,
                }
            }
            (None, None) => return StitchResult::NoOverlap,
        };
        self.margins = Some((top, bottom));
        let old_max = self.min_offset + body_h as i64;
        let new_min = self.min_offset.min(offset);
        let new_max = old_max.max(offset + band_h as i64);
        let new_h = (new_max - new_min) as u32;
        let total_h = new_h + top + bottom;
        if total_h as u64 * w as u64 > MAX_PIXELS || total_h > 32_000 {
            return StitchResult::LimitReached;
        }
        if offset < self.min_offset {
            // Scrolled above the first frame. Rebuild with this frame's header and new rows.
            let rows = (self.min_offset - offset) as u32;
            let mut next = RgbaImage::new(w, self.image.height() + rows);
            imageops::replace(&mut next, &self.image, 0, rows as i64);
            let head = imageops::crop_imm(&frame, 0, 0, w, top + rows).to_image();
            imageops::replace(&mut next, &head, 0, 0);
            self.image = next;
            self.min_offset = offset;
        }
        if offset + band_h as i64 > old_max {
            // Append below the content in place, then put this frame's footer at the end.
            let row = w as usize * 4;
            let from = (old_max - offset) as usize + top as usize;
            let kept = (self.image.height() - bottom) as usize;
            let mut raw = std::mem::replace(&mut self.image, RgbaImage::new(0, 0)).into_raw();
            raw.truncate(kept * row);
            raw.extend_from_slice(&frame.as_raw()[from * row..]);
            self.image = RgbaImage::from_raw(w, total_h, raw).expect("stitched rows");
        }
        self.last = frame;
        self.last_offset = offset;
        if new_h == body_h {
            StitchResult::Duplicate
        } else {
            StitchResult::Added(new_h - body_h)
        }
    }
}

// Counts identical rows at the top and bottom of two frames. Each side is capped at a third of
// the height so that the moving band stays large enough to match.
fn static_margins(a: &RgbaImage, b: &RgbaImage) -> (u32, u32) {
    let (w, h) = a.dimensions();
    let same = |y: u32| {
        (0..w).all(|x| {
            let (pa, pb) = (a.get_pixel(x, y), b.get_pixel(x, y));
            (0..3).all(|c| pa[c].abs_diff(pb[c]) <= 3)
        })
    };
    let limit = h / 3;
    let top = (0..limit).take_while(|&y| same(y)).count() as u32;
    let bottom = (0..limit).take_while(|&i| same(h - 1 - i)).count() as u32;
    (top, bottom)
}

// Sampled colors and edge flags of a run of rows, read once per frame so that the offset search
// compares plain arrays.
struct Signature {
    columns: usize,
    colors: Vec<[u8; 3]>,
    edges: Vec<bool>,
}

impl Signature {
    fn new(image: &RgbaImage, y: u32, rows: u32) -> Self {
        let w = image.width();
        let xs: Vec<u32> = ((w / 12)..(w - w / 12))
            .step_by((w / 64).max(1) as usize)
            .collect();
        let mut colors = Vec::with_capacity(xs.len() * rows as usize);
        let mut edges = Vec::with_capacity(colors.capacity());
        for y in y..y + rows {
            for &x in &xs {
                let p = image.get_pixel(x, y);
                let edge = image.get_pixel(x.saturating_sub(2), y);
                colors.push([p[0], p[1], p[2]]);
                edges.push((0..3).any(|c| p[c].abs_diff(edge[c]) > 12));
            }
        }
        Self {
            columns: xs.len(),
            colors,
            edges,
        }
    }
    fn height(&self) -> u32 {
        (self.colors.len() / self.columns.max(1)) as u32
    }
    fn rows(&self, y: u32, rows: u32) -> Self {
        let range = y as usize * self.columns..(y + rows) as usize * self.columns;
        Self {
            columns: self.columns,
            colors: self.colors[range.clone()].to_vec(),
            edges: self.edges[range].to_vec(),
        }
    }
}

fn score(a: &Signature, b: &Signature, ay: u32, by: u32, rows: u32) -> f64 {
    let n = a.columns;
    let sy = (rows / 60).max(1);
    let mut sum = 0.0;
    let mut count = 0.0;
    for y in (0..rows).step_by(sy as usize) {
        let (ia, ib) = ((ay + y) as usize * n, (by + y) as usize * n);
        for i in 0..n {
            let (pa, pb) = (a.colors[ia + i], b.colors[ib + i]);
            let weight = if a.edges[ia + i] || b.edges[ib + i] {
                1.0
            } else {
                0.15
            };
            let diff = (0..3).map(|c| pa[c].abs_diff(pb[c]) as u32).sum::<u32>();
            sum += diff as f64 * weight;
            count += 3.0 * weight;
        }
    }
    if count == 0.0 {
        return 255.0;
    }
    sum / count
}

// Searches every row offset. A coarse grid misses the exact alignment of sharp Retina text, and a
// wrong offset in repeated content can then score better than the slightly misaligned right one.
fn find_shift(a: &Signature, b: &Signature) -> Option<i64> {
    let h = a.height();
    let minimum = (h / 5).max(16);
    if h <= minimum {
        return None;
    }
    let mut best = (f64::MAX, 0_i64);
    let mut hypotheses = vec![];
    for delta in 1..h - minimum {
        let rows = h - delta;
        for (error, offset) in [
            (score(a, b, delta, 0, rows), delta as i64),
            (score(a, b, 0, delta, rows), -(delta as i64)),
        ] {
            hypotheses.push((error, offset));
            if error < best.0 {
                best = (error, offset);
            }
        }
    }
    distinct(&hypotheses, best, (h / 24).max(8) as u64).then_some(best.1)
}

// Finds where a frame band sits in the stitched body, allowing it to extend past either end.
fn locate(body: &Signature, band: &Signature) -> Option<i64> {
    let (body_h, band_h) = (body.height() as i64, band.height() as i64);
    let minimum = (band_h / 5).max(16);
    if band_h <= minimum || body_h < minimum {
        return None;
    }
    let mut best = (f64::MAX, 0_i64);
    let mut hypotheses = vec![];
    for position in minimum - band_h..=body_h - minimum {
        let (ay, by) = (position.max(0), (-position).max(0));
        let rows = body_h.min(position + band_h) - ay;
        let error = score(body, band, ay as u32, by as u32, rows as u32);
        hypotheses.push((error, position));
        if error < best.0 {
            best = (error, position);
        }
    }
    distinct(&hypotheses, best, (band_h as u64 / 24).max(8)).then_some(best.1)
}

// Accepts the best offset only when it clearly beats every offset away from it. Repeated rows,
// such as list items or table rows, give wrong offsets a moderate error, while the true offset of
// an unchanged page scores close to zero.
fn distinct(hypotheses: &[(f64, i64)], best: (f64, i64), radius: u64) -> bool {
    let second = hypotheses
        .iter()
        .filter(|(_, offset)| offset.abs_diff(best.1) > radius)
        .map(|(error, _)| *error)
        .fold(f64::MAX, f64::min);
    best.0 < 14.0 && second >= best.0 * 2.0 && second - best.0 >= 0.3
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;
    fn content() -> RgbaImage {
        RgbaImage::from_fn(96, 600, |x, y| {
            Rgba([
                ((x * 31 + y * 17) % 251) as u8,
                ((y * 53 + x * 7) % 253) as u8,
                ((x * y + 31 * y) % 255) as u8,
                255,
            ])
        })
    }
    #[test]
    fn scroll_down_back_up_and_past_origin_preserves_content() {
        let source = content();
        let frame = |y| imageops::crop_imm(&source, 0, y, 96, 200).to_image();
        let mut s = Stitcher::new(frame(100));
        assert_eq!(s.push(frame(160)), StitchResult::Added(60));
        assert_eq!(s.push(frame(220)), StitchResult::Added(60));
        assert_eq!(s.push(frame(160)), StitchResult::Duplicate);
        assert_eq!(s.push(frame(40)), StitchResult::Added(60));
        assert_eq!(
            s.image,
            imageops::crop_imm(&source, 0, 40, 96, 380).to_image()
        );
    }
    #[test]
    fn duplicate_and_unrelated_frames_do_not_damage_a_good_capture() {
        let source = content();
        let frame = imageops::crop_imm(&source, 0, 0, 96, 200).to_image();
        let mut s = Stitcher::new(frame.clone());
        assert_eq!(s.push(frame.clone()), StitchResult::Duplicate);
        let unrelated = RgbaImage::from_pixel(96, 200, Rgba([0, 0, 0, 255]));
        assert_eq!(s.push(unrelated), StitchResult::NoOverlap);
        assert_eq!(s.image, frame);
    }
    #[test]
    fn text_on_a_white_background_is_stitched_at_the_correct_offset() {
        let source = RgbaImage::from_fn(320, 900, |x, y| {
            let row = y / 24;
            let ink = (3..16).contains(&(y % 24)) && x > 20 && x < 70 + row * 17 % 210 && x % 7 < 4;
            if ink {
                Rgba([20, 24, 31, 255])
            } else {
                Rgba([255, 255, 255, 255])
            }
        });
        let frame = |y| imageops::crop_imm(&source, 0, y, 320, 300).to_image();
        let mut stitcher = Stitcher::new(frame(100));
        assert_eq!(stitcher.push(frame(193)), StitchResult::Added(93));
        assert_eq!(
            stitcher.image,
            imageops::crop_imm(&source, 0, 100, 320, 393).to_image()
        );
    }
    #[test]
    fn sticky_header_and_footer_are_kept_once_while_content_keeps_stitching() {
        let source = content();
        let header = RgbaImage::from_pixel(96, 24, Rgba([30, 90, 200, 255]));
        let footer = RgbaImage::from_pixel(96, 16, Rgba([240, 240, 240, 255]));
        let frame = |y| {
            let mut pixels = imageops::crop_imm(&source, 0, y, 96, 200).to_image();
            imageops::replace(&mut pixels, &header, 0, 0);
            imageops::replace(&mut pixels, &footer, 0, 184);
            pixels
        };
        let mut stitcher = Stitcher::new(frame(0));
        for y in (70..=350).step_by(70) {
            assert_eq!(
                stitcher.push(frame(y)),
                StitchResult::Added(70),
                "frame at {y}"
            );
        }
        let mut expected = RgbaImage::new(96, 550);
        let body = imageops::crop_imm(&source, 0, 24, 96, 510).to_image();
        imageops::replace(&mut expected, &header, 0, 0);
        imageops::replace(&mut expected, &body, 0, 24);
        imageops::replace(&mut expected, &footer, 0, 534);
        assert_eq!(stitcher.image, expected);
    }
    #[test]
    fn a_jump_past_the_previous_frame_is_found_again_in_the_stitched_content() {
        let source = content();
        let frame = |y| imageops::crop_imm(&source, 0, y, 96, 200).to_image();
        let mut stitcher = Stitcher::new(frame(0));
        assert_eq!(stitcher.push(frame(60)), StitchResult::Added(60));
        // Scrolled too far: nothing to match against yet.
        assert_eq!(stitcher.push(frame(400)), StitchResult::NoOverlap);
        assert_eq!(stitcher.push(frame(300)), StitchResult::NoOverlap);
        // Scrolling back a little resumes the capture.
        assert_eq!(stitcher.push(frame(200)), StitchResult::Added(140));
        assert_eq!(stitcher.push(frame(100)), StitchResult::Duplicate);
        // Too far from the previous frame, but still overlapping the stitched content.
        assert_eq!(stitcher.push(frame(330)), StitchResult::Added(130));
        assert_eq!(stitcher.push(frame(400)), StitchResult::Added(70));
        assert_eq!(
            stitcher.image,
            imageops::crop_imm(&source, 0, 0, 96, 600).to_image()
        );
    }
    #[test]
    fn repeated_rows_without_a_true_match_are_rejected_instead_of_misplaced() {
        // Identical list rows that differ only in a small row number.
        let source = RgbaImage::from_fn(240, 1200, |x, y| {
            let (row, line) = (y / 40, y % 40);
            let label = (8..30).contains(&line) && (20..140).contains(&x) && x % 6 < 3;
            let bit = (160..220).contains(&x) && ((row * 7 + 3) >> ((x - 160) / 12)) & 1 == 1;
            let number = (8..30).contains(&line) && bit;
            if label || number {
                Rgba([25, 30, 40, 255])
            } else {
                Rgba([250, 250, 252, 255])
            }
        });
        let frame = |y| imageops::crop_imm(&source, 0, y, 240, 300).to_image();
        let mut stitcher = Stitcher::new(frame(0));
        assert_eq!(stitcher.push(frame(100)), StitchResult::Added(100));
        let before = stitcher.image.clone();
        assert_eq!(stitcher.push(frame(700)), StitchResult::NoOverlap);
        assert_eq!(stitcher.image, before);
        assert_eq!(stitcher.push(frame(250)), StitchResult::Added(150));
        assert_eq!(
            stitcher.image,
            imageops::crop_imm(&source, 0, 0, 240, 550).to_image()
        );
    }
}
