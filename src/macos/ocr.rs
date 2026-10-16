use objc2::{AnyThread, rc::autoreleasepool};
use objc2_foundation::{NSArray, NSData, NSDictionary, NSString};
use objc2_vision::{VNImageRequestHandler, VNRecognizeTextRequest, VNRequestTextRecognitionLevel};

pub fn recognize(png: &[u8]) -> Result<String, String> {
    autoreleasepool(|_| {
        let request = VNRecognizeTextRequest::new();
        request.setRecognitionLevel(VNRequestTextRecognitionLevel::Accurate);
        request.setUsesLanguageCorrection(true);
        request.setAutomaticallyDetectsLanguage(true);
        request.setRecognitionLanguages(&NSArray::from_retained_slice(&[
            NSString::from_str("zh-Hans"),
            NSString::from_str("zh-Hant"),
            NSString::from_str("en-US"),
        ]));
        request.setCustomWords(&NSArray::from_retained_slice(&[
            NSString::from_str("flytoSnap"),
            NSString::from_str("FLYTO"),
        ]));
        let data = NSData::with_bytes(png);
        let handler = VNImageRequestHandler::initWithData_options(
            VNImageRequestHandler::alloc(),
            &data,
            &NSDictionary::new(),
        );
        handler
            .performRequests_error(&NSArray::from_slice(&[request.as_ref()]))
            .map_err(|e| e.localizedDescription().to_string())?;
        let mut lines = vec![];
        if let Some(results) = request.results() {
            for observation in results {
                if let Some(candidate) = observation.topCandidates(1).firstObject() {
                    let rect = unsafe { observation.boundingBox() };
                    lines.push((rect.origin.y, rect.origin.x, candidate.string().to_string()));
                }
            }
        }
        lines.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.total_cmp(&b.1)));
        Ok(lines
            .into_iter()
            .map(|(_, _, text)| text)
            .collect::<Vec<_>>()
            .join("\n"))
    })
}

pub fn verify() -> Result<(), String> {
    use super::render;
    use crate::model::{Editor, Point, Tool};
    use objc2_foundation::NSSize;
    let pixels = image::RgbaImage::from_pixel(1200, 400, image::Rgba([255, 255, 255, 255]));
    let source = render::native_image(&pixels, NSSize::new(1200.0, 400.0))?;
    let mut editor = Editor::new(1200.0, 400.0);
    editor.selection = Some(editor.bounds);
    let samples = ["flytoSnap 123", "简体中文 文字提取", "繁體中文 擷取文字"];
    for (line, sample) in samples.iter().enumerate() {
        let mut annotation =
            editor.new_annotation(Tool::Text, Point::new(40.0, 30.0 + line as f64 * 110.0));
        annotation.text = (*sample).into();
        annotation.font_size = 64.0;
        annotation.color = crate::model::Color(0.0, 0.0, 0.0);
        editor.annotations.push(annotation);
    }
    let png = render::export(&editor, &source, &source, 1.0)?;
    let text = recognize(&png)?;
    let compact = text.split_whitespace().collect::<String>();
    if samples
        .iter()
        .any(|s| !compact.contains(&s.split_whitespace().collect::<String>()))
    {
        return Err(format!(
            "OCR did not recognize the generated test text: {text:?}"
        ));
    }
    editor.annotations.clear();
    let blank = render::export(&editor, &source, &source, 1.0)?;
    if !recognize(&blank)?.is_empty() || recognize(b"invalid image").is_ok() {
        return Err("OCR blank-image or invalid-image handling failed".into());
    }
    println!(
        "On-device OCR check passed: Simplified Chinese, Traditional Chinese, English, blank and invalid images."
    );
    Ok(())
}
