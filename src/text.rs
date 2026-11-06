//! What the source editor and text fields share: shaping and painting one
//! logical line, soft wrapped, and the text conventions of the platform's
//! input system.

use crate::theme::Theme;
use gpui::*;
use std::ops::Range;
use std::{cell::RefCell, rc::Rc};

/// `offset`, a byte offset into `text`, in UTF-16 units, as input methods count.
pub fn to_utf16(text: &str, offset: usize) -> usize {
    text[..offset.min(text.len())].encode_utf16().count()
}

/// The byte offset of `units` UTF-16 units into `text`.
pub fn from_utf16(text: &str, units: usize) -> usize {
    let mut seen = 0;
    for (i, ch) in text.char_indices() {
        if seen >= units { return i; }
        seen += ch.len_utf16();
    }
    text.len()
}

pub fn range_to_utf16(text: &str, range: &Range<usize>) -> Range<usize> {
    to_utf16(text, range.start)..to_utf16(text, range.end)
}

pub fn range_from_utf16(text: &str, range: &Range<usize>) -> Range<usize> {
    from_utf16(text, range.start)..from_utf16(text, range.end)
}

/// `text` with every line ending as `\n`.
pub fn plain_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// Where a line's shape lands once layout has measured it. It stays empty for
/// a line without text.
pub type Shaped = Rc<RefCell<Option<WrappedLine>>>;

/// Requests a layout that fills the available width and is as tall as `text`
/// shapes, wrapped at that width when `wrap` is set.
pub fn request_line(text: SharedString, runs: Vec<TextRun>, size: Pixels, line_height: Pixels, wrap: bool, window: &mut Window) -> (LayoutId, Shaped) {
    let shaped = Shaped::default();
    let slot = shaped.clone();
    let mut style = Style::default();
    style.size.width = relative(1.).into();
    let id = window.request_measured_layout(style, move |known, available, window, _| {
        let width = known.width.or(match available.width { AvailableSpace::Definite(w) => Some(w), _ => None });
        if text.is_empty() { return gpui::size(width.unwrap_or(px(0.)), line_height); }
        let line = window.text_system().shape_text(text.clone(), size, &runs, width.filter(|_| wrap), None).unwrap_or_default().into_iter().next();
        let measured = line.as_ref().map_or(line_height, |l| l.size(line_height).height);
        *slot.borrow_mut() = line;
        gpui::size(width.unwrap_or(px(0.)), measured)
    });
    (id, shaped)
}

/// A line as it was last painted, for hit testing and placing a caret. With no
/// `layout` it was painted, and is empty.
#[derive(Clone)]
pub struct PaintedLine {
    pub bounds: Bounds<Pixels>,
    pub layout: Option<WrappedLine>,
    pub line_height: Pixels,
}

impl PaintedLine {
    /// Where byte `index` of the line sits, relative to the line.
    pub fn position(&self, index: usize) -> Option<Point<Pixels>> {
        match &self.layout {
            Some(layout) => layout.position_for_index(index, self.line_height),
            None => Some(Point::default()),
        }
    }

    /// The nearest byte to a point relative to the line.
    pub fn index_at(&self, local: Point<Pixels>) -> usize {
        self.layout.as_ref().map_or(0, |layout| {
            let y = local.y.clamp(px(0.), self.height() - px(1.));
            layout.closest_index_for_position(point(local.x, y), self.line_height).unwrap_or_else(|i| i)
        })
    }

    /// The nearest byte to a point in window coordinates.
    pub fn index_for(&self, position: Point<Pixels>) -> usize {
        self.index_at(position - self.bounds.origin)
    }

    /// The byte at a point in window coordinates, only if a glyph is there:
    /// the rest of a row is empty space, for what the nearest byte would be wrong.
    pub fn text_at(&self, position: Point<Pixels>) -> Option<usize> {
        if !self.bounds.contains(&position) { return None; }
        self.layout.as_ref()?.index_for_position(position - self.bounds.origin, self.line_height).ok()
    }

    pub fn height(&self) -> Pixels {
        self.layout.as_ref().map_or(self.line_height, |layout| layout.size(self.line_height).height)
    }

    /// Paints a wavy underline beneath `range`, on each visual row it reaches.
    fn underline(&self, range: &Range<usize>, color: Hsla, window: &mut Window) {
        let Some(layout) = &self.layout else { return };
        let ends = layout.wrap_boundaries().iter().map(|wrap| layout.runs()[wrap.run_ix].glyphs[wrap.glyph_ix].index).chain([layout.len()]);
        let mut start = 0;
        for (row, end) in ends.enumerate() {
            let (from, to) = (range.start.max(start), range.end.min(end));
            if from < to {
                // A wrap point reads as the end of the row above, which is right for `to` only.
                let left = if from == start { px(0.) } else { self.position(from).unwrap_or_default().x };
                let right = self.position(to).unwrap_or_default().x;
                let at = self.bounds.origin + point(left, self.line_height * row as f32 + self.line_height - px(3.));
                window.paint_underline(at, right - left, &UnderlineStyle { thickness: px(1.), color: Some(color), wavy: true });
            }
            start = end;
        }
    }
}

pub struct CaretMark {
    pub at: usize,
    pub width: Pixels,
    /// Space left above and below the bar.
    pub inset: Pixels,
}

/// What is drawn with a line's text.
pub struct LineMarks {
    pub selection: Option<Range<usize>>,
    /// How far a selection runs past the end, for the newline it includes.
    pub selection_tail: Pixels,
    pub caret: Option<CaretMark>,
    /// Bytes of the line to underline, and in what colour.
    pub decorations: Vec<(Range<usize>, Hsla)>,
}

pub fn paint_line(line: &PaintedLine, marks: &LineMarks, window: &mut Window, cx: &mut App) {
    let (bounds, height) = (line.bounds, line.line_height);
    let position = |index: usize| line.position(index).unwrap_or_default();
    if let Some(selection) = &marks.selection {
        let (a, b) = (position(selection.start), position(selection.end));
        let mut y = a.y;
        while y <= b.y {
            let left = if y == a.y { a.x } else { px(0.) };
            let right = if y == b.y { b.x + marks.selection_tail } else { bounds.size.width };
            window.paint_quad(fill(Bounds::from_corners(bounds.origin + point(left, y), bounds.origin + point(right, y + height)), Theme::selection()));
            y += height;
        }
    }
    if let Some(layout) = &line.layout {
        let _ = layout.paint(bounds.origin, height, TextAlign::Left, None, window, cx);
    }
    for (range, color) in &marks.decorations { line.underline(range, *color, window); }
    if let Some(caret) = &marks.caret {
        let at = bounds.origin + position(caret.at) + point(px(0.), caret.inset);
        window.paint_quad(fill(Bounds::new(at, gpui::size(caret.width, height - caret.inset * 2.)), Theme::primary()));
    }
}

#[cfg(test)]
mod tests {
    use super::{from_utf16, to_utf16};

    #[test]
    fn utf16_offsets_round_trip_across_surrogate_pairs() {
        let text = "a😀b";
        assert_eq!(to_utf16(text, 5), 3);
        assert_eq!(from_utf16(text, 3), 5);
        assert_eq!(from_utf16(text, 99), text.len());
    }
}
