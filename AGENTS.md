# flytoSnap

Native macOS screenshot application. Rust owns the application and editing state.
Use objc2 bindings for AppKit, Core Graphics, and ScreenCaptureKit.

- Match the Feishu screenshot interaction and visual reference.
- Keep all UI operations on the AppKit main thread.
- Keep platform calls in src/macos. Editing geometry and stitching belong in platform-independent modules.
- Every action must show a visible result or an actionable error.
- Capture, scrolling capture, recording, and OCR share one mode selector.
- OCR uses on-device Apple Vision. Record GIF and MP4 through native capture and encoding APIs.
- Support Simplified Chinese, Traditional Chinese, English, Japanese, Russian, French, Spanish, Portuguese, German, and Korean. Keep translations complete.
- Use the approved B2 ivory viewfinder icon and a light skeuomorphic interface. Keep the menu bar icon a legible template frame mark.
- Start in the menu bar without showing the main window.
- Use English half-width punctuation in comments and documentation.
- Validate with cargo test, cargo clippy, and a bundled native UI smoke test.
- Screen recording and Accessibility permissions are granted by the user through macOS.
