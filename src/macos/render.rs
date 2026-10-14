use crate::model::{Annotation, Color, Editor, Point, Rect, Tool};
use image::{ImageEncoder, RgbaImage, imageops};
use objc2::{AnyThread, rc::Retained, runtime::AnyObject};
use objc2_app_kit::*;
use objc2_core_graphics::{CGContext, CGLineCap, CGLineJoin};
use objc2_foundation::{
    NSAttributedStringKey, NSData, NSDictionary, NSPoint, NSRect, NSSize, NSString,
};
use std::path::Path;

pub const BLUE: Color = Color(0.20, 0.42, 1.0);
pub fn nr(r: Rect) -> NSRect {
    NSRect::new(NSPoint::new(r.x, r.y), NSSize::new(r.w, r.h))
}
pub fn color(c: Color, alpha: f64) -> Retained<NSColor> {
    NSColor::colorWithSRGBRed_green_blue_alpha(c.0, c.1, c.2, alpha)
}
pub fn rounded(r: Rect, radius: f64, c: Color, alpha: f64) {
    color(c, alpha).setFill();
    NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(nr(r), radius, radius).fill();
}
pub fn stroke(r: Rect, c: Color, width: f64) {
    color(c, 1.0).setStroke();
    let path = NSBezierPath::bezierPathWithRect(nr(r));
    path.setLineWidth(width);
    path.stroke();
}
pub fn text(s: &str, p: Point, size: f64, c: Color) {
    let font = NSFont::systemFontOfSize(size);
    let ink = color(c, 1.0);
    let attrs: Retained<NSDictionary<NSAttributedStringKey, AnyObject>> = NSDictionary::from_slices(
        &[unsafe { NSFontAttributeName }, unsafe {
            NSForegroundColorAttributeName
        }],
        &[&*font as &AnyObject, &*ink as &AnyObject],
    );
    unsafe {
        NSString::from_str(s).drawAtPoint_withAttributes(NSPoint::new(p.x, p.y), Some(&attrs));
    }
}
pub fn native_image(pixels: &RgbaImage, size: NSSize) -> Result<Retained<NSImage>, String> {
    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new_with_quality(
        &mut bytes,
        image::codecs::png::CompressionType::Fast,
        image::codecs::png::FilterType::Adaptive,
    )
    .write_image(
        pixels.as_raw(),
        pixels.width(),
        pixels.height(),
        image::ExtendedColorType::Rgba8,
    )
    .map_err(|e| e.to_string())?;
    let data = NSData::with_bytes(&bytes);
    let image = NSImage::initWithData(NSImage::alloc(), &data).ok_or("Could not decode image")?;
    image.setSize(size);
    Ok(image)
}
pub fn mosaic_image(pixels: &RgbaImage, size: NSSize) -> Result<Retained<NSImage>, String> {
    let block = (pixels.width() as f64 / size.width * 10.0).max(8.0) as u32;
    let small = imageops::resize(
        pixels,
        (pixels.width() / block).max(1),
        (pixels.height() / block).max(1),
        imageops::FilterType::Triangle,
    );
    let big = imageops::resize(
        &small,
        pixels.width(),
        pixels.height(),
        imageops::FilterType::Nearest,
    );
    native_image(&big, size)
}
pub fn draw_image(image: &NSImage, r: Rect) {
    unsafe {
        image.drawInRect_fromRect_operation_fraction_respectFlipped_hints(
            nr(r),
            NSRect::ZERO,
            NSCompositingOperation::SourceOver,
            1.0,
            true,
            None,
        );
    }
}
pub fn draw_annotations(editor: &Editor, mosaic: &NSImage, skip: Option<usize>) {
    let Some(graphics) = NSGraphicsContext::currentContext() else {
        return;
    };
    let cg = graphics.CGContext();
    let ctx = Some(&*cg);
    CGContext::save_g_state(ctx);
    if let Some(selection) = editor.selection {
        CGContext::clip_to_rect(ctx, nr(selection));
    }
    for (i, a) in editor.annotations.iter().enumerate() {
        if skip != Some(i) {
            draw_annotation(a, mosaic, editor.bounds, &cg);
        }
    }
    CGContext::restore_g_state(ctx);
}

