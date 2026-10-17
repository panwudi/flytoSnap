use super::{
    capture::{self, CaptureMessage, ScreenRequest},
    render::{self, BLUE, nr},
};
use crate::{
    i18n::{self, tr},
    interaction::{
        CaptureMode, CursorIntent, cursor_at, resize_cursor, scroll_panel_rects, scroll_start_rect,
    },
};
use crate::{
    model::{Color, Editor, PALETTE, Point, Rect, Settings, Tool},
    stitch::{StitchResult, Stitcher},
};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState, hotkey::HotKey};
use image::RgbaImage;
use objc2::{
    AnyThread, ClassType, DefinedClass, MainThreadOnly, Message, define_class, msg_send,
    rc::Retained,
    runtime::{AnyObject, ProtocolObject},
    sel,
};
use objc2_app_kit::*;
use objc2_foundation::*;
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    str::FromStr,
    sync::mpsc::{self, Receiver, Sender},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
mod checks;
mod features;
use features::*;
const CAPTURE_OVERLAY_LEVEL: isize = 25;
const CAPTURE_CONTROL_LEVEL: isize = 26;

thread_local! {static APP:RefCell<Option<App>>=const {RefCell::new(None)};}
fn mtm() -> MainThreadMarker {
    MainThreadMarker::new().expect("AppKit callback must run on the main thread")
}
fn state<R>(f: impl FnOnce(&mut App) -> R) -> R {
    APP.with(|a| f(a.borrow_mut().as_mut().expect("application initialized")))
}

struct Screen {
    id: u32,
    frame: NSRect,
    scale: f64,
}
struct Session {
    screen: Screen,
    editor: Editor,
    pixels: RgbaImage,
    image: Retained<NSImage>,
    mosaic: Retained<NSImage>,
    window: Retained<OverlayWindow>,
    view: Retained<Canvas>,
    buttons: Vec<Retained<NSButton>>,
    params: Vec<Retained<NSButton>>,
    text_input: Option<(Retained<NSTextView>, usize)>,
    bar_override: Option<Point>,
    bar_drag: Option<(Point, Point)>,
    cursor: Point,
    demo: bool,
    last_visible: NSRect,
    windows: Vec<Rect>,
    hover: Option<Rect>,
    pressed_hover: Option<Rect>,
    mode_buttons: Vec<Retained<NSButton>>,
    scroll_actions: Vec<Retained<NSButton>>,
    hovered: Option<(isize, Instant)>,
}
struct ScrollSession {
    stitcher: Stitcher,
    region: Rect,
    screen_index: usize,
    panel: Retained<NSPanel>,
    preview_panel: Retained<NSPanel>,
    preview: Retained<NSImageView>,
    status: Retained<NSTextField>,
    auto_button: Retained<NSButton>,
    auto: bool,
    pending: bool,
    last: Instant,
    unchanged: u8,
    auto_step: Option<Instant>,
}
struct Preferences {
    window: Retained<NSWindow>,
    shortcut: Retained<NSTextField>,
    directory: Retained<NSTextField>,
    status: Retained<NSTextField>,
    language: Retained<NSPopUpButton>,
}
struct OcrPanel {
    window: Retained<NSPanel>,
    text: Retained<NSTextView>,
    status: Retained<NSTextField>,
    copy: Retained<NSButton>,
}
struct RecordSetup {
    window: Retained<NSPanel>,
    index: usize,
    format: super::media::Format,
    mp4: Retained<NSButton>,
    gif: Retained<NSButton>,
    cursor: Retained<NSButton>,
}
struct Recording {
    handle: super::media::Handle,
    window: Retained<NSPanel>,
    status: Retained<NSTextField>,
    job: u64,
    started: Option<Instant>,
    finishing: bool,
}
struct MediaResult {
    window: Retained<NSPanel>,
    file: crate::temporary::RecordingFile,
    format: super::media::Format,
}
struct ClipboardRecording {
    file: crate::temporary::RecordingFile,
    change_count: isize,
}
struct PermissionHelp {
    window: Retained<NSPanel>,
    status: Retained<NSTextField>,
}
struct App {
    delegate: Retained<Delegate>,
    home: Retained<NSWindow>,
    home_status: Retained<NSTextField>,
    _status_item: Retained<NSStatusItem>,
    sessions: Vec<Session>,
    active: usize,
    pins: Vec<Retained<NSPanel>>,
    scroll: Option<ScrollSession>,
    preferences: Option<Preferences>,
    about: Option<Retained<NSPanel>>,
    manager: GlobalHotKeyManager,
    hotkey: HotKey,
    settings: Settings,
    tx: Sender<CaptureMessage>,
    rx: Receiver<CaptureMessage>,
    generation: u64,
    pending: usize,
    capture_started: Instant,
    prefer_scroll: bool,
    last_file: Option<PathBuf>,
    toast: Option<(String, Instant)>,
    toast_window: Option<Retained<NSPanel>>,
    mode: CaptureMode,
    headless: bool,
    feature_job: u64,
    ocr_tx: Sender<(u64, Result<String, String>)>,
    ocr_rx: Receiver<(u64, Result<String, String>)>,
    ocr_panel: Option<OcrPanel>,
    record_tx: Sender<super::media::Event>,
    record_rx: Receiver<super::media::Event>,
    record_setup: Option<RecordSetup>,
    recording: Option<Recording>,
    media_result: Option<MediaResult>,
    clipboard_recording: Option<ClipboardRecording>,
    permission_help: Option<PermissionHelp>,
    screen_access_requested: bool,
    escape_hotkey: HotKey,
    escape_registered: bool,
    menu_tracking: bool,
    updater: Option<super::updater::Updater>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind=MainThreadOnly]
    struct Delegate;
    unsafe impl NSObjectProtocol for Delegate {}
    unsafe impl NSApplicationDelegate for Delegate {
        #[unsafe(method(applicationShouldTerminate:))]
        fn should_quit(&self,_app:&NSApplication)->NSApplicationTerminateReply {
            if state(|a|a.recording.is_some()){stop_recording(false);toast("录屏完成后可退出应用");NSApplicationTerminateReply::TerminateCancel}else{NSApplicationTerminateReply::TerminateNow}
        }
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn launched(&self,_notification:&NSNotification) {
            initialize(self);
            if !std::env::args().any(|a|a=="--menubar-check"||a=="--window-check") {startup_feedback();}
        }
        #[unsafe(method(applicationWillTerminate:))]
        fn terminating(&self,_notification:&NSNotification) {dispose_recording_results();}
        #[unsafe(method(applicationDidResignActive:))]
        fn deactivated(&self,_notification:&NSNotification) {
            // Menu and toolbar focus changes are not an application switch.
        }
        #[unsafe(method(applicationShouldHandleReopen:hasVisibleWindows:))]
        fn reopened(&self,_app:&NSApplication,_visible:bool)->bool {
            let window=state(|a|a.sessions.get(a.active).map(|s|s.window.clone()));
            state(|a|a._status_item.setVisible(true));
            if let Some(window)=window {if !window.ignoresMouseEvents(){window.makeKeyAndOrderFront(None);}else{show_status_menu();}}
            else{show_status_menu();}
            false
        }
    }
    unsafe impl NSWindowDelegate for Delegate {
        #[unsafe(method(windowWillClose:))]
        fn window_closed(&self,notification:&NSNotification) {
            let is_editor=notification.object().and_then(|o|o.downcast::<OverlayWindow>().ok()).is_some();
            if is_editor {close_sessions();}
            else if let Some(window)=notification.object().and_then(|o|o.downcast::<NSWindow>().ok()){
                if state(|a|a.ocr_panel.as_ref().is_some_and(|s|s.window.as_super()==window.as_ref())) {
                    close_sessions();
                    return;
                }
                let record=state(|a|{
                    if a.about.as_ref().is_some_and(|panel|panel.as_super()==window.as_ref()){a.about=None;}
                    if a.record_setup.as_ref().is_some_and(|s|s.window.as_super()==window.as_ref()){a.record_setup=None;}
                    if a.media_result.as_ref().is_some_and(|s|s.window.as_super()==window.as_ref()){a.media_result=None;}
                    a.recording.as_ref().is_some_and(|s|s.window.as_super()==window.as_ref())
                });if record{stop_recording(false);}
            }
        }
    }
    unsafe impl NSMenuDelegate for Delegate {
        #[unsafe(method(menuWillOpen:))]
        fn menu_opened(&self,_menu:&NSMenu) {state(|a|a.menu_tracking=true);}
        #[unsafe(method(menuDidClose:))]
        fn menu_closed(&self,_menu:&NSMenu) {state(|a|a.menu_tracking=false);}
    }
    impl Delegate {
        #[unsafe(method(workspaceActivated:))]
        fn workspace_activated(&self,notification:&NSNotification) {
            let application=notification.userInfo()
                .and_then(|info|info.objectForKey(unsafe {NSWorkspaceApplicationKey}))
                .and_then(|object|object.downcast::<NSRunningApplication>().ok());
            if let Some(application)=application {
                let switched=application.processIdentifier()!=std::process::id() as i32
                    && application.activationPolicy()==NSApplicationActivationPolicy::Regular;
                if switched && state(|a|!a.menu_tracking&&!a.sessions.is_empty()&&a.scroll.is_none()&&a.recording.is_none()) {cancel_capture();}
            }
        }
        #[unsafe(method(action:))]
        fn action(&self,sender:&NSButton) { handle_action(sender.tag(),sender.window()); }
        #[unsafe(method(menuAction:))]
        fn menu_action(&self,sender:&NSMenuItem) { handle_action(sender.tag(),None); }
        #[unsafe(method(tick:))]
        fn tick(&self,_timer:&NSTimer) {tick();}
        #[unsafe(method(checkStatusItem:))]
        fn check_status(&self,_timer:&NSTimer) {
            match verify_live_status_item() {
                Ok(())=>NSApplication::sharedApplication(mtm()).terminate(None),
                Err(error)=>{eprintln!("Menu bar check failed: {error}");std::process::exit(1);}
            }
        }
    }
);

define_class!(
    #[unsafe(super(NSWindow))]
    #[thread_kind=MainThreadOnly]
    struct OverlayWindow;
    impl OverlayWindow {
        #[unsafe(method(canBecomeKeyWindow))]
        fn can_key(&self)->bool {true}
        #[unsafe(method(canBecomeMainWindow))]
        fn can_main(&self)->bool {true}
    }
);

define_class!(
    #[unsafe(super(NSTextView))]
    #[thread_kind=MainThreadOnly]
    struct TextInput;
    impl TextInput {
        #[unsafe(method(cancelOperation:))]
        fn cancel(&self,_sender:Option<&AnyObject>) {let _keep_alive=self.retain();cancel_capture();}
    }
);

struct CanvasIvars {
    index: usize,
}
define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind=MainThreadOnly]
    #[ivars=CanvasIvars]
    struct Canvas;
    impl Canvas {
        #[unsafe(method(isFlipped))]
        fn flipped(&self)->bool {true}
        #[unsafe(method(acceptsFirstResponder))]
        fn accepts(&self)->bool {true}
        #[unsafe(method(drawRect:))]
        fn draw(&self,_rect:NSRect) {draw_canvas(self.ivars().index);}
        #[unsafe(method(mouseDown:))]
        fn down(&self,event:&NSEvent) {mouse_down(self.ivars().index,event);}
        #[unsafe(method(mouseDragged:))]
        fn dragged(&self,event:&NSEvent) {mouse_drag(self.ivars().index,event);}
        #[unsafe(method(mouseUp:))]
        fn up(&self,event:&NSEvent) {mouse_up(self.ivars().index,event);}
        #[unsafe(method(mouseMoved:))]
        fn moved(&self,event:&NSEvent) {mouse_move(self.ivars().index,event);}
        #[unsafe(method(mouseEntered:))]
        fn entered(&self,event:&NSEvent) {mouse_move(self.ivars().index,event);}
        #[unsafe(method(mouseExited:))]
        fn exited(&self,_event:&NSEvent) {
            let interactive=state(|a|{if let Some(s)=a.sessions.get_mut(self.ivars().index){s.hovered=None;s.view.setNeedsDisplay(true);!s.window.ignoresMouseEvents()}else{false}});
            if interactive{NSCursor::arrowCursor().set();}
        }
        #[unsafe(method(cursorUpdate:))]
        fn cursor_update(&self,event:&NSEvent) {mouse_move(self.ivars().index,event);}
        #[unsafe(method(keyDown:))]
        fn key(&self,event:&NSEvent) {key_down(self.ivars().index,event);}
    }
);

define_class!(
    #[unsafe(super(NSButton))]
    #[thread_kind=MainThreadOnly]
    struct ActionButton;
    impl ActionButton {
        #[unsafe(method(viewDidMoveToWindow))]
        fn moved_to_window(&self) {
            unsafe {let _: () = msg_send![super(self),viewDidMoveToWindow];}
            if let Some(window)=self.window(){window.setAcceptsMouseMovedEvents(true);}
        }
        #[unsafe(method(resetCursorRects))]
        fn cursor_rects(&self) {self.addCursorRect_cursor(self.bounds(), &NSCursor::arrowCursor());}
        #[unsafe(method(cursorUpdate:))]
        fn cursor_update(&self,_event:&NSEvent) {button_hover(self);}
        #[unsafe(method(mouseEntered:))]
        fn entered(&self,_event:&NSEvent) {button_hover(self);}
        #[unsafe(method(mouseMoved:))]
        fn moved(&self,_event:&NSEvent) {button_hover(self);}
        #[unsafe(method(mouseExited:))]
        fn exited(&self,event:&NSEvent) {
            if let Some(index)=button_session(self) {mouse_move(index,event);}
        }
    }
);

fn track_pointer(view: &NSView) {
    let tracking = unsafe {
        NSTrackingArea::initWithRect_options_owner_userInfo(
            NSTrackingArea::alloc(),
            NSRect::ZERO,
            NSTrackingAreaOptions::MouseEnteredAndExited
                | NSTrackingAreaOptions::MouseMoved
                | NSTrackingAreaOptions::ActiveAlways
                | NSTrackingAreaOptions::InVisibleRect,
            Some(view),
            None,
        )
    };
    view.addTrackingArea(&tracking);
    let cursor = unsafe {
        NSTrackingArea::initWithRect_options_owner_userInfo(
            NSTrackingArea::alloc(),
            NSRect::ZERO,
            NSTrackingAreaOptions::CursorUpdate
                | NSTrackingAreaOptions::ActiveInKeyWindow
                | NSTrackingAreaOptions::InVisibleRect,
            Some(view),
            None,
        )
    };
    view.addTrackingArea(&cursor);
}

fn button_session(button: &NSButton) -> Option<usize> {
    let window = button.window()?;
    APP.with(|app| {
        app.borrow()
            .as_ref()?
            .sessions
            .iter()
            .position(|s| s.window.as_super() == window.as_ref())
    })
}

fn button_hover(button: &NSButton) {
    if let Some(index) = button_session(button) {
        state(|a| {
            let session = &mut a.sessions[index];
            if session.hovered.map(|h| h.0) != Some(button.tag()) {
                session.hovered = Some((button.tag(), Instant::now()));
                session.view.setNeedsDisplay(true);
            }
        });
    }
    NSCursor::arrowCursor().set();
}

