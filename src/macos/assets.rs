use super::render;
use crate::model::{Point, Rect};
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
pub fn export_icon(output: &Path) -> Result<(), String> {
    if let Some(source) = objc2_foundation::NSBundle::mainBundle().pathForResource_ofType(
        Some(objc2_foundation::ns_string!("AppIcon")),
        Some(objc2_foundation::ns_string!("png")),
    ) {
        return std::fs::read(source.to_string())
            .and_then(|bytes| std::fs::write(output, bytes))
            .map_err(|e| e.to_string());
    }
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/app-icon-source.png");
    let image = NSImage::initWithContentsOfFile(
        NSImage::alloc(),
        &objc2_foundation::NSString::from_str(&source.to_string_lossy()),
    )
    .ok_or("Could not load the application icon source")?;
    let png = bitmap(1024, |cg| {
        CGContext::save_g_state(Some(cg));
        CGContext::scale_ctm(Some(cg), 0.8, 0.8);
        // A vector silhouette keeps the production icon edge clean at every output size.
        let edge = NSBezierPath::bezierPath();
        edge.moveToPoint(NSPoint::new(220.0, 108.0));
        edge.lineToPoint(NSPoint::new(1046.0, 108.0));
        edge.curveToPoint_controlPoint1_controlPoint2(
            NSPoint::new(1125.0, 190.0),
            NSPoint::new(1098.0, 108.0),
            NSPoint::new(1122.0, 140.0),
        );
        edge.lineToPoint(NSPoint::new(1162.0, 937.0));
        edge.curveToPoint_controlPoint1_controlPoint2(
            NSPoint::new(1147.0, 1047.0),
            NSPoint::new(1164.0, 974.0),
            NSPoint::new(1149.0, 1008.0),
        );
        edge.curveToPoint_controlPoint1_controlPoint2(
            NSPoint::new(1073.0, 1116.0),
            NSPoint::new(1145.0, 1091.0),
            NSPoint::new(1115.0, 1116.0),
        );
        edge.lineToPoint(NSPoint::new(180.0, 1116.0));
        edge.curveToPoint_controlPoint1_controlPoint2(
            NSPoint::new(109.0, 1048.0),
            NSPoint::new(138.0, 1116.0),
            NSPoint::new(111.0, 1090.0),
        );
        edge.curveToPoint_controlPoint1_controlPoint2(
            NSPoint::new(93.0, 937.0),
            NSPoint::new(106.0, 1007.0),
            NSPoint::new(90.0, 977.0),
        );
        edge.lineToPoint(NSPoint::new(131.0, 190.0));
        edge.curveToPoint_controlPoint1_controlPoint2(
            NSPoint::new(220.0, 108.0),
            NSPoint::new(134.0, 139.0),
            NSPoint::new(166.0, 108.0),
        );
        edge.closePath();
        edge.addClip();
        render::draw_image(&image, Rect::new(0.0, 0.0, 1280.0, 1280.0));
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
    let source = objc2_foundation::NSBundle::mainBundle()
        .pathForResource_ofType(
            Some(objc2_foundation::ns_string!("AppIcon")),
            Some(objc2_foundation::ns_string!("png")),
        )
        .unwrap_or_else(|| {
            objc2_foundation::NSString::from_str(
                &Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("assets/app-icon.png")
                    .to_string_lossy(),
            )
        });
    let image = NSImage::initWithContentsOfFile(NSImage::alloc(), &source)
        .ok_or("Could not load the menu bar icon source")?;
    let png = bitmap(64, |_| {
        render::draw_image(&image, Rect::new(0.0, 0.0, 64.0, 64.0))
    })?;
    std::fs::write(output, png).map_err(|e| e.to_string())
}