fn line(cg: &CGContext, points: &[Point], c: Color, width: f64, alpha: f64) {
    let ctx = Some(cg);
    CGContext::set_rgb_stroke_color(ctx, c.0, c.1, c.2, alpha);
    CGContext::set_line_width(ctx, width);
    CGContext::set_line_cap(ctx, CGLineCap::Round);
    CGContext::set_line_join(ctx, CGLineJoin::Round);
    CGContext::begin_path(ctx);
    if let Some(p) = points.first() {
        CGContext::move_to_point(ctx, p.x, p.y);
    }
    for p in points.iter().skip(1) {
        CGContext::add_line_to_point(ctx, p.x, p.y);
    }
    CGContext::stroke_path(ctx);
}

fn draw_annotation(a: &Annotation, mosaic: &NSImage, bounds: Rect, cg: &CGContext) {
    let ctx = Some(cg);
    let b = a.bounds();
    CGContext::save_g_state(ctx);
    CGContext::set_rgb_stroke_color(ctx, a.color.0, a.color.1, a.color.2, 1.0);
    CGContext::set_line_width(ctx, a.width);
    match a.tool {
        Tool::Rectangle => CGContext::stroke_rect(ctx, nr(b)),
        Tool::Ellipse => {
            CGContext::add_ellipse_in_rect(ctx, nr(b));
            CGContext::stroke_path(ctx);
        }
        Tool::Arrow => {
            line(cg, &a.points, a.color, a.width, 1.0);
            if a.points.len() >= 2 {
                let start = a.points[0];
                let end = *a.points.last().unwrap();
                let angle = (end.y - start.y).atan2(end.x - start.x);
                let length = (a.width * 4.0).max(12.0);
                let left = Point::new(
                    end.x - length * (angle - 0.5).cos(),
                    end.y - length * (angle - 0.5).sin(),
                );
                let right = Point::new(
                    end.x - length * (angle + 0.5).cos(),
                    end.y - length * (angle + 0.5).sin(),
                );
                line(cg, &[left, end, right], a.color, a.width, 1.0);
            }
        }
        Tool::Pen => line(cg, &a.points, a.color, a.width, 1.0),
        Tool::Highlight => line(cg, &a.points, a.color, (a.width * 5.0).max(12.0), 0.32),
        Tool::MosaicRect | Tool::MosaicBrush => {
            if a.tool == Tool::MosaicRect {
                CGContext::clip_to_rect(ctx, nr(b));
            } else {
                CGContext::set_line_width(ctx, (a.width * 8.0).max(18.0));
                CGContext::set_line_cap(ctx, CGLineCap::Round);
                CGContext::set_line_join(ctx, CGLineJoin::Round);
                CGContext::begin_path(ctx);
                let p = a.points[0];
                CGContext::move_to_point(ctx, p.x, p.y);
                for p in a.points.iter().skip(1) {
                    CGContext::add_line_to_point(ctx, p.x, p.y);
                }
                CGContext::replace_path_with_stroked_path(ctx);
                CGContext::clip(ctx);
            }
            draw_image(mosaic, bounds);
        }
        Tool::Text => text(&a.text, Point::new(b.x, b.y), a.font_size, a.color),
        Tool::Tag => {
            let anchor = a.points[0];
            let label_w = (a.text.chars().count() as f64 * a.font_size * 0.9 + 24.0).max(60.0);
            let label_h =
                (a.text.lines().count().max(1) as f64 * a.font_size * 1.3 + 16.0).max(36.0);
            let x = if a.flipped {
                anchor.x + 16.0
            } else {
                anchor.x - label_w - 16.0
            };
            let y = anchor.y - label_h / 2.0;
            rounded(
                Rect::new(x, y, label_w, label_h),
                5.0,
                Color(0.27, 0.29, 0.33),
                1.0,
            );
            CGContext::set_rgb_fill_color(ctx, 0.27, 0.29, 0.33, 1.0);
            CGContext::begin_path(ctx);
            let edge = if a.flipped { x } else { x + label_w };
            CGContext::move_to_point(ctx, edge, anchor.y - 7.0);
            CGContext::add_line_to_point(ctx, anchor.x, anchor.y);
            CGContext::add_line_to_point(ctx, edge, anchor.y + 7.0);
            CGContext::close_path(ctx);
            CGContext::fill_path(ctx);
            text(
                &a.text,
                Point::new(x + 12.0, y + 8.0),
                a.font_size,
                Color(1.0, 1.0, 1.0),
            );
            CGContext::set_rgb_fill_color(ctx, a.color.0, a.color.1, a.color.2, 1.0);
            CGContext::fill_ellipse_in_rect(
                ctx,
                nr(Rect::new(anchor.x - 4.0, anchor.y - 4.0, 8.0, 8.0)),
            );
        }
        Tool::Watermark => {
            CGContext::set_alpha(ctx, a.opacity);
            let font = a.font_size.max(16.0);
            let gap_x = (a.text.chars().count() as f64 * font + 100.0).max(180.0);
            let gap_y = font * 5.0;
            let mut y = bounds.y + 30.0;
            while y < bounds.y + bounds.h {
                let mut x = bounds.x + 30.0;
                while x < bounds.x + bounds.w {
                    text(&a.text, Point::new(x, y), font, a.color);
                    x += gap_x;
                }
                y += gap_y;
            }
        }
        Tool::Select => {}
    }
    CGContext::restore_g_state(ctx);
}

