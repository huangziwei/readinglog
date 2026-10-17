//! A note's Markdown as the blocks a Journal row sets: each heading,
//! paragraph, quotation and list item, its words as plain text. The parser
//! takes the extensions the desktop's own renderer takes.

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

/// What a block is set as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Heading,
    Paragraph,
    /// A paragraph inside a quotation, set behind a rule.
    Quote,
    /// A paragraph of a list item.
    Item,
}

/// One block of a note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub kind: Kind,
    /// The bullet or number before an item's first line; empty on every other block.
    pub lead: String,
    /// How many lists stand around an item, 1 for one standing alone.
    pub depth: usize,
    /// The words, a hard break or a code block's line end as `\n`.
    pub words: String,
}

impl Block {
    fn new(kind: Kind, lead: String, depth: usize) -> Self {
        Self {
            kind,
            lead,
            depth,
            words: String::new(),
        }
    }
}

/// `text`'s blocks in order, none of them empty. Emphasis, code, links and
/// images give their words, raw HTML its own text, a table row its cells two
/// spaces apart, and a soft break a space.
pub fn blocks(text: &str) -> Vec<Block> {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS;
    let mut out: Vec<Block> = Vec::new();
    let mut open: Option<Block> = None;
    let mut quotes = 0usize;
    // Each list standing, by the number its next item takes; `None` for bullets.
    let mut lists: Vec<Option<u64>> = Vec::new();
    let mut items = 0usize;
    let close = |open: &mut Option<Block>, out: &mut Vec<Block>| {
        if let Some(mut block) = open.take() {
            block.words = block.words.trim().to_string();
            if !block.words.is_empty() {
                out.push(block);
            }
        }
    };
    for event in Parser::new_ext(text, options) {
        match event {
            Event::Start(Tag::Heading { .. }) => {
                close(&mut open, &mut out);
                open = Some(Block::new(Kind::Heading, String::new(), lists.len()));
            }
            Event::Start(
                Tag::Paragraph
                | Tag::CodeBlock(_)
                | Tag::HtmlBlock
                | Tag::TableHead
                | Tag::TableRow
                | Tag::FootnoteDefinition(_),
            ) => {
                if open.is_none() {
                    open = Some(Block::new(
                        kind_in(quotes, items),
                        String::new(),
                        lists.len(),
                    ));
                }
            }
            Event::Start(Tag::BlockQuote(_)) => {
                close(&mut open, &mut out);
                quotes += 1;
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                close(&mut open, &mut out);
                quotes = quotes.saturating_sub(1);
            }
            Event::Start(Tag::List(first)) => {
                close(&mut open, &mut out);
                lists.push(first);
            }
            Event::End(TagEnd::List(_)) => {
                close(&mut open, &mut out);
                lists.pop();
            }
            Event::Start(Tag::Item) => {
                close(&mut open, &mut out);
                items += 1;
                let lead = match lists.last_mut() {
                    Some(Some(n)) => {
                        *n += 1;
                        format!("{}.", *n - 1)
                    }
                    _ => "•".to_string(),
                };
                open = Some(Block::new(Kind::Item, lead, lists.len()));
            }
            Event::End(TagEnd::Item) => {
                close(&mut open, &mut out);
                items = items.saturating_sub(1);
            }
            Event::End(
                TagEnd::Heading(_)
                | TagEnd::Paragraph
                | TagEnd::CodeBlock
                | TagEnd::HtmlBlock
                | TagEnd::TableHead
                | TagEnd::TableRow
                | TagEnd::FootnoteDefinition,
            ) => close(&mut open, &mut out),
            Event::End(TagEnd::TableCell) => push(&mut open, quotes, items, &lists, "  "),
            Event::Text(said)
            | Event::Code(said)
            | Event::Html(said)
            | Event::InlineHtml(said)
            | Event::InlineMath(said)
            | Event::DisplayMath(said) => push(&mut open, quotes, items, &lists, &said),
            Event::FootnoteReference(label) => {
                push(&mut open, quotes, items, &lists, &format!("[{label}]"));
            }
            Event::TaskListMarker(done) => {
                push(
                    &mut open,
                    quotes,
                    items,
                    &lists,
                    if done { "[x] " } else { "[ ] " },
                );
            }
            Event::SoftBreak => push(&mut open, quotes, items, &lists, " "),
            Event::HardBreak => push(&mut open, quotes, items, &lists, "\n"),
            _ => {}
        }
    }
    close(&mut open, &mut out);
    out
}

/// What a paragraph is set as inside `quotes` quotations and `items` list items.
fn kind_in(quotes: usize, items: usize) -> Kind {
    match (items, quotes) {
        (1.., _) => Kind::Item,
        (0, 1..) => Kind::Quote,
        _ => Kind::Paragraph,
    }
}

/// `said` onto the block `open`, a block opened for it where none is.
fn push(open: &mut Option<Block>, quotes: usize, items: usize, lists: &[Option<u64>], said: &str) {
    let block =
        open.get_or_insert_with(|| Block::new(kind_in(quotes, items), String::new(), lists.len()));
    block.words.push_str(said);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str) -> Vec<(Kind, String, String)> {
        blocks(text)
            .into_iter()
            .map(|b| (b.kind, b.lead, b.words))
            .collect()
    }

    #[test]
    fn a_note_falls_into_headings_paragraphs_quotations_and_items() {
        let said = kinds(
            "# I don’t like this book\n\nvery *incoherent*, a [bundle](https://x) of\nscripts.\n\n\
             > not really a book\n\n- one\n- two\n\n1. first\n2. second\n\nbreak  \nhere <b>raw</b>",
        );
        let words = |k: Kind, lead: &str, w: &str| (k, lead.to_string(), w.to_string());
        assert_eq!(
            said,
            vec![
                words(Kind::Heading, "", "I don’t like this book"),
                words(Kind::Paragraph, "", "very incoherent, a bundle of scripts."),
                words(Kind::Quote, "", "not really a book"),
                words(Kind::Item, "•", "one"),
                words(Kind::Item, "•", "two"),
                words(Kind::Item, "1.", "first"),
                words(Kind::Item, "2.", "second"),
                words(Kind::Paragraph, "", "break\nhere <b>raw</b>"),
            ]
        );
    }

    #[test]
    fn plain_words_are_one_paragraph_and_blank_words_none() {
        assert_eq!(
            kinds("A thought."),
            vec![(Kind::Paragraph, String::new(), "A thought.".into())]
        );
        assert!(blocks("  \n\n ").is_empty());
        let nested = blocks("- outer\n  - inner");
        assert_eq!(
            nested.iter().map(|b| b.depth).collect::<Vec<_>>(),
            vec![1, 2]
        );
    }
}
