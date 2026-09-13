//! Suppress completion inside comments and quoted text, including lexer errors.

use base_db::TextSize;
use syntax::SyntaxNode;

#[derive(Clone, Copy)]
enum State {
    Code,
    LineComment,
    BlockComment(usize),
    Quote(char),
}

/// Malformed comments and quoted text can be emitted as error tokens, so token
/// kinds cannot reliably tell whether the cursor is inside protected text.
pub(crate) fn is_protected(
    file: &SyntaxNode,
    offset: TextSize,
) -> bool {
    let text = file.text().to_string();
    let offset = usize::from(offset);
    let mut characters = text.char_indices().peekable();
    let mut state = State::Code;
    while let Some((index, character)) = characters.next() {
        if index >= offset {
            break;
        }
        state = match state {
            State::Code => match (character, characters.peek().copied()) {
                ('/', Some((_, '/'))) => {
                    characters.next();
                    State::LineComment
                },
                ('/', Some((_, '*'))) => {
                    characters.next();
                    State::BlockComment(1)
                },
                ('"' | '\'', _) => State::Quote(character),
                _ => State::Code,
            },
            State::LineComment => {
                if matches!(
                    character,
                    '\n' | '\u{000B}' | '\u{000C}' | '\r' | '\u{0085}' | '\u{2028}' | '\u{2029}'
                ) {
                    State::Code
                } else {
                    State::LineComment
                }
            },
            State::BlockComment(depth) => match (character, characters.peek().copied()) {
                ('/', Some((_, '*'))) => {
                    characters.next();
                    State::BlockComment(depth + 1)
                },
                ('*', Some((end, '/'))) if end < offset => {
                    characters.next();
                    if depth == 1 {
                        State::Code
                    } else {
                        State::BlockComment(depth - 1)
                    }
                },
                _ => State::BlockComment(depth),
            },
            State::Quote(quote) => {
                if character == '\\' {
                    characters.next();
                    State::Quote(quote)
                } else if character == quote {
                    State::Code
                } else {
                    State::Quote(quote)
                }
            },
        };
    }
    !matches!(state, State::Code)
}

#[cfg(test)]
mod tests {
    use super::is_protected;

    fn check(
        fixture: &str,
        expected: bool,
    ) {
        let offset = fixture.find("$0").unwrap();
        let source = fixture.replace("$0", "");
        let file = syntax::parse(&source, syntax::Edition::Wgsl).syntax();
        assert_eq!(
            is_protected(&file, offset.try_into().unwrap()),
            expected,
            "{fixture}"
        );
    }

    #[test]
    fn comments_and_delimiter_boundaries() {
        for fixture in [
            "//$0",
            "//$0\n",
            "/$0/ comment",
            "/*$0",
            "/$0* comment */",
            "/* comment *$0/",
            "/* outer /* inner */ $0",
            "/* outer /* inner */ $0 */",
            "/* outer /* $0 inner */ */",
            "// /* comment */ $0",
            "/* // comment\n $0 */",
        ] {
            check(fixture, true);
        }
        for fixture in [
            "$0",
            "$0// comment",
            "$0/* comment */",
            "/* comment */$0",
            "/* outer /* inner */ done */$0",
        ] {
            check(fixture, false);
        }
    }

    #[test]
    fn quoted_text_and_escapes() {
        for fixture in [
            r#""$0"#,
            r#""{ $0""#,
            r#""escaped \" { $0""#,
            r#""escaped \"$0"#,
            r#""// /* $0""#,
            r#"'"{ $0'"#,
            r#"'escaped \' $0'"#,
        ] {
            check(fixture, true);
        }
        for fixture in [
            r#""done"$0"#,
            "'done'$0",
            r#""escaped \\"$0"#,
            r#"/* " */$0"#,
            "// \"\n$0",
        ] {
            check(fixture, false);
        }
    }

    #[test]
    fn all_wgsl_line_endings_end_comments() {
        for ending in [
            '\n', '\u{000B}', '\u{000C}', '\r', '\u{0085}', '\u{2028}', '\u{2029}',
        ] {
            check(&format!("// comment$0{ending}"), true);
            check(&format!("// comment{ending}$0"), false);
        }
    }
}