pub fn export(
    editor: &Editor,
    image: &NSImage,
    mosaic: &NSImage,
    scale: f64,
) -> Result<Vec<u8>, String> {
    let r = editor.selection.ok_or("请先选择截图区域")?;
    let w = (r.w * scale).round().max(1.0) as isize;
    let h = (r.h * scale).round().max(1.0) as isize;
    if w as u64 * h as u64 > 80_000_000 {
        return Err("选区超过 8000 万像素, 请缩小选区".into());
    }
    let bitmap=unsafe {NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
        NSBitmapImageRep::alloc(),std::ptr::null_mut(),w,h,8,4,true,false,NSDeviceRGBColorSpace,0,0)}.ok_or("无法创建截图画布")?;
    let gc = NSGraphicsContext::graphicsContextWithBitmapImageRep(&bitmap)
        .ok_or("无法创建绘图上下文")?;
    NSGraphicsContext::saveGraphicsState_class();
    let cg = gc.CGContext();
    CGContext::translate_ctm(Some(&cg), 0.0, h as f64);
    CGContext::scale_ctm(Some(&cg), scale, -scale);
    CGContext::translate_ctm(Some(&cg), -r.x, -r.y);
    let flipped = NSGraphicsContext::graphicsContextWithCGContext_flipped(&cg, true);
    NSGraphicsContext::setCurrentContext(Some(&flipped));
    draw_image(image, editor.bounds);
    draw_annotations(editor, mosaic, None);
    NSGraphicsContext::restoreGraphicsState_class();
    let data = unsafe {
        bitmap.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new())
    }
    .ok_or("无法导出截图")?;
    Ok(data.to_vec())
}

pub fn demo_pixels() -> RgbaImage {
    let mut image = RgbaImage::from_pixel(1440, 900, image::Rgba([242, 244, 248, 255]));
    for y in 0..900 {
        for x in 0..280 {
            image.put_pixel(x, y, image::Rgba([230, 235, 244, 255]));
        }
    }
    for y in 130..780 {
        for x in 340..1370 {
            image.put_pixel(x, y, image::Rgba([255, 255, 255, 255]));
        }
    }
    image
}

