use base_db::{EditionedFileId, FilePosition, TextRange};
use hir::{ChildContainer, Semantics, nearest_scope};
use hir_def::resolver::Resolver;
use ide_db::RootDatabase;
use syntax::{AstNode as _, SyntaxToken, ast};

use crate::{config::CompletionConfig, patterns::determine_location};

/// `CompletionContext` is created early during completion to figure out, where
/// exactly is the cursor, syntax-wise.
pub(crate) struct CompletionContext<'db> {
    pub(crate) semantics: Semantics<'db>,
    pub(crate) file_id: EditionedFileId,
    pub(crate) db: &'db RootDatabase,
    pub(crate) position: FilePosition,
    pub(crate) token: Option<SyntaxToken>,
    pub(crate) file: ast::SourceFile,
    pub(crate) container: Option<ChildContainer>,
    pub(crate) completion_location: Option<ImmediateLocation>,
    pub(crate) resolver: Resolver<'db>,
}

impl<'db> CompletionContext<'db> {
    pub(crate) fn new(
        db: &'db RootDatabase,
        position @ FilePosition { file_id, offset }: FilePosition,
        config: &'db CompletionConfig,
        trigger_character: Option<char>,
    ) -> Option<Self> {
        let _p = tracing::info_span!("CompletionContext::new").entered();
        let semantics = Semantics::new(db);
        let file_id = EditionedFileId::from_file(db, file_id);
        let file = semantics.parse(file_id);
        let tokens = file.syntax().token_at_offset(position.offset);
        let left = tokens.clone().left_biased();
        // At the start of a word, replace that word instead of inserting a
        // second name between it and the preceding punctuation or whitespace.
        let token = tokens
            .right_biased()
            .filter(|right| {
                right.text_range().start() == offset
                    && crate::patterns::is_word(right)
                    && left
                        .as_ref()
                        .is_none_or(|left| !crate::patterns::is_word(left))
            })
            .or(left);

        if crate::protected::is_protected(file.syntax(), position.offset) {
            return None;
        }

        let container = token
            .as_ref()
            .and_then(SyntaxToken::parent)
            .and_then(|parent| semantics.find_container(file_id, &parent));

        let mut resolver = Resolver::new(db, file_id);

        let nearest_scope = token
            .as_ref()
            .and_then(SyntaxToken::parent)
            .and_then(|node| nearest_scope(&node));

        if let Some(scope) = nearest_scope
            && let Some(definition) = container
            && let Some(definition) = definition.as_def_with_body_id()
        {
            resolver = semantics.analyze(definition).resolver_for(scope);
        }

        let completion_location =
            determine_location(file.syntax(), position.offset, token.as_ref(), &resolver);

        let context = Self {
            semantics,
            file_id,
            db,
            position,
            token,
            file,
            container,
            completion_location,
            resolver,
        };
        Some(context)
    }

    pub(crate) fn source_range(&self) -> base_db::TextRange {
        if let Some(token) = &self.token
            && crate::patterns::is_word(token)
        {
            token.text_range()
        } else {
            TextRange::empty(self.position.offset)
        }
    }
}

#[derive(Debug)]
pub(crate) enum ImmediateLocation {
    ItemList,
    StatementList {
        break_allowed: bool,
        continue_allowed: bool,
        return_allowed: bool,
        continuing_allowed: bool,
        else_allowed: bool,
    },
    SwitchCase,
    ForInitializer,
    AttributeName,
    Enumerants(&'static [&'static str]),
    Type,
    InsideStatement,
    FieldAccess {
        expression: ast::FieldExpression,
    },
}
