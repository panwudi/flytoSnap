use super::*;

fn show_offscreen(window: &NSWindow) -> Result<(), String> {
    let rect = window.frame();
    if NSScreen::screens(mtm()).iter().any(|screen| {
        let frame = screen.frame();
        rect.origin.x < frame.origin.x + frame.size.width
            && rect.origin.x + rect.size.width > frame.origin.x
            && rect.origin.y < frame.origin.y + frame.size.height
            && rect.origin.y + rect.size.height > frame.origin.y
    }) {
        return Err("A test window overlaps a physical display".into());
    }
    window.orderFrontRegardless();
    Ok(())
}

fn create_scene() -> Result<Retained<OverlayWindow>, String> {
    create_session(
        Screen {
            id: 0,
            frame: NSRect::new(
                NSPoint::new(-20_000.0, -20_000.0),
                NSSize::new(1440.0, 900.0),
            ),
            scale: 1.0,
        },
        demo_image(),
        false,
        vec![],
    );
    state(|a| a.sessions[0].editor.selection = Some(Rect::new(320.0, 130.0, 800.0, 540.0)));
    refresh(0);
    let window = state(|a| a.sessions[0].window.clone());
    show_offscreen(&window)?;
    Ok(window)
}

fn pump() {
    NSRunLoop::mainRunLoop().runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.03));
}

fn find_button(root: &NSView, tag: isize) -> Option<Retained<NSButton>> {
    for child in root.subviews() {
        if child.isHidden() {
            continue;
        }
        if let Ok(button) = child.clone().downcast::<NSButton>() {
            if button.tag() == tag {
                return Some(button);
            }
        }
        if let Some(button) = find_button(&child, tag) {
            return Some(button);
        }
    }
    None
}

fn target(window: &NSWindow, tag: isize) -> Result<Retained<NSButton>, String> {
    pump();
    let root = window.contentView().ok_or("Missing content view")?;
    let button = find_button(&root, tag).ok_or_else(|| format!("Missing visible button {tag}"))?;
    let b = button.bounds();
    let center = NSPoint::new(b.size.width / 2.0, b.size.height / 2.0);
    let superview = unsafe { root.superview() };
    let local = button.convertPoint_toView(center, superview.as_deref());
    let hit = root.hitTest(local).ok_or("No native hit-test target")?;
    let expected: &NSView = &button;
    if &*hit != expected {
        return Err(format!(
            "Native hit test missed button {tag}: got {}",
            hit.class()
        ));
    }
    let screen = window.convertPointToScreen(button.convertPoint_toView(center, None));
    let main_height = objc2_core_graphics::CGDisplayBounds(objc2_core_graphics::CGMainDisplayID())
        .size
        .height;
    let point = Point::new(screen.x, main_height - screen.y);
    let ignored = state(|a| {
        a.sessions
            .iter()
            .filter(|s| s.window.ignoresMouseEvents())
            .map(|s| s.window.windowNumber() as u32)
            .collect::<Vec<_>>()
    });
    let mut ordered = capture::application_windows()?;
    let visible = NSApplication::sharedApplication(mtm())
        .windows()
        .iter()
        .filter(|window| window.isVisible())
        .map(|window| window.windowNumber() as u32)
        .collect::<Vec<_>>();
    // Offscreen records are not necessarily returned in layer order.
    ordered.sort_by_key(|window| std::cmp::Reverse(window.level));
    let front = ordered
        .iter()
        .find(|w| visible.contains(&w.id) && !ignored.contains(&w.id) && w.bounds.contains(point))
        .ok_or("No WindowServer window at the test point")?;
    if front.id != window.windowNumber() as u32 {
        return Err(format!(
            "Button {tag} is covered: target layer={}, front window={}, front layer={}",
            window.level(),
            front.id,
            front.level
        ));
    }
    Ok(button)
}

fn click(window: &NSWindow, tag: isize) -> Result<(), String> {
    let button = target(window, tag)?;
    let bounds = button.bounds();
    let point = NSPoint::new(bounds.size.width / 2.0, bounds.size.height / 2.0);
    let location = button.convertPoint_toView(point, None);
    let event = |kind| {
        NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
        kind,location,NSEventModifierFlags::empty(),0.0,window.windowNumber(),None,0,1,1.0,
    ).ok_or("Could not create click event")
    };
    let up = event(NSEventType::LeftMouseUp)?;
    NSApplication::sharedApplication(mtm()).postEvent_atStart(&up, false);
    let down = event(NSEventType::LeftMouseDown)?;
    window.sendEvent(&down);
    pump();
    Ok(())
}

