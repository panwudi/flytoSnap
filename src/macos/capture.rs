use crate::model::Rect;
use block2::RcBlock;
use image::RgbaImage;
use objc2::{
    AnyThread,
    rc::{Retained, autoreleasepool},
};
use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep};
use objc2_core_graphics::CGImage;
use objc2_foundation::{NSArray, NSDictionary, NSError, NSNumber, NSString, ns_string};
use objc2_screen_capture_kit::{
    SCContentFilter, SCScreenshotManager, SCShareableContent, SCStreamConfiguration,
};
use std::sync::mpsc::Sender;

#[derive(Clone, Copy)]
pub struct ScreenRequest {
    pub id: u32,
    pub width: usize,
    pub height: usize,
}

pub struct CaptureMessage {
    pub generation: u64,
    pub id: u32,
    pub pixels: Result<RgbaImage, String>,
    pub windows: Vec<Rect>,
}

pub fn capture(screens: Vec<ScreenRequest>, generation: u64, tx: Sender<CaptureMessage>) {
    let handler = RcBlock::new(
        move |content: *mut SCShareableContent, error: *mut NSError| {
            autoreleasepool(|_| unsafe {
                let Some(content) = content.as_ref() else {
                    let detail = error
                        .as_ref()
                        .map(|e| e.localizedDescription().to_string())
                        .unwrap_or_else(|| "ScreenCaptureKit returned no content".into());
                    for screen in &screens {
                        let _ = tx.send(CaptureMessage {
                            generation,
                            id: screen.id,
                            pixels: Err(detail.clone()),
                            windows: vec![],
                        });
                    }
                    return;
                };
                let excluded: Vec<_> = content
                    .applications()
                    .iter()
                    .filter(|a| a.processID() == std::process::id() as i32)
                    .collect();
                let excluded = NSArray::from_retained_slice(&excluded);
                let displays = content.displays();
                for screen in &screens {
                    let Some(display) = displays.iter().find(|d| d.displayID() == screen.id) else {
                        let _ = tx.send(CaptureMessage {
                            generation,
                            id: screen.id,
                            pixels: Err("The display was disconnected".into()),
                            windows: vec![],
                        });
                        continue;
                    };
                    let filter =
                        SCContentFilter::initWithDisplay_excludingApplications_exceptingWindows(
                            SCContentFilter::alloc(),
                            &display,
                            &excluded,
                            &NSArray::new(),
                        );
                    let config = SCStreamConfiguration::new();
                    config.setWidth(screen.width);
                    config.setHeight(screen.height);
                    config.setShowsCursor(false);
                    config.setColorSpaceName(objc2_core_graphics::kCGColorSpaceSRGB);
                    let tx = tx.clone();
                    let id = screen.id;
                    let windows = window_frames(display.frame());
                    let complete = RcBlock::new(move |image: *mut CGImage, error: *mut NSError| {
                        let pixels = autoreleasepool(|_| {
                            let Some(image) = image.as_ref() else {
                                return Err(error
                                    .as_ref()
                                    .map(|e| e.localizedDescription().to_string())
                                    .unwrap_or_else(|| "Could not capture this display".into()));
                            };
                            let bitmap =
                                NSBitmapImageRep::initWithCGImage(NSBitmapImageRep::alloc(), image);
                            let data = bitmap
                                .representationUsingType_properties(
                                    NSBitmapImageFileType::PNG,
                                    &NSDictionary::new(),
                                )
                                .ok_or("Could not encode the screenshot")?;
                            image::load_from_memory(&data.to_vec())
                                .map(|i| i.to_rgba8())
                                .map_err(|e| e.to_string())
                        });
                        let _ = tx.send(CaptureMessage {
                            generation,
                            id,
                            pixels,
                            windows: windows.clone(),
                        });
                    });
                    SCScreenshotManager::captureImageWithFilter_configuration_completionHandler(
                        &filter,
                        &config,
                        Some(&complete),
                    );
                }
            });
        },
    );
    unsafe {
        SCShareableContent::getShareableContentExcludingDesktopWindows_onScreenWindowsOnly_completionHandler(false,true,&handler);
    }
}

