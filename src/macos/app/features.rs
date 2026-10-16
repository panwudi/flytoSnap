use super::super::{media, ocr};
use super::*;

struct ControlPanelIvars {
    offscreen: bool,
}
define_class!(
    #[unsafe(super(NSPanel))]
    #[thread_kind=MainThreadOnly]
    #[ivars=ControlPanelIvars]
    struct ControlPanel;
    impl ControlPanel {
        #[unsafe(method(constrainFrameRect:toScreen:))]
        fn constrain(&self, frame:NSRect, screen:Option<&NSScreen>)->NSRect {
            if self.ivars().offscreen {frame}
            else {unsafe {msg_send![super(self),constrainFrameRect:frame,toScreen:screen]}}
        }
    }
);

fn new_panel(width: f64, height: f64, style: NSWindowStyleMask) -> Retained<NSPanel> {
    let this = ControlPanel::alloc(mtm()).set_ivars(ControlPanelIvars {
        offscreen: state(|a| a.headless),
    });
    let panel: Retained<ControlPanel> = unsafe {
        msg_send![super(this),initWithContentRect:NSRect::new(NSPoint::ZERO,NSSize::new(width,height)),styleMask:style,backing:NSBackingStoreType::Buffered,defer:false]
    };
    panel.into_super()
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind=MainThreadOnly]
    struct PanelContent;
    impl PanelContent {
        #[unsafe(method(drawRect:))]
        fn paint(&self,_rect:NSRect){render::rounded(Rect::new(0.0,0.0,self.bounds().size.width,self.bounds().size.height),0.0,Color(0.97,0.975,0.985),1.0);}
    }
);
define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind=MainThreadOnly]
    struct FloatingPanelContent;
    impl FloatingPanelContent {
        #[unsafe(method(drawRect:))]
        fn paint(&self,_rect:NSRect){render::rounded(Rect::new(0.0,0.0,self.bounds().size.width,self.bounds().size.height),0.0,Color(0.97,0.975,0.985),1.0);}
        #[unsafe(method(resetCursorRects))]
        fn cursor_rects(&self) {self.addCursorRect_cursor(self.bounds(), &NSCursor::arrowCursor());}
        #[unsafe(method(cursorUpdate:))]
        fn cursor_update(&self,_event:&NSEvent) {NSCursor::arrowCursor().set();}
        #[unsafe(method(mouseEntered:))]
        fn entered(&self,_event:&NSEvent) {NSCursor::arrowCursor().set();}
        #[unsafe(method(mouseMoved:))]
        fn moved(&self,_event:&NSEvent) {NSCursor::arrowCursor().set();}
    }
);