pub(super) fn wait_for_ocr() -> Result<Retained<NSPanel>, String> {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        feature_tick();
        let (window, ready, status) = state(|a| {
            let panel = a.ocr_panel.as_ref().ok_or("OCR result panel disappeared")?;
            Ok::<_, String>((
                panel.window.clone(),
                panel.copy.isEnabled(),
                panel.status.stringValue().to_string(),
            ))
        })?;
        if ready {
            return Ok(window);
        }
        if status != tr("正在识别文字...") || Instant::now() >= deadline {
            return Err(format!("OCR did not finish successfully: {status}"));
        }
        pump();
    }
}

fn check_ocr() -> Result<(), String> {
    let overlay = create_scene()?;
    click(&overlay, 404)?;
    let panel = wait_for_ocr()?;
    show_offscreen(&panel)?;
    if panel.level() <= overlay.level() {
        return Err("OCR result is behind the capture overlay".into());
    }
    for tag in [501, 502, 503] {
        let button = target(&panel, tag)?;
        verify_arrow_events(&button)?;
    }
    let text = state(|a| a.ocr_panel.as_ref().unwrap().text.clone());
    let recognized = text.string().to_string();
    if !text.isEditable() || !recognized.contains(&tr("项目进度")) || recognized.contains("Laura")
    {
        return Err(format!(
            "OCR selection crop or editing failed: {recognized:?}"
        ));
    }
    let board = NSPasteboard::pasteboardWithUniqueName();
    let edited = "Edited text\n简体中文 / 繁體中文 123";
    text.setString(&NSString::from_str(edited));
    copy_ocr_to(&board).map_err(tr)?;
    if board
        .stringForType(unsafe { NSPasteboardTypeString })
        .map(|s| s.to_string())
        .as_deref()
        != Some(edited)
    {
        return Err("OCR copy did not preserve the edited multiline text".into());
    }
    let change_count = board.changeCount();
    text.setString(&NSString::from_str(" \n"));
    if copy_ocr_to(&board).is_ok() || board.changeCount() != change_count {
        return Err("Empty OCR result overwrote the clipboard".into());
    }
    let (old_job, tx) = state(|a| (a.feature_job, a.ocr_tx.clone()));
    click(&panel, 502)?;
    if panel.isVisible()
        || state(|a| a.ocr_panel.is_some() || a.sessions[0].editor.selection.is_some())
    {
        return Err("OCR reselect did not dismiss the result and clear selection".into());
    }
    tx.send((old_job, Ok("stale result".into())))
        .map_err(|e| e.to_string())?;
    feature_tick();
    if state(|a| a.ocr_panel.is_some()) {
        return Err("A stale OCR job restored its panel".into());
    }
    state(|a| a.sessions[0].editor.selection = Some(Rect::new(320.0, 130.0, 800.0, 540.0)));
    begin_ocr(0);
    let panel = state(|a| a.ocr_panel.as_ref().unwrap().window.clone());
    show_offscreen(&panel)?;
    panel.close();
    if overlay.isVisible() || state(|a| !a.sessions.is_empty() || a.ocr_panel.is_some()) {
        return Err("Closing OCR during recognition left a capture overlay active".into());
    }
    println!(
        "OCR interaction check passed: native mode click, selection-only recognition, editable results, Unicode clipboard, empty-copy protection, reselect, stale jobs, close cleanup."
    );
    Ok(())
}

fn notify_workspace_activation(application: &NSRunningApplication) {
    let info = NSDictionary::from_slices(
        &[unsafe { NSWorkspaceApplicationKey }],
        &[application as &AnyObject],
    );
    // Widen NSString keys to AnyObject for NSNotification's untyped userInfo.
    let info: Retained<NSDictionary> = unsafe { Retained::cast_unchecked(info) };
    let notification = unsafe {
        NSNotification::notificationWithName_object_userInfo(
            NSWorkspaceDidActivateApplicationNotification,
            Some(&NSWorkspace::sharedWorkspace()),
            Some(&info),
        )
    };
    let delegate = state(|a| a.delegate.clone());
    unsafe {
        let _: () = msg_send![&*delegate,workspaceActivated:&*notification];
    }
}