fn window_frames(display: objc2_core_foundation::CGRect) -> Vec<Rect> {
    use objc2_core_foundation::CFRetained;
    use objc2_core_graphics::{CGWindowListCopyWindowInfo, CGWindowListOption};
    let Some(info) = CGWindowListCopyWindowInfo(
        CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements,
        0,
    ) else {
        return vec![];
    };
    // Core Graphics returns an immutable CFArray of CFDictionary objects. Both are toll-free bridged.
    let array: Retained<NSArray<NSDictionary>> =
        unsafe { Retained::retain(CFRetained::as_ptr(&info).as_ptr().cast()) }.unwrap();
    let number = |dict: &NSDictionary, key: &NSString| {
        dict.objectForKey(key)
            .and_then(|v| v.downcast::<NSNumber>().ok())
            .map(|n| n.doubleValue())
    };
    array
        .iter()
        .filter_map(|window| {
            if number(&window, ns_string!("kCGWindowLayer"))? != 0.0
                || number(&window, ns_string!("kCGWindowOwnerPID"))? == std::process::id() as f64
            {
                return None;
            }
            let bounds = window
                .objectForKey(ns_string!("kCGWindowBounds"))?
                .downcast::<NSDictionary>()
                .ok()?;
            let rect = Rect::new(
                number(&bounds, ns_string!("X"))? - display.origin.x,
                number(&bounds, ns_string!("Y"))? - display.origin.y,
                number(&bounds, ns_string!("Width"))?,
                number(&bounds, ns_string!("Height"))?,
            )
            .clamp(Rect::new(0.0, 0.0, display.size.width, display.size.height));
            (rect.w >= 40.0 && rect.h >= 24.0).then_some(rect)
        })
        .collect()
}

pub fn verify_window_metadata() -> Result<usize, String> {
    let display = objc2_core_graphics::CGDisplayBounds(objc2_core_graphics::CGMainDisplayID());
    let windows = window_frames(display);
    if windows.iter().any(|r| {
        ![r.x, r.y, r.w, r.h].iter().all(|v| v.is_finite())
            || r.x < 0.0
            || r.y < 0.0
            || r.x + r.w > display.size.width + 1.0
            || r.y + r.h > display.size.height + 1.0
    }) {
        return Err("Invalid display-relative window geometry".into());
    }
    Ok(windows.len())
}

pub struct ApplicationWindow {
    pub id: u32,
    pub level: i32,
    pub bounds: Rect,
}

pub fn application_windows() -> Result<Vec<ApplicationWindow>, String> {
    use objc2_core_foundation::CFRetained;
    use objc2_core_graphics::{CGWindowListCopyWindowInfo, CGWindowListOption};
    let info = CGWindowListCopyWindowInfo(CGWindowListOption::OptionAll, 0)
        .ok_or("WindowServer returned no window list")?;
    let array: Retained<NSArray<NSDictionary>> =
        unsafe { Retained::retain(CFRetained::as_ptr(&info).as_ptr().cast()) }.unwrap();
    let number = |dict: &NSDictionary, key: &NSString| {
        dict.objectForKey(key)?
            .downcast::<NSNumber>()
            .ok()
            .map(|n| n.doubleValue())
    };
    Ok(array
        .iter()
        .filter_map(|window| {
            if number(&window, ns_string!("kCGWindowOwnerPID"))? as u32 != std::process::id() {
                return None;
            }
            let bounds = window
                .objectForKey(ns_string!("kCGWindowBounds"))?
                .downcast::<NSDictionary>()
                .ok()?;
            Some(ApplicationWindow {
                id: number(&window, ns_string!("kCGWindowNumber"))? as u32,
                level: number(&window, ns_string!("kCGWindowLayer"))? as i32,
                bounds: Rect::new(
                    number(&bounds, ns_string!("X"))?,
                    number(&bounds, ns_string!("Y"))?,
                    number(&bounds, ns_string!("Width"))?,
                    number(&bounds, ns_string!("Height"))?,
                ),
            })
        })
        .collect())
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    pub fn CGPreflightScreenCaptureAccess() -> bool;
    pub fn CGRequestScreenCaptureAccess() -> bool;
}

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    pub fn AXIsProcessTrusted() -> bool;
}

pub fn scroll_at(x: f64, y: f64, pixels: i32) {
    use objc2_core_foundation::CGPoint;
    use objc2_core_graphics::{CGEvent, CGEventTapLocation, CGScrollEventUnit};
    if let Some(event) =
        CGEvent::new_scroll_wheel_event2(None, CGScrollEventUnit::Pixel, 1, pixels, 0, 0)
    {
        CGEvent::set_location(Some(&event), CGPoint::new(x, y));
        CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&event));
    }
}