struct PinIvars {
    image: Retained<NSImage>,
    png: Vec<u8>,
    locked: Cell<bool>,
    start: Cell<NSPoint>,
    origin: Cell<NSPoint>,
}
define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind=MainThreadOnly]
    #[ivars=PinIvars]
    struct PinView;
    impl PinView {
        #[unsafe(method(isFlipped))] fn flipped(&self)->bool {true}
        #[unsafe(method(acceptsFirstResponder))] fn accepts(&self)->bool {true}
        #[unsafe(method(drawRect:))]
        fn draw(&self,_rect:NSRect) {
            let b=self.bounds();render::draw_image(&self.ivars().image,Rect::new(0.0,0.0,b.size.width,b.size.height));
            render::stroke(Rect::new(0.5,0.5,b.size.width-1.0,b.size.height-1.0),BLUE,1.0);
            if self.ivars().locked.get() {render::rounded(Rect::new(8.0,8.0,52.0,24.0),4.0,Color(0.12,0.14,0.18),0.85);render::ui_text("已锁定",Point::new(15.0,12.0),11.0,Color(1.0,1.0,1.0));}
        }
        #[unsafe(method(mouseDown:))]
        fn down(&self,_event:&NSEvent) {
            if !self.ivars().locked.get() {self.ivars().start.set(NSEvent::mouseLocation());if let Some(w)=self.window(){self.ivars().origin.set(w.frame().origin);}}
        }
        #[unsafe(method(mouseDragged:))]
        fn dragged(&self,_event:&NSEvent) {
            if !self.ivars().locked.get() {if let Some(w)=self.window(){let p=NSEvent::mouseLocation();let s=self.ivars().start.get();let o=self.ivars().origin.get();w.setFrameOrigin(NSPoint::new(o.x+p.x-s.x,o.y+p.y-s.y));}}
        }
        #[unsafe(method(scrollWheel:))]
        fn wheel(&self,event:&NSEvent) {
            if let Some(w)=self.window() {
                if event.modifierFlags().contains(NSEventModifierFlags::Option) {
                    let alpha=(w.alphaValue()-event.scrollingDeltaY()*0.015).clamp(0.2,1.0);w.setAlphaValue(alpha);toast(&format!("透明度 {:.0}%",alpha*100.0));
                } else if !self.ivars().locked.get() {
                    let old=w.frame();let factor=(1.0-event.scrollingDeltaY()*0.015).clamp(0.8,1.2);
                    let width=(old.size.width*factor).clamp(80.0,2500.0);let height=old.size.height*width/old.size.width;
                    w.setFrame_display(NSRect::new(NSPoint::new(old.origin.x+(old.size.width-width)/2.0,old.origin.y+(old.size.height-height)/2.0),NSSize::new(width,height)),true);
                }
            }
        }
        #[unsafe(method(rightMouseDown:))]
        fn menu(&self,event:&NSEvent) {
            let menu=NSMenu::new(mtm());
            for (title,selector) in [("复制图片",sel!(copyPin:)),("保存图片...",sel!(savePin:)),(if self.ivars().locked.get(){"解锁"}else{"锁定"},sel!(lockPin:)),("恢复大小和透明度",sel!(resetPin:)),("关闭贴图",sel!(closePin:))] {
                let item=unsafe {NSMenuItem::initWithTitle_action_keyEquivalent(NSMenuItem::alloc(mtm()),&ns_ui(title),Some(selector),ns_string!(""))};
                unsafe {item.setTarget(Some(self));}menu.addItem(&item);
            }
            NSMenu::popUpContextMenu_withEvent_forView(&menu,event,self);
        }
        #[unsafe(method(copyPin:))] fn copy(&self,_sender:&AnyObject) {copy_png(&self.ivars().png);}
        #[unsafe(method(savePin:))] fn save(&self,_sender:&AnyObject) {save_png(&self.ivars().png,false);}
        #[unsafe(method(lockPin:))] fn lock(&self,_sender:&AnyObject) {self.ivars().locked.set(!self.ivars().locked.get());self.setNeedsDisplay(true);toast(if self.ivars().locked.get(){"贴图已锁定"}else{"贴图已解锁"});}
        #[unsafe(method(closePin:))] fn close_pin(&self,_sender:&AnyObject) {if let Some(w)=self.window(){w.orderOut(None);state(|a|a.pins.retain(|p|p.as_super()!=w.as_ref()));}toast("贴图已关闭");}
        #[unsafe(method(resetPin:))] fn reset(&self,_sender:&AnyObject) {if let Some(w)=self.window(){w.setAlphaValue(1.0);w.setContentSize(self.ivars().image.size());}toast("已恢复贴图大小和透明度");}
        #[unsafe(method(keyDown:))] fn key(&self,event:&NSEvent) {
            match event.keyCode() {53=>{if let Some(w)=self.window(){w.orderOut(None);}},_=>{if event.modifierFlags().contains(NSEventModifierFlags::Command)&&event.charactersIgnoringModifiers().is_some_and(|s|s.to_string()=="c") {copy_png(&self.ivars().png);}}}
        }
    }
);

const TOOLS: [(isize, &str, &str); 17] = [
    (100, "rectangle", "矩形 (R)"),
    (101, "circle", "椭圆 (O)"),
    (102, "arrow.right", "箭头 (A)"),
    (103, "pencil.tip", "画笔 (P)"),
    (104, "highlighter", "高亮 (H)"),
    (105, "checkerboard.rectangle", "马赛克 (M)"),
    (106, "textformat", "文字 (T)"),
    (107, "tag", "标签 (G)"),
    (108, "drop", "水印 (W)"),
    (109, "pin", "固定到屏幕"),
    (110, "arrow.down.square", "滚动截图"),
    (111, "cursorarrow", "选择并调整标注 (V)"),
    (112, "arrow.uturn.backward", "撤销 (⌘Z)"),
    (113, "arrow.uturn.forward", "重做 (⌘⇧Z)"),
    (114, "square.and.arrow.down", "保存 (⌘S)"),
    (115, "xmark", "取消 (Esc)"),
    (116, "checkmark", "保存到剪贴板 (Enter)"),
];

fn button(title: &str, symbol: Option<&str>, tag: isize, target: &Delegate) -> Retained<NSButton> {
    let b: Retained<ActionButton> =
        unsafe { msg_send![ActionButton::alloc(mtm()),initWithFrame:NSRect::ZERO] };
    track_pointer(&b);
    b.setTitle(&ns_ui(title));
    b.setBordered(false);
    b.setRefusesFirstResponder(true);
    b.setTag(tag);
    b.setFont(Some(&NSFont::systemFontOfSize(13.0)));
    b.setToolTip(Some(&ns_ui(title)));
    b.setAccessibilityLabel(Some(&ns_ui(title)));
    if let Some(symbol) = symbol {
        if let Some(image) = NSImage::imageWithSystemSymbolName_accessibilityDescription(
            &ns_ui(symbol),
            Some(&ns_ui(title)),
        ) {
            b.setImage(Some(&image));
            b.setImagePosition(NSCellImagePosition::ImageOnly);
        }
    }
    unsafe {
        b.setTarget(Some(target));
        b.setAction(Some(sel!(action:)));
    }
    b.into_super()
}
fn ns_ui(s: &str) -> Retained<NSString> {
    NSString::from_str(&tr(s))
}

fn set_caption(button: &NSButton, title: &str, color: Color, size: f64) {
    let font = NSFont::systemFontOfSize(size);
    let ink = render::color(color, 1.0);
    let attrs: Retained<NSDictionary<NSAttributedStringKey, AnyObject>> = NSDictionary::from_slices(
        &[unsafe { NSFontAttributeName }, unsafe {
            NSForegroundColorAttributeName
        }],
        &[&*font as &AnyObject, &*ink as &AnyObject],
    );
    let caption = unsafe {
        NSAttributedString::initWithString_attributes(
            NSAttributedString::alloc(),
            &ns_ui(title),
            Some(&attrs),
        )
    };
    button.setAttributedTitle(&caption);
}
fn mode_layout(s: &Session) -> (Rect, [Rect; 4]) {
    let visible = s.view.visibleRect();
    let widths = CaptureMode::ALL.map(|mode| (ui_text_width(mode.title(), 16.0) + 28.0).max(96.0));
    let width = widths.iter().sum::<f64>() + 16.0;
    let bar = Rect::new(
        visible.origin.x + (visible.size.width - width) / 2.0,
        visible.origin.y + 18.0,
        width,
        46.0,
    );
    let mut x = bar.x + 8.0;
    let tabs = std::array::from_fn(|i| {
        let r = Rect::new(x, bar.y + 3.0, widths[i], 40.0);
        x += widths[i];
        r
    });
    (bar, tabs)
}
fn controls(s: &Session) -> Vec<(isize, Rect, String)> {
    s.buttons
        .iter()
        .chain(s.params.iter())
        .chain(s.mode_buttons.iter())
        .chain(s.scroll_actions.iter())
        .filter(|b| !b.isHidden())
        .map(|b| {
            let r = b.frame();
            (
                b.tag(),
                Rect::new(r.origin.x, r.origin.y, r.size.width, r.size.height),
                b.toolTip()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| b.title().to_string()),
            )
        })
        .collect()
}
thread_local! {static DIAGONALS:std::cell::OnceCell<(Retained<NSCursor>,Retained<NSCursor>)>=const{std::cell::OnceCell::new()};}
fn native_cursor(intent: CursorIntent) -> Retained<NSCursor> {
    #[allow(deprecated)]
    match intent {
        CursorIntent::Arrow => NSCursor::arrowCursor(),
        CursorIntent::Crosshair => NSCursor::crosshairCursor(),
        CursorIntent::OpenHand => NSCursor::openHandCursor(),
        CursorIntent::ClosedHand => NSCursor::closedHandCursor(),
        CursorIntent::IBeam => NSCursor::IBeamCursor(),
        CursorIntent::Horizontal => NSCursor::resizeLeftRightCursor(),
        CursorIntent::Vertical => NSCursor::resizeUpDownCursor(),
        CursorIntent::DiagonalDown | CursorIntent::DiagonalUp => {
            let available: bool = unsafe {
                msg_send![NSCursor::class(),respondsToSelector:sel!(frameResizeCursorFromPosition:inDirections:)]
            };
            if available {
                NSCursor::frameResizeCursorFromPosition_inDirections(
                    if intent == CursorIntent::DiagonalDown {
                        NSCursorFrameResizePosition::TopLeft
                    } else {
                        NSCursorFrameResizePosition::TopRight
                    },
                    NSCursorFrameResizeDirections::All,
                )
            } else {
                DIAGONALS.with(|cell| {
                    let pair = cell.get_or_init(|| {
                        (
                            super::assets::diagonal_cursor(false),
                            super::assets::diagonal_cursor(true),
                        )
                    });
                    if intent == CursorIntent::DiagonalDown {
                        pair.0.clone()
                    } else {
                        pair.1.clone()
                    }
                })
            }
        }
    }
}
fn draw_tooltip(s: &Session) {
    let Some((tag, when)) = s.hovered else {
        return;
    };
    if when.elapsed() < Duration::from_millis(400) {
        return;
    }
    let Some((_, anchor, text)) = controls(s).into_iter().find(|b| b.0 == tag) else {
        return;
    };
    let width = (text
        .chars()
        .map(|c| if c.is_ascii() { 7.0 } else { 13.0 })
        .sum::<f64>()
        + 22.0)
        .clamp(72.0, 360.0);
    let visible = s.view.visibleRect();
    let x = (anchor.x + anchor.w / 2.0 - width / 2.0).clamp(
        visible.origin.x + 4.0,
        (visible.origin.x + visible.size.width - width - 4.0).max(visible.origin.x + 4.0),
    );
    let y = if anchor.y - 42.0 > visible.origin.y {
        anchor.y - 42.0
    } else {
        anchor.y + anchor.h + 8.0
    };
    render::rounded(
        Rect::new(x, y, width, 34.0),
        7.0,
        Color(0.23, 0.25, 0.29),
        0.97,
    );
    render::ui_text(
        &text,
        Point::new(x + 11.0, y + 9.0),
        12.0,
        Color(1.0, 1.0, 1.0),
    );
}

fn label(s: &str, frame: NSRect, size: f64) -> Retained<NSTextField> {
    let v = NSTextField::labelWithString(&ns_ui(s), mtm());
    v.setFrame(frame);
    v.setFont(Some(&NSFont::systemFontOfSize(size)));
    if frame.size.height >= 36.0 {
        v.setLineBreakMode(NSLineBreakMode::ByWordWrapping);
        v.setMaximumNumberOfLines(0);
    }
    v
}

fn scroll_action_rect(bounds: Rect, selection: Rect) -> Rect {
    let base = scroll_start_rect(bounds, selection);
    let width = (ui_text_width("开始滚动截图", 14.0) + 76.0)
        .max(base.w)
        .min(bounds.w - 16.0);
    Rect::new(
        (selection.x + (selection.w - width) / 2.0)
            .clamp(bounds.x + 8.0, bounds.x + bounds.w - width - 8.0),
        base.y,
        width,
        base.h,
    )
}

fn ui_text_width(text: &str, size: f64) -> f64 {
    let font = NSFont::systemFontOfSize(size);
    let attributes =
        NSDictionary::from_slices(&[unsafe { NSFontAttributeName }], &[&*font as &AnyObject]);
    unsafe { ns_ui(text).sizeWithAttributes(Some(&attributes)).width }
}

fn relocalize_view(view: &NSView) {
    for child in view.subviews() {
        if let Ok(button) = child.clone().downcast::<NSButton>() {
            button.setTitle(&ns_ui(&button.title().to_string()));
            if let Some(tip) = button.toolTip() {
                button.setToolTip(Some(&ns_ui(&tip.to_string())));
            }
        } else if let Ok(label) = child.clone().downcast::<NSTextField>() {
            if !label.isEditable() {
                label.setStringValue(&ns_ui(&label.stringValue().to_string()));
            }
        }
        relocalize_view(&child);
    }
    view.setNeedsDisplay(true);
}
fn relocalize_menu(menu: &NSMenu) {
    for item in menu.itemArray() {
        item.setTitle(&ns_ui(&item.title().to_string()));
        if let Some(submenu) = item.submenu() {
            relocalize_menu(&submenu);
        }
    }
}
fn relocalize_all() {
    let (windows, menu, count) = state(|a| {
        let mut windows = vec![a.home.clone()];
        if let Some(p) = &a.preferences {
            windows.push(p.window.clone());
        }
        if let Some(window) = &a.about {
            windows.push(window.as_super().retain());
        }
        if let Some(p) = &a.ocr_panel {
            windows.push(p.window.as_super().retain());
        }
        if let Some(p) = &a.record_setup {
            windows.push(p.window.as_super().retain());
        }
        if let Some(p) = &a.recording {
            windows.push(p.window.as_super().retain());
        }
        if let Some(p) = &a.media_result {
            windows.push(p.window.as_super().retain());
        }
        if let Some(p) = &a.permission_help {
            windows.push(p.window.as_super().retain());
        }
        windows.extend(a.sessions.iter().map(|s| s.window.as_super().retain()));
        (windows, a._status_item.menu(mtm()), a.sessions.len())
    });
    for window in windows {
        window.setTitle(&ns_ui(&window.title().to_string()));
        if let Some(root) = window.contentView() {
            relocalize_view(&root);
        }
    }
    if let Some(menu) = menu {
        relocalize_menu(&menu);
    }
    if let Some(menu) = NSApplication::sharedApplication(mtm()).mainMenu() {
        relocalize_menu(&menu);
    }
    for index in 0..count {
        refresh(index);
    }
    super::update_localization::localize_windows();
}
fn settings_path() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into()))
        .join("Library/Application Support/flytoSnap/settings.json")
}
fn load_settings() -> Settings {
    std::fs::read(settings_path())
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}
fn show_home() {
    let home = state(|a| a.home.clone());
    home.makeKeyAndOrderFront(None);
    #[allow(deprecated)]
    NSApplication::sharedApplication(mtm()).activateIgnoringOtherApps(true);
}

