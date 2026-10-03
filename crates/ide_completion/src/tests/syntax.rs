//! Completion behavior at incomplete syntax boundaries.
use crate::{
    CompletionItemKind,
    tests::{TEST_CONFIG, get_all_items},
};

fn labels(source: &str) -> Vec<String> {
    get_all_items(&TEST_CONFIG, source, None)
        .into_iter()
        .map(|item| item.label.primary.to_string())
        .collect()
}

fn keywords(source: &str) -> Vec<String> {
    get_all_items(&TEST_CONFIG, source, None)
        .into_iter()
        .filter(|item| item.kind == CompletionItemKind::Keyword)
        .map(|item| item.label.primary.to_string())
        .collect()
}

#[test]
fn module_and_statement_boundaries() {
    for source in [
        "$0",
        "  \n $0",
        "str$0",
        "struct S {}\n$0",
        "const x = 1;\n$0",
        "@compute $0",
        "/* done */ $0",
    ] {
        let names = keywords(source);
        assert!(
            names.iter().any(|name| name == "struct"),
            "{source}: {names:?}"
        );
        assert!(!names.iter().any(|name| name == "let"), "{source}");
    }
    for source in [
        "fn f() { $0 }",
        "fn f() { le$0 }",
        "fn f() { let x = 1; $0 }",
        "fn f() { if true {} $0 }",
    ] {
        let names = keywords(source);
        assert!(
            names.iter().any(|name| name == "let"),
            "{source}: {names:?}"
        );
        assert!(!names.iter().any(|name| name == "struct"), "{source}");
    }
}

#[test]
fn control_flow_keywords() {
    for (source, present, absent) in [
        ("fn f() { $0 }", "return", "break"),
        ("fn f() { loop { $0 } }", "continuing", "else"),
        ("fn f() { while true { $0 } }", "continue", "continuing"),
        ("fn f() { for (;;) { $0 } }", "break", "continuing"),
        ("fn f() { switch 1 { $0 } }", "case", "let"),
        (
            "fn f() { switch 1 { default: { $0 } } }",
            "break",
            "continue",
        ),
        ("fn f() { if true {} $0 }", "else", "continue"),
        ("fn f() { if true {} else {} $0 }", "let", "else"),
        ("fn f() { loop { continuing { $0 } } }", "let", "continue"),
        ("fn f() { loop { continuing { $0 } } }", "break", "return"),
        ("fn f() { loop { continuing { $0 } } }", "break", "discard"),
    ] {
        let names = keywords(source);
        assert!(
            names.iter().any(|name| name == present),
            "{source}: {names:?}"
        );
        assert!(
            !names.iter().any(|name| name == absent),
            "{source}: {names:?}"
        );
    }
}

#[test]
fn syntax_does_not_leak() {
    for source in [
        "// $0",
        "/* $0 */",
        "fn f() { // $0\n}",
        "fn f() { let x = \"$0\"; }",
        "fn f() { let x = $0; }",
        "fn f() { return $0; }",
        "fn f() { if $0 {} }",
        "fn f() { foo($0); }",
        "fn f() { let x = foo.$0; }",
        "fn f() { let x = foo.@$0; }",
        "fn f() { loop { continuing {} $0 } }",
        "fn f() { for (; $0;) {} }",
        "struct S { $0 }",
        "fn f($0) {}",
        "var $0",
        "fn $0",
        "fn f() { let x = @$0; }",
        "fn f() { let x = @builtin($0); }",
        "@group($0) var<uniform> x: u32;",
    ] {
        assert!(
            keywords(source).is_empty(),
            "{source}: {:?}",
            keywords(source)
        );
    }
}

#[test]
fn attributes_and_edits() {
    for source in [
        "@$0",
        "@wor$0",
        "@work$0group_size",
        "fn f(@$0) {}",
        "fn f() -> @$0 u32 {}",
        "struct S { @$0 x: u32 }",
    ] {
        assert!(
            labels(source).iter().any(|name| name == "workgroup_size"),
            "{source}"
        );
    }
    for (source, expected) in [
        ("@$0", "@workgroup_size"),
        ("@wor$0", "@workgroup_size"),
        ("@work$0group_size", "@workgroup_size"),
    ] {
        let item = get_all_items(&TEST_CONFIG, source, Some('@'))
            .into_iter()
            .find(|item| item.label.primary == "workgroup_size")
            .unwrap();
        let mut text = source.replace("$0", "");
        item.text_edit.apply(&mut text);
        assert_eq!(text, expected);
    }
}

