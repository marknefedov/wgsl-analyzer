use base_db::TextSize;
use hir_def::resolver::{BuiltInKind, Resolver, ScopeDef};
use syntax::{AstNode as _, SyntaxKind, SyntaxNode, SyntaxToken, ast};

use crate::context::ImmediateLocation;

/// Attribute names and enumerants have context-sensitive token kinds. Check the
/// spelling as well as the kind so their entire prefix is replaced.
pub(crate) fn is_word(token: &SyntaxToken) -> bool {
    token.kind() == SyntaxKind::Identifier
        || token.kind().is_keyword()
        || token
            .text()
            .chars()
            .next()
            .is_some_and(|ch| ch == '_' || ch.is_alphabetic())
            && token
                .text()
                .chars()
                .all(|ch| ch == '_' || ch.is_alphanumeric())
}

#[derive(Default)]
struct Frame<'text> {
    opener: &'text str,
    role: &'text str,
    start: usize,
    segment: usize,
    closed: &'text str,
    templates: usize,
}

/// Work from significant tokens before the word being edited. Parser recovery
/// can attach a partial declaration to the previous statement; delimiter state
/// remains useful even when the recovered node is not the intended construct.
pub(crate) fn determine_location(
    file: &SyntaxNode,
    offset: TextSize,
    token: Option<&SyntaxToken>,
    resolver: &Resolver<'_>,
) -> Option<ImmediateLocation> {
    let start = token
        .filter(|token| is_word(token))
        .map_or(offset, |token| token.text_range().start());
    let tokens: Vec<_> = file
        .descendants_with_tokens()
        .filter_map(syntax::SyntaxElement::into_token)
        .take_while(|token| token.text_range().end() <= start)
        .filter(|token| !token.kind().is_trivia())
        .collect();
    let words: Vec<_> = tokens.iter().map(SyntaxToken::text).collect();
    // An expression can occur inside a type (for example, an array length).
    // Resolve members before classifying the surrounding declaration.
    if words.last() == Some(&".") {
        return tokens
            .last()?
            .parent()?
            .ancestors()
            .find_map(ast::FieldExpression::cast)
            .map(|expression| ImmediateLocation::FieldAccess { expression });
    }
    let frames = delimiter_frames(&words)?;
    let frame = frames.last()?;
    let segment = without_attributes(&words[frame.segment..]);
    let in_function = frames.iter().any(|frame| frame.role == "fn");
    if segment.last() == Some(&"@") {
        return (attribute_position(frame, &segment[..segment.len() - 1])
            || body_attribute(tokens.last()?))
        .then_some(ImmediateLocation::AttributeName);
    }
    if frame.opener == "(" && frame.start >= 2 && words[frame.start - 2] == "@" {
        let parent = frames.get(frames.len() - 2)?;
        let before_attribute = without_attributes(&words[parent.segment..frame.start - 2]);
        if !attribute_position(parent, before_attribute)
            && !body_attribute(&tokens[frame.start - 2])
        {
            return None;
        }
        return attribute_arguments(frame, &words);
    }
    if let ["var", "<", rest @ ..] = segment
        && !rest.contains(&">")
    {
        return match rest {
            [] if in_function => Some(ImmediateLocation::Enumerants(&["function"])),
            [] => Some(ImmediateLocation::Enumerants(&[
                "private",
                "workgroup",
                "uniform",
                "storage",
            ])),
            ["storage", ","] if !in_function => {
                Some(ImmediateLocation::Enumerants(&["read", "read_write"]))
            },
            _ => None,
        };
    }
    if let Some(location) = type_or_template_position(segment, resolver) {
        return Some(location);
    }
    if segment
        .iter()
        .rposition(|word| matches!(*word, ":" | "->"))
        .is_some_and(|colon| !segment[colon + 1..].contains(&"="))
    {
        return None;
    }
    if segment.is_empty() {
        return list_location(&frames);
    }
    let node = token?.parent()?;
    node.ancestors()
        .find_map(ast::Statement::cast)
        .is_some()
        .then_some(ImmediateLocation::InsideStatement)
}

