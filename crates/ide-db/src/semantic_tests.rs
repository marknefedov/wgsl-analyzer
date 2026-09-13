use std::sync::{Arc as StdArc, Mutex};

use base_db::EditionedFileId;
use hir::{HasSource as _, Semantics, definition::Definition};
use syntax::AstNode as _;

use super::*;

#[test]
fn name_resolution_does_not_execute_type_inference() {
    let events = StdArc::new(Mutex::new(Vec::new()));
    let captured = StdArc::clone(&events);
    let mut db = RootDatabase {
        storage: salsa::Storage::new(Some(Box::new(move |event| {
            captured.lock().unwrap().push(event);
        }))),
        files: Arc::default(),
        nonce: Nonce::new(),
    };
    set_all_packages_with_durability(&mut db, [], Durability::HIGH);
    CapabilitiesInput::update_capabilities(&mut db, Capabilities::default());
    let fixture = test_fixture::ChangeFixture::parse(
        "fn main(value: f32) -> f32 { let copy = value; return copy; }\nstruct Data { field: f32 }\nfn read(data: Data) -> f32 { return data.field; }",
    );
    let file_id = fixture.files[0];
    fixture.change.apply(&mut db);
    let file_id = EditionedFileId::from_file(&db, file_id);
    let semantics = Semantics::new(&db);
    let root = semantics.parse(file_id);
    let mut definition = None;
    for token in root
        .syntax()
        .descendants_with_tokens()
        .filter_map(rowan::NodeOrToken::into_token)
    {
        if token.text() == "value" {
            let resolved = Definition::from_token(&semantics, file_id, &token).unwrap();
            if let Some(previous) = &definition {
                assert_eq!(&resolved, previous);
            }
            definition = Some(resolved);
        }
    }
    let executed = || {
        events
            .lock()
            .unwrap()
            .iter()
            .filter_map(|event| {
                if let salsa::EventKind::WillExecute { database_key } = event.kind {
                    Some(
                        db.ingredient_debug_name(database_key.ingredient_index())
                            .into_owned(),
                    )
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
    };
    assert!(!executed().iter().any(|name| name == "infer_query"));

    // A type query is a positive control: it must actually execute inference.
    let Definition::Local(local) = definition.unwrap() else {
        panic!("expected local")
    };
    let source = local.source(&db).unwrap();
    let analyzer = semantics.analyze(hir_def::db::DefinitionWithBodyId::Function(local.parent));
    assert!(analyzer.type_of_binding(&source.value).is_some());
    assert!(executed().iter().any(|name| name == "infer_query"));

    events.lock().unwrap().clear();
    let field = root
        .syntax()
        .descendants_with_tokens()
        .filter_map(rowan::NodeOrToken::into_token)
        .filter(|token| token.text() == "field")
        .last()
        .unwrap();
    assert!(matches!(
        Definition::from_token(&semantics, file_id, &field),
        Some(Definition::Field(_))
    ));
    assert!(executed().iter().any(|name| name == "infer_query"));
}
