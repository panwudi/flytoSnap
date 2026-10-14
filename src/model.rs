use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
    pub fn distance(self, other: Self) -> f64 {
        (self.x - other.x).hypot(self.y - other.y)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self { x, y, w, h }
    }
    pub fn between(a: Point, b: Point) -> Self {
        Self::new(
            a.x.min(b.x),
            a.y.min(b.y),
            (a.x - b.x).abs(),
            (a.y - b.y).abs(),
        )
    }
    pub fn contains(self, p: Point) -> bool {
        p.x >= self.x && p.y >= self.y && p.x <= self.x + self.w && p.y <= self.y + self.h
    }
    pub fn inset(self, d: f64) -> Self {
        Self::new(self.x + d, self.y + d, self.w - 2.0 * d, self.h - 2.0 * d)
    }
    pub fn clamp(self, bounds: Self) -> Self {
        let x = self.x.clamp(bounds.x, bounds.x + bounds.w);
        let y = self.y.clamp(bounds.y, bounds.y + bounds.h);
        Self::new(
            x,
            y,
            ((self.x + self.w).min(bounds.x + bounds.w) - x).max(0.0),
            ((self.y + self.h).min(bounds.y + bounds.h) - y).max(0.0),
        )
    }
    pub fn handles(self) -> [Point; 8] {
        let (l, t, r, b, cx, cy) = (
            self.x,
            self.y,
            self.x + self.w,
            self.y + self.h,
            self.x + self.w / 2.0,
            self.y + self.h / 2.0,
        );
        [
            Point::new(l, t),
            Point::new(cx, t),
            Point::new(r, t),
            Point::new(r, cy),
            Point::new(r, b),
            Point::new(cx, b),
            Point::new(l, b),
            Point::new(l, cy),
        ]
    }
    pub fn handle_at(self, p: Point) -> Option<usize> {
        self.handles().iter().position(|h| h.distance(p) <= 7.0)
    }
    pub fn resized(self, handle: usize, p: Point) -> Self {
        let (mut l, mut t, mut r, mut b) = (self.x, self.y, self.x + self.w, self.y + self.h);
        if [0, 6, 7].contains(&handle) {
            l = p.x.min(r - 4.0);
        }
        if [0, 1, 2].contains(&handle) {
            t = p.y.min(b - 4.0);
        }
        if [2, 3, 4].contains(&handle) {
            r = p.x.max(l + 4.0);
        }
        if [4, 5, 6].contains(&handle) {
            b = p.y.max(t + 4.0);
        }
        Self::new(l, t, r - l, b - t)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Select,
    Rectangle,
    Ellipse,
    Arrow,
    Pen,
    Highlight,
    MosaicBrush,
    MosaicRect,
    Text,
    Tag,
    Watermark,
}

