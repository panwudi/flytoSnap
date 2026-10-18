use super::*;

pub(super) const INK: Color = Color(0.19, 0.22, 0.28);
pub(super) const SECONDARY: Color = Color(0.43, 0.45, 0.48);
pub(super) const SURFACE: Color = Color(0.966, 0.957, 0.923);
pub(super) const BRAND: Color = Color(239.0 / 255.0, 130.0 / 255.0, 0.0);
const CREAM: Color = Color(0.998, 0.991, 0.963);
const EDGE: Color = Color(0.74, 0.74, 0.71);

fn path(rect: Rect, radius: f64) -> Retained<NSBezierPath> {
    NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(nr(rect), radius, radius)
}
fn gradient(rect: Rect, radius: f64, top: Color, bottom: Color) {
    if let Some(fill) = NSGradient::initWithStartingColor_endingColor(
        NSGradient::alloc(),
        &render::color(top, 1.0),
        &render::color(bottom, 1.0),
    ) {
        fill.drawInBezierPath_angle(&path(rect, radius), 90.0);
    }
}
fn outline(rect: Rect, radius: f64, color: Color, alpha: f64) {
    render::color(color, alpha).setStroke();
    let p = path(rect, radius);
    p.setLineWidth(0.7);
    p.stroke();
}
pub(super) fn enamel(rect: Rect, radius: f64, pressed: bool) {
    if pressed {
        gradient(rect, radius, Color(0.87, 0.87, 0.83), SURFACE);
    } else {
        render::rounded(
            Rect::new(rect.x, rect.y + 1.2, rect.w, rect.h),
            radius,
            Color(0.35, 0.37, 0.40),
            0.16,
        );
        gradient(rect, radius, CREAM, Color(0.938, 0.931, 0.902));
    }
    outline(rect, radius, EDGE, 0.75);
    outline(
        rect.inset(1.0),
        (radius - 1.0).max(0.0),
        Color(1.0, 1.0, 1.0),
        if pressed { 0.22 } else { 0.85 },
    );
}
pub(super) fn panel_surface(bounds: Rect, floating: bool) {
    if floating {
        enamel(bounds.inset(1.0), 11.0, false);
    } else {
        gradient(bounds, 0.0, CREAM, SURFACE);
    }
}
pub(super) fn mode_shell(rect: Rect) {
    gradient(rect, 10.0, Color(0.32, 0.36, 0.43), Color(0.22, 0.26, 0.33));
    outline(rect.inset(0.5), 9.5, Color(0.12, 0.15, 0.21), 0.7);
    outline(rect.inset(1.3), 8.7, Color(0.85, 0.88, 0.91), 0.18);
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind=MainThreadOnly]
    struct SettingsSurface;
    impl SettingsSurface {
        #[unsafe(method(drawRect:))]
        fn draw(&self,_rect:NSRect) {
            let bounds=self.bounds();
            let rect=Rect::new(0.5,0.5,bounds.size.width-1.0,bounds.size.height-1.0);
            gradient(rect,13.0,Color(0.975,0.970,0.950),Color(0.995,0.991,0.975));
            outline(rect,13.0,Color(0.76,0.77,0.75),0.65);
            outline(rect.inset(1.0),12.0,Color(1.0,1.0,1.0),0.9);
        }
    }
);

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind=MainThreadOnly]
    struct InputSurface;
    impl InputSurface {
        #[unsafe(method(drawRect:))]
        fn draw(&self,_rect:NSRect) {
            let bounds=self.bounds();
            let rect=Rect::new(0.5,0.5,bounds.size.width-1.0,bounds.size.height-1.0);
            gradient(rect,7.0,Color(0.93,0.93,0.90),Color(0.999,0.997,0.985));
            outline(rect,7.0,Color(0.71,0.73,0.73),0.7);
            outline(rect.inset(1.0),6.0,Color(1.0,1.0,1.0),0.75);
        }
    }
);

