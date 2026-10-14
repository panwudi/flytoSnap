use super::render;
use crate::model::{Color, Point, Rect};
use objc2::{AnyThread, rc::Retained};
use objc2_app_kit::*;
use objc2_core_graphics::{CGContext, CGLineCap, CGLineJoin};
use objc2_foundation::{NSDictionary, NSPoint, NSSize};
use std::path::Path;

pub fn bitmap(size: usize, draw: impl FnOnce(&CGContext)) -> Result<Vec<u8>, String> {
    let image=unsafe {NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(NSBitmapImageRep::alloc(),std::ptr::null_mut(),size as isize,size as isize,8,4,true,false,NSDeviceRGBColorSpace,0,0)}.ok_or("Could not allocate bitmap")?;
    let graphics = NSGraphicsContext::graphicsContextWithBitmapImageRep(&image)
        .ok_or("Could not create graphics context")?;
    let cg = graphics.CGContext();
    NSGraphicsContext::saveGraphicsState_class();
    CGContext::translate_ctm(Some(&cg), 0.0, size as f64);
    CGContext::scale_ctm(Some(&cg), 1.0, -1.0);
    let flipped = NSGraphicsContext::graphicsContextWithCGContext_flipped(&cg, true);
    NSGraphicsContext::setCurrentContext(Some(&flipped));
    draw(&cg);
    NSGraphicsContext::restoreGraphicsState_class();
    unsafe {
        image.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new())
    }
    .map(|d| d.to_vec())
    .ok_or("Could not encode bitmap".into())
}
fn polygon(cg: &CGContext, points: &[(f64, f64)], color: Color) {
    let ctx = Some(cg);
    CGContext::set_rgb_fill_color(ctx, color.0, color.1, color.2, 1.0);
    CGContext::begin_path(ctx);
    if let Some(p) = points.first() {
        CGContext::move_to_point(ctx, p.0, p.1);
    }
    for p in points.iter().skip(1) {
        CGContext::add_line_to_point(ctx, p.0, p.1);
    }
    CGContext::close_path(ctx);
    CGContext::fill_path(ctx);
}
pub fn export_icon(output: &Path) -> Result<(), String> {
    let png = bitmap(1024, |cg| {
        render::rounded(
            Rect::new(44.0, 44.0, 936.0, 936.0),
            208.0,
            Color(71.0 / 255.0, 79.0 / 255.0, 105.0 / 255.0),
            1.0,
        );
        CGContext::save_g_state(Some(cg));
        CGContext::translate_ctm(Some(cg), 24.0, 24.0);
        CGContext::scale_ctm(Some(cg), 5.0, 5.0);
        polygon(
            cg,
            &[
                (144.0, 29.0),
                (163.0, 29.0),
                (163.0, 49.0),
                (144.0, 49.0),
                (142.0, 51.0),
                (142.0, 142.0),
                (120.0, 120.0),
                (120.0, 73.0),
                (72.0, 72.0),
                (50.0, 50.0),
                (142.0, 50.0),
                (144.0, 45.0),
            ],
            Color(239.0 / 255.0, 130.0 / 255.0, 0.0),
        );
        polygon(
            cg,
            &[
                (49.0, 50.0),
                (72.0, 73.0),
                (72.0, 119.0),
                (119.0, 120.0),
                (142.0, 143.0),
                (49.0, 143.0),
                (48.0, 164.0),
                (29.0, 164.0),
                (28.0, 145.0),
                (49.0, 143.0),
            ],
            Color(244.0 / 255.0, 246.0 / 255.0, 250.0 / 255.0),
        );
        render::rounded(
            Rect::new(80.0, 84.0, 34.0, 25.0),
            5.0,
            Color(244.0 / 255.0, 246.0 / 255.0, 250.0 / 255.0),
            1.0,
        );
        render::rounded(
            Rect::new(90.0, 80.0, 14.0, 8.0),
            3.0,
            Color(244.0 / 255.0, 246.0 / 255.0, 250.0 / 255.0),
            1.0,
        );
        CGContext::set_rgb_fill_color(Some(cg), 71.0 / 255.0, 79.0 / 255.0, 105.0 / 255.0, 1.0);
        CGContext::fill_ellipse_in_rect(Some(cg), render::nr(Rect::new(90.0, 89.0, 14.0, 14.0)));
        CGContext::restore_g_state(Some(cg));
    })?;
    std::fs::write(output, png).map_err(|e| e.to_string())
}
pub fn diagonal_cursor(up: bool) -> Retained<NSCursor> {
    let png = bitmap(24, |cg| {
        let ctx = Some(cg);
        CGContext::set_line_cap(ctx, CGLineCap::Round);
        CGContext::set_line_join(ctx, CGLineJoin::Round);
        let points = if up {
            [Point::new(4.0, 20.0), Point::new(20.0, 4.0)]
        } else {
            [Point::new(4.0, 4.0), Point::new(20.0, 20.0)]
        };
        for (width, value) in [(4.0, 1.0), (2.0, 0.05)] {
            CGContext::set_line_width(ctx, width);
            CGContext::set_rgb_stroke_color(ctx, value, value, value, 1.0);
            CGContext::begin_path(ctx);
            CGContext::move_to_point(ctx, points[0].x, points[0].y);
            CGContext::add_line_to_point(ctx, points[1].x, points[1].y);
            CGContext::stroke_path(ctx);
            for (i, p) in points.iter().enumerate() {
                let d = if i == 0 { 1.0 } else { -1.0 };
                CGContext::begin_path(ctx);
                CGContext::move_to_point(ctx, p.x + 6.0 * d, p.y);
                CGContext::add_line_to_point(ctx, p.x, p.y);
                CGContext::add_line_to_point(ctx, p.x, p.y + 6.0 * d * if up { -1.0 } else { 1.0 });
                CGContext::stroke_path(ctx);
            }
        }
    })
    .expect("cursor bitmap");
    let data = objc2_foundation::NSData::with_bytes(&png);
    let image = NSImage::initWithData(NSImage::alloc(), &data).expect("cursor image");
    image.setSize(NSSize::new(24.0, 24.0));
    NSCursor::initWithImage_hotSpot(NSCursor::alloc(), &image, NSPoint::new(12.0, 12.0))
}

pub fn export_menu_icon(output: &Path) -> Result<(), String> {
    let png = bitmap(64, |cg| {
        let ctx = Some(cg);
        CGContext::set_rgb_stroke_color(ctx, 0.0, 0.0, 0.0, 1.0);
        CGContext::set_line_width(ctx, 6.0);
        CGContext::set_line_cap(ctx, CGLineCap::Round);
        CGContext::set_line_join(ctx, CGLineJoin::Round);
        for points in [
            [(12.0, 28.0), (12.0, 50.0), (36.0, 50.0)],
            [(28.0, 12.0), (50.0, 12.0), (50.0, 36.0)],
        ] {
            CGContext::begin_path(ctx);
            CGContext::move_to_point(ctx, points[0].0, points[0].1);
            for p in points.iter().skip(1) {
                CGContext::add_line_to_point(ctx, p.0, p.1);
            }
            CGContext::stroke_path(ctx);
        }
        render::rounded(
            Rect::new(24.0, 25.0, 18.0, 14.0),
            3.0,
            Color(0.0, 0.0, 0.0),
            1.0,
        );
    })?;
    std::fs::write(output, png).map_err(|e| e.to_string())
}
