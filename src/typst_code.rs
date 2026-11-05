//! Typst code at the caret, from `typst_ide`: names after `#`, fields after a
//! dot, and whatever else it offers when asked with ⌃Space.

use crate::completion::{CompletionKind, Found, Item, Provider, Query, Reach};
use crate::session::DocumentSession;
use gpui::*;
use thesio_core::ide::{CodeStart, Suggestion, continues_completion, word_start};

pub struct TypstCode {
    session: WeakEntity<DocumentSession>,
    starts: CodeStart,
}

impl TypstCode {
    pub fn new(session: WeakEntity<DocumentSession>) -> Self { Self { session, starts: CodeStart::default() } }
}

fn item(suggestion: Suggestion) -> Item {
    let mut fields = suggestion.insert.fields.into_iter();
    Item {
        kind: CompletionKind::Typst,
        title: suggestion.title,
        detail: suggestion.kind,
        info: suggestion.detail,
        insert: suggestion.insert.text,
        selection: fields.next(),
        stops: fields.collect(),
        from: Some(suggestion.from),
    }
}

impl Provider for TypstCode {
    fn kind(&self) -> CompletionKind { CompletionKind::Typst }

    fn find(&self, text: &str, caret: usize, reach: Reach) -> Option<Found> {
        let word = word_start(text, caret);
        let start = match reach {
            _ if self.starts.opens(text, word) => word,
            Reach::Typed => return None,
            Reach::Asked => word,
            Reach::Open(start) => Some(start).filter(|start| *start <= caret && continues_completion(&text[*start..caret]))?,
        };
        Some(Found { range: start..caret, query: text[start..caret].to_string() })
    }

    fn items(&self, query: Query, cx: &mut App) -> Task<Vec<Item>> {
        let Some(session) = self.session.upgrade() else { return Task::ready(vec![]) };
        let answer = session.read(cx).complete(query.document, query.caret, query.asked, cx);
        cx.spawn(async move |_| answer.await.into_iter().map(item).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::{Provider, Reach, TypstCode, WeakEntity};

    fn found(text: &str, reach: Reach) -> Option<(std::ops::Range<usize>, String)> {
        let provider = TypstCode::new(WeakEntity::new_invalid());
        provider.find(text, text.len(), reach).map(|found| (found.range, found.query))
    }

    #[test]
    fn typing_opens_only_after_a_hash_or_a_field_dot() {
        assert_eq!(found("Text #he", Reach::Typed), Some((6..8, "he".to_string())));
        assert_eq!(found("#calc.ab", Reach::Typed), Some((6..8, "ab".to_string())));
        assert_eq!(found("The end.", Reach::Typed), None);
        assert_eq!(found("Plain word", Reach::Typed), None);
        assert_eq!(found("@key", Reach::Typed), None);
    }

    #[test]
    fn asking_finds_the_word_anywhere() {
        assert_eq!(found("#text(fi", Reach::Asked), Some((6..8, "fi".to_string())));
    }

    #[test]
    fn an_open_popup_follows_a_path_and_ends_at_a_space() {
        assert_eq!(found("#image(\"figures/pl", Reach::Open(8)), Some((8..18, "figures/pl".to_string())));
        assert_eq!(found("#he ", Reach::Open(1)), None);
        assert_eq!(found("ab", Reach::Open(9)), None, "the caret left it behind");
    }

    #[test]
    fn a_new_field_moves_an_open_popup_to_its_start() {
        assert_eq!(found("#calc.", Reach::Open(1)), Some((6..6, String::new())));
    }
}