impl Tool {
    pub fn title(self) -> &'static str {
        match self {
            Self::Select => "选择",
            Self::Rectangle => "矩形",
            Self::Ellipse => "椭圆",
            Self::Arrow => "箭头",
            Self::Pen => "画笔",
            Self::Highlight => "高亮",
            Self::MosaicBrush => "画笔马赛克",
            Self::MosaicRect => "框选马赛克",
            Self::Text => "文字",
            Self::Tag => "标签",
            Self::Watermark => "水印",
        }
    }
    pub fn is_text(self) -> bool {
        matches!(self, Self::Text | Self::Tag | Self::Watermark)
    }
    pub fn is_freehand(self) -> bool {
        matches!(self, Self::Pen | Self::Highlight | Self::MosaicBrush)
    }
    pub fn is_mosaic(self) -> bool {
        matches!(self, Self::MosaicBrush | Self::MosaicRect)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color(pub f64, pub f64, pub f64);
pub const PALETTE: [Color; 7] = [
    Color(1.0, 0.27, 0.28),
    Color(1.0, 0.77, 0.0),
    Color(0.19, 0.75, 0.31),
    Color(0.20, 0.42, 1.0),
    Color(0.08, 0.10, 0.13),
    Color(0.55, 0.58, 0.62),
    Color(1.0, 1.0, 1.0),
];

#[derive(Clone, Debug)]
pub struct Annotation {
    pub tool: Tool,
    pub points: Vec<Point>,
    pub color: Color,
    pub width: f64,
    pub font_size: f64,
    pub opacity: f64,
    pub text: String,
    pub flipped: bool,
}

impl Annotation {
    pub fn bounds(&self) -> Rect {
        if self.tool == Tool::Tag && !self.points.is_empty() {
            let anchor = self.points[0];
            let w = (self.text.chars().count() as f64 * self.font_size * 0.9 + 24.0).max(60.0);
            let h =
                (self.text.lines().count().max(1) as f64 * self.font_size * 1.3 + 16.0).max(36.0);
            return Rect::new(
                if self.flipped {
                    anchor.x + 16.0
                } else {
                    anchor.x - w - 16.0
                },
                anchor.y - h / 2.0,
                w,
                h,
            );
        }
        let Some(first) = self.points.first() else {
            return Rect::default();
        };
        let (mut l, mut t, mut r, mut b) = (first.x, first.y, first.x, first.y);
        for p in &self.points {
            l = l.min(p.x);
            t = t.min(p.y);
            r = r.max(p.x);
            b = b.max(p.y);
        }
        Rect::new(l, t, (r - l).max(1.0), (b - t).max(1.0))
    }
    pub fn translate(&mut self, dx: f64, dy: f64) {
        for p in &mut self.points {
            p.x += dx;
            p.y += dy;
        }
    }
    pub fn hit(&self, p: Point) -> bool {
        let b = self.bounds();
        let tolerance = self.width.max(3.0) / 2.0 + 5.0;
        match self.tool {
            Tool::Text | Tool::Tag | Tool::Watermark | Tool::MosaicRect => {
                b.inset(-5.0).contains(p)
            }
            Tool::Rectangle => b.inset(-tolerance).contains(p) && !b.inset(tolerance).contains(p),
            Tool::Ellipse => {
                let rx = b.w / 2.0;
                let ry = b.h / 2.0;
                let d = (((p.x - b.x - rx) / rx).powi(2) + ((p.y - b.y - ry) / ry).powi(2)).sqrt();
                (d - 1.0).abs() <= tolerance / rx.min(ry).max(1.0)
            }
            _ => self
                .points
                .windows(2)
                .any(|s| segment_distance(p, s[0], s[1]) <= tolerance),
        }
    }
}

fn segment_distance(p: Point, a: Point, b: Point) -> f64 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let length = dx * dx + dy * dy;
    if length == 0.0 {
        return p.distance(a);
    }
    let t = ((p.x - a.x) * dx + (p.y - a.y) * dy) / length;
    p.distance(Point::new(
        a.x + t.clamp(0.0, 1.0) * dx,
        a.y + t.clamp(0.0, 1.0) * dy,
    ))
}

#[derive(Clone, Debug, Default)]
struct Snapshot {
    selection: Option<Rect>,
    annotations: Vec<Annotation>,
}

#[derive(Clone, Debug)]
enum Gesture {
    Select(Point),
    Draw(usize),
    MoveSelection(Point, Rect),
    ResizeSelection(usize, Rect),
    MoveAnnotation(usize, Point, Annotation),
    ResizeAnnotation(usize, usize, Annotation),
}

#[derive(Clone, Debug)]
pub struct Editor {
    pub bounds: Rect,
    pub selection: Option<Rect>,
    pub annotations: Vec<Annotation>,
    pub selected: Option<usize>,
    pub tool: Tool,
    pub color: Color,
    pub width: f64,
    pub font_size: f64,
    pub opacity: f64,
    history: Vec<Snapshot>,
    future: Vec<Snapshot>,
    gesture: Option<Gesture>,
}