fn without_attributes<'text>(mut words: &'text [&'text str]) -> &'text [&'text str] {
    while words.first() == Some(&"@") && words.len() >= 2 {
        let mut end = 2;
        if words.get(end) == Some(&"(") {
            let mut depth = 0;
            loop {
                match words.get(end) {
                    Some(&"(") => depth += 1,
                    Some(&")") => depth -= 1,
                    None => return words,
                    _ => {},
                }
                end += 1;
                if depth == 0 {
                    break;
                }
            }
        }
        words = &words[end..];
    }
    words
}

fn type_or_template_position(
    words: &[&str],
    resolver: &Resolver<'_>,
) -> Option<ImmediateLocation> {
    let start = words.iter().rposition(|word| {
        matches!(*word, ":" | "->") || *word == "=" && words.first() == Some(&"alias")
    });
    if words
        .first()
        .is_some_and(|word| matches!(*word, "case" | "default"))
    {
        return None;
    }
    let words = without_attributes(&words[start.map_or(0, |start| start + 1)..]);
    if start.is_some() && words.is_empty() {
        return Some(ImmediateLocation::Type);
    }
    // Only type-valued template slots: array's size, pointer qualifiers and
    // storage texture formats/access modes must not receive type completions.
    let mut templates = Vec::new();
    for (index, word) in words.iter().enumerate() {
        match *word {
            "<" if index > 0 => templates.push((words[index - 1], 0)),
            ">" => {
                templates.pop();
            },
            "," => {
                if let Some((_, argument)) = templates.last_mut() {
                    *argument += 1;
                }
            },
            "=" | ";" => templates.clear(),
            _ => {},
        }
    }
    let (name, argument) = templates.last()?;
    if matches!(*name, "array" | "binding_array") && *argument == 1 {
        return Some(ImmediateLocation::InsideStatement);
    }
    if !matches!(words.last(), Some(&"<" | &",")) {
        return None;
    }
    let type_slot = match *name {
        "ptr" => *argument == 1,
        name if name.starts_with("texture_storage_") => false,
        _ => *argument == 0,
    };
    let mut type_generator = false;
    resolver.process_all_names(|candidate, definition| {
        if candidate.as_str() == *name
            && (matches!(definition, ScopeDef::BuiltIn(BuiltInKind::TypeGenerator(_)))
                || *name == "bitcast"
                    && matches!(definition, ScopeDef::BuiltIn(BuiltInKind::Function(_))))
        {
            type_generator = true;
        }
    });
    (type_slot && type_generator).then_some(ImmediateLocation::Type)
}

fn delimiter_frames<'text>(words: &'text [&'text str]) -> Option<Vec<Frame<'text>>> {
    let mut frames = vec![Frame::default()];
    for (index, word) in words.iter().copied().enumerate() {
        let frame = frames.last_mut()?;
        match word {
            "{" | "(" | "[" => {
                let segment = without_attributes(&words[frame.segment..index]);
                let role = if word == "{" && segment.starts_with(&["else", "if"]) {
                    "if"
                } else if word == "{" {
                    segment
                        .iter()
                        .copied()
                        .find(|word| {
                            matches!(
                                *word,
                                "fn" | "struct"
                                    | "loop"
                                    | "for"
                                    | "while"
                                    | "switch"
                                    | "if"
                                    | "else"
                                    | "continuing"
                                    | "case"
                                    | "default"
                            )
                        })
                        .unwrap_or("block")
                } else if word == "(" && segment.first() == Some(&"fn") {
                    "parameters"
                } else if word == "(" && segment.first() == Some(&"for") {
                    "for"
                } else {
                    ""
                };
                frames.push(Frame {
                    opener: word,
                    role,
                    start: index,
                    segment: index + 1,
                    closed: "",
                    templates: 0,
                });
            },
            "}" | ")" | "]" => {
                let expected = match word {
                    "}" => "{",
                    ")" => "(",
                    _ => "[",
                };
                if frames.last()?.opener == expected && frames.len() > 1 {
                    let closed = frames.pop()?;
                    if word == "}" {
                        let parent = frames.last_mut()?;
                        parent.segment = index + 1;
                        parent.closed = closed.role;
                    }
                }
            },
            ";" if frame.opener == "{" || frame.opener.is_empty() => {
                let terminal =
                    without_attributes(&words[frame.segment..index]).starts_with(&["break", "if"]);
                frame.segment = index + 1;
                frame.closed = if terminal { "break if" } else { "" };
            },
            "<" => frame.templates += 1,
            ">" => frame.templates = frame.templates.saturating_sub(1),
            "," if frame.templates == 0
                && (frame.role == "struct" || frame.role == "parameters") =>
            {
                frame.segment = index + 1;
            },
            _ => {},
        }
    }
    Some(frames)
}

fn attribute_arguments(
    frame: &Frame<'_>,
    words: &[&str],
) -> Option<ImmediateLocation> {
    // Enumerated attribute arguments are not expressions. A comma after the
    // final argument is a trailing comma, not another completion position.
    if frame.opener == "(" && frame.start >= 2 && words[frame.start - 2] == "@" {
        let arguments = &words[frame.start + 1..];
        match (words[frame.start - 1], arguments) {
            ("builtin", []) => Some(ImmediateLocation::Enumerants(&[
                "vertex_index",
                "instance_index",
                "position",
                "front_facing",
                "frag_depth",
                "sample_index",
                "sample_mask",
                "local_invocation_id",
                "local_invocation_index",
                "global_invocation_id",
                "workgroup_id",
                "num_workgroups",
                "subgroup_invocation_id",
                "subgroup_size",
            ])),
            ("interpolate", []) => Some(ImmediateLocation::Enumerants(&[
                "perspective",
                "linear",
                "flat",
            ])),
            ("interpolate", ["flat", ","]) => {
                Some(ImmediateLocation::Enumerants(&["first", "either"]))
            },
            ("interpolate", ["perspective" | "linear", ","]) => {
                Some(ImmediateLocation::Enumerants(&[
                    "center", "centroid", "sample",
                ]))
            },
            _ => None,
        }
    } else {
        None
    }
}

fn attribute_position(
    frame: &Frame<'_>,
    segment: &[&str],
) -> bool {
    segment.is_empty()
        && (frame.opener.is_empty() || frame.opener == "{" || frame.role == "parameters")
        || segment
            .iter()
            .rposition(|word| *word == "->")
            .is_some_and(|arrow| without_attributes(&segment[arrow + 1..]).is_empty())
}

/// The parser knows where a condition/header ends and its body attributes
/// begin. Use that boundary rather than treating every expression as a place
/// to write an attribute.
fn body_attribute(token: &SyntaxToken) -> bool {
    let previous = std::iter::successors(token.prev_token(), SyntaxToken::prev_token)
        .find(|token| !token.kind().is_trivia());
    if !previous.is_some_and(|token| {
        is_word(&token)
            || matches!(
                token.kind(),
                SyntaxKind::IntLiteral
                    | SyntaxKind::FloatLiteral
                    | SyntaxKind::ParenthesisRight
                    | SyntaxKind::BracketRight
                    | SyntaxKind::TemplateEnd
                    | SyntaxKind::Colon
            )
    }) {
        return false;
    }
    token
        .parent()
        .into_iter()
        .flat_map(|node| node.ancestors())
        .find(|node| node.kind() == SyntaxKind::AttributeList)
        .and_then(|node| node.parent())
        .is_some_and(|node| {
            matches!(
                node.kind(),
                SyntaxKind::FunctionDeclaration
                    | SyntaxKind::IfClause
                    | SyntaxKind::ElseIfClause
                    | SyntaxKind::ElseClause
                    | SyntaxKind::LoopStatement
                    | SyntaxKind::ForStatement
                    | SyntaxKind::WhileStatement
                    | SyntaxKind::SwitchStatement
                    | SyntaxKind::ContinuingStatement
                    | SyntaxKind::SwitchBodyCase
            )
        })
}

fn list_location(frames: &[Frame<'_>]) -> Option<ImmediateLocation> {
    let frame = frames.last()?;
    let in_function = frames.iter().any(|frame| frame.role == "fn");
    if frame.opener == "(" && frame.role == "for" {
        return Some(ImmediateLocation::ForInitializer);
    }
    if frame.opener.is_empty() {
        return Some(ImmediateLocation::ItemList);
    }
    if frame.role == "switch" {
        return Some(ImmediateLocation::SwitchCase);
    }
    if frame.opener == "{" && in_function && frame.role != "struct" {
        if frame.role == "loop" && frame.closed == "continuing"
            || frame.role == "continuing" && frame.closed == "break if"
        {
            return None;
        }
        let control: Vec<_> = frames
            .iter()
            .rev()
            .take_while(|frame| frame.role != "fn")
            .collect();
        let continuing = control.iter().any(|frame| frame.role == "continuing");
        return Some(ImmediateLocation::StatementList {
            // A direct continuing block permits the terminating `break if`.
            break_allowed: frame.role == "continuing"
                || control
                    .iter()
                    .take_while(|frame| frame.role != "continuing")
                    .any(|frame| matches!(frame.role, "loop" | "for" | "while" | "switch")),
            continue_allowed: control
                .iter()
                .take_while(|frame| frame.role != "continuing")
                .any(|frame| matches!(frame.role, "loop" | "for" | "while")),
            return_allowed: !continuing,
            continuing_allowed: frame.role == "loop" && frame.closed != "continuing",
            else_allowed: frame.closed == "if",
        });
    }
    None
}