pub fn verify(output: &Path) -> Result<(), String> {
    std::fs::create_dir_all(output).map_err(|e| e.to_string())?;
    let pixels = RgbaImage::from_fn(800, 600, |x, y| {
        image::Rgba([(x / 4) as u8, (y / 3) as u8, 80, 255])
    });
    let size = NSSize::new(400.0, 300.0);
    let source = native_image(&pixels, size)?;
    let mosaic = mosaic_image(&pixels, size)?;
    let mut editor = Editor::new(400.0, 300.0);
    editor.selection = Some(Rect::new(20.0, 30.0, 360.0, 240.0));
    let crop = export(&editor, &source, &mosaic, 2.0)?;
    let result = image::load_from_memory(&crop)
        .map_err(|e| e.to_string())?
        .to_rgba8();
    if result.dimensions() != (720, 480) {
        return Err(format!(
            "Incorrect Retina crop dimensions: {:?}",
            result.dimensions()
        ));
    }
    for (x, y) in [(2, 2), (700, 2), (2, 470), (700, 470)] {
        let got = result.get_pixel(x, y);
        let expected = pixels.get_pixel(x + 40, y + 60);
        if (0..3).any(|i| got[i].abs_diff(expected[i]) > 4) {
            return Err(format!(
                "Incorrect crop orientation or offset at {x},{y}: {got:?} vs {expected:?}"
            ));
        }
    }
    std::fs::write(output.join("retina-crop.png"), crop).map_err(|e| e.to_string())?;
    let demo = demo_pixels();
    let size = NSSize::new(1440.0, 900.0);
    let source = native_image(&demo, size)?;
    let mosaic = mosaic_image(&demo, size)?;
    let mut editor = Editor::new(1440.0, 900.0);
    editor.selection = Some(Rect::new(320.0, 120.0, 1050.0, 660.0));
    for (tool, start, end, color, label) in [
        (
            Tool::Rectangle,
            Point::new(370.0, 180.0),
            Point::new(720.0, 270.0),
            Color(1.0, 0.27, 0.28),
            "",
        ),
        (
            Tool::Ellipse,
            Point::new(760.0, 180.0),
            Point::new(930.0, 270.0),
            Color(0.19, 0.75, 0.31),
            "",
        ),
        (
            Tool::Arrow,
            Point::new(1010.0, 230.0),
            Point::new(900.0, 340.0),
            BLUE,
            "",
        ),
        (
            Tool::Text,
            Point::new(370.0, 330.0),
            Point::new(870.0, 380.0),
            Color(0.12, 0.15, 0.2),
            "原生中文标注 / Native text",
        ),
        (
            Tool::Tag,
            Point::new(1190.0, 420.0),
            Point::new(1230.0, 450.0),
            Color(1.0, 0.77, 0.0),
            "重点内容",
        ),
        (
            Tool::Pen,
            Point::new(370.0, 520.0),
            Point::new(600.0, 560.0),
            BLUE,
            "",
        ),
        (
            Tool::Highlight,
            Point::new(370.0, 390.0),
            Point::new(700.0, 390.0),
            Color(1.0, 0.77, 0.0),
            "",
        ),
        (
            Tool::MosaicRect,
            Point::new(770.0, 510.0),
            Point::new(960.0, 560.0),
            BLUE,
            "",
        ),
        (
            Tool::MosaicBrush,
            Point::new(1040.0, 525.0),
            Point::new(1200.0, 545.0),
            BLUE,
            "",
        ),
    ] {
        let mut annotation = editor.new_annotation(tool, start);
        annotation.points[1] = end;
        annotation.color = color;
        annotation.text = label.into();
        annotation.font_size = 28.0;
        editor.annotations.push(annotation);
    }
    let png = export(&editor, &source, &mosaic, 1.0)?;
    let output_image = image::load_from_memory(&png).map_err(|e| e.to_string())?;
    if output_image.width() != 1050 || output_image.height() != 660 {
        return Err("Annotation export has incorrect dimensions".into());
    }
    std::fs::write(output.join("annotations.png"), png).map_err(|e| e.to_string())?;
    println!(
        "Native render check passed: Retina crop, orientation, CJK text, annotation composition. Artifacts: {}",
        output.display()
    );
    Ok(())
}