fn other_regular_app() -> Result<Retained<NSRunningApplication>, String> {
    NSWorkspace::sharedWorkspace()
        .runningApplications()
        .into_iter()
        .find(|app| {
            app.processIdentifier() != std::process::id() as i32
                && app.activationPolicy() == NSApplicationActivationPolicy::Regular
        })
        .ok_or("No other regular app is available for the focus notification check".into())
}

pub(super) fn simulate_other_app_activation() -> Result<(), String> {
    let application = other_regular_app()?;
    notify_workspace_activation(&application);
    Ok(())
}

fn check_large_selection_focus() -> Result<(), String> {
    let window = create_scene()?;
    let notification = unsafe {
        NSNotification::notificationWithName_object(NSApplicationDidResignActiveNotification, None)
    };
    let delegate = state(|a| a.delegate.clone());
    let other = other_regular_app()?;
    let menu = state(|a| a._status_item.menu(mtm()).unwrap());
    for region in [
        Rect::new(0.0, 0.0, 1440.0, 900.0),
        Rect::new(0.0, 24.0, 1440.0, 876.0),
        Rect::new(4.0, 8.0, 1432.0, 884.0),
        Rect::new(320.0, 130.0, 800.0, 540.0),
    ] {
        state(|a| a.sessions[0].editor.selection = Some(region));
        refresh(0);
        unsafe {
            let _: () = msg_send![&*delegate,applicationDidResignActive:&*notification];
        }
        if state(|a| a.sessions.is_empty()) {
            return Err("Temporary deactivation discarded a large capture selection before its toolbar action".into());
        }
        notify_workspace_activation(&NSRunningApplication::currentApplication());
        unsafe {
            let _: () = msg_send![&*delegate,menuWillOpen:&*menu];
        }
        notify_workspace_activation(&other);
        if state(|a| a.sessions.is_empty()) {
            return Err("Opening the status menu discarded the capture selection".into());
        }
        unsafe {
            let _: () = msg_send![&*delegate,menuDidClose:&*menu];
        }
        click(&window, 100)?;
        click(&window, 213)?;
        if !state(|a| {
            a.sessions[0].editor.tool == Tool::Rectangle
                && a.sessions[0].editor.selection == Some(region)
                && a.sessions[0].editor.color == PALETTE[3]
        }) {
            return Err("Large-selection toolbar or color action changed the selection or failed to activate".into());
        }
    }
    notify_workspace_activation(&other);
    if window.isVisible() || state(|a| !a.sessions.is_empty()) {
        return Err("A confirmed switch to another app left capture active".into());
    }
    println!(
        "Capture focus check passed: full-screen and large selections, temporary deactivation, status-menu tracking, native toolbar/color clicks, confirmed app-switch cleanup."
    );
    Ok(())
}

fn drag_menu(
    window: &NSWindow,
    view: &NSView,
    start: Point,
    delta: Point,
    tracked: bool,
) -> Result<(), String> {
    let event = |kind, point: Point| {
        let location = view.convertPoint_toView(NSPoint::new(point.x, point.y), None);
        NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(kind,location,NSEventModifierFlags::empty(),0.0,window.windowNumber(),None,0,1,1.0).ok_or("Could not construct menu drag event")
    };
    let down = event(NSEventType::LeftMouseDown, start)?;
    let end = Point::new(start.x + delta.x, start.y + delta.y);
    let dragged = event(NSEventType::LeftMouseDragged, end)?;
    let up = event(NSEventType::LeftMouseUp, end)?;
    if tracked {
        let app = NSApplication::sharedApplication(mtm());
        app.postEvent_atStart(&dragged, false);
        app.postEvent_atStart(&up, false);
        window.sendEvent(&down);
    } else {
        window.sendEvent(&down);
        window.sendEvent(&dragged);
        window.sendEvent(&up);
    }
    pump();
    Ok(())
}

