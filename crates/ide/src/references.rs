use base_db::{EditionedFileId, FileExtension, FilePosition, FileRange, SourceDatabase as _};
use hir::{Semantics, definition::Definition};
use ide_db::{FxHashSet, RootDatabase};
use salsa::Database as _;
use syntax::{AstNode as _, SyntaxKind, ast};

use crate::{goto_definition::TryToNavigationTarget as _, helpers};

/// The declaration is kept separate for both LSP's includeDeclaration and rename.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceSearchResult {
    pub declaration: FileRange,
    pub references: Vec<FileRange>,
}

pub(crate) fn references(
    db: &RootDatabase,
    position: FilePosition,
) -> Option<ReferenceSearchResult> {
    let semantics = Semantics::new(db);
    let file_id = EditionedFileId::from_file(db, position.file_id);
    let parse = file_id.parse(db);
    let token =
        helpers::pick_best_token(parse.syntax().token_at_offset(position.offset), |kind| {
            usize::from(kind == SyntaxKind::Identifier)
        })?;
    if token.kind() != SyntaxKind::Identifier {
        return None;
    }
    let target = Definition::from_token(&semantics, file_id, &token)?;
    let navigation = target.try_to_navigation_target(db)?;
    let declaration = FileRange {
        file_id: navigation.file_id,
        range: navigation.focus_range.unwrap_or(navigation.full_range),
    };
    let declaration_text = db.file_text(declaration.file_id).text(db);
    let name = &declaration_text
        [usize::from(declaration.range.start())..usize::from(declaration.range.end())];
    let mut files = FxHashSet::default();
    if matches!(target, Definition::Local(_)) {
        files.insert(position.file_id);
    } else {
        let mut roots = FxHashSet::default();
        roots.insert(db.file_source_root(position.file_id).source_root_id(db));
        for package in base_db::all_packages(db).iter() {
            roots.insert(
                db.file_source_root(package.data(db).manifest_file_id)
                    .source_root_id(db),
            );
        }
        for root in roots {
            let root = db.source_root(root).source_root(db);
            files.extend(
                root.iter()
                    .filter(|file| FileExtension::from_file(&root, *file).is_ok()),
            );
        }
    }
    let mut references = Vec::new();
    for file in files {
        db.unwind_if_revision_cancelled();
        let file_id = EditionedFileId::from_file(db, file);
        let root = file_id.parse(db).syntax();
        // Imported items can be referenced under a different spelling, including
        // names re-exported by another module. Keep these as candidate names too.
        let mut names = FxHashSet::default();
        names.insert(name.to_owned());
        for item in root.descendants().filter_map(ast::ImportItem::cast) {
            for name in [item.name(), item.alias()].into_iter().flatten() {
                names.insert(name.text().to_string());
            }
        }
        for token in root
            .descendants_with_tokens()
            .filter_map(rowan::NodeOrToken::into_token)
        {
            if token.kind() != SyntaxKind::Identifier || !names.contains(token.text()) {
                continue;
            }
            db.unwind_if_revision_cancelled();
            let range = FileRange {
                file_id: file,
                range: token.text_range(),
            };
            if range != declaration
                && Definition::from_token(&semantics, file_id, &token).as_ref() == Some(&target)
            {
                references.push(range);
            }
        }
    }
    references.sort_by_key(|reference| {
        (
            reference.file_id,
            reference.range.start(),
            reference.range.end(),
        )
    });
    references.dedup();
    Some(ReferenceSearchResult {
        declaration,
        references,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_fixture::WithFixture as _;

    /// Annotations identify the exact declaration and reference ranges. Check the
    /// result from every occurrence, including the declaration itself.
    fn check(fixture: &str) {
        let (db, files) = RootDatabase::with_many_files(fixture);
        let mut declaration = None;
        let mut expected = Vec::new();
        for file in files {
            let file_id = file.file_id(&db);
            for (range, annotation) in
                test_utils::extract_annotations(db.file_text(file_id).text(&db))
            {
                let range = FileRange { file_id, range };
                match annotation.as_str() {
                    "declaration" => assert!(declaration.replace(range).is_none()),
                    "reference" => expected.push(range),
                    _ => panic!("unexpected annotation: {annotation}"),
                }
            }
        }
        expected.sort_by_key(|range| (range.file_id, range.range.start(), range.range.end()));
        let declaration = declaration.unwrap();
        for occurrence in expected.iter().chain(std::iter::once(&declaration)) {
            // Both the beginning and end of an identifier must work.
            for offset in [occurrence.range.start(), occurrence.range.end()] {
                let result = references(
                    &db,
                    FilePosition {
                        file_id: occurrence.file_id,
                        offset,
                    },
                )
                .unwrap_or_else(|| panic!("no symbol at {occurrence:?}, offset {offset:?}"));
                assert_eq!(result.declaration, declaration);
                assert_eq!(result.references, expected);
            }
        }
    }

    #[test]
    fn local_shadowing() {
        check(
            r#"
fn main() {
    let value = 1;
      //^^^^^ declaration
    let copy = value;
             //^^^^^ reference
    {
        let value = value;
                  //^^^^^ reference
        let copy = value;
    }
    let after = value;
              //^^^^^ reference
}
fn other() { let value = 2; let copy = value; }
"#,
        );
    }

    #[test]
    fn parameter() {
        check(
            r#"
fn main(value: i32) -> i32 {
      //^^^^^ declaration
    return value;
         //^^^^^ reference
}
"#,
        );
    }

    #[test]
    fn fields_and_types() {
        check(
            r#"
struct A { value: i32 }
         //^^^^^ declaration
struct B { value: i32 }
fn main(a: A, b: B) {
    let x = a.value;
            //^^^^^ reference
    let y = b.value;
}
"#,
        );
        check(
            r#"
struct A { value: i32 }
     //^ declaration
alias Alias = A;
            //^ reference
fn main(a: A) -> A {
         //^ reference
               //^ reference
    return A(1);
         //^ reference
}
"#,
        );
    }

    #[test]
    fn globals_and_calls() {
        check(
            r#"
const value = 1;
    //^^^^^ declaration
override other = value;
               //^^^^^ reference
fn main() { let x = value; }
                  //^^^^^ reference
// value is not a reference in a comment.
"#,
        );
        check(
            r#"
fn called() {}
 //^^^^^^ declaration
fn main() { called(); }
          //^^^^^^ reference
"#,
        );
    }

    #[test]
    fn unused_declaration() {
        check("fn unused() {}\n //^^^^^^ declaration\n");
    }

    #[test]
    fn imports_and_aliases() {
        check(
            r#"
//- /shaders/package.wesl package:test root:/shaders edition:2026_pre
const value = 1;
    //^^^^^ declaration
//- /shaders/other.wesl
import package::value as renamed;
              //^^^^^ reference
                       //^^^^^^^ reference
fn main() {
    let x = renamed;
          //^^^^^^^ reference
    let y = package::value;
                   //^^^^^ reference
}
"#,
        );
    }

    #[test]
    fn dependency_and_module_alias() {
        check(
            r#"
//- /library/package.wesl package:library root:/library edition:2026_pre library
const value = 1;
    //^^^^^ declaration
//- /app/package.wesl package:app root:/app dependencies:library edition:2026_pre
import library as renamed;
fn main() {
    let x = renamed::value;
                   //^^^^^ reference
    let y = library::value;
                   //^^^^^ reference
}
//- /unrelated/package.wesl package:unrelated root:/unrelated edition:2026_pre
const value = 2;
fn main() { let x = value; }
"#,
        );
    }

    #[test]
    fn references_with_invalid_signature_types() {
        check(
            r#"
fn main(value: Missing) {
      //^^^^^ declaration
    let v = vec3<f32>(value);
                    //^^^^^ reference
    let m = mat2x2<f32>(value);
                      //^^^^^ reference
}
"#,
        );
    }

    #[test]
    fn non_symbols() {
        for fixture in [
            "fn main() { $0 }",
            "fn main() { $0missing(); }",
            "fn main() { $0sin(1.0); }",
            "// $0comment",
            "fn main() { let v = vec2(1.0); let x = v.$0x; }",
        ] {
            let (db, position) = RootDatabase::with_position(fixture);
            assert_eq!(references(&db, position), None);
        }
    }
}
