use super::Completions;
use crate::{
    context::{CompletionContext, ImmediateLocation},
    item::{CompletionItem, CompletionItemKind},
};

pub(crate) fn complete_syntax(
    accumulator: &mut Completions,
    context: &CompletionContext<'_>,
) {
    let mut add = |names: &[&str]| {
        for name in names {
            CompletionItem::new(CompletionItemKind::Keyword, context.source_range(), *name)
                .add_to(accumulator, context.db);
        }
    };
    match &context.completion_location {
        Some(ImmediateLocation::ItemList) => {
            add(&[
                "struct",
                "fn",
                "var",
                "const",
                "override",
                "alias",
                "enable",
                "requires",
                "const_assert",
                "diagnostic",
            ]);
            if context.file_id.edition(context.db).at_least_wesl_0_0_1() {
                add(&["import"]);
            }
        },
        Some(ImmediateLocation::StatementList {
            break_allowed,
            continue_allowed,
            return_allowed,
            continuing_allowed,
            else_allowed,
        }) => {
            add(&[
                "let",
                "var",
                "const",
                "const_assert",
                "if",
                "for",
                "while",
                "loop",
                "switch",
            ]);
            if *break_allowed {
                add(&["break"]);
            }
            if *continue_allowed {
                add(&["continue"]);
            }
            if *return_allowed {
                add(&["return", "discard"]);
            }
            if *continuing_allowed {
                add(&["continuing"]);
            }
            if *else_allowed {
                add(&["else"]);
            }
        },
        Some(ImmediateLocation::SwitchCase) => add(&["case", "default"]),
        Some(ImmediateLocation::ForInitializer) => add(&["let", "var", "const"]),
        Some(ImmediateLocation::AttributeName) => {
            add(&[
                "align",
                "binding",
                "blend_src",
                "builtin",
                "compute",
                "const",
                "diagnostic",
                "early_depth_test",
                "fragment",
                "group",
                "id",
                "interpolate",
                "invariant",
                "location",
                "must_use",
                "size",
                "vertex",
                "workgroup_size",
            ]);
            if context.file_id.edition(context.db).at_least_wesl_0_0_1() {
                add(&["if", "elif", "else"]);
            }
        },
        Some(ImmediateLocation::Enumerants(names)) => add(names),
        _ => {},
    }
}