fn initialize(delegate: &Delegate) {
    let delegate = delegate.retain();
    let application = NSApplication::sharedApplication(mtm());
    if let Some(path) = NSBundle::mainBundle()
        .pathForResource_ofType(Some(ns_string!("AppIcon")), Some(ns_string!("png")))
    {
        if let Some(icon) = NSImage::initWithContentsOfFile(NSImage::alloc(), &path) {
            unsafe {
                application.setApplicationIconImage(Some(&icon));
            }
        }
    }
    let window_check = std::env::args().any(|s| s == "--window-check");
    let headless = window_check || std::env::args().any(|s| s == "--ui-snapshot");
    let menubar_check = std::env::args().any(|s| s == "--menubar-check");
    application.setActivationPolicy(if headless && !window_check {
        NSApplicationActivationPolicy::Prohibited
    } else {
        NSApplicationActivationPolicy::Accessory
    });
    let mut settings = load_settings();
    let system = NSLocale::preferredLanguages()
        .firstObject()
        .map(|s| s.to_string())
        .unwrap_or_else(|| "en".into());
    let requested = std::env::args()
        .skip_while(|a| a != "--lang")
        .nth(1)
        .unwrap_or_else(|| settings.language.clone());
    i18n::set(i18n::preference(&requested, &system));
    if headless {
        settings.language = requested;
    }
    let main_menu = NSMenu::new(mtm());
    let app_menu = NSMenu::new(mtm());
    let app_item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm()),
            ns_string!("flytoSnap"),
            None,
            ns_string!(""),
        )
    };
    app_item.setSubmenu(Some(&app_menu));
    main_menu.addItem(&app_item);
    let about = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm()),
            &ns_ui("关于 flytoSnap"),
            Some(sel!(menuAction:)),
            ns_string!(""),
        )
    };
    about.setTag(14);
    unsafe {
        about.setTarget(Some(&*delegate));
    }
    app_menu.addItem(&about);
    app_menu.addItem(&NSMenuItem::separatorItem(mtm()));
    let quit = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm()),
            &ns_ui("退出 flytoSnap"),
            Some(sel!(terminate:)),
            ns_string!("q"),
        )
    };
    unsafe {
        quit.setTarget(Some(&application));
    }
    app_menu.addItem(&quit);
    application.setMainMenu(Some(&main_menu));
    let manager = GlobalHotKeyManager::new().expect("could not initialize global hotkey manager");
    let hotkey = HotKey::from_str(&settings.shortcut)
        .unwrap_or_else(|_| HotKey::from_str("Super+Shift+KeyA").unwrap());
    let registration = if headless || menubar_check {
        Ok(())
    } else {
        manager.register(hotkey)
    };
    let menu = NSMenu::new(mtm());
    menu.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    let version = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm()),
            &NSString::from_str(&format!("flytoSnap {}", env!("CARGO_PKG_VERSION"))),
            None,
            ns_string!(""),
        )
    };
    version.setEnabled(false);
    menu.addItem(&version);
    menu.addItem(&NSMenuItem::separatorItem(mtm()));
    for (title, tag) in [
        ("开始截图", 1),
        ("打开图片...", 2),
        ("滚动截图", 3),
        ("录屏", 8),
        ("停止录制", 605),
        ("取消当前截图", 11),
        ("提取文字", 10),
        ("隐藏 / 显示全部贴图", 4),
        ("设置...", 5),
        ("检查更新...", 12),
        ("自动下载并安装更新", 13),
        ("打开最近保存的位置", 6),
        ("显示主窗口", 7),
        ("关于 flytoSnap", 14),
        ("退出 flytoSnap", 9),
    ] {
        let item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm()),
                &ns_ui(title),
                Some(sel!(menuAction:)),
                ns_string!(""),
            )
        };
        item.setTag(tag);
        unsafe {
            item.setTarget(Some(&delegate));
        }
        menu.addItem(&item);
    }
    menu.setAutoenablesItems(false);
    if let Some(stop) = menu.itemWithTag(605) {
        stop.setEnabled(false);
    }
    if let Some(cancel) = menu.itemWithTag(11) {
        cancel.setEnabled(false);
    }
    let status_item =
        NSStatusBar::systemStatusBar().statusItemWithLength(if headless { 0.0 } else { 28.0 });
    if headless {
        status_item.setVisible(false);
    } else {
        status_item.setAutosaveName(Some(ns_string!("flytoSnap.MenuBar")));
        status_item.setBehavior(NSStatusItemBehavior::empty());
        status_item.setVisible(true);
    }
    status_item.setMenu(Some(&menu));
    if let Some(b) = status_item.button(mtm()) {
        b.setToolTip(Some(&ns_ui("flytoSnap - 截图与录屏")));
        b.setImagePosition(NSCellImagePosition::ImageOnly);
        let brand = NSBundle::mainBundle()
            .pathForResource_ofType(Some(ns_string!("MenuIcon")), Some(ns_string!("png")))
            .and_then(|path| NSImage::initWithContentsOfFile(NSImage::alloc(), &path));
        if let Some(image) = brand {
            image.setTemplate(true);
            image.setSize(NSSize::new(20.0, 20.0));
            b.setImage(Some(&image));
        } else {
            if let Some(image) = NSImage::imageWithSystemSymbolName_accessibilityDescription(
                ns_string!("scissors"),
                Some(ns_string!("flytoSnap")),
            ) {
                image.setTemplate(true);
                b.setImage(Some(&image));
            }
        }
        if b.image().is_none() {
            b.setTitle(ns_string!("F"));
            b.setImagePosition(NSCellImagePosition::NoImage);
        }
    }
    let home = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm()),
            NSRect::new(NSPoint::ZERO, NSSize::new(540.0, 400.0)),
            NSWindowStyleMask::Titled
                | NSWindowStyleMask::Closable
                | NSWindowStyleMask::Miniaturizable,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    unsafe {
        home.setReleasedWhenClosed(false);
    }
    home.setTitle(ns_string!("flytoSnap"));
    home.center();
    let root = home.contentView().unwrap();
    root.addSubview(&label(
        "flytoSnap",
        NSRect::new(NSPoint::new(34.0, 312.0), NSSize::new(470.0, 42.0)),
        32.0,
    ));
    root.addSubview(&label(
        "截图, 标注, 固定在屏幕上",
        NSRect::new(NSPoint::new(36.0, 274.0), NSSize::new(460.0, 30.0)),
        17.0,
    ));
    for (title, tag, y) in [
        ("开始截图   ⌘⇧A", 1, 202.0),
        ("打开图片进行标注...", 2, 154.0),
        ("滚动截图", 3, 106.0),
    ] {
        let b = button(title, None, tag, &delegate);
        b.setBordered(true);
        b.setBezelStyle(NSBezelStyle::Push);
        b.setFrame(NSRect::new(NSPoint::new(34.0, y), NSSize::new(285.0, 38.0)));
        root.addSubview(&b);
    }
    let home_status = label(
        if unsafe { capture::CGPreflightScreenCaptureAccess() } {
            "屏幕录制权限已就绪"
        } else {
            "首次截图时, 请允许屏幕录制权限"
        },
        NSRect::new(NSPoint::new(36.0, 61.0), NSSize::new(470.0, 26.0)),
        12.0,
    );
    root.addSubview(&home_status);
    let quit = button("退出", None, 9, &delegate);
    quit.setFrame(NSRect::new(
        NSPoint::new(335.0, 24.0),
        NSSize::new(70.0, 28.0),
    ));
    root.addSubview(&quit);
    let preferences = button("设置...", None, 5, &delegate);
    preferences.setFrame(NSRect::new(
        NSPoint::new(420.0, 24.0),
        NSSize::new(80.0, 28.0),
    ));
    root.addSubview(&preferences);
    root.addSubview(&label(
        "选区后可直接标注. Enter 复制, Esc 取消.",
        NSRect::new(NSPoint::new(36.0, 25.0), NSSize::new(286.0, 24.0)),
        11.0,
    ));
    let (tx, rx) = mpsc::channel();
    let (ocr_tx, ocr_rx) = mpsc::channel();
    let (record_tx, record_rx) = mpsc::channel();
    APP.with(|a| {
        *a.borrow_mut() = Some(App {
            delegate: delegate.clone(),
            home: home.clone(),
            home_status,
            _status_item: status_item,
            sessions: vec![],
            active: 0,
            pins: vec![],
            scroll: None,
            preferences: None,
            about: None,
            manager,
            hotkey,
            settings,
            tx,
            rx,
            generation: 0,
            pending: 0,
            capture_started: Instant::now(),
            prefer_scroll: false,
            last_file: None,
            toast: None,
            toast_window: None,
            mode: CaptureMode::Screenshot,
            headless,
            feature_job: 0,
            ocr_tx,
            ocr_rx,
            ocr_panel: None,
            record_tx,
            record_rx,
            record_setup: None,
            recording: None,
            media_result: None,
            clipboard_recording: None,
            permission_help: None,
            screen_access_requested: false,
            escape_hotkey: HotKey::from_str("Escape").expect("valid Escape hotkey"),
            escape_registered: false,
            menu_tracking: false,
            updater: None,
        })
    });
    if !headless {
        unsafe {
            NSWorkspace::sharedWorkspace()
                .notificationCenter()
                .addObserver_selector_name_object(
                    &delegate,
                    sel!(workspaceActivated:),
                    Some(NSWorkspaceDidActivateApplicationNotification),
                    None,
                );
        }
    }
    if !headless && !menubar_check {
        unsafe {
            let timer = NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                0.08,
                &delegate,
                sel!(tick:),
                None,
                true,
            );
            NSRunLoop::mainRunLoop().addTimer_forMode(&timer, NSRunLoopCommonModes);
        }
    }
    if menubar_check {
        unsafe {
            NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                1.0,
                &delegate,
                sel!(checkStatusItem:),
                None,
                false,
            );
        }
    }
    if let Err(e) = registration {
        state(|a| {
            a.home_status
                .setStringValue(&ns_ui(&format!("快捷键注册失败: {e}. 请在设置中更换.")))
        });
        if !headless {
            toast(&format!("快捷键注册失败: {e}. 请在设置中更换."));
        }
    }
    if std::env::args().any(|s| s == "--demo") {
        open_demo();
    }
}

fn screens() -> Vec<Screen> {
    NSScreen::screens(mtm())
        .iter()
        .filter_map(|screen| {
            let id = screen
                .deviceDescription()
                .objectForKey(ns_string!("NSScreenNumber"))?
                .downcast::<NSNumber>()
                .ok()?
                .unsignedIntValue();
            Some(Screen {
                id,
                frame: screen.frame(),
                scale: screen.backingScaleFactor(),
            })
        })
        .collect()
}
fn show_status_menu() {
    let menu = state(|a| a._status_item.menu(mtm()));
    if let Some(menu) = menu {
        menu.popUpMenuPositioningItem_atLocation_inView(None, NSEvent::mouseLocation(), None);
    }
}
fn startup_feedback() {
    restore_recording_clipboard();
    if !screen_access_ready() {
        show_permission_help();
    } else {
        toast("flytoSnap 已就绪, 按 ⌘⇧A 开始截图");
    }
    match super::updater::Updater::load().and_then(|updater| {
        updater.start()?;
        Ok(updater)
    }) {
        Ok(updater) => state(|a| a.updater = Some(updater)),
        Err(error) => eprintln!("Updater unavailable: {error}"),
    }
}
pub(super) fn update_busy() -> bool {
    APP.with(|app| {
        app.try_borrow()
            .ok()
            .and_then(|a| {
                a.as_ref().map(|a| {
                    a.pending > 0
                        || !a.sessions.is_empty()
                        || a.recording.is_some()
                        || a.media_result.is_some()
                        || a.ocr_panel.is_some()
                        || a.record_setup.is_some()
                        || a.permission_help.is_some()
                })
            })
            .unwrap_or(true)
    })
}
fn screen_access_ready() -> bool {
    if unsafe { capture::CGPreflightScreenCaptureAccess() } {
        return true;
    }
    let should_request = state(|a| {
        let first = !a.screen_access_requested;
        a.screen_access_requested = true;
        first
    });
    if should_request {
        unsafe {
            capture::CGRequestScreenCaptureAccess();
        }
    }
    unsafe { capture::CGPreflightScreenCaptureAccess() }
}
fn set_capture_active(active: bool) {
    let error = state(|a| {
        if let Some(menu) = a._status_item.menu(mtm()) {
            if let Some(cancel) = menu.itemWithTag(11) {
                cancel.setEnabled(active);
            }
        }
        if a.headless || a.escape_registered == active {
            return None;
        }
        if active {
            match a.manager.register(a.escape_hotkey) {
                Ok(()) => a.escape_registered = true,
                Err(error) => return Some(error.to_string()),
            }
        } else {
            let _ = a.manager.unregister(a.escape_hotkey);
            a.escape_registered = false;
        }
        None
    });
    if error.is_some() {
        toast("Esc 快捷键不可用, 可从菜单栏取消当前截图");
    }
}
fn cancel_capture() {
    if state(|a| a.recording.is_some()) {
        stop_recording(true);
    } else {
        close_sessions();
        dismiss_permission_help();
        toast("已取消截图");
    }
}
fn start_capture(prefer_scroll: bool) {
    if state(|a| a.recording.is_some()) {
        toast("正在录屏, 请先停止当前录制");
        return;
    }
    close_sessions();
    state(|a| {
        a.mode = if prefer_scroll {
            CaptureMode::Scroll
        } else {
            CaptureMode::Screenshot
        }
    });
    if !screen_access_ready() {
        show_permission_help();
        return;
    }
    dismiss_permission_help();
    let requests = screens()
        .iter()
        .map(|s| ScreenRequest {
            id: s.id,
            width: (s.frame.size.width * s.scale).round() as usize,
            height: (s.frame.size.height * s.scale).round() as usize,
        })
        .collect::<Vec<_>>();
    if requests.is_empty() {
        toast("没有可用的显示器");
        return;
    }
    let (generation, tx) = state(|a| {
        a.prefer_scroll = prefer_scroll;
        a.generation += 1;
        a.pending = requests.len();
        a.capture_started = Instant::now();
        a.home_status.setStringValue(&ns_ui("正在截取屏幕..."));
        (a.generation, a.tx.clone())
    });
    set_capture_active(true);
    capture::capture(requests, generation, capture_control_windows(), tx);
}

fn capture_control_windows() -> Vec<u32> {
    state(|a| {
        let mut windows = a
            .sessions
            .iter()
            .map(|s| s.window.windowNumber() as u32)
            .collect::<Vec<_>>();
        if let Some(scroll) = &a.scroll {
            windows.extend([
                scroll.panel.windowNumber() as u32,
                scroll.preview_panel.windowNumber() as u32,
            ]);
        }
        if let Some(toast) = &a.toast_window {
            windows.push(toast.windowNumber() as u32);
        }
        windows
    })
}

fn create_session(screen: Screen, pixels: RgbaImage, demo: bool, windows: Vec<Rect>) {
    let index = state(|a| a.sessions.len());
    let viewport_size = screen.frame.size;
    let size = if demo {
        NSSize::new(
            pixels.width() as f64 / screen.scale,
            pixels.height() as f64 / screen.scale,
        )
    } else {
        viewport_size
    };
    let image = match render::native_image(&pixels, size) {
        Ok(i) => i,
        Err(e) => {
            toast(&e);
            return;
        }
    };
    let mosaic = match render::mosaic_image(&pixels, size) {
        Ok(i) => i,
        Err(e) => {
            toast(&e);
            return;
        }
    };
    let style = if demo {
        NSWindowStyleMask::Titled | NSWindowStyleMask::Closable
    } else {
        NSWindowStyleMask::Borderless
    };
    let window: Retained<OverlayWindow> = unsafe {
        msg_send![OverlayWindow::alloc(mtm()),initWithContentRect:screen.frame,styleMask:style,backing:NSBackingStoreType::Buffered,defer:false]
    };
    unsafe {
        window.setReleasedWhenClosed(false);
    }
    window.setTitle(&ns_ui("flytoSnap - 图片标注"));
    window.setAcceptsMouseMovedEvents(true);
    if !demo {
        window.setLevel(CAPTURE_OVERLAY_LEVEL);
        window.setOpaque(false);
        window.setBackgroundColor(Some(NSColor::clearColor().as_ref()));
        window.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary,
        );
    }
    let this = Canvas::alloc(mtm()).set_ivars(CanvasIvars { index });
    let view: Retained<Canvas> = unsafe {
        msg_send![super(this),initWithFrame:NSRect::new(NSPoint::ZERO,NSSize::new(size.width.max(viewport_size.width),size.height.max(viewport_size.height)))]
    };
    let appearance = NSAppearance::appearanceNamed(unsafe { NSAppearanceNameAqua });
    view.setAppearance(appearance.as_deref());
    if demo {
        let scroll = NSScrollView::initWithFrame(
            NSScrollView::alloc(mtm()),
            NSRect::new(NSPoint::ZERO, viewport_size),
        );
        scroll.setHasVerticalScroller(size.height > viewport_size.height);
        scroll.setHasHorizontalScroller(size.width > viewport_size.width);
        scroll.setDocumentView(Some(&view));
        window.setContentView(Some(&scroll));
        view.scrollPoint(NSPoint::ZERO);
    } else {
        window.setContentView(Some(&view));
    }
    let delegate = state(|a| a.delegate.clone());
    window.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    let mut mode_buttons = vec![];
    for mode in CaptureMode::ALL {
        let b = button(mode.title(), None, mode.tag(), &delegate);
        view.addSubview(&b);
        mode_buttons.push(b);
    }
    let mut scroll_actions = vec![];
    for (title, symbol, tag) in [("开始滚动截图", None, 304), ("取消", Some("xmark"), 115)]
    {
        let b = button(title, symbol, tag, &delegate);
        if symbol.is_none() {
            set_caption(&b, title, Color(0.16, 0.18, 0.22), 14.0);
        }
        b.setHidden(true);
        view.addSubview(&b);
        scroll_actions.push(b);
    }
    track_pointer(&view);
    let mut buttons = vec![];
    for (tag, symbol, title) in TOOLS {
        let b = button(title, Some(symbol), tag, &delegate);
        b.setHidden(true);
        view.addSubview(&b);
        buttons.push(b);
    }
    let mut params = vec![];
    for i in 0..3 {
        let b = button(
            ["细线", "中等线条", "粗线"][i],
            Some("circle.fill"),
            200 + i as isize,
            &delegate,
        );
        b.setHidden(true);
        view.addSubview(&b);
        params.push(b);
    }
    for i in 0..7 {
        let b = button(
            ["红色", "黄色", "绿色", "蓝色", "黑色", "灰色", "白色"][i],
            None,
            210 + i as isize,
            &delegate,
        );
        b.setHidden(true);
        view.addSubview(&b);
        params.push(b);
    }
    for (tag, symbol, title) in [
        (220, "hand.draw", "画笔马赛克"),
        (221, "rectangle.dashed", "框选马赛克"),
        (222, "textformat.size", "字体大小"),
        (223, "circle.lefthalf.filled", "水印透明度"),
    ] {
        let b = button(title, Some(symbol), tag, &delegate);
        b.setHidden(true);
        view.addSubview(&b);
        params.push(b);
    }
    state(|a| {
        a.sessions.push(Session {
            screen,
            editor: Editor::new(size.width, size.height),
            pixels,
            image,
            mosaic,
            window: window.clone(),
            view: view.clone(),
            buttons,
            params,
            text_input: None,
            bar_override: None,
            bar_drag: None,
            cursor: Point::new(size.width / 2.0, size.height / 2.0),
            demo,
            last_visible: NSRect::ZERO,
            windows,
            hover: None,
            pressed_hover: None,
            mode_buttons,
            scroll_actions,
            hovered: None,
        });
        a.active = index;
    });
    refresh(index);
    set_capture_active(true);
    if !state(|a| a.headless) {
        window.makeKeyAndOrderFront(None);
        window.makeFirstResponder(Some(&view));
        #[allow(deprecated)]
        NSApplication::sharedApplication(mtm()).activateIgnoringOtherApps(true);
    }
}