impl Editor {
    pub fn new(w: f64, h: f64) -> Self {
        Self {
            bounds: Rect::new(0.0, 0.0, w, h),
            selection: None,
            annotations: vec![],
            selected: None,
            tool: Tool::Select,
            color: PALETTE[0],
            width: 4.0,
            font_size: 18.0,
            opacity: 0.3,
            history: vec![],
            future: vec![],
            gesture: None,
        }
    }
    fn snapshot(&self) -> Snapshot {
        Snapshot {
            selection: self.selection,
            annotations: self.annotations.clone(),
        }
    }
    pub fn checkpoint(&mut self) {
        self.history.push(self.snapshot());
        if self.history.len() > 100 {
            self.history.remove(0);
        }
        self.future.clear();
    }
    pub fn can_undo(&self) -> bool {
        !self.history.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }
    pub fn undo(&mut self) {
        if let Some(s) = self.history.pop() {
            self.future.push(self.snapshot());
            self.restore(s);
        }
    }
    pub fn redo(&mut self) {
        if let Some(s) = self.future.pop() {
            self.history.push(self.snapshot());
            self.restore(s);
        }
    }
    fn restore(&mut self, s: Snapshot) {
        self.selection = s.selection;
        self.annotations = s.annotations;
        self.selected = None;
        self.gesture = None;
    }
    pub fn choose(&mut self, tool: Tool) {
        self.tool = tool;
        self.selected = None;
    }
    pub fn new_annotation(&self, tool: Tool, p: Point) -> Annotation {
        Annotation {
            tool,
            points: vec![p, p],
            color: self.color,
            width: self.width,
            font_size: self.font_size,
            opacity: self.opacity,
            text: String::new(),
            flipped: false,
        }
    }
    pub fn begin(&mut self, p: Point) -> bool {
        if !self.bounds.contains(p) {
            return false;
        }
        if let Some(selection) = self.selection {
            if self.tool == Tool::Select {
                if let Some(i) = self.selected {
                    if let Some(h) = self.annotations[i].bounds().handle_at(p) {
                        self.checkpoint();
                        self.gesture =
                            Some(Gesture::ResizeAnnotation(i, h, self.annotations[i].clone()));
                        return true;
                    }
                }
                if let Some(i) = self.annotations.iter().rposition(|a| a.hit(p)) {
                    self.checkpoint();
                    self.selected = Some(i);
                    self.color = self.annotations[i].color;
                    self.width = self.annotations[i].width;
                    self.font_size = self.annotations[i].font_size;
                    self.opacity = self.annotations[i].opacity;
                    self.gesture = Some(Gesture::MoveAnnotation(i, p, self.annotations[i].clone()));
                    return true;
                }
            }
            if let Some(handle) = selection.handle_at(p) {
                self.checkpoint();
                self.gesture = Some(Gesture::ResizeSelection(handle, selection));
                self.selected = None;
                return true;
            }
            if selection.contains(p) {
                if self.tool.is_text() {
                    return false;
                }
                self.checkpoint();
                if self.tool == Tool::Select {
                    self.selected = None;
                    self.gesture = Some(Gesture::MoveSelection(p, selection));
                } else {
                    let a = self.new_annotation(self.tool, p);
                    self.annotations.push(a);
                    self.gesture = Some(Gesture::Draw(self.annotations.len() - 1));
                }
                return true;
            }
        }
        self.checkpoint();
        self.selection = Some(Rect::between(p, p));
        self.annotations.clear();
        self.selected = None;
        self.gesture = Some(Gesture::Select(p));
        true
    }
    pub fn drag(&mut self, p: Point) {
        let p = Point::new(p.x.clamp(0.0, self.bounds.w), p.y.clamp(0.0, self.bounds.h));
        match self.gesture.clone() {
            Some(Gesture::Select(start)) => self.selection = Some(Rect::between(start, p)),
            Some(Gesture::Draw(i)) => {
                let a = &mut self.annotations[i];
                if a.tool.is_freehand() {
                    if a.points.last().is_none_or(|q| q.distance(p) > 0.6) {
                        a.points.push(p);
                    }
                } else {
                    a.points[1] = p;
                }
            }
            Some(Gesture::MoveSelection(start, original)) => {
                self.selection = Some(Rect::new(
                    (original.x + p.x - start.x).clamp(0.0, (self.bounds.w - original.w).max(0.0)),
                    (original.y + p.y - start.y).clamp(0.0, (self.bounds.h - original.h).max(0.0)),
                    original.w,
                    original.h,
                ));
            }
            Some(Gesture::ResizeSelection(handle, original)) => {
                self.selection = Some(original.resized(handle, p).clamp(self.bounds))
            }
            Some(Gesture::MoveAnnotation(i, start, original)) => {
                self.annotations[i] = original;
                self.annotations[i].translate(p.x - start.x, p.y - start.y);
            }
            Some(Gesture::ResizeAnnotation(i, h, original)) => {
                let old = original.bounds();
                let new = old.resized(h, p);
                self.annotations[i] = original;
                for q in &mut self.annotations[i].points {
                    // Preserve the point geometry while resizing the annotation.
                    q.x = new.x + (q.x - old.x) / old.w * new.w;
                    q.y = new.y + (q.y - old.y) / old.h * new.h;
                }
                if self.annotations[i].tool.is_text() {
                    self.annotations[i].font_size =
                        (self.annotations[i].font_size * new.w / old.w).clamp(8.0, 120.0);
                    self.font_size = self.annotations[i].font_size;
                }
            }
            None => {}
        }
    }
    pub fn finish(&mut self) {
        if let Some(Gesture::Draw(i)) = self.gesture.take() {
            if self.annotations[i].points[0].distance(*self.annotations[i].points.last().unwrap())
                < 1.0
                && !self.annotations[i].tool.is_freehand()
            {
                self.annotations.remove(i);
            }
        }
        if self.selection.is_some_and(|s| s.w < 4.0 || s.h < 4.0) {
            self.selection = None;
        }
    }
    pub fn delete_selected(&mut self) {
        if let Some(i) = self.selected.take() {
            self.checkpoint();
            self.annotations.remove(i);
        }
    }
    pub fn apply_style(&mut self) {
        if let Some(i) = self.selected {
            self.checkpoint();
            let a = &mut self.annotations[i];
            a.color = self.color;
            a.width = self.width;
            a.font_size = self.font_size;
            a.opacity = self.opacity;
            if a.tool == Tool::Text && a.points.len() > 1 {
                let width = (a.text.lines().map(|l| l.chars().count()).max().unwrap_or(1) as f64
                    * a.font_size
                    * 0.9
                    + 12.0)
                    .max(24.0);
                let height = a.text.lines().count().max(1) as f64 * a.font_size * 1.4;
                a.points[1] = Point::new(a.points[0].x + width, a.points[0].y + height);
            }
        }
    }
    pub fn flip_tag(&mut self, p: Point) -> bool {
        if let Some(i) = self
            .annotations
            .iter()
            .rposition(|a| a.tool == Tool::Tag && a.points[0].distance(p) <= 12.0)
        {
            self.checkpoint();
            self.annotations[i].flipped = !self.annotations[i].flipped;
            return true;
        }
        false
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub shortcut: String,
    pub save_directory: String,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            shortcut: "Super+Shift+KeyA".into(),
            save_directory: std::env::var("HOME")
                .map(|h| format!("{h}/Pictures/flytoSnap"))
                .unwrap_or_else(|_| ".".into()),
        }
    }
}