#[test]
fn qualifiers_and_attribute_arguments() {
    for (source, expected) in [
        ("var<$0", vec!["private", "workgroup", "uniform", "storage"]),
        (
            "var<sto$0",
            vec!["private", "workgroup", "uniform", "storage"],
        ),
        ("fn f() { var<$0 }", vec!["function"]),
        ("var<storage, $0", vec!["read", "read_write"]),
        ("var<uniform, $0", vec![]),
        ("fn f() { var<storage, $0 }", vec![]),
        ("var<storage, read, $0", vec![]),
        ("@interpolate($0)", vec!["perspective", "linear", "flat"]),
        ("@interpolate(flat, $0)", vec!["first", "either"]),
        (
            "@interpolate(linear, $0)",
            vec!["center", "centroid", "sample"],
        ),
        ("@interpolate(linear, sample, $0)", vec![]),
        ("@builtin(position, $0)", vec![]),
    ] {
        assert_eq!(labels(source), expected, "{source}");
    }
    assert!(
        labels("@builtin(global_$0)")
            .iter()
            .any(|name| name == "global_invocation_id")
    );
}

#[test]
fn types_use_the_resolver() {
    for source in [
        "var x: $0;",
        "var<storage> x: $0;",
        "fn f(x: $0) {}",
        "fn f() -> $0 {}",
        "fn f(x: u32) -> $0 {}",
        "fn f(x: ptr<storage, $0>) {}",
        "struct S { x: ptr<storage, $0> }",
        "alias A = $0;",
        "struct S { x: $0 }",
        "fn f() { var x: array<$0>; }",
        "var x: array<vec4<$0>>;",
        "fn f() { let x = vec4<$0>(); }",
        "fn f() { let x: vec4f = vec4<$0>(); }",
        "var x: ptr<storage, $0>;",
    ] {
        let source = format!("struct UserType {{ x: u32 }}\nalias UserAlias = u32;\n{source}");
        let names = labels(&source);
        for expected in ["u32", "vec4", "UserType", "UserAlias"] {
            assert!(
                names.iter().any(|name| name == expected),
                "{source}: missing {expected}"
            );
        }
        for absent in ["let", "sin", "read_write"] {
            assert!(
                !names.iter().any(|name| name == absent),
                "{source}: unexpected {absent}"
            );
        }
    }
    for source in ["var x: ptr<$0>;", "var x: texture_storage_2d<$0>;"] {
        assert!(!labels(source).iter().any(|name| name == "u32"), "{source}");
    }
}

#[test]
fn dialect_keywords() {
    assert!(!labels("$0").iter().any(|name| name == "import"));
    assert!(!labels("@$0").iter().any(|name| name == "elif"));
    assert!(
        labels("//- /main.wesl edition:2026_pre\n$0")
            .iter()
            .any(|name| name == "import")
    );
    assert!(
        labels("//- /main.wesl edition:2026_pre\n@$0")
            .iter()
            .any(|name| name == "elif")
    );
}

#[test]
#[expect(
    clippy::non_ascii_literal,
    reason = "Exercise byte ranges for Unicode identifiers"
)]
fn replacement_ranges() {
    for (source, label, expected) in [
        ("str$0uct", "struct", "struct"),
        ("var<sto$0rage>", "storage", "var<storage>"),
        ("@builtin(pos$0ition)", "position", "@builtin(position)"),
        (
            "@interpolate(flat, ei$0ther)", // spellchecker:disable-line
            "either",
            "@interpolate(flat, either)",
        ),
        (
            "struct Ω { x: u32 } var x: Ω$0;",
            "Ω",
            "struct Ω { x: u32 } var x: Ω;",
        ),
        ("var x: ve$0c4;", "vec4", "var x: vec4;"),
    ] {
        let item = get_all_items(&TEST_CONFIG, source, None)
            .into_iter()
            .find(|item| item.label.primary == label)
            .unwrap();
        let mut text = source.replace("$0", "");
        item.text_edit.apply(&mut text);
        assert_eq!(text, expected);
    }
}

