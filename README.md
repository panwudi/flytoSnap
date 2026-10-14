# flytoSnap

A native macOS screenshot utility implemented in Rust. The screenshot editor follows the Feishu screenshot workflow and uses AppKit, Core Graphics, and ScreenCaptureKit through objc2.

Requires macOS 14 or later and a Rust toolchain. No OCR model is bundled.

## Build and launch

```sh
bash scripts/build-app.sh
open dist/flytoSnap.app
```

The development build is signed locally. For distribution, replace the local signature with Developer ID signing and notarization.

Use the menu bar scissors icon or Command+Shift+A to start capturing. Grant Screen Recording permission to flytoSnap when macOS asks. If necessary, quit and relaunch after granting permission. The hotkey and save directory can be changed in Settings.

## Editing

- Drag to select a region. Drag its eight handles to resize it.
- Hover a window and click to select its visible area, or drag for a custom region.
- Choose rectangle, ellipse, arrow, pen, highlighter, mosaic, text, tag, or watermark.
- Mosaic supports brush and rectangle modes.
- Text uses the native macOS input system. Click elsewhere to finish editing.
- Double-click an existing text annotation or tag to edit its contents.
- Tags have a colored anchor. Click the anchor to flip their direction.
- Choose the selection tool to move or resize existing annotations.
- Command+Z undoes, Command+Shift+Z redoes, Command+S saves, and Enter copies the screenshot.
- Escape exits the editor, including while entering text. Command+Q quits the application.
- Open an existing PNG, JPEG, or TIFF from the main window or menu bar to annotate it.

## Pinned images

Pinned images float above other windows. Drag to move, scroll to resize, and Option+scroll to change transparency. Right-click for copy, save, lock, reset, and close. The menu bar can hide or show all pinned images.

## Scrolling capture

Choose Scrolling Capture and select only the scrolling content. After selection, scroll slowly; the preview updates as frames are stitched. Use automatic scrolling after granting Accessibility permission, or scroll manually without that permission. Finish to open the long image in the annotation editor.

Avoid selecting fixed headers, scrollbars, animated content, or video. If overlap detection fails, the current image remains intact and the preview asks you to scroll back slightly. Captures are limited to 80 million pixels and 32,000 pixels in height.

## Demo and validation

```sh
open dist/flytoSnap.app --args --demo
cargo test
cargo clippy --all-targets -- -D warnings
dist/flytoSnap.app/Contents/MacOS/flytosnap --render-check target/native-check
```

The demo editor uses a generated image and requires no Screen Recording permission. The render check does not open windows. It verifies Retina cropping and writes native annotation rendering samples to the selected directory. OCR is planned for the next milestone using Apple Vision.

This is an initial local build. Automated checks cover editor geometry, undo, shortcut parsing, bidirectional scrolling, native image composition, and Retina cropping. End-to-end Screen Recording, Accessibility, and multi-monitor behavior still need manual verification on the target Mac.

## References

- [Feishu screenshot features](https://www.feishu.cn/hc/zh-CN/articles/319286513754)
- [Feishu scrolling capture](https://www.feishu.cn/hc/zh-CN/articles/145659580677)
- [objc2 framework bindings](https://github.com/madsmtm/objc2)

## License

MIT. See [LICENSE](LICENSE).
