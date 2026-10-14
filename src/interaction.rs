use crate::model::{Editor, Point, Rect, Tool};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CaptureMode {
    #[default]
    Screenshot,
    Scroll,
    Record,
    Text,
}
impl CaptureMode {
    pub const ALL: [Self; 4] = [Self::Screenshot, Self::Scroll, Self::Record, Self::Text];
    pub fn title(self) -> &'static str {
        match self {
            Self::Screenshot => "截图",
            Self::Scroll => "滚动截图",
            Self::Record => "录屏",
            Self::Text => "提取文字",
        }
    }
    pub fn tag(self) -> isize {
        match self {
            Self::Screenshot => 401,
            Self::Scroll => 402,
            Self::Record => 403,
            Self::Text => 404,
        }
    }
    pub fn from_tag(tag: isize) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.tag() == tag)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorIntent {
    Arrow,
    Crosshair,
    OpenHand,
    ClosedHand,
    IBeam,
    Horizontal,
    Vertical,
    DiagonalDown,
    DiagonalUp,
}
pub fn resize_cursor(handle: usize) -> CursorIntent {
    match handle {
        1 | 5 => CursorIntent::Vertical,
        3 | 7 => CursorIntent::Horizontal,
        0 | 4 => CursorIntent::DiagonalDown,
        _ => CursorIntent::DiagonalUp,
    }
}

pub fn cursor_at(
    editor: &Editor,
    point: Point,
    controls: &[Rect],
    grip: Option<Rect>,
) -> CursorIntent {
    if controls.iter().any(|r| r.contains(point)) {
        return CursorIntent::Arrow;
    }
    if grip.is_some_and(|r| r.contains(point)) {
        return CursorIntent::OpenHand;
    }
    if let Some(i) = editor.selected {
        if let Some(handle) = editor.annotations[i].bounds().handle_at(point) {
            return resize_cursor(handle);
        }
    }
    if let Some(selection) = editor.selection {
        if let Some(handle) = selection.handle_at(point) {
            return resize_cursor(handle);
        }
        if selection.contains(point) {
            if editor.tool.is_text() {
                return CursorIntent::IBeam;
            }
            if editor.tool == Tool::Select {
                return CursorIntent::OpenHand;
            }
        }
    }
    CursorIntent::Crosshair
}

pub fn outside_regions(bounds: Rect, selection: Rect) -> [Rect; 4] {
    let r = selection.clamp(bounds);
    [
        Rect::new(bounds.x, bounds.y, bounds.w, r.y - bounds.y),
        Rect::new(bounds.x, r.y, r.x - bounds.x, r.h),
        Rect::new(r.x + r.w, r.y, bounds.x + bounds.w - r.x - r.w, r.h),
        Rect::new(
            bounds.x,
            r.y + r.h,
            bounds.w,
            bounds.y + bounds.h - r.y - r.h,
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn controls_override_crosshair_and_all_eight_edges_have_resize_cursors() {
        let mut e = Editor::new(800.0, 600.0);
        e.selection = Some(Rect::new(100.0, 100.0, 400.0, 300.0));
        assert_eq!(
            cursor_at(
                &e,
                Point::new(300.0, 460.0),
                &[Rect::new(100.0, 430.0, 400.0, 44.0)],
                None
            ),
            CursorIntent::Arrow
        );
        for (i, p) in e.selection.unwrap().handles().into_iter().enumerate() {
            assert_eq!(cursor_at(&e, p, &[], None), resize_cursor(i));
        }
        e.tool = Tool::Text;
        assert_eq!(
            cursor_at(&e, Point::new(200.0, 200.0), &[], None),
            CursorIntent::IBeam
        );
    }
    #[test]
    fn dark_regions_cover_only_the_area_outside_the_selection() {
        let bounds = Rect::new(0.0, 0.0, 800.0, 600.0);
        let selection = Rect::new(200.0, 150.0, 300.0, 200.0);
        let regions = outside_regions(bounds, selection);
        assert_eq!(
            regions.iter().map(|r| r.w * r.h).sum::<f64>(),
            bounds.w * bounds.h - selection.w * selection.h
        );
        assert!(
            regions
                .iter()
                .all(|r| !r.contains(Point::new(300.0, 200.0)))
        );
    }
}