fn check_mode_menu_drag() -> Result<(), String> {
    let window = create_scene()?;
    window.makeKeyWindow();
    let view = state(|a| a.sessions[0].view.clone());
    let selection = state(|a| {
        let selection = a.sessions[0].editor.selection;
        a.sessions[0].editor.selection = None;
        selection
    });
    refresh(0);
    let initial = state(|a| mode_layout(&a.sessions[0]).0);
    drag_menu(
        &window,
        &view,
        Point::new(initial.x + 17.0, initial.y + 22.0),
        Point::new(110.0, 70.0),
        false,
    )?;
    let moved = state(|a| mode_layout(&a.sessions[0]).0);
    if moved == initial || state(|a| a.sessions[0].editor.selection.is_some()) {
        return Err(format!(
            "Mode grip failed: initial={initial:?}, moved={moved:?}, selection={:?}, drag={:?}, override={:?}",
            state(|a| a.sessions[0].editor.selection),
            state(|a| a.sessions[0].mode_drag),
            state(|a| a.sessions[0].mode_override)
        ));
    }
    state(|a| a.sessions[0].editor.selection = selection);
    refresh(0);
    let button = target(&window, 402)?;
    let frame = button.convertRect_toView(button.bounds(), Some(&view));
    drag_menu(
        &window,
        &view,
        Point::new(
            frame.origin.x + frame.size.width / 2.0,
            frame.origin.y + 20.0,
        ),
        Point::new(-80.0, 50.0),
        true,
    )?;
    if state(|a| {
        a.mode != CaptureMode::Screenshot
            || a.sessions[0].editor.selection != selection
            || a.sessions[0].mode_drag.is_some()
    }) {
        return Err(
            "Dragging a mode button activated its action or changed the capture selection".into(),
        );
    }
    let before_click = state(|a| mode_layout(&a.sessions[0]).0);
    click(&window, 402)?;
    if state(|a| {
        a.mode != CaptureMode::Scroll
            || a.sessions[0].editor.selection != selection
            || mode_layout(&a.sessions[0]).0 != before_click
    }) {
        return Err("A mode click after dragging lost its action or moved the menu".into());
    }
    click(&window, 401)?;
    let (menu, editing_bar) = state(|a| (mode_layout(&a.sessions[0]).0, bar(&a.sessions[0])));
    drag_menu(
        &window,
        &view,
        Point::new(menu.x + 17.0, menu.y + 22.0),
        Point::new(editing_bar.x - menu.x, editing_bar.y - menu.y),
        false,
    )?;
    click(&window, 402)?;
    click(&window, 401)?;
    if state(|a| a.sessions[0].editor.selection != selection) {
        return Err("An overlapping mode menu changed the selection".into());
    }
    for delta in [Point::new(-9000.0, -9000.0), Point::new(9000.0, 9000.0)] {
        let menu = state(|a| mode_layout(&a.sessions[0]).0);
        drag_menu(
            &window,
            &view,
            Point::new(menu.x + 17.0, menu.y + 22.0),
            delta,
            false,
        )?;
        let menu = state(|a| mode_layout(&a.sessions[0]).0);
        if menu.x < 8.0
            || menu.y < 8.0
            || menu.x + menu.w > 1440.0 - 8.0
            || menu.y + menu.h > 900.0 - 8.0
        {
            return Err("The draggable mode menu escaped the screen".into());
        }
    }
    let menu = state(|a| mode_layout(&a.sessions[0]).0);
    let hover = pointer_event(
        &view,
        NSPoint::new(menu.x + 17.0, menu.y + 22.0),
        NSEventType::MouseMoved,
    )?;
    mouse_move(0, &hover);
    if NSCursor::currentCursor() != NSCursor::openHandCursor() {
        return Err("The mode menu grip did not show a hand cursor".into());
    }
    // Exercise Escape while the native mode-button tracking loop owns the mouse.
    let button = target(&window, 402)?;
    let frame = button.convertRect_toView(button.bounds(), Some(&view));
    let location = view.convertPoint_toView(
        NSPoint::new(frame.origin.x + 20.0, frame.origin.y + 20.0),
        None,
    );
    let escape=NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(NSEventType::KeyDown,NSPoint::ZERO,NSEventModifierFlags::empty(),0.0,window.windowNumber(),None,ns_string!("\u{1b}"),ns_string!("\u{1b}"),false,53).ok_or("Could not construct Escape event")?;
    NSApplication::sharedApplication(mtm()).postEvent_atStart(&escape, false);
    let down=NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(NSEventType::LeftMouseDown,location,NSEventModifierFlags::empty(),0.0,window.windowNumber(),None,0,1,1.0).ok_or("Could not construct mode tracking event")?;
    window.sendEvent(&down);
    if window.isVisible() || state(|a| !a.sessions.is_empty()) {
        return Err("Escape during a menu drag left the overlay alive".into());
    }
    println!(
        "Mode menu check passed: grip and button drags, click threshold, selection preservation, screen boundaries, hand cursor, Escape during tracking."
    );
    Ok(())
}