fn panel(title: &str, width: f64, height: f64) -> Retained<NSPanel> {
    let panel = new_panel(
        width,
        height,
        NSWindowStyleMask::Titled | NSWindowStyleMask::Closable,
    );
    unsafe {
        panel.setReleasedWhenClosed(false);
    }
    panel.setTitle(&ns_ui(title));
    panel.setAcceptsMouseMovedEvents(true);
    panel.setFloatingPanel(true);
    // setFloatingPanel resets the level to NSFloatingWindowLevel.
    panel.setLevel(CAPTURE_CONTROL_LEVEL);
    panel.setHidesOnDeactivate(false);
    panel.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary,
    );
    panel.center();
    let appearance = NSAppearance::appearanceNamed(unsafe { NSAppearanceNameAqua });
    panel.setAppearance(appearance.as_deref());
    panel.setBackgroundColor(Some(NSColor::whiteColor().as_ref()));
    let root: Retained<PanelContent> = unsafe {
        msg_send![PanelContent::alloc(mtm()),initWithFrame:NSRect::new(NSPoint::ZERO,NSSize::new(width,height))]
    };
    panel.setContentView(Some(&root));
    let delegate = state(|a| a.delegate.clone());
    panel.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    panel
}
fn present(panel: &NSPanel) {
    if !state(|a| a.headless) {
        panel.makeKeyAndOrderFront(None);
        #[allow(deprecated)]
        NSApplication::sharedApplication(mtm()).activateIgnoringOtherApps(true);
    }
}
pub(super) fn floating_panel(width: f64, height: f64) -> Retained<NSPanel> {
    let panel = new_panel(
        width,
        height,
        NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel,
    );
    unsafe {
        panel.setReleasedWhenClosed(false);
    }
    panel.setAcceptsMouseMovedEvents(true);
    panel.setFloatingPanel(true);
    panel.setLevel(CAPTURE_CONTROL_LEVEL);
    panel.setHidesOnDeactivate(false);
    panel.setOpaque(false);
    panel.setHasShadow(true);
    panel.setBackgroundColor(Some(NSColor::clearColor().as_ref()));
    panel.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary,
    );
    panel.setAppearance(NSAppearance::appearanceNamed(unsafe { NSAppearanceNameAqua }).as_deref());
    let root: Retained<FloatingPanelContent> = unsafe {
        msg_send![FloatingPanelContent::alloc(mtm()),initWithFrame:NSRect::new(NSPoint::ZERO,NSSize::new(width,height))]
    };
    track_pointer(&root);
    panel.setContentView(Some(&root));
    panel
}
pub(super) fn present_floating(panel: &NSPanel) {
    if !state(|a| a.headless) {
        panel.orderFrontRegardless();
    }
}
fn control(root: &NSView, title: &str, tag: isize, frame: NSRect) -> Retained<NSButton> {
    let delegate = state(|a| a.delegate.clone());
    let b = button(title, None, tag, &delegate);
    b.setBordered(true);
    b.setBezelStyle(NSBezelStyle::Push);
    b.setFrame(frame);
    set_caption(&b, title, Color(0.14, 0.16, 0.2), 13.0);
    root.addSubview(&b);
    b
}

pub(super) fn dismiss_permission_help() {
    if let Some(help) = state(|a| a.permission_help.take()) {
        help.window.orderOut(None);
    }
}

pub(super) fn show_permission_help() {
    if let Some(window) = state(|a| a.permission_help.as_ref().map(|p| p.window.clone())) {
        present(&window);
        return;
    }
    let window = panel("屏幕录制权限", 660.0, 400.0);
    let root = window.contentView().unwrap();
    let status = label(
        "系统尚未允许当前版本截取屏幕",
        NSRect::new(NSPoint::new(24.0, 328.0), NSSize::new(612.0, 48.0)),
        15.0,
    );
    status.setMaximumNumberOfLines(2);
    status.setLineBreakMode(NSLineBreakMode::ByWordWrapping);
    root.addSubview(&status);
    for (text, y, height) in [
        (
            "1. 打开系统设置, 在屏幕与系统音频录制中允许 flytoSnap.",
            284.0,
            44.0,
        ),
        (
            "2. 若开关已打开但仍失败, 移除旧条目, 再添加当前应用.",
            226.0,
            44.0,
        ),
        (
            "3. 完全退出 flytoSnap 后重新打开, 或点击重新检测.",
            168.0,
            44.0,
        ),
        (
            "升级旧测试版可能需要重新授权. 应用不会自动重置权限.",
            118.0,
            40.0,
        ),
    ] {
        let text = label(
            text,
            NSRect::new(NSPoint::new(24.0, y), NSSize::new(612.0, height)),
            13.0,
        );
        text.setMaximumNumberOfLines(2);
        text.setLineBreakMode(NSLineBreakMode::ByWordWrapping);
        root.addSubview(&text);
    }
    for (title, tag, x, width) in [
        ("打开系统设置", 701, 24.0, 160.0),
        ("显示当前应用", 703, 194.0, 160.0),
        ("重新检测", 702, 364.0, 150.0),
        ("稍后", 704, 524.0, 112.0),
    ] {
        control(
            &root,
            title,
            tag,
            NSRect::new(NSPoint::new(x, 30.0), NSSize::new(width, 36.0)),
        );
    }
    state(|a| {
        a.home_status
            .setStringValue(&ns_ui("系统尚未允许当前版本截取屏幕"));
        a.permission_help = Some(PermissionHelp {
            window: window.clone(),
            status,
        });
    });
    present(&window);
}