fn close_sessions() {
    let (sessions, scroll, record_setup, ocr_panel) = state(|a| {
        a.generation += 1;
        a.feature_job += 1;
        a.pending = 0;
        a.prefer_scroll = false;
        a.active = 0;
        (
            std::mem::take(&mut a.sessions),
            a.scroll.take(),
            a.record_setup.take(),
            a.ocr_panel.take(),
        )
    });
    set_capture_active(false);
    if let Some(scroll) = scroll {
        scroll.panel.orderOut(None);
        scroll.preview_panel.orderOut(None);
    }
    if let Some(setup) = record_setup {
        setup.window.orderOut(None);
    }
    if let Some(panel) = ocr_panel {
        panel.window.orderOut(None);
    }
    for session in sessions {
        session.window.setDelegate(None);
        session.window.setIgnoresMouseEvents(false);
        session.window.orderOut(None);
        session.window.close();
    }
    NSCursor::arrowCursor().set();
}
fn pass_through_capture(index: usize) {
    let windows = state(|a| {
        a.sessions
            .iter()
            .map(|s| s.window.clone())
            .collect::<Vec<_>>()
    });
    for (i, window) in windows.iter().enumerate() {
        window.disableCursorRects();
        window.discardCursorRects();
        window.setIgnoresMouseEvents(true);
        if i != index {
            window.orderOut(None);
        }
    }
}
fn bar(s: &Session) -> Rect {
    let Some(r) = s.editor.selection else {
        return Rect::default();
    };
    let w = TOOLS.len() as f64 * 35.0 + 28.0;
    let h = 44.0;
    if let Some(p) = s.bar_override {
        return Rect::new(p.x, p.y, w, h);
    }
    let visible = s.view.visibleRect();
    let x = (r.x + r.w - w).clamp(
        visible.origin.x + 8.0,
        (visible.origin.x + visible.size.width - w - 8.0).max(visible.origin.x + 8.0),
    );
    let y = if r.y + r.h + h + 76.0 < visible.origin.y + visible.size.height {
        r.y + r.h + 12.0
    } else if r.y - h - 68.0 >= visible.origin.y + 8.0 {
        r.y - h - 68.0
    } else {
        (visible.origin.y + visible.size.height - 112.0).max(visible.origin.y + 8.0)
    };
    Rect::new(x, y, w, h)
}
fn param_bar(s: &Session) -> Rect {
    let b = bar(s);
    let visible = s.view.visibleRect();
    Rect::new(
        (b.x + 160.0)
            .min((visible.origin.x + visible.size.width - 350.0).max(visible.origin.x + 8.0)),
        if b.y + 104.0 < visible.origin.y + visible.size.height {
            b.y + 54.0
        } else {
            b.y - 54.0
        },
        340.0,
        42.0,
    )
}

fn style_tool(s: &Session) -> Tool {
    s.editor
        .selected
        .and_then(|i| s.editor.annotations.get(i))
        .map(|a| a.tool)
        .unwrap_or(s.editor.tool)
}

fn refresh(index: usize) {
    state(|a| {
        let Some(s) = a.sessions.get_mut(index) else {
            return;
        };
        let b = bar(s);
        let p = param_bar(s);
        let show = s.editor.selection.is_some()
            && a.scroll.is_none()
            && a.mode == CaptureMode::Screenshot
            && a.recording.is_none();
        let (_, tabs) = mode_layout(s);
        for (i, b) in s.mode_buttons.iter().enumerate() {
            b.setFrame(nr(tabs[i]));
            b.setHidden(a.scroll.is_some() || a.recording.is_some());
            set_caption(
                b,
                CaptureMode::ALL[i].title(),
                if a.mode == CaptureMode::ALL[i] {
                    Color(0.15, 0.17, 0.21)
                } else {
                    Color(1.0, 1.0, 1.0)
                },
                16.0,
            );
        }
        let param_tool = style_tool(s);
        let start = s
            .editor
            .selection
            .map(|r| scroll_action_rect(s.editor.bounds, r));
        for (i, button) in s.scroll_actions.iter().enumerate() {
            button.setHidden(
                start.is_none()
                    || a.mode != CaptureMode::Scroll
                    || a.scroll.is_some()
                    || a.recording.is_some(),
            );
            if let Some(rect) = start {
                button.setFrame(nr(if i == 0 {
                    Rect::new(rect.x + 8.0, rect.y + 3.0, rect.w - 52.0, rect.h - 6.0)
                } else {
                    Rect::new(rect.x + rect.w - 42.0, rect.y + 3.0, 36.0, rect.h - 6.0)
                }));
            }
        }
        for (i, button) in s.buttons.iter().enumerate() {
            button.setHidden(!show);
            button.setFrame(nr(Rect::new(
                b.x + 22.0 + i as f64 * 35.0,
                b.y + 5.0,
                32.0,
                34.0,
            )));
            button.setContentTintColor(Some(&render::color(
                if tool_for(TOOLS[i].0) == Some(s.editor.tool)
                    || (TOOLS[i].0 == 105 && s.editor.tool.is_mosaic())
                {
                    BLUE
                } else {
                    if TOOLS[i].0 == 116 {
                        Color(0.22, 0.68, 0.24)
                    } else if TOOLS[i].0 == 115 {
                        Color(0.94, 0.24, 0.20)
                    } else {
                        Color(0.15, 0.18, 0.22)
                    }
                },
                1.0,
            )));
            button.setEnabled(match TOOLS[i].0 {
                112 => s.editor.can_undo(),
                113 => s.editor.can_redo(),
                _ => true,
            });
        }
        for (i, button) in s.params.iter().enumerate() {
            let tag = button.tag();
            let visible = show
                && param_tool != Tool::Select
                && match tag {
                    200..=202 => !param_tool.is_text(),
                    210..=216 => !param_tool.is_mosaic(),
                    220 | 221 => param_tool.is_mosaic(),
                    222 => param_tool.is_text(),
                    223 => param_tool == Tool::Watermark,
                    _ => false,
                };
            button.setHidden(!visible);
            if tag == 220 || tag == 221 {
                button.setContentTintColor(Some(&render::color(
                    if (tag == 220 && param_tool == Tool::MosaicBrush)
                        || (tag == 221 && param_tool == Tool::MosaicRect)
                    {
                        BLUE
                    } else {
                        Color(0.15, 0.18, 0.22)
                    },
                    1.0,
                )));
            }
            let x = match tag {
                200..=202 => p.x + 9.0 + (i as f64) * 24.0,
                210..=216 => p.x + 120.0 + (tag - 210) as f64 * 28.0,
                220 | 221 => p.x + 120.0 + (tag - 220) as f64 * 42.0,
                222 => p.x + 7.0,
                223 => p.x + 75.0,
                _ => p.x,
            };
            let width = if tag == 222 {
                70.0
            } else if tag == 223 {
                44.0
            } else {
                24.0
            };
            button.setFrame(nr(Rect::new(x, p.y + 6.0, width, 30.0)));
            if tag == 222 {
                button.setImagePosition(NSCellImagePosition::NoImage);
                button.setTitle(&ns_ui(&format!("{:.0} pt", s.editor.font_size)));
            }
            if tag == 223 {
                button.setImagePosition(NSCellImagePosition::NoImage);
                button.setTitle(&ns_ui(&format!("{:.0}%", s.editor.opacity * 100.0)));
            }
            if (210..=216).contains(&tag) {
                let c = PALETTE[(tag - 210) as usize];
                button.setTitle(if s.editor.color == c {
                    ns_string!("✓")
                } else {
                    ns_string!("")
                });
                button.setContentTintColor(Some(&render::color(
                    if c == PALETTE[6] {
                        Color(0.1, 0.1, 0.1)
                    } else {
                        Color(1.0, 1.0, 1.0)
                    },
                    1.0,
                )));
            }
        }
        s.view.setNeedsDisplay(true);
    });
    let item = state(|a| {
        a.sessions
            .get(index)
            .map(|s| (s.window.clone(), s.view.clone()))
    });
    if let Some((window, view)) = item {
        window.invalidateCursorRectsForView(&view);
    }
}

fn draw_canvas(index: usize) {
    APP.with(|app| {
        let app = app.borrow();
        let Some(a) = app.as_ref() else {
            return;
        };
        let Some(s) = a.sessions.get(index) else {
            return;
        };
        let bounds = s.editor.bounds;
        let param_tool = style_tool(s);
        let scrolling = a.scroll.is_some();
        let recording = a.recording.is_some();
        if recording || scrolling {
            if let Some(gc) = NSGraphicsContext::currentContext() {
                objc2_core_graphics::CGContext::clear_rect(Some(&gc.CGContext()), nr(bounds));
            }
        }
        if !scrolling && !recording {
            render::draw_image(&s.image, bounds);
        }
        if let Some(r) = s.editor.selection {
            let alpha = if recording {
                0.0
            } else if scrolling {
                0.18
            } else {
                0.48
            };
            render::selection_mask(bounds, r, alpha);
            if !scrolling && !recording {
                render::draw_annotations(
                    &s.editor,
                    &s.mosaic,
                    s.text_input.as_ref().map(|(_, i)| *i),
                );
                let visible = s.view.visibleRect();
                let hint_width = (ui_text_width(a.mode.confirm_hint(), 11.0) + 18.0).max(150.0);
                let hint = Rect::new(
                    visible.origin.x + visible.size.width - hint_width - 10.0,
                    visible.origin.y + 10.0,
                    hint_width,
                    28.0,
                );
                render::rounded(hint, 5.0, Color(0.12, 0.14, 0.18), 0.88);
                render::ui_text(
                    a.mode.confirm_hint(),
                    Point::new(hint.x + 9.0, hint.y + 7.0),
                    11.0,
                    Color(1.0, 1.0, 1.0),
                );
            }
            render::stroke(r, BLUE, 1.0);
            if !scrolling && !recording {
                for p in r.handles() {
                    render::rounded(Rect::new(p.x - 3.0, p.y - 3.0, 6.0, 6.0), 3.0, BLUE, 1.0);
                }
            }
            if a.mode == CaptureMode::Scroll && !scrolling && !recording {
                let start = scroll_action_rect(bounds, r);
                render::rounded(start, 7.0, Color(0.73, 0.75, 0.78), 1.0);
                render::rounded(start.inset(0.8), 6.2, Color(1.0, 1.0, 1.0), 0.98);
            }
            let text = format!(
                "{} × {}",
                (r.w * s.screen.scale).round() as u32,
                (r.h * s.screen.scale).round() as u32
            );
            let badge = Rect::new(
                r.x,
                (r.y - 30.0).max(4.0),
                text.len() as f64 * 7.5 + 16.0,
                24.0,
            );
            render::rounded(badge, 4.0, Color(1.0, 1.0, 1.0), 1.0);
            render::ui_text(
                &text,
                Point::new(badge.x + 8.0, badge.y + 4.0),
                12.0,
                Color(0.1, 0.12, 0.15),
            );
            if !scrolling && !recording && a.mode == CaptureMode::Screenshot {
                let b = bar(s);
                render::rounded(b, 8.0, Color(1.0, 1.0, 1.0), 0.98);
                for i in 0..6 {
                    render::rounded(
                        Rect::new(
                            b.x + 9.0 + (i % 2) as f64 * 4.0,
                            b.y + 16.0 + (i / 2) as f64 * 4.0,
                            2.0,
                            2.0,
                        ),
                        1.0,
                        Color(0.5, 0.52, 0.56),
                        1.0,
                    );
                }
                if let Some(i) = TOOLS.iter().position(|t| {
                    tool_for(t.0) == Some(s.editor.tool)
                        || (t.0 == 105 && s.editor.tool.is_mosaic())
                }) {
                    render::rounded(
                        Rect::new(b.x + 22.0 + i as f64 * 35.0, b.y + 7.0, 31.0, 30.0),
                        4.0,
                        Color(0.90, 0.93, 1.0),
                        1.0,
                    );
                }
                if param_tool != Tool::Select {
                    let p = param_bar(s);
                    render::rounded(p, 8.0, Color(1.0, 1.0, 1.0), 0.98);
                    if !param_tool.is_text() {
                        for i in 0..3 {
                            let radius = 2.5 + i as f64 * 1.5;
                            render::rounded(
                                Rect::new(
                                    p.x + 21.0 + i as f64 * 24.0 - radius,
                                    p.y + 21.0 - radius,
                                    radius * 2.0,
                                    radius * 2.0,
                                ),
                                radius,
                                if (s.editor.width - [2.0, 4.0, 7.0][i]).abs() < 0.1 {
                                    BLUE
                                } else {
                                    Color(0.6, 0.62, 0.66)
                                },
                                1.0,
                            );
                        }
                    }
                    if !param_tool.is_mosaic() {
                        for (i, c) in PALETTE.iter().enumerate() {
                            let r =
                                Rect::new(p.x + 121.0 + i as f64 * 28.0, p.y + 10.0, 22.0, 22.0);
                            render::rounded(r, 3.0, *c, 1.0);
                            if i == 6 {
                                render::stroke(r, Color(0.8, 0.81, 0.84), 0.7);
                            }
                        }
                    }
                }
                if let Some(i) = s.editor.selected {
                    let annotation = &s.editor.annotations[i];
                    let r = annotation.bounds();
                    render::stroke(r, BLUE, 1.0);
                    for p in r.handles() {
                        render::rounded(Rect::new(p.x - 2.0, p.y - 2.0, 4.0, 4.0), 1.0, BLUE, 1.0);
                    }
                }
            }
        } else {
            if !scrolling {
                if let Some(rect) = s.hover {
                    render::selection_mask(bounds, rect, 0.48);
                } else {
                    render::rounded(bounds, 0.0, Color(0.0, 0.0, 0.0), 0.32);
                }
            }
            let msg = a.mode.selection_prompt();
            let text_width = tr(msg)
                .chars()
                .map(|c| if c.is_ascii() { 7.0 } else { 13.0 })
                .sum::<f64>();
            let width = (text_width + 32.0).min(bounds.w - 32.0);
            let p = Point::new((bounds.w - width) / 2.0 + 16.0, 100.0);
            render::rounded(
                Rect::new(p.x - 16.0, p.y - 9.0, width, 38.0),
                6.0,
                Color(0.12, 0.14, 0.18),
                0.9,
            );
            render::ui_text(msg, p, 13.0, Color(1.0, 1.0, 1.0));
            if !s.demo {
                if let Some(rect) = s.hover {
                    render::stroke(rect, BLUE, 1.0);
                }
                draw_loupe(s);
            }
        }
        if !scrolling && !recording {
            let (hud, tabs) = mode_layout(s);
            render::rounded(hud, 8.0, Color(0.14, 0.16, 0.20), 0.94);
            for (i, mode) in CaptureMode::ALL.into_iter().enumerate() {
                if mode == a.mode {
                    render::rounded(tabs[i].inset(3.0), 6.0, Color(1.0, 1.0, 1.0), 1.0);
                }
            }
            draw_tooltip(s);
        }
    });
}

