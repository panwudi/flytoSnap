# flytoSnap

Native macOS screenshot application. Rust owns the application and editing state.
Use objc2 bindings for AppKit, Core Graphics, and ScreenCaptureKit.

- Match the Feishu screenshot interaction and visual reference.
- Keep all UI operations on the AppKit main thread.
- Keep platform calls in src/macos. Editing geometry and stitching belong in platform-independent modules.
- Every action must show a visible result or an actionable error.
- OCR is deferred. Do not ship an inactive OCR button.
- Use English half-width punctuation in comments and documentation.
- Validate with cargo test, cargo clippy, and a bundled native UI smoke test.
- Screen recording and Accessibility permissions are granted by the user through macOS.