fn permission_status(message: &str) {
    state(|a| {
        if let Some(help) = &a.permission_help {
            help.status.setStringValue(&ns_ui(message));
        }
    });
}

pub(super) fn open_screen_permission_settings() {
    let url = NSURL::URLWithString(ns_string!(
        "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture"
    ));
    if url.is_some_and(|url| NSWorkspace::sharedWorkspace().openURL(&url)) {
        permission_status("系统设置已打开, 更改权限后请重新检测或重启应用");
    } else {
        permission_status("无法打开系统设置, 请从苹果菜单手动打开");
    }
}

pub(super) fn reveal_current_app() {
    let url = NSBundle::mainBundle().bundleURL();
    NSWorkspace::sharedWorkspace().activateFileViewerSelectingURLs(&NSArray::from_slice(&[&*url]));
    permission_status("已在 Finder 中显示当前应用, 请将它添加到权限列表");
}

pub(super) fn retry_screen_permission() {
    if unsafe { capture::CGPreflightScreenCaptureAccess() } {
        let mode = state(|a| a.mode);
        dismiss_permission_help();
        toast("屏幕录制权限已就绪");
        start_capture_mode(mode);
    } else {
        permission_status("权限仍未生效, 请移除旧条目并重新添加, 然后退出再打开");
    }
}

pub(super) fn start_capture_mode(mode: CaptureMode) {
    if state(|a| a.recording.is_some()) {
        toast("正在录屏, 请先停止当前录制");
        return;
    }
    start_capture(mode == CaptureMode::Scroll);
    state(|a| a.mode = mode);
}
pub(super) fn switch_mode(mode: CaptureMode) {
    if state(|a| a.recording.is_some()) {
        toast("正在录屏, 请先停止当前录制");
        return;
    }
    let index = state(|a| a.active);
    if state(|a| index >= a.sessions.len()) {
        return;
    }
    commit_text(index);
    state(|a| {
        a.mode = mode;
        a.prefer_scroll = mode == CaptureMode::Scroll;
        a.feature_job += 1;
        if let Some(panel) = a.ocr_panel.take() {
            panel.window.orderOut(None);
        }
        if let Some(setup) = a.record_setup.take() {
            setup.window.orderOut(None);
        }
    });
    let count = state(|a| a.sessions.len());
    for i in 0..count {
        refresh(i);
    }
    if state(|a| a.sessions[index].editor.selection.is_some()) {
        match mode {
            CaptureMode::Scroll => {}
            CaptureMode::Record => show_record_setup(index),
            CaptureMode::Text => begin_ocr(index),
            _ => {}
        }
    }
}
pub(super) fn begin_ocr(index: usize) {
    if !state(|a| {
        a.sessions
            .get(index)
            .is_some_and(|s| s.editor.selection.is_some())
    }) {
        toast("框选需要提取文字的区域. Esc 取消");
        return;
    }
    commit_text(index);
    let png = state(|a| {
        let s = &a.sessions[index];
        render::export(&s.editor, &s.image, &s.mosaic, s.screen.scale)
    });
    let png = match png {
        Ok(png) => png,
        Err(error) => {
            toast(&error);
            return;
        }
    };
    let window = panel("提取文字", 560.0, 390.0);
    let frame = state(|a| a.sessions[index].screen.frame);
    window.setFrameOrigin(NSPoint::new(
        frame.origin.x + (frame.size.width - window.frame().size.width) / 2.0,
        frame.origin.y + (frame.size.height - window.frame().size.height) / 2.0,
    ));
    let root = window.contentView().unwrap();
    let status = label(
        "正在识别文字...",
        NSRect::new(NSPoint::new(20.0, 344.0), NSSize::new(520.0, 28.0)),
        14.0,
    );
    root.addSubview(&status);
    let scroll = NSScrollView::initWithFrame(
        NSScrollView::alloc(mtm()),
        NSRect::new(NSPoint::new(20.0, 68.0), NSSize::new(520.0, 270.0)),
    );
    scroll.setHasVerticalScroller(true);
    let text: Retained<TextInput> = unsafe {
        msg_send![TextInput::alloc(mtm()),initWithFrame:NSRect::new(NSPoint::ZERO, NSSize::new(500.0, 270.0))]
    };
    let text = text.into_super();
    text.setRichText(false);
    text.setEditable(false);
    text.setAllowsUndo(true);
    text.setFont(Some(&NSFont::systemFontOfSize(16.0)));
    text.setTextContainerInset(NSSize::new(12.0, 12.0));
    text.setVerticallyResizable(true);
    text.setHorizontallyResizable(false);
    if let Some(container) = unsafe { text.textContainer() } {
        container.setWidthTracksTextView(true);
        container.setContainerSize(NSSize::new(500.0, f64::MAX));
    }
    text.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
    scroll.setDocumentView(Some(&text));
    root.addSubview(&scroll);
    let copy = control(
        &root,
        "复制文字",
        501,
        NSRect::new(NSPoint::new(400.0, 18.0), NSSize::new(140.0, 34.0)),
    );
    copy.setEnabled(false);
    control(
        &root,
        "重新选择",
        502,
        NSRect::new(NSPoint::new(20.0, 18.0), NSSize::new(140.0, 34.0)),
    );
    control(
        &root,
        "取消 (Esc)",
        503,
        NSRect::new(NSPoint::new(172.0, 18.0), NSSize::new(120.0, 34.0)),
    );
    let (job, tx) = state(|a| {
        if let Some(old) = a.ocr_panel.take() {
            old.window.orderOut(None);
        }
        a.feature_job += 1;
        a.ocr_panel = Some(OcrPanel {
            window: window.clone(),
            text,
            status,
            copy,
        });
        (a.feature_job, a.ocr_tx.clone())
    });
    present(&window);
    std::thread::spawn(move || {
        let result = ocr::recognize(&png);
        let _ = tx.send((job, result));
    });
}
pub(super) fn copy_ocr() {
    match copy_ocr_to(&NSPasteboard::generalPasteboard()) {
        Ok(()) => {
            state(|a| {
                if let Some(panel) = &a.ocr_panel {
                    panel.status.setStringValue(&ns_ui("已复制文字"));
                }
            });
            toast("已复制文字");
        }
        Err(error) => toast(error),
    }
}