fn draw_loupe(s: &Session) {
    let p = s.cursor;
    let x = (p.x + 22.0).min(s.editor.bounds.w - 140.0).max(8.0);
    let y = (p.y + 22.0).min(s.editor.bounds.h - 152.0).max(8.0);
    render::rounded(
        Rect::new(x, y, 132.0, 140.0),
        6.0,
        Color(1.0, 1.0, 1.0),
        1.0,
    );
    let scale = s.screen.scale;
    let px = (p.x * scale).clamp(0.0, (s.pixels.width() - 1) as f64) as u32;
    let py = (p.y * scale).clamp(0.0, (s.pixels.height() - 1) as f64) as u32;
    let left = px.saturating_sub(10);
    let top = py.saturating_sub(10);
    let w = 21.min(s.pixels.width() - left);
    let h = 21.min(s.pixels.height() - top);
    let crop = image::imageops::crop_imm(&s.pixels, left, top, w, h).to_image();
    let enlarged = image::imageops::resize(&crop, 120, 108, image::imageops::FilterType::Nearest);
    if let Ok(image) = render::native_image(&enlarged, NSSize::new(120.0, 108.0)) {
        render::draw_image(&image, Rect::new(x + 6.0, y + 6.0, 120.0, 108.0));
    }
    render::stroke(Rect::new(x + 59.0, y + 54.0, 10.0, 10.0), BLUE, 1.0);
    let pixel = s.pixels.get_pixel(px, py);
    render::ui_text(
        &format!(
            "#{:02X}{:02X}{:02X}  {},{}",
            pixel[0], pixel[1], pixel[2], px, py
        ),
        Point::new(x + 7.0, y + 119.0),
        10.0,
        Color(0.2, 0.22, 0.25),
    );
}

fn event_point(index: usize, event: &NSEvent) -> Point {
    state(|a| {
        let p = a.sessions[index]
            .view
            .convertPoint_fromView(event.locationInWindow(), None);
        Point::new(p.x, p.y)
    })
}
fn mouse_down(index: usize, event: &NSEvent) {
    if state(|a| index >= a.sessions.len()) {
        return;
    }
    state(|a| a.active = index);
    commit_text(index);
    let p = event_point(index, event);
    if event.clickCount() == 2 {
        let existing = state(|a| {
            a.sessions[index]
                .editor
                .annotations
                .iter()
                .enumerate()
                .rev()
                .find(|(_, annotation)| annotation.tool.is_text() && annotation.hit(p))
                .map(|(i, a)| (i, a.tool, a.points[0]))
        });
        if let Some((i, tool, anchor)) = existing {
            begin_text(index, anchor, tool, Some(i));
            refresh(index);
            return;
        }
    }
    let start_text = state(|a| {
        let s = &mut a.sessions[index];
        let b = bar(s);
        s.pressed_hover = if s.editor.selection.is_none() {
            s.windows.iter().copied().find(|r| r.contains(p))
        } else {
            None
        };
        if s.editor.selection.is_some() && Rect::new(b.x, b.y, 22.0, b.h).contains(p) {
            s.bar_drag = Some((p, Point::new(b.x, b.y)));
            return None;
        }
        if s.editor.flip_tag(p) {
            return None;
        }
        if s.editor.selection.is_some_and(|r| r.contains(p)) && s.editor.tool.is_text() {
            Some(s.editor.tool)
        } else {
            s.editor.begin(p);
            None
        }
    });
    if let Some(tool) = start_text {
        begin_text(index, p, tool, None);
    }
    refresh(index);
}
fn mouse_drag(index: usize, event: &NSEvent) {
    if state(|a| index >= a.sessions.len()) {
        return;
    }
    let p = event_point(index, event);
    state(|a| {
        let s = &mut a.sessions[index];
        if let Some((start, origin)) = s.bar_drag {
            s.bar_override = Some(Point::new(
                (origin.x + p.x - start.x).clamp(
                    4.0,
                    (s.editor.bounds.w - TOOLS.len() as f64 * 35.0 - 32.0).max(4.0),
                ),
                (origin.y + p.y - start.y).clamp(4.0, s.editor.bounds.h - 48.0),
            ));
        } else {
            s.editor.drag(p);
        }
    });
    let intent = state(|a| {
        let s = &a.sessions[index];
        if let Some(h) = s.editor.drag_resize_handle() {
            resize_cursor(h)
        } else if s.bar_drag.is_some() || s.editor.drag_moves_item() {
            CursorIntent::ClosedHand
        } else {
            CursorIntent::Crosshair
        }
    });
    native_cursor(intent).set();
    refresh(index);
}
fn mouse_up(index: usize, _event: &NSEvent) {
    finish_selection(index);
}
fn finish_selection(index: usize) {
    if state(|a| index >= a.sessions.len()) {
        return;
    }
    let mode = state(|a| {
        let s = &mut a.sessions[index];
        s.bar_drag = None;
        s.editor.finish();
        if s.editor.selection.is_none() {
            s.editor.selection = s.pressed_hover.take();
        } else {
            s.pressed_hover = None;
        }
        s.editor.selection.map(|_| a.mode)
    });
    refresh(index);
    match mode {
        Some(CaptureMode::Scroll) => {}
        Some(CaptureMode::Record) => show_record_setup(index),
        Some(CaptureMode::Text) => begin_ocr(index),
        _ => {}
    }
}
fn mouse_move(index: usize, event: &NSEvent) {
    if state(|a| {
        a.sessions
            .get(index)
            .is_none_or(|s| s.window.ignoresMouseEvents())
    }) {
        return;
    }
    let p = event_point(index, event);
    let intent = state(|a| {
        let s = &mut a.sessions[index];
        s.cursor = p;
        let buttons = controls(s);
        let hovered = buttons.iter().find(|b| b.1.contains(p)).map(|b| b.0);
        if hovered != s.hovered.map(|v| v.0) {
            s.hovered = hovered.map(|tag| (tag, Instant::now()));
            s.view.setNeedsDisplay(true);
        }
        if s.editor.selection.is_none() {
            s.hover = s.windows.iter().copied().find(|r| r.contains(p));
            s.view.setNeedsDisplay(true);
        }
        cursor_at(
            &s.editor,
            p,
            &buttons.iter().map(|b| b.1).collect::<Vec<_>>(),
            s.editor.selection.map(|_| {
                let b = bar(s);
                Rect::new(b.x, b.y, 22.0, b.h)
            }),
        )
    });
    native_cursor(intent).set();
}
fn key_down(index: usize, event: &NSEvent) {
    if state(|a| index >= a.sessions.len()) {
        return;
    }
    state(|a| a.active = index);
    if event.keyCode() == 53 {
        cancel_capture();
        return;
    }
    if event.keyCode() == 36 {
        match state(|a| a.mode) {
            CaptureMode::Screenshot => output(116),
            CaptureMode::Scroll => {
                if state(|a| a.scroll.is_some()) {
                    finish_scroll(true);
                } else {
                    start_scroll(index);
                }
            }
            CaptureMode::Record => begin_recording(),
            CaptureMode::Text => {
                if state(|a| a.ocr_panel.as_ref().is_some_and(|p| p.copy.isEnabled())) {
                    copy_ocr();
                } else {
                    begin_ocr(index);
                }
            }
        }
        return;
    }
    let cmd = event
        .modifierFlags()
        .contains(NSEventModifierFlags::Command);
    let shift = event.modifierFlags().contains(NSEventModifierFlags::Shift);
    let key = event
        .charactersIgnoringModifiers()
        .map(|s| s.to_string().to_lowercase())
        .unwrap_or_default();
    if cmd {
        match key.as_str() {
            "c" => {
                if state(|a| a.mode == CaptureMode::Text) {
                    copy_ocr();
                } else {
                    output(116);
                }
            }
            "s" => output(114),
            "z" => handle_action(if shift { 113 } else { 112 }, None),
            "a" => {
                state(|a| {
                    let s = &mut a.sessions[index];
                    s.editor.checkpoint();
                    s.editor.selection = Some(s.editor.bounds);
                });
                refresh(index);
            }
            _ => {}
        }
        return;
    }
    if [51, 117].contains(&event.keyCode()) {
        state(|a| a.sessions[index].editor.delete_selected());
        refresh(index);
        return;
    }
    let tag = match key.as_str() {
        "r" => 100,
        "o" => 101,
        "a" => 102,
        "p" => 103,
        "h" => 104,
        "m" => 105,
        "t" => 106,
        "g" => 107,
        "w" => 108,
        "v" => 111,
        _ => return,
    };
    handle_action(tag, None);
}
fn tool_for(tag: isize) -> Option<Tool> {
    match tag {
        100 => Some(Tool::Rectangle),
        101 => Some(Tool::Ellipse),
        102 => Some(Tool::Arrow),
        103 => Some(Tool::Pen),
        104 => Some(Tool::Highlight),
        105 => Some(Tool::MosaicBrush),
        106 => Some(Tool::Text),
        107 => Some(Tool::Tag),
        108 => Some(Tool::Watermark),
        111 => Some(Tool::Select),
        _ => None,
    }
}

fn handle_action(tag: isize, window: Option<Retained<NSWindow>>) {
    if let Some(window) = window {
        state(|a| {
            if let Some(i) = a
                .sessions
                .iter()
                .position(|s| s.window.as_super() == window.as_ref())
            {
                a.active = i;
            }
        });
    }
    if let Some(mode) = CaptureMode::from_tag(tag) {
        switch_mode(mode);
        return;
    }
    match tag {
        14 => {
            show_about();
            return;
        }
        901 | 902 => {
            let url = if tag == 901 {
                "https://github.com/panwudi/flytoSnap"
            } else {
                "https://github.com/panwudi/flytoSnap/blob/main/LICENSE"
            };
            if let Some(url) = NSURL::URLWithString(&NSString::from_str(url)) {
                if !NSWorkspace::sharedWorkspace().openURL(&url) {
                    toast("无法打开项目地址");
                }
            }
            return;
        }
        903 => {
            if let Some(window) = state(|a| a.about.as_ref().cloned()) {
                window.close();
            }
            return;
        }
        12 => {
            if update_busy() {
                toast("请先结束当前截图或录屏, 再检查更新");
                return;
            }
            let controller = state(|a| a.updater.take());
            if let Some(updater) = controller {
                if updater.can_check() {
                    updater.check();
                } else {
                    toast("正在检查或下载更新, 请稍候");
                }
                state(|a| a.updater = Some(updater));
            } else {
                toast("更新组件不可用, 请从 GitHub Releases 下载完整安装包");
            }
            return;
        }
        13 => {
            let controller = state(|a| a.updater.take());
            if let Some(updater) = controller {
                let enabled = !updater.automatic();
                updater.set_automatic(enabled);
                state(|a| a.updater = Some(updater));
                toast(if enabled {
                    "自动更新已开启"
                } else {
                    "自动更新已关闭"
                });
            } else {
                toast("更新组件不可用, 请从 GitHub Releases 下载完整安装包");
            }
            return;
        }
        11 => {
            cancel_capture();
            return;
        }
        304 => {
            start_scroll(state(|a| a.active));
            return;
        }
        305 => {
            finish_scroll_output(116);
            return;
        }
        306 => {
            finish_scroll_output(114);
            return;
        }
        701 => {
            open_screen_permission_settings();
            return;
        }
        702 => {
            retry_screen_permission();
            return;
        }
        703 => {
            reveal_current_app();
            return;
        }
        704 => {
            dismiss_permission_help();
            toast("已取消截图, 可从菜单栏重新开始");
            return;
        }
        1 => {
            start_capture(false);
            return;
        }
        2 => {
            open_image();
            return;
        }
        3 => {
            start_capture(true);
            return;
        }
        8 => {
            start_capture_mode(CaptureMode::Record);
            return;
        }
        10 => {
            start_capture_mode(CaptureMode::Text);
            return;
        }
        501 => {
            copy_ocr();
            return;
        }
        502 => {
            reselect_ocr();
            return;
        }
        503 => {
            cancel_capture();
            return;
        }
        601 => {
            begin_recording();
            return;
        }
        602 | 603 => {
            set_record_format(if tag == 602 {
                super::media::Format::Mp4
            } else {
                super::media::Format::Gif
            });
            return;
        }
        604 => {
            cancel_record_setup();
            return;
        }
        605 => {
            stop_recording(false);
            return;
        }
        606 => {
            stop_recording(true);
            return;
        }
        607 => {
            save_recording();
            return;
        }
        608 => {
            copy_recording();
            return;
        }
        609 => {
            play_recording();
            return;
        }
        4 => {
            toggle_pins();
            return;
        }
        5 => {
            show_preferences();
            return;
        }
        6 => {
            reveal_last();
            return;
        }
        7 => {
            show_home();
            return;
        }
        9 => {
            NSApplication::sharedApplication(mtm()).terminate(None);
            return;
        }
        30 => {
            apply_preferences();
            return;
        }
        301 => {
            toggle_auto();
            return;
        }
        302 => {
            finish_scroll(true);
            return;
        }
        303 => {
            finish_scroll(false);
            return;
        }
        _ => {}
    }
    let index = state(|a| a.active);
    if state(|a| index >= a.sessions.len()) {
        return;
    }
    commit_text(index);
    if let Some(tool) = tool_for(tag) {
        state(|a| a.sessions[index].editor.choose(tool));
    } else {
        match tag {
            109 | 114 | 116 => {
                output(tag);
                return;
            }
            110 => {
                switch_mode(CaptureMode::Scroll);
                return;
            }
            112 => state(|a| a.sessions[index].editor.undo()),
            113 => state(|a| a.sessions[index].editor.redo()),
            115 => {
                cancel_capture();
                return;
            }
            200..=202 => state(|a| {
                let e = &mut a.sessions[index].editor;
                e.width = [2.0, 4.0, 7.0][(tag - 200) as usize];
                e.apply_style();
            }),
            210..=216 => state(|a| {
                let e = &mut a.sessions[index].editor;
                e.color = PALETTE[(tag - 210) as usize];
                e.apply_style();
            }),
            220 | 221 => state(|a| {
                a.sessions[index].editor.choose(if tag == 220 {
                    Tool::MosaicBrush
                } else {
                    Tool::MosaicRect
                })
            }),
            222 => state(|a| {
                let e = &mut a.sessions[index].editor;
                let sizes = [12.0, 16.0, 18.0, 24.0, 32.0, 48.0];
                let i = sizes.iter().position(|s| *s == e.font_size).unwrap_or(2);
                e.font_size = sizes[(i + 1) % sizes.len()];
                e.apply_style();
            }),
            223 => state(|a| {
                let e = &mut a.sessions[index].editor;
                e.opacity = if e.opacity >= 0.7 {
                    0.15
                } else {
                    e.opacity + 0.15
                };
                e.apply_style();
            }),
            _ => {}
        }
    }
    refresh(index);
}

fn begin_text(index: usize, p: Point, tool: Tool, existing: Option<usize>) {
    let (view, window, font, color, content) = state(|a| {
        let s = &mut a.sessions[index];
        s.editor.checkpoint();
        if let Some(i) = existing {
            let annotation = &s.editor.annotations[i];
            s.editor.font_size = annotation.font_size;
            s.editor.color = annotation.color;
            s.editor.opacity = annotation.opacity;
        }
        let i = existing.unwrap_or_else(|| {
            let mut annotation = s.editor.new_annotation(tool, p);
            annotation.points[1] = Point::new(p.x + 260.0, p.y + 70.0);
            s.editor.annotations.push(annotation);
            s.editor.annotations.len() - 1
        });
        let input: Retained<TextInput> = unsafe {
            msg_send![TextInput::alloc(mtm()),initWithFrame:nr(Rect::new(p.x,p.y,260.0,74.0))]
        };
        let input = input.into_super();
        input.setRichText(false);
        input.setDrawsBackground(true);
        input.setBackgroundColor(&render::color(Color(1.0, 1.0, 1.0), 0.96));
        input.setTextColor(Some(&render::color(s.editor.color, 1.0)));
        input.setFont(Some(&NSFont::systemFontOfSize(s.editor.font_size)));
        input.setString(&NSString::from_str(&s.editor.annotations[i].text));
        input.setToolTip(Some(&ns_ui("输入文字, 点击选区其他位置完成")));
        input.setAccessibilityLabel(Some(&ns_ui(tool.title())));
        s.text_input = Some((input.clone(), i));
        (
            s.view.clone(),
            s.window.clone(),
            s.editor.font_size,
            s.editor.color,
            input,
        )
    });
    let _ = (font, color);
    view.addSubview(&content);
    window.makeFirstResponder(Some(&content));
}
fn commit_text(index: usize) {
    let input = state(|a| a.sessions.get_mut(index).and_then(|s| s.text_input.take()));
    if let Some((input, i)) = input {
        let content = input.string().to_string();
        input.removeFromSuperview();
        let (window, view) = state(|a| {
            let s = &mut a.sessions[index];
            if content.trim().is_empty() {
                s.editor.annotations.remove(i);
            } else {
                let annotation = &mut s.editor.annotations[i];
                annotation.text = content;
                let p = annotation.points[0];
                let width = (annotation
                    .text
                    .lines()
                    .map(|l| l.chars().count())
                    .max()
                    .unwrap_or(1) as f64
                    * annotation.font_size
                    * 0.9
                    + 12.0)
                    .max(24.0);
                let height =
                    annotation.text.lines().count().max(1) as f64 * annotation.font_size * 1.4;
                annotation.points[1] = Point::new(p.x + width, p.y + height);
                s.editor.selected = Some(i);
            }
            (s.window.clone(), s.view.clone())
        });
        window.makeFirstResponder(Some(&view));
        refresh(index);
    }
}

