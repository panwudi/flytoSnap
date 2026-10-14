use super::super::{media, ocr};
use super::*;

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind=MainThreadOnly]
    struct PanelContent;
    impl PanelContent {
        #[unsafe(method(drawRect:))]
        fn paint(&self,_rect:NSRect){render::rounded(Rect::new(0.0,0.0,self.bounds().size.width,self.bounds().size.height),0.0,Color(0.97,0.975,0.985),1.0);}
    }
);

fn panel(title: &str, width: f64, height: f64) -> Retained<NSPanel> {
    let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
        NSPanel::alloc(mtm()),
        NSRect::new(NSPoint::ZERO, NSSize::new(width, height)),
        NSWindowStyleMask::Titled | NSWindowStyleMask::Closable,
        NSBackingStoreType::Buffered,
        false,
    );
    unsafe {
        panel.setReleasedWhenClosed(false);
    }
    panel.setTitle(&ns_ui(title));
    panel.setLevel(26);
    panel.setFloatingPanel(true);
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
            CaptureMode::Scroll => start_scroll(index),
            CaptureMode::Record => show_record_setup(index),
            CaptureMode::Text => begin_ocr(index),
            _ => {}
        }
    }
}
pub(super) fn begin_ocr(index: usize) {
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
    let text = NSTextView::initWithFrame(
        NSTextView::alloc(mtm()),
        NSRect::new(NSPoint::ZERO, NSSize::new(500.0, 270.0)),
    );
    text.setRichText(false);
    text.setFont(Some(&NSFont::systemFontOfSize(16.0)));
    text.setVerticallyResizable(true);
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
    let text = state(|a| a.ocr_panel.as_ref().map(|p| p.text.string().to_string()));
    if let Some(text) = text {
        let board = NSPasteboard::generalPasteboard();
        board.clearContents();
        if board.setString_forType(&NSString::from_str(&text), unsafe {
            NSPasteboardTypeString
        }) {
            toast("已复制文字");
        } else {
            toast("复制失败, 请重试");
        }
    }
}

pub(super) fn show_record_setup(index: usize) {
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
    if let Some(setup) = state(|a| a.record_setup.take()) {
        setup.window.orderOut(None);
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
        a.sessions[index].window.setIgnoresMouseEvents(true);
        a.recording = Some(Recording {
            handle,
            window: window.clone(),
            status,
            job,
            started: None,
            finishing: false,
        });
    });
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
fn show_result(path: PathBuf, format: media::Format) {
    let window = panel("录屏完成", 540.0, 190.0);
    let root = window.contentView().unwrap();
    root.addSubview(&label(
        "录屏文件已保存在本机",
        NSRect::new(NSPoint::new(20.0, 138.0), NSSize::new(500.0, 26.0)),
        16.0,
    ));
    root.addSubview(&label(
        &path.file_name().unwrap_or_default().to_string_lossy(),
        NSRect::new(NSPoint::new(20.0, 99.0), NSSize::new(500.0, 28.0)),
        12.0,
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
    state(|a| {
        if let Some(old) = a.media_result.take() {
            old.window.orderOut(None);
        }
        a.media_result = Some(MediaResult {
            window: window.clone(),
            path,
            format,
        });
    });
    present(&window);
}
pub(super) fn copy_recording() {
    let data = state(|a| a.media_result.as_ref().map(|r| (r.path.clone(), r.format)));
    if let Some((path, format)) = data {
        let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
        let board = NSPasteboard::generalPasteboard();
        board.clearContents();
        let ok = url.absoluteString().is_some_and(|value| {
            board.setString_forType(&value, unsafe { NSPasteboardTypeFileURL })
        });
        if format == media::Format::Gif {
            if let Ok(bytes) = std::fs::read(&path) {
                board.setData_forType(
                    Some(&NSData::with_bytes(&bytes)),
                    &NSString::from_str("com.compuserve.gif"),
                );
            }
        }
        if ok {
            toast("已复制录屏, 可粘贴到支持文件的应用");
        } else {
            toast("复制失败, 请重试");
        }
    }
}
pub(super) fn play_recording() {
    if let Some(path) = state(|a| a.media_result.as_ref().map(|r| r.path.clone())) {
        let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
        if !NSWorkspace::sharedWorkspace().openURL(&url) {
            toast("无法打开录屏预览");
        }
    }
}
pub(super) fn save_recording() {
    let data = state(|a| a.media_result.as_ref().map(|r| (r.path.clone(), r.format)));
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
        match std::fs::copy(&source, &path) {
            Ok(_) => {
                state(|a| a.last_file = Some(path.clone()));
                toast(&format!(
                    "已保存 {}",
                    path.file_name().unwrap_or_default().to_string_lossy()
                ));
            }
            Err(error) => toast(&format!("保存失败: {error}")),
        }
    }
}
pub(super) fn feature_tick() {
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
                        panel.status.setStringValue(&ns_ui("识别结果可直接编辑"));
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
                path,
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
                        show_result(path, format);
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