pub(super) fn copy_ocr_to(board: &NSPasteboard) -> Result<(), &'static str> {
    let text = state(|a| {
        a.ocr_panel
            .as_ref()
            .filter(|p| p.copy.isEnabled())
            .map(|p| p.text.string().to_string())
    })
    .ok_or("正在识别文字...")?;
    if text.trim().is_empty() {
        return Err("没有可复制的文字");
    }
    board.clearContents();
    if board.setString_forType(&NSString::from_str(&text), unsafe {
        NSPasteboardTypeString
    }) {
        Ok(())
    } else {
        Err("复制失败, 请重试")
    }
}

pub(super) fn reselect_ocr() {
    let count = state(|a| {
        a.feature_job += 1;
        if let Some(panel) = a.ocr_panel.take() {
            panel.window.orderOut(None);
        }
        for s in &mut a.sessions {
            s.editor.selection = None;
            s.editor.choose(Tool::Select);
        }
        a.sessions.len()
    });
    for index in 0..count {
        refresh(index);
    }
    if !state(|a| a.headless) {
        if let Some(window) = state(|a| a.sessions.get(a.active).map(|s| s.window.clone())) {
            window.makeKeyAndOrderFront(None);
        }
    }
}

pub(super) fn show_record_setup(index: usize) {
    if state(|a| index >= a.sessions.len()) {
        return;
    }
    if state(|a| a.record_setup.is_some() || a.recording.is_some()) {
        return;
    }
    if state(|a| a.sessions[index].demo) {
        toast("请先选择录屏区域");
        return;
    }
    let Some(region) = state(|a| a.sessions[index].editor.selection) else {
        toast("请先选择录屏区域");
        return;
    };
    let window = panel("录屏设置", 440.0, 250.0);
    let frame = state(|a| a.sessions[index].screen.frame);
    window.setFrameOrigin(NSPoint::new(
        frame.origin.x + (frame.size.width - window.frame().size.width) / 2.0,
        frame.origin.y + (frame.size.height - window.frame().size.height) / 2.0,
    ));
    let root = window.contentView().unwrap();
    root.addSubview(&label(
        "格式",
        NSRect::new(NSPoint::new(24.0, 192.0), NSSize::new(80.0, 28.0)),
        14.0,
    ));
    let mp4 = control(
        &root,
        "MP4",
        602,
        NSRect::new(NSPoint::new(105.0, 185.0), NSSize::new(140.0, 36.0)),
    );
    let gif = control(
        &root,
        "GIF",
        603,
        NSRect::new(NSPoint::new(256.0, 185.0), NSSize::new(140.0, 36.0)),
    );
    mp4.setButtonType(NSButtonType::PushOnPushOff);
    gif.setButtonType(NSButtonType::PushOnPushOff);
    mp4.setState(1);
    gif.setState(0);
    let cursor = control(
        &root,
        "显示鼠标",
        0,
        NSRect::new(NSPoint::new(24.0, 134.0), NSSize::new(240.0, 32.0)),
    );
    cursor.setButtonType(NSButtonType::Switch);
    cursor.setState(1);
    unsafe {
        cursor.setAction(None);
    }
    root.addSubview(&label(
        &format!("{} × {}", region.w.round() as u32, region.h.round() as u32),
        NSRect::new(NSPoint::new(25.0, 95.0), NSSize::new(360.0, 24.0)),
        12.0,
    ));
    root.addSubview(&label(
        "仅录制屏幕画面",
        NSRect::new(NSPoint::new(25.0, 69.0), NSSize::new(360.0, 24.0)),
        12.0,
    ));
    control(
        &root,
        "开始录制",
        601,
        NSRect::new(NSPoint::new(256.0, 20.0), NSSize::new(160.0, 34.0)),
    );
    control(
        &root,
        "取消",
        604,
        NSRect::new(NSPoint::new(24.0, 20.0), NSSize::new(130.0, 34.0)),
    );
    state(|a| {
        a.record_setup = Some(RecordSetup {
            window: window.clone(),
            index,
            format: media::Format::Mp4,
            mp4,
            gif,
            cursor,
        })
    });
    present(&window);
}
pub(super) fn set_record_format(format: media::Format) {
    state(|a| {
        if let Some(s) = &mut a.record_setup {
            s.format = format;
            s.mp4
                .setState(if format == media::Format::Mp4 { 1 } else { 0 });
            s.gif
                .setState(if format == media::Format::Gif { 1 } else { 0 });
        }
    });
}
pub(super) fn cancel_record_setup() {
    if state(|a| a.record_setup.is_some()) {
        close_sessions();
        toast("录屏已取消");
    }
}
pub(super) fn begin_recording() {
    let data = state(|a| {
        let setup = a.record_setup.as_ref()?;
        let s = a.sessions.get(setup.index)?;
        let region = s.editor.selection?;
        let scale = s.screen.scale;
        let downscale = (3840.0 / (region.w * scale).max(region.h * scale)).min(1.0);
        Some((
            setup.index,
            s.screen.id,
            region,
            ((region.w * scale * downscale).floor() as usize) & !1,
            ((region.h * scale * downscale).floor() as usize) & !1,
            setup.format,
            setup.cursor.state() == 1,
        ))
    });
    let Some((index, id, region, width, height, format, cursor)) = data else {
        toast("请先选择录屏区域");
        return;
    };
    if width < 16 || height < 16 {
        toast("区域过小, 请扩大选区");
        return;
    }
    let (job, tx) = state(|a| {
        a.feature_job += 1;
        (a.feature_job, a.record_tx.clone())
    });
    let handle = match media::start(
        media::Options {
            display_id: id,
            region: nr(region),
            width,
            height,
            format,
            shows_cursor: cursor,
        },
        job,
        tx,
    ) {
        Ok(handle) => handle,
        Err(error) => {
            toast(&format!("录屏失败: {error}"));
            return;
        }
    };
    let window = panel("录屏", 420.0, 112.0);
    let root = window.contentView().unwrap();
    let status = label(
        "正在开始录屏...",
        NSRect::new(NSPoint::new(18.0, 68.0), NSSize::new(380.0, 26.0)),
        15.0,
    );
    root.addSubview(&status);
    control(
        &root,
        "停止录制",
        605,
        NSRect::new(NSPoint::new(245.0, 16.0), NSSize::new(155.0, 34.0)),
    );
    control(
        &root,
        "取消",
        606,
        NSRect::new(NSPoint::new(18.0, 16.0), NSSize::new(120.0, 34.0)),
    );
    let frame = state(|a| a.sessions[index].screen.frame);
    window.setFrameOrigin(NSPoint::new(
        frame.origin.x + frame.size.width - window.frame().size.width - 18.0,
        frame.origin.y + 40.0,
    ));
    state(|a| {
        if let Some(setup) = a.record_setup.take() {
            setup.window.orderOut(None);
        }
        a.recording = Some(Recording {
            handle,
            window: window.clone(),
            status,
            job,
            started: None,
            finishing: false,
        });
    });
    pass_through_capture(index);
    native_cursor(CursorIntent::Arrow).set();
    refresh(index);
    present(&window);
}
pub(super) fn stop_recording(cancelled: bool) {
    let handle = state(|a| {
        let record = a.recording.as_mut()?;
        if record.finishing {
            return None;
        }
        record.finishing = true;
        record.status.setStringValue(&ns_ui("正在完成录屏..."));
        Some(record.handle.clone())
    });
    if let Some(handle) = handle {
        handle.stop(cancelled);
    }
}
fn show_result(file: crate::temporary::RecordingFile, format: media::Format) {
    let window = panel("录屏完成", 540.0, 150.0);
    let root = window.contentView().unwrap();
    root.addSubview(&label(
        "可预览, 复制或保存",
        NSRect::new(NSPoint::new(20.0, 92.0), NSSize::new(500.0, 28.0)),
        16.0,
    ));
    control(
        &root,
        "播放预览",
        609,
        NSRect::new(NSPoint::new(20.0, 24.0), NSSize::new(140.0, 34.0)),
    );
    control(
        &root,
        "保存录屏...",
        607,
        NSRect::new(NSPoint::new(174.0, 24.0), NSSize::new(158.0, 34.0)),
    );
    control(
        &root,
        "复制录屏",
        608,
        NSRect::new(NSPoint::new(346.0, 24.0), NSSize::new(174.0, 34.0)),
    );
    let old = state(|a| {
        a.media_result.replace(MediaResult {
            window: window.clone(),
            file,
            format,
        })
    });
    if let Some(old) = old {
        old.window.orderOut(None);
    }
    present(&window);
}
pub(super) fn copy_recording() {
    let board = NSPasteboard::generalPasteboard();
    copy_recording_to(&board);
}
fn copy_recording_to(board: &NSPasteboard) {
    let data = state(|a| a.media_result.as_ref().map(|r| (r.file.clone(), r.format)));
    if let Some((file, format)) = data {
        let url = NSURL::fileURLWithPath(&NSString::from_str(&file.path().to_string_lossy()));
        board.clearContents();
        let ok = url.absoluteString().is_some_and(|value| {
            board.setString_forType(&value, unsafe { NSPasteboardTypeFileURL })
        });
        if format == media::Format::Gif {
            if let Ok(bytes) = std::fs::read(file.path()) {
                board.setData_forType(
                    Some(&NSData::with_bytes(&bytes)),
                    &NSString::from_str("com.compuserve.gif"),
                );
            }
        }
        if ok {
            let change_count = board.changeCount();
            let remembered = file.remember_clipboard(change_count);
            state(|a| a.clipboard_recording = Some(ClipboardRecording { file, change_count }));
            if remembered.is_ok() {
                toast("已复制录屏, 可粘贴到支持文件的应用");
            } else {
                toast("复制失败, 请重试");
            }
        } else {
            toast("复制失败, 请重试");
        }
    }
}
pub(super) fn play_recording() {
    if let Some(file) = state(|a| a.media_result.as_ref().map(|r| r.file.clone())) {
        let url = NSURL::fileURLWithPath(&NSString::from_str(&file.path().to_string_lossy()));
        if !NSWorkspace::sharedWorkspace().openURL(&url) {
            toast("无法打开录屏预览");
        }
    }
}
pub(super) fn save_recording() {
    let data = state(|a| a.media_result.as_ref().map(|r| (r.file.clone(), r.format)));
    let Some((source, format)) = data else {
        return;
    };
    let save = NSSavePanel::savePanel(mtm());
    save.setLevel(28);
    save.setCanCreateDirectories(true);
    save.setNameFieldStringValue(&NSString::from_str(&format!(
        "flytoSnap-{}.{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        format.extension()
    )));
    let directory = state(|a| a.settings.save_directory.clone());
    save.setDirectoryURL(Some(&NSURL::fileURLWithPath(&NSString::from_str(
        &directory,
    ))));
    if save.runModal() != NSModalResponseOK {
        toast("已取消保存录屏");
        return;
    }
    if let Some(path) = save
        .URL()
        .and_then(|u| u.path())
        .map(|p| PathBuf::from(p.to_string()))
    {
        match std::fs::copy(source.path(), &path) {
            Ok(_) => {
                state(|a| a.last_file = Some(path.clone()));
                toast("录屏已保存");
            }
            Err(error) => toast(&format!("保存失败: {error}")),
        }
    }
}
pub(super) fn restore_recording_clipboard() {
    let change_count = NSPasteboard::generalPasteboard().changeCount();
    let file = crate::temporary::restore_clipboard(change_count);
    crate::temporary::clean_abandoned(file.as_ref().map(|file| file.path()));
    state(|a| a.clipboard_recording = file.map(|file| ClipboardRecording { file, change_count }));
}