fn output(tag: isize) {
    let index = state(|a| a.active);
    commit_text(index);
    let png = state(|a| {
        let s = &a.sessions[index];
        render::export(&s.editor, &s.image, &s.mosaic, s.screen.scale)
    });
    match png {
        Ok(png) => match tag {
            109 => {
                let scale = state(|a| a.sessions[index].screen.scale);
                pin_png(png, scale);
                close_sessions();
                toast("已固定到屏幕. 右键查看操作.");
            }
            114 => {
                save_png(&png, true);
            }
            _ => {
                if copy_png(&png) {
                    close_sessions();
                }
            }
        },
        Err(e) => toast(&e),
    }
}
fn copy_png(png: &[u8]) -> bool {
    let board = NSPasteboard::generalPasteboard();
    board.clearContents();
    let data = NSData::with_bytes(png);
    if board.setData_forType(Some(&data), unsafe { NSPasteboardTypePNG }) {
        toast("已复制图片");
        true
    } else {
        toast("复制失败, 请重试");
        false
    }
}
fn filename() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("flytoSnap-{seconds}.png")
}
fn save_png(png: &[u8], close: bool) {
    let panel = NSSavePanel::savePanel(mtm());
    panel.setLevel(28);
    panel.setNameFieldStringValue(&ns_ui(&filename()));
    panel.setCanCreateDirectories(true);
    let dir = state(|a| a.settings.save_directory.clone());
    let url = NSURL::fileURLWithPath(&ns_ui(&dir));
    panel.setDirectoryURL(Some(&url));
    if panel.runModal() == NSModalResponseOK {
        if let Some(path) = panel
            .URL()
            .and_then(|u| u.path())
            .map(|p| PathBuf::from(p.to_string()))
        {
            match std::fs::write(&path, png) {
                Ok(()) => {
                    state(|a| a.last_file = Some(path.clone()));
                    if close {
                        close_sessions();
                    }
                    toast(&format!(
                        "已保存 {}",
                        path.file_name().unwrap_or_default().to_string_lossy()
                    ));
                }
                Err(e) => toast(&format!("保存失败: {e}")),
            }
        }
    } else {
        toast("已取消保存, 图片仍可继续编辑");
    }
}
fn reveal_last() {
    let file = state(|a| a.last_file.clone());
    if let Some(path) = file {
        let url = NSURL::fileURLWithPath(&ns_ui(&path.to_string_lossy()));
        NSWorkspace::sharedWorkspace()
            .activateFileViewerSelectingURLs(&NSArray::from_slice(&[&*url]));
    } else {
        toast("还没有保存过图片");
    }
}

fn pin_png(png: Vec<u8>, scale: f64) {
    let Ok(pixels) = image::load_from_memory(&png).map(|i| i.to_rgba8()) else {
        toast("无法打开贴图");
        return;
    };
    let size = NSSize::new(
        pixels.width() as f64 / scale,
        pixels.height() as f64 / scale,
    );
    let Ok(image) = render::native_image(&pixels, size) else {
        return;
    };
    let panel = {
        NSPanel::initWithContentRect_styleMask_backing_defer(
            NSPanel::alloc(mtm()),
            NSRect::new(NSPoint::new(100.0, 100.0), size),
            NSWindowStyleMask::Borderless,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    unsafe {
        panel.setReleasedWhenClosed(false);
    }
    panel.setTitle(&ns_ui("flytoSnap - 固定图片"));
    panel.setLevel(25);
    panel.setFloatingPanel(true);
    panel.setHasShadow(true);
    panel.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary,
    );
    let this = PinView::alloc(mtm()).set_ivars(PinIvars {
        image,
        png,
        locked: Cell::new(false),
        start: Cell::new(NSPoint::ZERO),
        origin: Cell::new(NSPoint::ZERO),
    });
    let view: Retained<PinView> =
        unsafe { msg_send![super(this),initWithFrame:NSRect::new(NSPoint::ZERO,size)] };
    view.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
    );
    panel.setContentView(Some(&view));
    panel.center();
    panel.makeKeyAndOrderFront(None);
    state(|a| a.pins.push(panel));
}
fn toggle_pins() {
    let (pins, show) = state(|a| (a.pins.clone(), !a.pins.iter().any(|p| p.isVisible())));
    if pins.is_empty() {
        toast("还没有固定的图片");
        return;
    }
    for pin in pins {
        if show {
            pin.orderFrontRegardless();
        } else {
            pin.orderOut(None);
        }
    }
    toast(if show {
        "已显示全部贴图"
    } else {
        "已隐藏全部贴图"
    });
}