pub fn shortcut_display(value: &str) -> String {
    value
        .replace("Super", "Command")
        .replace("Key", "")
        .replace("Digit", "")
}

pub fn shortcut_normalize(value: &str) -> String {
    let mut value = value.trim().replace(' ', "");
    for (symbol, name) in [
        ('⌘', "Super+"),
        ('⌃', "Control+"),
        ('⌥', "Alt+"),
        ('⇧', "Shift+"),
    ] {
        value = value.replace(symbol, name);
    }
    value
        .split('+')
        .filter(|s| !s.is_empty())
        .map(|token| match token.to_lowercase().as_str() {
            "command" | "cmd" | "super" | "meta" => "Super".into(),
            "control" | "ctrl" => "Control".into(),
            "option" | "alt" => "Alt".into(),
            "shift" => "Shift".into(),
            _ if token.len() == 1 && token.as_bytes()[0].is_ascii_alphabetic() => {
                format!("Key{}", token.to_uppercase())
            }
            _ if token.len() == 1 && token.as_bytes()[0].is_ascii_digit() => {
                format!("Digit{token}")
            }
            _ => token.into(),
        })
        .collect::<Vec<_>>()
        .join("+")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn flipping_a_tag_moves_its_hit_area_and_can_be_undone() {
        let mut editor = Editor::new(500.0, 400.0);
        let mut annotation = editor.new_annotation(Tool::Tag, Point::new(250.0, 180.0));
        annotation.text = "重点".into();
        editor.annotations.push(annotation);
        assert!(editor.annotations[0].bounds().x < 250.0);
        assert!(editor.flip_tag(Point::new(250.0, 180.0)));
        assert!(editor.annotations[0].bounds().x > 250.0);
        editor.undo();
        assert!(editor.annotations[0].bounds().x < 250.0);
    }
    #[test]
    fn mac_shortcuts_accept_symbols_and_familiar_modifier_names() {
        assert_eq!(shortcut_normalize("⌘⇧A"), "Super+Shift+KeyA");
        assert_eq!(
            shortcut_normalize("Command + Shift + a"),
            "Super+Shift+KeyA"
        );
        assert_eq!(shortcut_normalize("Ctrl+Option+3"), "Control+Alt+Digit3");
        assert_eq!(shortcut_display("Super+Shift+KeyA"), "Command+Shift+A");
    }
    #[test]
    fn reversed_selection_and_resizing_stay_inside_screen() {
        let mut e = Editor::new(800.0, 600.0);
        e.begin(Point::new(300.0, 200.0));
        e.drag(Point::new(100.0, 50.0));
        e.finish();
        assert_eq!(e.selection, Some(Rect::new(100.0, 50.0, 200.0, 150.0)));
        e.begin(Point::new(300.0, 200.0));
        e.drag(Point::new(900.0, 700.0));
        e.finish();
        assert_eq!(e.selection, Some(Rect::new(100.0, 50.0, 700.0, 550.0)));
    }
    #[test]
    fn annotations_can_be_moved_and_undo_redone_without_losing_selection() {
        let mut e = Editor::new(800.0, 600.0);
        e.selection = Some(Rect::new(50.0, 50.0, 500.0, 400.0));
        e.choose(Tool::Arrow);
        e.begin(Point::new(100.0, 100.0));
        e.drag(Point::new(200.0, 150.0));
        e.finish();
        e.choose(Tool::Select);
        e.begin(Point::new(150.0, 125.0));
        e.drag(Point::new(170.0, 145.0));
        e.finish();
        assert_eq!(e.annotations[0].points[0], Point::new(120.0, 120.0));
        e.undo();
        assert_eq!(e.annotations[0].points[0], Point::new(100.0, 100.0));
        e.redo();
        assert_eq!(e.annotations[0].points[0], Point::new(120.0, 120.0));
        e.undo();
        e.undo();
        assert!(e.annotations.is_empty());
        assert!(e.selection.is_some());
    }
    #[test]
    fn rectangle_hit_testing_does_not_steal_its_empty_interior() {
        let mut e = Editor::new(500.0, 500.0);
        let mut a = e.new_annotation(Tool::Rectangle, Point::new(50.0, 50.0));
        a.points[1] = Point::new(200.0, 200.0);
        assert!(a.hit(Point::new(50.0, 125.0)));
        assert!(!a.hit(Point::new(125.0, 125.0)));
        e.annotations.push(a);
        e.selected = Some(0);
        e.delete_selected();
        assert!(e.annotations.is_empty());
        e.undo();
        assert_eq!(e.annotations.len(), 1);
    }
}