fn release_replaced_clipboard(board: &NSPasteboard) {
    let change_count = board.changeCount();
    let old = state(|a| {
        if a.clipboard_recording
            .as_ref()
            .is_some_and(|lease| lease.change_count != change_count)
        {
            a.clipboard_recording.take()
        } else {
            None
        }
    });
    if let Some(old) = old {
        old.file.forget_clipboard();
    }
}

pub(super) fn dispose_recording_results() {
    let (result, clipboard) = state(|a| (a.media_result.take(), a.clipboard_recording.take()));
    if let Some(clipboard) = clipboard {
        if clipboard.change_count == NSPasteboard::generalPasteboard().changeCount() {
            clipboard.file.keep_for_paste();
        } else {
            clipboard.file.forget_clipboard();
        }
    }
    drop(result);
}

pub(super) fn verify_recording_result(directory: &std::path::Path) -> Result<(), String> {
    let root = std::fs::canonicalize(directory)
        .map_err(|e| e.to_string())?
        .join("temporary-recordings");
    let file =
        crate::temporary::RecordingFile::create_in(&root, "mp4").map_err(|e| e.to_string())?;
    std::fs::write(file.path(), b"recording preview fixture").map_err(|e| e.to_string())?;
    let path = file.path().to_owned();
    let exported = directory.join("saved-recording-test.mp4");
    std::fs::copy(&path, &exported).map_err(|e| e.to_string())?;
    show_result(file, media::Format::Mp4);
    let window = state(|a| a.media_result.as_ref().unwrap().window.clone());
    let content = window.contentView().unwrap();
    if content
        .subviews()
        .iter()
        .filter_map(|view| view.clone().downcast::<NSTextField>().ok())
        .any(|text| {
            let text = text.stringValue().to_string();
            text.contains("Recording.mp4") || text.contains(&path.to_string_lossy().to_string())
        })
    {
        return Err("Recording result exposed its temporary filename".into());
    }
    snapshot_view(&content, &directory.join("recording-result.png"))?;
    let board = NSPasteboard::pasteboardWithUniqueName();
    copy_recording_to(&board);
    window.close();
    if state(|a| a.media_result.is_some()) || !path.exists() {
        return Err("Closing the result invalidated a recording still on the clipboard".into());
    }
    board.clearContents();
    release_replaced_clipboard(&board);
    if path.exists() || !exported.exists() {
        return Err("Temporary cleanup failed or removed the explicit export".into());
    }
    let file =
        crate::temporary::RecordingFile::create_in(&root, "gif").map_err(|e| e.to_string())?;
    std::fs::write(file.path(), b"GIF89a").map_err(|e| e.to_string())?;
    let path = file.path().to_owned();
    show_result(file, media::Format::Gif);
    let window = state(|a| a.media_result.as_ref().unwrap().window.clone());
    window.close();
    if path.exists() {
        return Err("Closing an uncopied recording did not remove its temporary file".into());
    }
    println!(
        "Recording result check passed: no filename, close cleanup, clipboard lifetime, explicit export preservation."
    );
    Ok(())
}