fn toast(message: &str) {
    let old = state(|a| {
        a.toast = Some((message.into(), Instant::now()));
        a.toast_window.take()
    });
    if let Some(old) = old {
        old.orderOut(None);
    }
    if state(|a| a.headless) {
        return;
    }
    let panel = {
        NSPanel::initWithContentRect_styleMask_backing_defer(
            NSPanel::alloc(mtm()),
            NSRect::new(
                NSPoint::ZERO,
                NSSize::new(
                    (message.chars().count() as f64 * 13.0 + 48.0).clamp(220.0, 720.0),
                    46.0,
                ),
            ),
            NSWindowStyleMask::Borderless,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    unsafe {
        panel.setReleasedWhenClosed(false);
    }
    panel.setBackgroundColor(Some(&render::color(Color(0.14, 0.16, 0.20), 1.0)));
    panel.setLevel(27);
    panel.setIgnoresMouseEvents(true);
    panel.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary,
    );
    if let Some(root) = panel.contentView() {
        let text = label(
            message,
            NSRect::new(
                NSPoint::new(18.0, 11.0),
                NSSize::new(panel.frame().size.width - 36.0, 26.0),
            ),
            13.0,
        );
        text.setTextColor(Some(NSColor::whiteColor().as_ref()));
        root.addSubview(&text);
    }
    let screen = NSScreen::mainScreen(mtm())
        .map(|s| s.visibleFrame())
        .unwrap_or(NSRect::new(NSPoint::ZERO, NSSize::new(1440.0, 900.0)));
    panel.setFrameOrigin(NSPoint::new(
        screen.origin.x + (screen.size.width - panel.frame().size.width) / 2.0,
        screen.origin.y + 70.0,
    ));
    panel.orderFrontRegardless();
    state(|a| a.toast_window = Some(panel));
}
fn permission_alert(title: &str, message: &str, url: &str) {
    let alert = NSAlert::new(mtm());
    alert.setMessageText(&ns_ui(title));
    alert.setInformativeText(&ns_ui(message));
    alert.addButtonWithTitle(&ns_ui("打开系统设置"));
    alert.addButtonWithTitle(&ns_ui("稍后"));
    alert.window().setLevel(29);
    if alert.runModal() == NSAlertFirstButtonReturn {
        if let Some(url) = NSURL::URLWithString(&ns_ui(url)) {
            NSWorkspace::sharedWorkspace().openURL(&url);
        }
    }
}

fn open_image() {
    close_sessions();
    let panel = NSOpenPanel::openPanel(mtm());
    panel.setCanChooseDirectories(false);
    panel.setAllowsMultipleSelection(false);
    if panel.runModal() != NSModalResponseOK {
        return;
    }
    if let Some(path) = panel.URL().and_then(|u| u.path()) {
        match image::open(path.to_string()) {
            Ok(image) => open_pixels(image.to_rgba8()),
            Err(e) => toast(&format!("无法打开图片: {e}")),
        }
    }
}
fn open_pixels(pixels: RgbaImage) {
    close_sessions();
    if pixels.width() as u64 * pixels.height() as u64 > 80_000_000 {
        toast("图片超过 8000 万像素, 请先缩小图片");
        return;
    }
    let frame = NSScreen::mainScreen(mtm())
        .map(|s| s.visibleFrame())
        .unwrap_or(NSRect::new(NSPoint::ZERO, NSSize::new(1440.0, 900.0)));
    let fit = (frame.size.width * 0.82 / pixels.width() as f64).min(1.0);
    let size = NSSize::new(
        (pixels.width() as f64 * fit)
            .max(680.0)
            .min(frame.size.width * 0.9),
        (pixels.height() as f64 * fit + 120.0)
            .min(frame.size.height * 0.80)
            .max(360.0),
    );
    let screen = Screen {
        id: 0,
        frame: NSRect::new(
            NSPoint::new(
                frame.origin.x + (frame.size.width - size.width) / 2.0,
                frame.origin.y + (frame.size.height - size.height) / 2.0,
            ),
            size,
        ),
        scale: 1.0 / fit,
    };
    create_session(screen, pixels, true, vec![]);
    state(|a| a.sessions[0].editor.selection = Some(a.sessions[0].editor.bounds));
    refresh(0);
}
fn demo_image() -> RgbaImage {
    let pixels = render::demo_pixels();
    let size = NSSize::new(1440.0, 900.0);
    let image = render::native_image(&pixels, size).unwrap();
    let mut editor = Editor::new(1440.0, 900.0);
    editor.selection = Some(editor.bounds);
    for (text, p, font, color) in [
        ("消息", Point::new(30.0, 34.0), 24.0, Color(0.1, 0.15, 0.22)),
        (
            "Laura",
            Point::new(330.0, 37.0),
            26.0,
            Color(0.1, 0.15, 0.22),
        ),
        (
            "项目进度",
            Point::new(375.0, 165.0),
            28.0,
            Color(0.1, 0.15, 0.22),
        ),
        (
            "在周报里同步一下本周专项进展吧",
            Point::new(375.0, 235.0),
            22.0,
            Color(0.2, 0.24, 0.3),
        ),
        (
            "截图后可以画箭头, 标注文字, 添加标签和马赛克.",
            Point::new(375.0, 290.0),
            19.0,
            Color(0.4, 0.44, 0.5),
        ),
        (
            "选区下方是编辑工具栏",
            Point::new(375.0, 350.0),
            19.0,
            Color(0.4, 0.44, 0.5),
        ),
        (
            "Laura",
            Point::new(32.0, 145.0),
            20.0,
            Color(0.2, 0.24, 0.3),
        ),
        (
            "产品设计",
            Point::new(32.0, 200.0),
            20.0,
            Color(0.2, 0.24, 0.3),
        ),
    ] {
        let mut a = editor.new_annotation(Tool::Text, p);
        a.text = tr(text);
        a.font_size = font;
        a.color = color;
        editor.annotations.push(a);
    }
    let png = render::export(&editor, &image, &image, 1.0).unwrap();
    image::load_from_memory(&png).unwrap().to_rgba8()
}

fn open_demo() {
    open_pixels(demo_image());
    state(|a| {
        a.sessions[0].editor.selection =
            Some(Rect::new(260.0, 100.0, 700.0, 400.0).clamp(a.sessions[0].editor.bounds));
    });
    refresh(0);
}

fn show_preferences() {
    if let Some(window) = state(|a| a.preferences.as_ref().map(|p| p.window.clone())) {
        if !state(|a| a.headless) {
            window.makeKeyAndOrderFront(None);
        }
        return;
    }
    let (settings, delegate) = state(|a| (a.settings.clone(), a.delegate.clone()));
    let window = panel("flytoSnap 设置", 640.0, 420.0).into_super();
    if state(|a| a.headless) {
        window.setFrameOrigin(NSPoint::new(-20_000.0, -20_000.0));
    }
    let root = window.contentView().unwrap();
    root.addSubview(&label(
        "截图快捷键",
        NSRect::new(NSPoint::new(26.0, 335.0), NSSize::new(180.0, 26.0)),
        14.0,
    ));
    let shortcut = NSTextField::initWithFrame(
        NSTextField::alloc(mtm()),
        NSRect::new(NSPoint::new(214.0, 331.0), NSSize::new(398.0, 30.0)),
    );
    shortcut.setStringValue(&ns_ui(&crate::model::shortcut_display(&settings.shortcut)));
    root.addSubview(&shortcut);
    root.addSubview(&label(
        "例如 Command+Shift+A, 或 Control+Option+S",
        NSRect::new(NSPoint::new(214.0, 290.0), NSSize::new(398.0, 36.0)),
        11.0,
    ));
    root.addSubview(&label(
        "保存目录",
        NSRect::new(NSPoint::new(26.0, 239.0), NSSize::new(180.0, 26.0)),
        14.0,
    ));
    let directory = NSTextField::initWithFrame(
        NSTextField::alloc(mtm()),
        NSRect::new(NSPoint::new(214.0, 235.0), NSSize::new(398.0, 30.0)),
    );
    directory.setStringValue(&NSString::from_str(&settings.save_directory));
    root.addSubview(&directory);
    root.addSubview(&label(
        "界面语言",
        NSRect::new(NSPoint::new(26.0, 175.0), NSSize::new(180.0, 26.0)),
        14.0,
    ));
    let language = NSPopUpButton::initWithFrame_pullsDown(
        NSPopUpButton::alloc(mtm()),
        NSRect::new(NSPoint::new(214.0, 171.0), NSSize::new(398.0, 32.0)),
        false,
    );
    language.addItemWithTitle(&ns_ui("自动 (跟随系统)"));
    for option in i18n::Language::ALL {
        language.addItemWithTitle(&NSString::from_str(option.native_name()));
    }
    language.selectItemAtIndex(
        i18n::Language::ALL
            .iter()
            .position(|option| option.code() == settings.language)
            .map(|i| i as isize + 1)
            .unwrap_or(0),
    );
    root.addSubview(&language);
    let status = label(
        "快捷键冲突时可以在这里更换",
        NSRect::new(NSPoint::new(26.0, 100.0), NSSize::new(588.0, 56.0)),
        12.0,
    );
    root.addSubview(&status);
    let apply = button("保存设置", None, 30, &delegate);
    apply.setBordered(true);
    apply.setBezelStyle(NSBezelStyle::Push);
    apply.setFrame(NSRect::new(
        NSPoint::new(404.0, 26.0),
        NSSize::new(210.0, 34.0),
    ));
    root.addSubview(&apply);
    let about = button("关于 flytoSnap", None, 14, &delegate);
    about.setFrame(NSRect::new(
        NSPoint::new(26.0, 46.0),
        NSSize::new(220.0, 28.0),
    ));
    root.addSubview(&about);
    root.addSubview(&label(
        &format!("flytoSnap {}  ·  © 2026 panwudi", env!("CARGO_PKG_VERSION")),
        NSRect::new(NSPoint::new(26.0, 13.0), NSSize::new(400.0, 24.0)),
        11.0,
    ));
    state(|a| {
        a.preferences = Some(Preferences {
            window: window.clone(),
            shortcut,
            directory,
            status,
            language,
        })
    });
    if !state(|a| a.headless) {
        window.makeKeyAndOrderFront(None);
    }
}
fn apply_preferences() {
    let result: Result<(), String> = state(|a| {
        let Some(p) = a.preferences.as_ref() else {
            return Ok(());
        };
        let shortcut = crate::model::shortcut_normalize(&p.shortcut.stringValue().to_string());
        let directory = p.directory.stringValue().to_string();
        let hotkey = HotKey::from_str(&shortcut).map_err(|e| format!("快捷键格式不正确: {e}"))?;
        if hotkey != a.hotkey {
            a.manager
                .register(hotkey)
                .map_err(|e| format!("快捷键无法使用: {e}"))?;
        }
        let settings = Settings {
            shortcut,
            save_directory: directory,
            language: p
                .language
                .indexOfSelectedItem()
                .checked_sub(1)
                .and_then(|i| usize::try_from(i).ok())
                .and_then(|i| i18n::Language::ALL.get(i))
                .map(|l| l.code())
                .unwrap_or("auto")
                .into(),
        };
        let save = (|| {
            let path = settings_path();
            std::fs::create_dir_all(path.parent().unwrap())?;
            let temporary = path.with_extension("json.tmp");
            std::fs::write(&temporary, serde_json::to_vec_pretty(&settings).unwrap())?;
            std::fs::rename(temporary, path)
        })()
        .map_err(|e: std::io::Error| format!("无法保存设置: {e}"));
        if let Err(e) = save {
            if hotkey != a.hotkey {
                let _ = a.manager.unregister(hotkey);
            }
            return Err(e);
        }
        if hotkey != a.hotkey {
            let _ = a.manager.unregister(a.hotkey);
        }
        a.hotkey = hotkey;
        a.settings = settings;
        Ok(())
    });
    if result.is_ok() {
        let preferred = state(|a| a.settings.language.clone());
        let system = NSLocale::preferredLanguages()
            .firstObject()
            .map(|s| s.to_string())
            .unwrap_or_else(|| "en".into());
        i18n::set(i18n::preference(&preferred, &system));
        relocalize_all();
    }
    let message = match result {
        Ok(()) => "设置已保存".into(),
        Err(e) => e,
    };
    state(|a| {
        if let Some(p) = &a.preferences {
            p.status.setStringValue(&ns_ui(&message));
        }
    });
    toast(&message);
}

fn start_scroll(index: usize) {
    if state(|a| index >= a.sessions.len()) {
        return;
    }
    commit_text(index);
    if state(|a| a.scroll.is_some()) {
        return;
    }
    if state(|a| a.sessions[index].demo) {
        toast("滚动截图需要先选择屏幕上的区域");
        return;
    }
    let data = state(|a| {
        let s = &a.sessions[index];
        s.editor.selection.map(|r| {
            let scale = s.screen.scale;
            let x = (r.x * scale).round() as u32;
            let y = (r.y * scale).round() as u32;
            let w = (r.w * scale).round().min((s.pixels.width() - x) as f64) as u32;
            let h = (r.h * scale).round().min((s.pixels.height() - y) as f64) as u32;
            (
                r,
                image::imageops::crop_imm(&s.pixels, x, y, w, h).to_image(),
            )
        })
    });
    let Some((region, pixels)) = data else {
        toast("请先选择滚动区域");
        return;
    };
    if pixels.height() < 60 || pixels.width() < 40 {
        toast("滚动区域太小, 请扩大选区");
        return;
    }
    let (screen_frame, bounds) = state(|a| {
        (
            a.sessions[index].screen.frame,
            a.sessions[index].editor.bounds,
        )
    });
    let (toolbar_rect, preview_rect) = scroll_panel_rects(bounds, region);
    let panel = floating_panel(toolbar_rect.w, toolbar_rect.h);
    panel.setTitle(&ns_ui("滚动截图"));
    let root = panel.contentView().unwrap();
    let status = label(
        "缓慢滚动页面, 右侧预览会实时更新",
        NSRect::new(
            NSPoint::new(12.0, 42.0),
            NSSize::new(toolbar_rect.w - 24.0, 34.0),
        ),
        11.0,
    );
    status.setLineBreakMode(NSLineBreakMode::ByWordWrapping);
    status.setMaximumNumberOfLines(2);
    root.addSubview(&status);
    let delegate = state(|a| a.delegate.clone());
    let auto_button = button("自动滚动", None, 301, &delegate);
    set_caption(&auto_button, "自动滚动", Color(0.16, 0.18, 0.22), 13.0);
    auto_button.setFrame(NSRect::new(
        NSPoint::new(8.0, 8.0),
        NSSize::new(154.0, 32.0),
    ));
    root.addSubview(&auto_button);
    for (title, symbol, tag, x) in [
        ("完成并编辑", "pencil", 302, 174.0),
        ("保存长截图", "arrow.down", 306, 214.0),
        ("取消", "xmark", 303, 254.0),
        ("复制长截图", "checkmark", 305, 294.0),
    ] {
        let b = button(title, Some(symbol), tag, &delegate);
        b.setFrame(NSRect::new(NSPoint::new(x, 8.0), NSSize::new(36.0, 32.0)));
        if tag == 303 {
            b.setContentTintColor(Some(&render::color(Color(0.88, 0.23, 0.22), 1.0)));
        }
        if tag == 305 {
            b.setContentTintColor(Some(&render::color(Color(0.22, 0.6, 0.28), 1.0)));
        }
        root.addSubview(&b);
    }
    let preview_panel = floating_panel(preview_rect.w, preview_rect.h);
    preview_panel.setTitle(&ns_ui("长截图预览"));
    let preview = NSImageView::initWithFrame(
        NSImageView::alloc(mtm()),
        NSRect::new(
            NSPoint::new(4.0, 4.0),
            NSSize::new(preview_rect.w - 8.0, preview_rect.h - 8.0),
        ),
    );
    preview.setImageScaling(NSImageScaling::ScaleProportionallyUpOrDown);
    preview_panel.contentView().unwrap().addSubview(&preview);
    let position = |window: &NSPanel, rect: Rect| {
        window.setFrameOrigin(NSPoint::new(
            screen_frame.origin.x + rect.x,
            screen_frame.origin.y + screen_frame.size.height - rect.y - rect.h,
        ));
    };
    position(&panel, toolbar_rect);
    position(&preview_panel, preview_rect);
    let image = match render::native_image(
        &pixels,
        NSSize::new(pixels.width() as f64, pixels.height() as f64),
    ) {
        Ok(image) => image,
        Err(error) => {
            toast(&error);
            return;
        }
    };
    preview.setImage(Some(&image));
    let count = state(|a| {
        a.prefer_scroll = false;
        a.pending = 0;
        a.generation += 1;
        a.scroll = Some(ScrollSession {
            stitcher: Stitcher::new(pixels),
            region,
            screen_index: index,
            panel: panel.clone(),
            preview_panel: preview_panel.clone(),
            preview,
            status,
            auto_button,
            auto: false,
            pending: false,
            last: Instant::now(),
            unchanged: 0,
            auto_step: None,
        });
        a.sessions.len()
    });
    pass_through_capture(index);
    for i in 0..count {
        refresh(i);
    }
    NSCursor::arrowCursor().set();
    present_floating(&panel);
    present_floating(&preview_panel);
}
fn toggle_auto() {
    let enabling = state(|a| a.scroll.as_ref().is_some_and(|s| !s.auto));
    if enabling && !unsafe { capture::AXIsProcessTrusted() } {
        permission_alert(
            "自动滚动需要辅助功能权限",
            "请在系统设置的辅助功能中允许 flytoSnap 控制滚动. 手动滚动无需此权限.",
            "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
        );
        return;
    }
    state(|a| {
        if let Some(s) = &mut a.scroll {
            s.auto = !s.auto;
            s.unchanged = 0;
            s.auto_step = None;
            s.auto_button.setTitle(&ns_ui(if s.auto {
                "停止自动滚动"
            } else {
                "自动滚动"
            }));
            s.status.setStringValue(&ns_ui(if s.auto {
                "正在自动滚动..."
            } else {
                "自动滚动已停止, 可继续手动滚动"
            }));
        }
    });
}
fn finish_scroll(edit: bool) {
    let scroll = state(|a| a.scroll.take());
    let Some(scroll) = scroll else {
        return;
    };
    scroll.panel.orderOut(None);
    scroll.preview_panel.orderOut(None);
    let pixels = scroll.stitcher.image;
    close_sessions();
    if edit {
        open_pixels(pixels);
        toast("长截图已完成, 可以继续标注");
    } else {
        toast("已取消滚动截图");
    }
}
fn finish_scroll_output(tag: isize) {
    finish_scroll(true);
    if state(|a| !a.sessions.is_empty()) {
        output(tag);
    }
}
fn tick() {
    super::update_localization::localize_windows();
    feature_tick();
    if !update_busy() {
        let install = state(|a| a.updater.as_ref().and_then(|u| u.deferred_install()));
        if let Some(install) = install {
            install.call(());
        }
    }
    state(|a| {
        if let Some(item) = a._status_item.menu(mtm()).and_then(|m| m.itemWithTag(13)) {
            item.setState(if a.updater.as_ref().is_some_and(|u| u.automatic()) {
                1
            } else {
                0
            });
        }
    });
    state(|a| {
        for s in &a.sessions {
            if s.hovered
                .is_some_and(|(_, when)| when.elapsed() >= Duration::from_millis(400))
            {
                s.view.setNeedsDisplay(true);
            }
        }
    });
    while let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
        dispatch_hotkey(event);
    }
    let messages = state(|a| a.rx.try_iter().collect::<Vec<_>>());
    for message in messages {
        handle_capture(message);
    }
    let changed = state(|a| {
        a.sessions
            .iter_mut()
            .enumerate()
            .filter_map(|(i, s)| {
                let visible = s.view.visibleRect();
                if visible != s.last_visible {
                    s.last_visible = visible;
                    Some(i)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
    });
    for i in changed {
        refresh(i);
    }
    let expired = state(|a| {
        if a.toast
            .as_ref()
            .is_some_and(|(_, when)| when.elapsed() > Duration::from_secs(3))
        {
            a.toast = None;
            a.toast_window.take()
        } else {
            None
        }
    });
    if let Some(panel) = expired {
        panel.orderOut(None);
    }
    let timeout = state(|a| a.pending > 0 && a.capture_started.elapsed() > Duration::from_secs(12));
    if timeout {
        close_sessions();
        toast("截图超时, 请检查屏幕录制权限后重试");
        show_home();
    }
    let request = state(|a| {
        let scroll = a.scroll.as_mut()?;
        if scroll.pending && scroll.last.elapsed() > Duration::from_secs(12) {
            scroll.pending = false;
            scroll.auto = false;
            scroll.auto_step = None;
            scroll
                .status
                .setStringValue(&ns_ui("屏幕捕获超时, 自动滚动已停止"));
            scroll.auto_button.setTitle(&ns_ui("自动滚动"));
        }
        if scroll.pending || scroll.last.elapsed() < Duration::from_millis(450) {
            return None;
        }
        let session = &a.sessions[scroll.screen_index];
        if scroll.auto {
            if let Some(when) = scroll.auto_step {
                if when.elapsed() < Duration::from_millis(140) {
                    return None;
                }
            } else {
                let main = NSScreen::screens(mtm())
                    .firstObject()
                    .map(|s| s.frame().size.height)
                    .unwrap_or(900.0);
                let x = session.screen.frame.origin.x + scroll.region.x + scroll.region.w / 2.0;
                let y = main - session.screen.frame.origin.y - session.screen.frame.size.height
                    + scroll.region.y
                    + scroll.region.h / 2.0;
                capture::scroll_at(x, y, -(scroll.region.h * 0.35) as i32);
                scroll.auto_step = Some(Instant::now());
                return None;
            }
        }
        scroll.auto_step = None;
        scroll.pending = true;
        scroll.last = Instant::now();
        a.generation += 1;
        Some((
            ScreenRequest {
                id: session.screen.id,
                width: session.pixels.width() as usize,
                height: session.pixels.height() as usize,
            },
            a.generation,
            a.tx.clone(),
        ))
    });
    if let Some((request, generation, tx)) = request {
        capture::capture(vec![request], generation, capture_control_windows(), tx);
    }
}
fn dispatch_hotkey(event: GlobalHotKeyEvent) {
    if event.state != HotKeyState::Pressed {
        return;
    }
    let active = state(|a| !a.sessions.is_empty() || a.pending > 0);
    if state(|a| a.escape_hotkey.id()) == event.id && active {
        cancel_capture();
    } else if state(|a| a.hotkey.id()) == event.id {
        if state(|a| a.recording.is_some()) {
            toast("正在录屏, 请先停止当前录制");
        } else if active {
            cancel_capture();
        } else {
            start_capture(false);
        }
    }
}
fn handle_capture(message: CaptureMessage) {
    if state(|a| a.generation != message.generation) {
        return;
    }
    let scrolling = state(|a| a.scroll.is_some());
    if scrolling {
        let result = state(|a| {
            let scroll = a.scroll.as_mut().unwrap();
            scroll.pending = false;
            let session = &a.sessions[scroll.screen_index];
            match message.pixels {
                Ok(pixels) => {
                    let scale = session.screen.scale;
                    let x = (scroll.region.x * scale).round() as u32;
                    let y = (scroll.region.y * scale).round() as u32;
                    let w = (scroll.region.w * scale).round() as u32;
                    let h = (scroll.region.h * scale).round() as u32;
                    if x + w > pixels.width() || y + h > pixels.height() {
                        return Err("显示器尺寸发生变化, 请重新截图".into());
                    }
                    let frame = image::imageops::crop_imm(&pixels, x, y, w, h).to_image();
                    let result = scroll.stitcher.push(frame);
                    if result == StitchResult::Duplicate {
                        scroll.unchanged = scroll.unchanged.saturating_add(1);
                    } else {
                        scroll.unchanged = 0;
                    }
                    if scroll.auto && scroll.unchanged >= 3 {
                        scroll.auto = false;
                    }
                    let message = match result {
                        StitchResult::Added(_) => format!(
                            "已拼接 {} × {} 像素",
                            scroll.stitcher.image.width(),
                            scroll.stitcher.image.height()
                        ),
                        StitchResult::Duplicate => {
                            if scroll.unchanged >= 3 {
                                "画面已停止变化, 可完成当前截图".into()
                            } else {
                                "画面未变化, 等待滚动".into()
                            }
                        }
                        StitchResult::NoOverlap => {
                            scroll.auto = false;
                            "未找到重叠内容, 请向回滚动一点".into()
                        }
                        StitchResult::LimitReached => {
                            scroll.auto = false;
                            "已达到长度上限, 请完成当前截图".into()
                        }
                    };
                    scroll.status.setStringValue(&ns_ui(&message));
                    if !scroll.auto {
                        scroll.auto_button.setTitle(&ns_ui("自动滚动"));
                    }
                    if matches!(result, StitchResult::Added(_)) {
                        let image = render::native_image(
                            &scroll.stitcher.image,
                            NSSize::new(
                                scroll.stitcher.image.width() as f64,
                                scroll.stitcher.image.height() as f64,
                            ),
                        )?;
                        scroll.preview.setImage(Some(&image));
                    }
                    Ok(())
                }
                Err(e) => {
                    scroll.auto = false;
                    Err(e)
                }
            }
        });
        if let Err(e) = result {
            toast(&format!("滚动截图失败: {e}"));
        }
        return;
    }
    state(|a| a.pending = a.pending.saturating_sub(1));
    match message.pixels {
        Ok(pixels) => {
            if let Some(screen) = screens().into_iter().find(|s| s.id == message.id) {
                create_session(screen, pixels, false, message.windows);
            }
        }
        Err(e) => {
            toast(&format!("截图失败: {e}"));
            if !unsafe { capture::CGPreflightScreenCaptureAccess() } {
                close_sessions();
                state(|a| {
                    a.generation += 1;
                    a.pending = 0;
                });
                show_permission_help();
            } else {
                show_home();
            }
        }
    }
}

pub fn run() {
    let app = NSApplication::sharedApplication(mtm());
    let arguments = std::env::args().collect::<Vec<_>>();
    if arguments.iter().any(|a| a == "--updater-check") {
        app.setActivationPolicy(NSApplicationActivationPolicy::Prohibited);
        if let Err(error) = super::updater::verify() {
            eprintln!("Updater check failed: {error}");
            std::process::exit(1);
        }
        return;
    }
    if let Some(index) = arguments.iter().position(|a| a == "--self-check") {
        app.setActivationPolicy(NSApplicationActivationPolicy::Prohibited);
        let path = arguments
            .get(index + 1)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("target/self-check"));
        let result = (|| {
            render::verify(&path)?;
            super::ocr::verify()?;
            super::media::verify(&path)?;
            let count = i18n::verify_catalog()?;
            println!(
                "Localization check passed: {count} entries across {} languages.",
                i18n::Language::ALL.len()
            );
            Ok::<(), String>(())
        })();
        if let Err(error) = result {
            eprintln!("Self-check failed: {error}");
            std::process::exit(1);
        }
        return;
    }
    if let Some(index) = arguments.iter().position(|a| a == "--export-icon") {
        app.setActivationPolicy(NSApplicationActivationPolicy::Prohibited);
        let path = arguments
            .get(index + 1)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("target/app-icon.png"));
        if let Err(error) = super::assets::export_icon(&path) {
            eprintln!("Icon export failed: {error}");
            std::process::exit(1);
        }
        if let Err(error) = super::assets::export_menu_icon(&path.with_file_name("menu-icon.png")) {
            eprintln!("Menu icon export failed: {error}");
            std::process::exit(1);
        }
        return;
    }
    let mut args = std::env::args();
    if args.any(|a| a == "--render-check") {
        app.setActivationPolicy(NSApplicationActivationPolicy::Prohibited);
        let output = args
            .next()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("target/render-check"));
        if let Err(error) = render::verify(&output) {
            eprintln!("Native render check failed: {error}");
            std::process::exit(1);
        }
        match capture::verify_window_metadata() {
            Ok(count) => {
                println!("Window metadata check passed: {count} visible window rectangles")
            }
            Err(error) => {
                eprintln!("Window metadata check failed: {error}");
                std::process::exit(1);
            }
        }
        return;
    }
    let delegate = unsafe { msg_send![Delegate::alloc(mtm()), init] };
    let delegate: Retained<Delegate> = delegate;
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    if arguments.iter().any(|a| a == "--window-check") {
        initialize(&delegate);
        if let Err(error) = checks::window_check() {
            eprintln!("Window interaction check failed: {error}");
            std::process::exit(1);
        }
        return;
    }
    if let Some(index) = arguments.iter().position(|a| a == "--ui-snapshot") {
        initialize(&delegate);
        let path = arguments
            .get(index + 1)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("target/ui-snapshot"));
        if let Err(error) = ui_snapshot(&path) {
            eprintln!("UI snapshot failed: {error}");
            std::process::exit(1);
        }
        return;
    }
    app.run();
}