#[test]
fn replacement_at_word_start() {
    for (source, label, expected) in [
        ("@$0group", "group", "@group"),
        ("@ $0group", "group", "@ group"),
        ("var<$0storage>", "storage", "var<storage>"),
        ("var x: $0vec4;", "vec4", "var x: vec4;"),
    ] {
        let item = get_all_items(&TEST_CONFIG, source, None)
            .into_iter()
            .find(|item| item.label.primary == label)
            .unwrap();
        let mut text = source.replace("$0", "");
        item.text_edit.apply(&mut text);
        assert_eq!(text, expected);
    }
}

#[test]
fn bitcast_type_argument() {
    let source = "fn f() { let value = 1; let x = bitcast<$0>(value); }";
    let names = labels(source);
    assert!(names.iter().any(|name| name == "u32"));
    assert!(!names.iter().any(|name| name == "value"));
    assert!(!names.iter().any(|name| name == "sin"));
}

#[test]
fn attribute_chains_and_for_initializers() {
    for source in [
        "fn f() -> @location(0) @$0 vec4f {}",
        "fn f() -> @location(0) @interpolate($0) vec4f {}",
    ] {
        let expected = if source.contains("interpolate(") {
            "perspective"
        } else {
            "interpolate"
        };
        assert!(
            labels(source).iter().any(|name| name == expected),
            "{source}"
        );
    }
    for source in ["fn f() { for ($0; ; ) {} }", "fn f() { for (va$0; ; ) {} }"] {
        let names = keywords(source);
        assert!(names.iter().any(|name| name == "var"), "{source}");
        assert!(!names.iter().any(|name| name == "return"), "{source}");
    }
    assert!(labels("fn f(){ loop { continuing { break if true; $0 } } }").is_empty());
}

#[test]
fn array_lengths_preserve_expression_completion() {
    let source = "const COUNT = 4u; fn f() { var values: array<u32, CO$0>; }";
    assert!(labels(source).iter().any(|name| name == "COUNT"));
    assert!(keywords(source).is_empty());
    let source = "fn f() { let counts = vec2u(2, 4); var values: array<u32, counts.x$0>; }";
    let (db, position) = crate::tests::position(source);
    let context =
        crate::context::CompletionContext::new(&db, position, &TEST_CONFIG, None).unwrap();
    assert!(matches!(
        context.completion_location,
        Some(crate::context::ImmediateLocation::FieldAccess { .. })
    ));
    assert!(keywords(source).is_empty());
}

#[test]
fn malformed_text_has_no_completions() {
    for source in [
        "fn f() { /* $0",
        "/* outer /* inner */ $0",
        "fn f() { /* @$0",
        "fn f() { let x = \"{ $0\"; }",
        "fn f() { let x = \"@$0\"; }",
        "fn f() { let x = \"vec2u(0).$0\"; }",
        "fn f() { /* /* */ { $0",
    ] {
        assert!(labels(source).is_empty(), "{source}");
    }
}

#[test]
fn attributes_before_bodies() {
    for source in [
        "fn f() @$0 {}",
        "fn f() { loop @$0 {} }",
        "fn f() { if true @$0 {} }",
        "fn f() { for(;;) @$0 {} }",
        "fn f() { while true @$0 {} }",
        "fn f() { switch 1 @$0 {} }",
        "fn f() { loop { continuing @$0 {} } }",
        "fn f() { if true {} else @$0 {} }",
    ] {
        assert!(
            labels(source).iter().any(|name| name == "diagnostic"),
            "{source}"
        );
    }
    for source in ["fn f() { if true + @$0 {} }", "fn f() { foo(@$0); }"] {
        assert!(keywords(source).is_empty(), "{source}");
    }
}