pub(super) fn feature_tick() {
    if !state(|a| a.headless) {
        release_replaced_clipboard(&NSPasteboard::generalPasteboard());
    }
    state(|a| {
        if let Some(menu) = a._status_item.menu(mtm()) {
            if let Some(stop) = menu.itemWithTag(605) {
                stop.setEnabled(a.recording.as_ref().is_some_and(|r| !r.finishing));
            }
        }
    });
    let ocr = state(|a| a.ocr_rx.try_iter().collect::<Vec<_>>());
    for (job, result) in ocr {
        state(|a| {
            if job != a.feature_job {
                return;
            }
            let Some(panel) = &a.ocr_panel else {
                return;
            };
            match result {
                Ok(text) => {
                    if text.trim().is_empty() {
                        panel
                            .status
                            .setStringValue(&ns_ui("未发现文字, 请重选区域"));
                        panel.copy.setEnabled(false);
                    } else {
                        panel.text.setString(&NSString::from_str(&text));
                        panel.text.setEditable(true);
                        panel.status.setStringValue(&ns_ui(&format!(
                            "已识别 {} 个字符, 可直接编辑",
                            text.chars().filter(|c| !c.is_whitespace()).count()
                        )));
                        panel.copy.setEnabled(true);
                    }
                }
                Err(error) => {
                    panel
                        .status
                        .setStringValue(&ns_ui(&format!("文字识别失败: {error}")));
                    panel.copy.setEnabled(false);
                }
            }
        });
    }
    let events = state(|a| a.record_rx.try_iter().collect::<Vec<_>>());
    for event in events {
        match event {
            media::Event::Started(job) => state(|a| {
                if let Some(r) = &mut a.recording {
                    if r.job == job {
                        r.started = Some(Instant::now());
                    }
                }
            }),
            media::Event::Finished {
                job,
                file,
                format,
                cancelled,
            } => {
                let old = state(|a| {
                    if a.recording.as_ref().is_some_and(|r| r.job == job) {
                        a.recording.take()
                    } else {
                        None
                    }
                });
                if let Some(old) = old {
                    old.window.orderOut(None);
                    close_sessions();
                    if cancelled {
                        toast("录屏已取消");
                    } else {
                        show_result(file, format);
                    }
                }
            }
            media::Event::Failed(job, error) => {
                let old = state(|a| {
                    if a.recording.as_ref().is_some_and(|r| r.job == job) {
                        a.recording.take()
                    } else {
                        None
                    }
                });
                if let Some(old) = old {
                    old.window.orderOut(None);
                    old.handle.stop(true);
                    close_sessions();
                    toast(&format!("录屏失败: {error}"));
                }
            }
        }
    }
    state(|a| {
        if let Some(record) = &a.recording {
            if !record.finishing {
                if let Some(started) = record.started {
                    let seconds = started.elapsed().as_secs();
                    let frames = record
                        .handle
                        .frames
                        .load(std::sync::atomic::Ordering::Relaxed);
                    record.status.setStringValue(&ns_ui(&format!(
                        "正在录制 {:02}:{:02}  ({frames})",
                        seconds / 60,
                        seconds % 60
                    )));
                }
            }
        }
    });
}
