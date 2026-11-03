//! IME text still being composed for a surface that receives committed text
//! itself. Ranges are UTF-16 offsets, as the platform input system reports them.
use gpui_kit::UTF16Selection;
use std::ops::Range;

#[derive(Default)]
pub(crate) struct Composition {
    text: String,
    selection: Range<usize>,
}

impl Composition {
    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    pub(crate) fn text_for_range(
        &self,
        range: Range<usize>,
        actual: &mut Option<Range<usize>>,
    ) -> String {
        let chars: Vec<u16> = self.text.encode_utf16().collect();
        let end = range.end.min(chars.len());
        let start = range.start.min(end);
        *actual = Some(start..end);
        String::from_utf16_lossy(&chars[start..end])
    }

    pub(crate) fn selection(&self) -> UTF16Selection {
        UTF16Selection {
            range: self.selection.clone(),
            reversed: false,
        }
    }

    pub(crate) fn marked_range(&self) -> Option<Range<usize>> {
        (!self.text.is_empty()).then(|| 0..self.text.encode_utf16().count())
    }

    pub(crate) fn clear(&mut self) {
        self.text.clear();
        self.selection = 0..0;
    }

    pub(crate) fn mark(&mut self, text: &str, selected: Option<Range<usize>>) {
        self.text = text.into();
        let end = text.encode_utf16().count();
        self.selection = selected.unwrap_or(end..end);
    }
}