pub(super) fn input(root: &NSView, rect: Rect) -> Retained<NSTextField> {
    let well: Retained<InputSurface> =
        unsafe { msg_send![InputSurface::alloc(mtm()),initWithFrame:nr(rect)] };
    root.addSubview(&well);
    let field = NSTextField::initWithFrame(
        NSTextField::alloc(mtm()),
        nr(Rect::new(rect.x + 10.0, rect.y + 7.0, rect.w - 20.0, 20.0)),
    );
    field.setBezeled(false);
    field.setBordered(false);
    field.setDrawsBackground(false);
    field.setFont(Some(&NSFont::systemFontOfSize(14.0)));
    field.setTextColor(Some(&render::color(INK, 1.0)));
    field
}
pub(super) fn surface(root: &NSView, rect: Rect) {
    let view: Retained<SettingsSurface> =
        unsafe { msg_send![SettingsSurface::alloc(mtm()),initWithFrame:nr(rect)] };
    root.addSubview(&view);
}
pub(super) fn text(
    root: &NSView,
    title: &str,
    rect: Rect,
    size: f64,
    secondary: bool,
) -> Retained<NSTextField> {
    let view = label(title, nr(rect), size);
    view.setTextColor(Some(&render::color(
        if secondary { SECONDARY } else { INK },
        1.0,
    )));
    root.addSubview(&view);
    view
}
pub(super) fn icon(root: &NSView, rect: Rect) {
    let preview = state(|a| a.headless)
        .then(|| {
            std::env::args()
                .skip_while(|arg| arg != "--preview-icon")
                .nth(1)
        })
        .flatten();
    let file = preview
        .map(|path| NSString::from_str(&path))
        .or_else(|| {
            NSBundle::mainBundle()
                .pathForResource_ofType(Some(ns_string!("AppIcon")), Some(ns_string!("png")))
        })
        .unwrap_or_else(|| NSString::from_str("target/app-icon.png"));
    if let Some(image) = NSImage::initWithContentsOfFile(NSImage::alloc(), &file) {
        let view = NSImageView::initWithFrame(NSImageView::alloc(mtm()), nr(rect));
        view.setImage(Some(&image));
        view.setImageScaling(NSImageScaling::ScaleProportionallyUpOrDown);
        root.addSubview(&view);
    }
}
pub(super) fn primary(button: &NSButton) {
    button.setBordered(false);
    NSView::setNeedsDisplay(button, true);
}
fn is_primary(tag: isize) -> bool {
    matches!(tag, 1 | 30 | 116 | 304 | 305 | 501 | 601 | 608 | 903)
}

pub(super) fn draw_button(button: &NSButton, hovered: bool) -> bool {
    let tag = button.tag();
    // Preserve native checkbox drawing and keyboard behavior.
    if tag == 0 || (210..=216).contains(&tag) {
        return false;
    }
    let mode = (401..=404).contains(&tag);
    let selected = button.state() == 1 && matches!(tag, 401..=404 | 602 | 603);
    let primary = is_primary(tag);
    let has_image = button.image().is_some();
    let bordered = button.isBordered();
    let pressed = button.cell().is_some_and(|cell| cell.isHighlighted());
    let bounds = button.bounds();
    let rect = Rect::new(1.5, 2.0, bounds.size.width - 3.0, bounds.size.height - 5.0);
    if primary {
        let top = if pressed {
            Color(0.89, 0.43, 0.0)
        } else if hovered {
            Color(1.0, 0.68, 0.25)
        } else {
            Color(1.0, 0.65, 0.18)
        };
        let bottom = if pressed {
            Color(0.97, 0.57, 0.06)
        } else {
            BRAND
        };
        gradient(rect, 7.0, top, bottom);
        outline(rect, 7.0, Color(0.69, 0.34, 0.0), 0.7);
        outline(
            rect.inset(1.0),
            6.0,
            Color(1.0, 0.88, 0.60),
            if pressed { 0.15 } else { 0.75 },
        );
    } else if selected || (bordered && !has_image) || (!mode && (hovered || pressed)) {
        enamel(rect, 6.0, pressed || selected);
        if selected {
            render::rounded(
                Rect::new(
                    rect.x + rect.w / 2.0 - 9.0,
                    rect.y + rect.h - 4.0,
                    18.0,
                    2.0,
                ),
                1.0,
                BRAND,
                1.0,
            );
        }
    } else if mode && (hovered || pressed) {
        render::rounded(
            rect,
            6.0,
            Color(1.0, 1.0, 1.0),
            if pressed { 0.2 } else { 0.10 },
        );
    }
    if has_image {
        return false;
    }
    if !(primary || bordered || mode) {
        return false;
    }
    let size = if mode { 15.0 } else { 13.0 };
    let font = if primary {
        NSFont::boldSystemFontOfSize(size)
    } else {
        NSFont::systemFontOfSize(size)
    };
    let ink = render::color(
        if mode && !selected { CREAM } else { INK },
        if button.isEnabled() { 1.0 } else { 0.45 },
    );
    let attributes = NSDictionary::from_slices(
        &[unsafe { NSFontAttributeName }, unsafe {
            NSForegroundColorAttributeName
        }],
        &[&*font as &AnyObject, &*ink as &AnyObject],
    );
    let title = button.title();
    unsafe {
        let size = title.sizeWithAttributes(Some(&attributes));
        title.drawAtPoint_withAttributes(
            NSPoint::new(
                (bounds.size.width - size.width) / 2.0,
                (bounds.size.height - size.height) / 2.0 + if pressed { 1.0 } else { 0.0 },
            ),
            Some(&attributes),
        );
    }
    true
}