fn snapshot_view(view: &NSView, path: &std::path::Path) -> Result<RgbaImage, String> {
    view.layoutSubtreeIfNeeded();
    let bounds = view.bounds();
    let bitmap = view
        .bitmapImageRepForCachingDisplayInRect(bounds)
        .ok_or("Could not allocate UI snapshot")?;
    view.cacheDisplayInRect_toBitmapImageRep(bounds, &bitmap);
    let data = unsafe {
        bitmap.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new())
    }
    .ok_or("Could not encode UI snapshot")?;
    let bytes = data.to_vec();
    std::fs::write(path, &bytes).map_err(|e| e.to_string())?;
    image::load_from_memory(&bytes)
        .map(|i| i.to_rgba8())
        .map_err(|e| e.to_string())
}
fn verify_live_status_item() -> Result<(), String> {
    let button = state(|a| {
        if !a._status_item.isVisible() {
            return Err("Status item is hidden".to_string());
        }
        if a.home.isVisible() || !a.sessions.is_empty() {
            return Err("Unexpected application window".to_string());
        }
        a._status_item
            .button(mtm())
            .ok_or_else(|| "Missing status item button".to_string())
    })?;
    let window = button
        .window()
        .ok_or("Status item has no menu bar window")?;
    let frame = window.frame();
    println!(
        "Menu bar item: window_visible={}, frame={frame:?}, button={:?}, image={}",
        window.isVisible(),
        button.frame(),
        button.image().is_some()
    );
    if button.isHidden() || button.frame().size.width < 18.0 || button.image().is_none() {
        return Err("Status item button has no visible icon".into());
    }
    if !window.isVisible() {
        return Err("Status item window is not visible".into());
    }
    let directory = PathBuf::from("target/menubar-check");
    std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    snapshot_view(&button, &directory.join("button.png"))?;
    state(|a| {
        a.manager.register(a.escape_hotkey)?;
        a.manager.unregister(a.escape_hotkey)
    })
    .map_err(|e| format!("Could not register and release the capture Escape hotkey: {e}"))?;
    println!(
        "Menu bar check passed: visible native icon, menu, Escape registration, no main window, no permission request."
    );
    Ok(())
}
fn pointer_event(
    view: &NSView,
    point: NSPoint,
    kind: NSEventType,
) -> Result<Retained<NSEvent>, String> {
    let location = view.convertPoint_toView(point, None);
    let window_number = view
        .window()
        .ok_or("Pointer target has no window")?
        .windowNumber();
    if kind == NSEventType::MouseMoved {
        NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
            kind, location, NSEventModifierFlags::empty(), 0.0, window_number, None, 0, 0, 0.0,
        )
    } else {
        unsafe {
            NSEvent::enterExitEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_trackingNumber_userData(
                kind, location, NSEventModifierFlags::empty(), 0.0, window_number, None, 0, 0, std::ptr::null_mut(),
            )
        }
    }.ok_or_else(||"Could not create native pointer event".into())
}
fn verify_arrow_events(view: &NSView) -> Result<(), String> {
    if !view
        .window()
        .is_some_and(|window| window.acceptsMouseMovedEvents())
    {
        return Err("The host window does not deliver mouse-moved events".into());
    }
    if !view.trackingAreas().iter().any(|area| {
        area.options().contains(
            NSTrackingAreaOptions::ActiveAlways
                | NSTrackingAreaOptions::MouseEnteredAndExited
                | NSTrackingAreaOptions::MouseMoved
                | NSTrackingAreaOptions::InVisibleRect,
        )
    }) {
        return Err("Pointer tracking is missing for inactive windows or resized controls".into());
    }
    let original = NSCursor::currentCursor();
    let result = (|| {
        let bounds = view.bounds();
        let point = NSPoint::new(
            bounds.origin.x + bounds.size.width / 2.0,
            bounds.origin.y + bounds.size.height / 2.0,
        );
        for kind in [
            NSEventType::MouseEntered,
            NSEventType::MouseMoved,
            NSEventType::CursorUpdate,
        ] {
            NSCursor::crosshairCursor().set();
            let event = pointer_event(view, point, kind)?;
            unsafe {
                if kind == NSEventType::MouseEntered {
                    let _: () = msg_send![view,mouseEntered:&*event];
                } else if kind == NSEventType::MouseMoved {
                    let _: () = msg_send![view,mouseMoved:&*event];
                } else {
                    let _: () = msg_send![view,cursorUpdate:&*event];
                }
            }
            if NSCursor::currentCursor() != NSCursor::arrowCursor() {
                return Err(
                    "A native control inherited the capture crosshair instead of the arrow".into(),
                );
            }
        }
        Ok(())
    })();
    original.set();
    result
}
fn verify_button_cursors(view: &NSView) -> Result<usize, String> {
    let mut count = 0;
    for child in view.subviews() {
        if child.isHidden() {
            continue;
        }
        if let Ok(button) = child.clone().downcast::<NSButton>() {
            verify_arrow_events(&button).map_err(|e| format!("Button {}: {e}", button.tag()))?;
            count += 1;
        }
        count += verify_button_cursors(&child)?;
    }
    Ok(count)
}
fn ui_snapshot(directory: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    show_preferences();
    show_about();
    let (preferences, about) = state(|a| {
        (
            a.preferences.as_ref().unwrap().window.clone(),
            a.about.as_ref().unwrap().clone(),
        )
    });
    snapshot_view(
        &preferences.contentView().unwrap(),
        &directory.join("settings.png"),
    )?;
    snapshot_view(&about.contentView().unwrap(), &directory.join("about.png"))?;
    if state(|a| a.preferences.as_ref().unwrap().language.numberOfItems()) != 11 {
        return Err("Settings language selector does not contain Auto and ten languages".into());
    }
    fn field_text(view: &NSView, output: &mut Vec<String>) {
        for child in view.subviews() {
            if let Ok(field) = child.clone().downcast::<NSTextField>() {
                output.push(field.stringValue().to_string());
            }
            field_text(&child, output);
        }
    }
    let mut metadata = vec![];
    field_text(&about.contentView().unwrap(), &mut metadata);
    for expected in [
        env!("CARGO_PKG_VERSION"),
        "panwudi",
        "© 2026",
        "https://github.com/panwudi/flytoSnap",
        "MIT",
    ] {
        if !metadata.iter().any(|text| text.contains(expected)) {
            return Err(format!("About metadata is missing: {expected}"));
        }
    }
    let excluded = capture_control_windows();
    if excluded.contains(&(preferences.windowNumber() as u32))
        || excluded.contains(&(about.windowNumber() as u32))
    {
        return Err("Settings or About was excluded from self capture".into());
    }
    handle_action(903, None);
    if state(|a| a.about.is_some()) {
        return Err("About close did not dispose of the window".into());
    }
    verify_recording_result(directory)?;
    let pixels = demo_image();
    let screen = Screen {
        id: 0,
        frame: NSRect::new(
            NSPoint::new(-10_000.0, -10_000.0),
            NSSize::new(1440.0, 900.0),
        ),
        scale: 1.0,
    };
    create_session(screen, pixels, false, vec![]);
    let overlay_id = state(|a| a.sessions[0].window.windowNumber() as u32);
    if !capture_control_windows().contains(&overlay_id) {
        return Err("Capture overlay was not excluded from self capture".into());
    }
    if state(|a| {
        a.sessions[0]
            .mode_buttons
            .iter()
            .any(|b| b.isHidden() || b.frame().size.width < 1.0)
    }) {
        return Err("Mode selector was not visible before region selection".into());
    }
    state(|a| {
        let s = &mut a.sessions[0];
        s.editor.selection = Some(Rect::new(320.0, 130.0, 1020.0, 540.0));
        s.editor.choose(Tool::Rectangle);
        s.hovered = Some((116, Instant::now() - Duration::from_secs(1)));
    });
    refresh(0);
    let view = state(|a| a.sessions[0].view.clone());
    let mut cursor_checks = verify_button_cursors(&view)?;
    let image = snapshot_view(&view, &directory.join("screenshot.png"))?;
    let sx = image.width() as f64 / 1440.0;
    let sy = image.height() as f64 / 900.0;
    let outside = image.get_pixel((100.0 * sx) as u32, (600.0 * sy) as u32);
    let inside = image.get_pixel((1100.0 * sx) as u32, (550.0 * sy) as u32);
    if outside[0] >= 190 || inside[0] <= 230 {
        return Err(format!(
            "Selection mask is incorrect: outside={outside:?}, inside={inside:?}"
        ));
    }
    state(|a| {
        a.mode = CaptureMode::Record;
        a.sessions[0].hovered = None;
    });
    refresh(0);
    show_record_setup(0);
    let (setup_level, overlay_level) = state(|a| {
        (
            a.record_setup.as_ref().unwrap().window.level(),
            a.sessions[0].window.level(),
        )
    });
    if setup_level <= overlay_level {
        return Err(format!(
            "Recording setup is hidden behind the capture overlay: panel level={setup_level}, overlay level={overlay_level}"
        ));
    }
    let root = state(|a| {
        a.record_setup
            .as_ref()
            .unwrap()
            .window
            .contentView()
            .unwrap()
    });
    snapshot_view(&root, &directory.join("recording.png"))?;
    cursor_checks += verify_button_cursors(&root)?;
    let window = state(|a| a.sessions[0].window.clone());
    let frame = window.frame();
    let mut altered = frame;
    altered.size.width = frame.size.width - 160.0;
    altered.size.height = frame.size.height - 120.0;
    window.setFrame_display(altered, false);
    window.setFrame_display(frame, false);
    let button = state(|a| a.sessions[0].mode_buttons[0].clone());
    unsafe {
        let _: () = msg_send![&*button,performClick:Option::<&AnyObject>::None];
    }
    if state(|a| a.mode) != CaptureMode::Screenshot {
        return Err("Native mode button did not change mode".into());
    }
    switch_mode(CaptureMode::Text);
    let ocr = checks::wait_for_ocr()?;
    let root = ocr.contentView().unwrap();
    snapshot_view(&root, &directory.join("ocr.png"))?;
    cursor_checks += verify_button_cursors(&root)?;
    switch_mode(CaptureMode::Screenshot);
    show_permission_help();
    let help = state(|a| a.permission_help.as_ref().unwrap().window.clone());
    let root = help.contentView().unwrap();
    snapshot_view(&root, &directory.join("permissions.png"))?;
    if help.isVisible() || state(|a| a.screen_access_requested) {
        return Err("Permission UI check displayed a window or requested access".into());
    }
    show_permission_help();
    if state(|a| a.permission_help.as_ref().unwrap().window.clone()) != help {
        return Err("Permission recovery created duplicate panels".into());
    }
    dismiss_permission_help();
    if state(|a| a.permission_help.is_some()) {
        return Err("Permission recovery panel did not dismiss".into());
    }
    let scroll_mode = state(|a| a.sessions[0].mode_buttons[1].clone());
    unsafe {
        let _: () = msg_send![&*scroll_mode,performClick:Option::<&AnyObject>::None];
    }
    finish_selection(0);
    if state(|a| a.scroll.is_some() || a.sessions[0].scroll_actions.iter().any(|b| b.isHidden())) {
        return Err(
            "Scrolling capture started before confirmation or its start button was hidden".into(),
        );
    }
    snapshot_view(&view, &directory.join("scroll-ready.png"))?;
    cursor_checks += verify_button_cursors(&view)?;
    let start = state(|a| a.sessions[0].scroll_actions[0].clone());
    verify_arrow_events(&start)?;
    if state(|a| a.sessions[0].hovered.map(|h| h.0)) != Some(304) {
        return Err("Native button hover did not reach the tooltip state".into());
    }
    let original = NSCursor::currentCursor();
    for (handle, point) in state(|a| a.sessions[0].editor.selection.unwrap().handles())
        .into_iter()
        .enumerate()
    {
        let event = pointer_event(
            &view,
            NSPoint::new(point.x, point.y),
            NSEventType::MouseExited,
        )?;
        unsafe {
            let _: () = msg_send![&*start,mouseExited:&*event];
        }
        if NSCursor::currentCursor() != native_cursor(resize_cursor(handle)) {
            original.set();
            return Err(
                "Leaving a native button did not restore the selection resize cursor".into(),
            );
        }
    }
    original.set();
    unsafe {
        let _: () = msg_send![&*start,performClick:Option::<&AnyObject>::None];
    }
    let (toolbar, preview, stale_generation) = state(|a| {
        let scroll = a
            .scroll
            .as_ref()
            .ok_or("Start scrolling button did not start capture")?;
        Ok::<_, &str>((
            scroll.panel.clone(),
            scroll.preview_panel.clone(),
            a.generation,
        ))
    })?;
    if toolbar.hidesOnDeactivate() || preview.hidesOnDeactivate() || !window.ignoresMouseEvents() {
        return Err("Scroll controls hide when the source app gets focus".into());
    }
    if toolbar.level() <= window.level() || preview.level() <= window.level() {
        return Err("Scrolling controls are hidden behind the capture overlay".into());
    }
    if window.areCursorRectsEnabled() {
        return Err("A pass-through capture overlay still owns cursor rectangles".into());
    }
    if toolbar.isKeyWindow() {
        return Err("Inactive toolbar cursor check unexpectedly has keyboard focus".into());
    }
    let toolbar_root = toolbar.contentView().unwrap();
    cursor_checks += verify_button_cursors(&toolbar_root)?;
    verify_arrow_events(&toolbar_root)?;
    verify_arrow_events(&preview.contentView().unwrap())?;
    snapshot_view(
        &toolbar.contentView().unwrap(),
        &directory.join("scroll-controls.png"),
    )?;
    snapshot_view(
        &preview.contentView().unwrap(),
        &directory.join("scroll-preview.png"),
    )?;
    let live_overlay = snapshot_view(&view, &directory.join("scroll-overlay.png"))?;
    let inside = live_overlay.get_pixel((1100.0 * sx) as u32, (550.0 * sy) as u32);
    if inside[3] != 0 {
        return Err("Scrolling overlay retained the frozen screenshot inside the selection".into());
    }
    dispatch_hotkey(GlobalHotKeyEvent {
        id: state(|a| a.escape_hotkey.id()),
        state: HotKeyState::Pressed,
    });
    if state(|a| !a.sessions.is_empty() || a.scroll.is_some() || a.pending != 0)
        || window.isVisible()
        || toolbar.isVisible()
        || preview.isVisible()
        || window.ignoresMouseEvents()
    {
        return Err("Escape did not fully dispose of the scrolling capture session".into());
    }
    handle_capture(CaptureMessage {
        generation: stale_generation,
        id: 0,
        pixels: Ok(demo_image()),
        windows: vec![],
    });
    if state(|a| {
        !a.sessions.is_empty()
            || a._status_item
                .menu(mtm())
                .unwrap()
                .itemWithTag(11)
                .unwrap()
                .isEnabled()
    }) {
        return Err("A late capture callback restored a canceled overlay".into());
    }
    for cancel_tag in [115, 11, 0] {
        create_session(
            Screen {
                id: 0,
                frame: NSRect::new(
                    NSPoint::new(-10_000.0, -10_000.0),
                    NSSize::new(1440.0, 900.0),
                ),
                scale: 1.0,
            },
            demo_image(),
            false,
            vec![],
        );
        if cancel_tag == 0 {
            checks::simulate_other_app_activation()?;
        } else {
            handle_action(cancel_tag, None);
        }
        if state(|a| !a.sessions.is_empty()) {
            return Err("Cancel or a confirmed app switch left a capture session alive".into());
        }
    }
    if state(|a| a.sessions.iter().any(|s| s.window.isVisible()) || a.home.isVisible()) {
        return Err("An offscreen test window became visible".into());
    }
    println!(
        "Native UI check passed: mask, controls, modes, permission recovery, scroll confirmation, persistent controls, Escape cleanup, stale callback rejection, silent startup. Language: {}",
        i18n::language_name(i18n::current())
    );
    println!(
        "Native pointer check passed: {cursor_checks} buttons, scroll toolbar and preview, inactive panels, hover labels, eight resize cursors on exit."
    );
    close_sessions();
    Ok(())
}
