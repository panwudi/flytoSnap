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
    pub image: RgbaImage,
    last: RgbaImage,
    last_offset: i64,
    min_offset: i64,
}

impl Stitcher {
    pub fn new(frame: RgbaImage) -> Self {
        Self {
            image: frame.clone(),
            last: frame,
            last_offset: 0,
            min_offset: 0,
        }
    }
    pub fn push(&mut self, frame: RgbaImage) -> StitchResult {
        if frame.dimensions() != self.last.dimensions() {
            return StitchResult::NoOverlap;
        }
        let (w, h) = frame.dimensions();
        if score(&self.last, &frame, 0, 0, h) < 0.7 {
            return StitchResult::Duplicate;
        }
        let Some(delta) = find_shift(&self.last, &frame) else {
            return StitchResult::NoOverlap;
        };
        let offset = self.last_offset + delta;
        let old_max = self.min_offset + self.image.height() as i64;
        let new_min = self.min_offset.min(offset);
        let new_max = old_max.max(offset + h as i64);
        let new_h = (new_max - new_min) as u32;
        if new_h as u64 * w as u64 > MAX_PIXELS || new_h > 32_000 {
            return StitchResult::LimitReached;
        }
        let added = new_h - self.image.height();
        if added > 0 {
            let mut next = RgbaImage::new(w, new_h);
            imageops::replace(&mut next, &self.image, 0, self.min_offset - new_min);
            if offset < self.min_offset {
                let strip = imageops::crop_imm(&frame, 0, 0, w, (self.min_offset - offset) as u32)
                    .to_image();
                imageops::replace(&mut next, &strip, 0, 0);
            }
            if offset + h as i64 > old_max {
                let from = (old_max - offset).max(0) as u32;
                let strip = imageops::crop_imm(&frame, 0, from, w, h - from).to_image();
                imageops::replace(&mut next, &strip, 0, old_max - new_min);
            }
            self.image = next;
            self.min_offset = new_min;
        }
        self.last = frame;
        self.last_offset = offset;
        if added == 0 {
            StitchResult::Duplicate
        } else {
            StitchResult::Added(added)
        }
    }
}

fn score(a: &RgbaImage, b: &RgbaImage, ay: u32, by: u32, rows: u32) -> f64 {
    let w = a.width();
    let sx = (w / 64).max(1);
    let sy = (rows / 60).max(1);
    let mut sum = 0.0;
    let mut count = 0.0;
    for y in (0..rows).step_by(sy as usize) {
        for x in ((w / 12)..(w - w / 12)).step_by(sx as usize) {
            let pa = a.get_pixel(x, ay + y);
            let pb = b.get_pixel(x, by + y);
            let edge_a = a.get_pixel(x.saturating_sub(2), ay + y);
            let edge_b = b.get_pixel(x.saturating_sub(2), by + y);
            let salient =
                (0..3).any(|c| pa[c].abs_diff(edge_a[c]) > 12 || pb[c].abs_diff(edge_b[c]) > 12);
            let weight = if salient { 1.0 } else { 0.15 };
            for c in 0..3 {
                sum += (pa[c] as f64 - pb[c] as f64).abs() * weight;
                count += weight;
            }
        }
    }
    if count == 0.0 {
        return 255.0;
    }
    sum / count
}

fn find_shift(a: &RgbaImage, b: &RgbaImage) -> Option<i64> {
    let h = a.height();
    let minimum = (h / 5).max(16);
    if h <= minimum {
        return None;
    }
    let step = (h / 180).max(1);
    let mut best = (f64::MAX, 0_i64);
    let mut hypotheses = vec![];
    for delta in (1..h - minimum).step_by(step as usize) {
        let rows = h - delta;
        for sign in [1_i64, -1] {
            let error = if sign > 0 {
                score(a, b, delta, 0, rows)
            } else {
                score(a, b, 0, delta, rows)
            };
            hypotheses.push((error, sign * delta as i64));
            if error < best.0 {
                best = (error, sign * delta as i64);
            }
        }
    }
    let center = best.1.unsigned_abs() as u32;
    let sign = best.1.signum();
    for delta in center.saturating_sub(step)..=(center + step).min(h - minimum - 1) {
        if delta == 0 {
            continue;
        }
        let error = if sign > 0 {
            score(a, b, delta, 0, h - delta)
        } else {
            score(a, b, 0, delta, h - delta)
        };
        if error < best.0 {
            best = (error, sign * delta as i64);
        }
    }
    let ambiguous = hypotheses.iter().any(|(error, delta)| {
        delta.abs_diff(best.1) > (h / 24).max(8) as u64 && *error < best.0 + 0.2
    });
    (best.0 < 14.0 && !ambiguous).then_some(best.1)
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
}
