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

pub(super) fn window_check() -> Result<(), String> {
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(12));
        eprintln!("Window interaction check timed out");
        std::process::exit(1);
    });
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
    println!(
        "Window interaction check passed: WindowServer layer assignments, covered-panel regression detection, native hit testing and mouse clicks, record setup visibility, GIF/MP4 selection, scroll toolbar targeting, passive overlay cursor release, cancellation. No screen capture or permission request."
    );
    Ok(())
}
