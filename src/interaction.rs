use crate::model::{Editor, Point, Rect, Tool};

pub fn mode_menu_rect(bounds: Rect, width: f64, position: Option<Point>) -> Rect {
    let margin = 8.0_f64.min(bounds.w / 4.0).min(bounds.h / 4.0);
    let width = width.min(bounds.w - margin * 2.0).max(1.0);
    let height = 46.0_f64.min(bounds.h - margin * 2.0).max(1.0);
    let position = position.unwrap_or(Point::new(
        bounds.x + (bounds.w - width) / 2.0,
        bounds.y + 18.0,
    ));
    Rect::new(
        position.x.clamp(
            bounds.x + margin,
            (bounds.x + bounds.w - width - margin).max(bounds.x + margin),
        ),
        position.y.clamp(
            bounds.y + margin,
            (bounds.y + bounds.h - height - margin).max(bounds.y + margin),
        ),
        width,
        height,
    )
}

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
    pub fn selection_prompt(self) -> &'static str {
        match self {
            Self::Screenshot => "拖动选择截图区域   Esc 取消",
            Self::Scroll => "框选滚动区域, 然后点击开始滚动截图. Esc 取消",
            Self::Record => "框选录屏区域, 松开后选择 MP4 或 GIF. Esc 取消",
            Self::Text => "框选需要提取文字的区域. Esc 取消",
        }
    }
    pub fn confirm_hint(self) -> &'static str {
        match self {
            Self::Screenshot => "Esc 退出 / Enter 复制",
            Self::Scroll => "Esc 退出 / Enter 开始",
            Self::Record => "Esc 退出 / Enter 录制",
            Self::Text => "Esc 退出 / Enter 复制文字",
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

pub fn scroll_start_rect(bounds: Rect, selection: Rect) -> Rect {
    let width = 258.0_f64.min(bounds.w - 16.0);
    Rect::new(
        (selection.x + (selection.w - width) / 2.0)
            .clamp(bounds.x + 8.0, bounds.x + bounds.w - width - 8.0),
        (selection.y + selection.h - 54.0).clamp(bounds.y + 8.0, bounds.y + bounds.h - 50.0),
        width,
        42.0,
    )
}

pub fn scroll_panel_rects(bounds: Rect, selection: Rect) -> (Rect, Rect) {
    let margin = 10.0;
    let toolbar_width = 350.0_f64.min(bounds.w - 2.0 * margin);
    let toolbar_height = 78.0;
    let toolbar_y = if selection.y + selection.h + toolbar_height + margin <= bounds.y + bounds.h {
        selection.y + selection.h + margin
    } else if selection.y - toolbar_height - margin >= bounds.y {
        selection.y - toolbar_height - margin
    } else {
        selection.y + selection.h - toolbar_height - margin
    };
    let toolbar = Rect::new(
        (selection.x + selection.w - toolbar_width).clamp(
            bounds.x + margin,
            bounds.x + bounds.w - toolbar_width - margin,
        ),
        toolbar_y.clamp(
            bounds.y + 32.0,
            bounds.y + bounds.h - toolbar_height - margin,
        ),
        toolbar_width,
        toolbar_height,
    );
    let preview_width = 196.0;
    let preview_height =
        (selection.h * preview_width / selection.w).clamp(150.0, (bounds.h * 0.55).max(150.0));
    let preview_x = if selection.x + selection.w + preview_width + margin <= bounds.x + bounds.w {
        selection.x + selection.w + margin
    } else if selection.x - preview_width - margin >= bounds.x {
        selection.x - preview_width - margin
    } else {
        bounds.x + bounds.w - preview_width - margin
    };
    let preview = Rect::new(
        preview_x,
        selection.y.clamp(
            bounds.y + 32.0,
            bounds.y + bounds.h - preview_height - margin,
        ),
        preview_width,
        preview_height,
    );
    (toolbar, preview)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mode_menu_stays_inside_viewport_after_drag_resize_and_locale_changes() {
        for bounds in [
            Rect::new(0.0, 0.0, 1440.0, 900.0),
            Rect::new(300.0, 200.0, 640.0, 400.0),
        ] {
            for width in [440.0, 820.0] {
                for position in [
                    None,
                    Some(Point::new(-5000.0, -5000.0)),
                    Some(Point::new(9000.0, 9000.0)),
                ] {
                    let menu = mode_menu_rect(bounds, width, position);
                    assert!(menu.x >= bounds.x + 8.0 && menu.y >= bounds.y + 8.0);
                    assert!(menu.x + menu.w <= bounds.x + bounds.w - 8.0);
                    assert!(menu.y + menu.h <= bounds.y + bounds.h - 8.0);
                }
            }
        }
    }

    #[test]
    fn scroll_controls_stay_on_screen_for_edge_and_fullscreen_regions() {
        let screen = Rect::new(0.0, 0.0, 1440.0, 900.0);
        for region in [
            screen,
            Rect::new(1.0, 1.0, 80.0, 80.0),
            Rect::new(1200.0, 700.0, 240.0, 200.0),
            Rect::new(320.0, 130.0, 800.0, 540.0),
        ] {
            let (toolbar, preview) = scroll_panel_rects(screen, region);
            for rect in [toolbar, preview, scroll_start_rect(screen, region)] {
                assert!(rect.x >= 0.0 && rect.y >= 0.0);
                assert!(rect.x + rect.w <= screen.w && rect.y + rect.h <= screen.h);
            }
        }
    }
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