pub(super) fn window_check() -> Result<(), String> {
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(30));
        eprintln!("Window interaction check timed out");
        std::process::exit(1);
    });
    check_mode_menu_drag()?;
    check_large_selection_focus()?;
    let overlay = create_scene()?;
    click(&overlay, 403)?;
    let setup = state(|a| a.record_setup.as_ref().map(|s| s.window.clone()))
        .ok_or("Recording mode did not open its setup")?;
    show_offscreen(&setup)?;
    if setup.level() <= overlay.level() {
        return Err("Recording setup is behind the capture overlay".into());
    }
    overlay.setLevel(CAPTURE_CONTROL_LEVEL + 1);
    let covered = target(&setup, 603);
    overlay.setLevel(CAPTURE_OVERLAY_LEVEL);
    if !covered.is_err_and(|error| error.contains("is covered")) {
        let layers = capture::application_windows()?
            .iter()
            .map(|w| (w.id, w.level, w.bounds))
            .collect::<Vec<_>>();
        return Err(format!(
            "WindowServer check did not detect the previous covered-panel regression: overlay={}, setup={}, layers={layers:?}",
            overlay.windowNumber(),
            setup.windowNumber()
        ));
    }
    show_offscreen(&setup)?;
    click(&setup, 603)?;
    if !state(|a| {
        a.record_setup
            .as_ref()
            .is_some_and(|s| s.format == super::super::media::Format::Gif)
    }) {
        return Err("GIF format did not activate".into());
    }
    click(&setup, 602)?;
    if !state(|a| {
        a.record_setup
            .as_ref()
            .is_some_and(|s| s.format == super::super::media::Format::Mp4)
    }) {
        return Err("MP4 format did not activate".into());
    }
    click(&setup, 604)?;
    if state(|a| !a.sessions.is_empty() || a.record_setup.is_some()) {
        return Err("Cancel left recording setup or capture active".into());
    }
    let overlay = create_scene()?;
    click(&overlay, 402)?;
    click(&overlay, 304)?;
    let (toolbar, preview) = state(|a| {
        a.scroll
            .as_ref()
            .map(|s| (s.panel.clone(), s.preview_panel.clone()))
    })
    .ok_or("Scroll start did not create controls")?;
    show_offscreen(&toolbar)?;
    show_offscreen(&preview)?;
    if toolbar.level() <= overlay.level() || preview.level() <= overlay.level() {
        return Err("Scroll controls are behind the capture overlay".into());
    }
    if overlay.areCursorRectsEnabled() {
        return Err("Pass-through overlay still owns cursor rectangles".into());
    }
    for tag in [301, 302, 303, 305, 306] {
        let button = target(&toolbar, tag)?;
        verify_arrow_events(&button)?;
        let event = pointer_event(
            &button,
            NSPoint::new(
                button.bounds().size.width / 2.0,
                button.bounds().size.height / 2.0,
            ),
            NSEventType::MouseMoved,
        )?;
        toolbar.sendEvent(&event);
        if NSCursor::currentCursor() != NSCursor::arrowCursor() {
            return Err(format!("Window dispatch replaced arrow at button {tag}"));
        }
    }
    let cursor = NSCursor::currentCursor();
    let event = pointer_event(
        &state(|a| a.sessions[0].view.clone()),
        NSPoint::new(500.0, 500.0),
        NSEventType::MouseMoved,
    )?;
    mouse_move(0, &event);
    if NSCursor::currentCursor() != cursor {
        return Err("Ignored overlay changed the toolbar cursor".into());
    }
    click(&toolbar, 303)?;
    if state(|a| !a.sessions.is_empty() || a.scroll.is_some()) {
        return Err("Cancel left scrolling capture active".into());
    }
    check_ocr()?;
    println!(
        "Window interaction check passed: WindowServer layer assignments, covered-panel regression detection, native hit testing and mouse clicks, record setup visibility, GIF/MP4 selection, scroll toolbar targeting, passive overlay cursor release, cancellation. No screen capture or permission request."
    );
    Ok(())
}
