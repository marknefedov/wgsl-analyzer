use expect_test::expect;

use crate::tests::check_infer;

#[test]
fn vector_constructor_error_from_struct_signature() {
    check_infer(
        "struct Data { value: Missing }\nvar<private> data: Data;\nfn main() { let result = vec3<f32>(data.value); }",
        expect![[r#"
            21..28 'Missing': `Missing` not found in scope
            44..48 'data': ref<private, Data, read_write>
            72..78 'result': vec3<f32>
            81..102 'vec3<f...value)': vec3<f32>
            91..95 'data': ref<private, Data, read_write>
            91..101 'data.value': ref<private, [error], read_write>
        "#]],
    );
}

#[test]
fn matrix_constructor_error_from_struct_signature() {
    check_infer(
        "struct Data { value: Missing }\nvar<private> data: Data;\nfn main() { let result = mat2x2<f32>(data.value); }",
        expect![[r#"
            21..28 'Missing': `Missing` not found in scope
            44..48 'data': ref<private, Data, read_write>
            72..78 'result': mat2x2<f32>
            81..104 'mat2x2...value)': mat2x2<f32>
            93..97 'data': ref<private, Data, read_write>
            93..103 'data.value': ref<private, [error], read_write>
        "#]],
    );
}

#[test]
fn array_generator_error_argument() {
    check_infer(
        "
fn foo() {
    let y = &0;
    let x = array(y);
}
        ",
        expect![[r#"
            19..20 'y': [error]
            23..25 '&0': [error]
            24..25 '0': integer
            35..36 'x': array<[error], 1>
            39..47 'array(y)': array<[error], 1>
            45..46 'y': [error]
            23..25 '&0': cannot use unary operator `&` on type `AbstractInt`
        "#]],
    );
}

#[test]
fn vector_generator_error_argument() {
    check_infer(
        "
fn foo() {
    let y = &0;
    let x = vec2(y);
}
        ",
        expect![[r#"
            19..20 'y': [error]
            23..25 '&0': [error]
            24..25 '0': integer
            35..36 'x': vec2<[error]>
            39..46 'vec2(y)': vec2<[error]>
            44..45 'y': [error]
            23..25 '&0': cannot use unary operator `&` on type `AbstractInt`
        "#]],
    );
}

#[test]
fn matrix_generator_error_argument() {
    check_infer(
        "
fn foo() {
    let y = &0.0;
    let x = mat2x2(y, y, y, y);
}
        ",
        expect![[r#"
            19..20 'y': [error]
            23..27 '&0.0': [error]
            24..27 '0.0': float
            37..38 'x': mat2x2<[error]>
            41..59 'mat2x2... y, y)': mat2x2<[error]>
            48..49 'y': [error]
            51..52 'y': [error]
            54..55 'y': [error]
            57..58 'y': [error]
            23..27 '&0.0': cannot use unary operator `&` on type `AbstractFloat`
        "#]],
    );
}

#[test]
fn scalar_constructor_error_argument() {
    check_infer(
        "
fn foo() {
    let y = &0;
    let x = u32(y);
}
        ",
        expect![[r#"
            19..20 'y': [error]
            23..25 '&0': [error]
            24..25 '0': integer
            35..36 'x': u32
            39..45 'u32(y)': u32
            43..44 'y': [error]
            23..25 '&0': cannot use unary operator `&` on type `AbstractInt`
        "#]],
    );
}

#[test]
fn array_constructor_error_argument() {
    check_infer(
        "
fn foo() {
    let y = &0;
    let x = array<i32, 1>(y);
}
        ",
        expect![[r#"
            19..20 'y': [error]
            23..25 '&0': [error]
            24..25 '0': integer
            35..36 'x': array<i32, 1>
            39..55 'array<... 1>(y)': array<i32, 1>
            53..54 'y': [error]
            23..25 '&0': cannot use unary operator `&` on type `AbstractInt`
        "#]],
    );
}

#[test]
fn vector_constructor_error_argument() {
    check_infer(
        "
fn foo() {
    let y = &0;
    let x = vec2f(y);
}
        ",
        expect![[r#"
            19..20 'y': [error]
            23..25 '&0': [error]
            24..25 '0': integer
            35..36 'x': vec2<f32>
            39..47 'vec2f(y)': vec2<f32>
            45..46 'y': [error]
            23..25 '&0': cannot use unary operator `&` on type `AbstractInt`
        "#]],
    );
}

#[test]
fn matrix_constructor_error_argument() {
    check_infer(
        "
fn foo() {
    let y = &0.0;
    let x = mat2x2f(y);
}
        ",
        expect![[r#"
            19..20 'y': [error]
            23..27 '&0.0': [error]
            24..27 '0.0': float
            37..38 'x': mat2x2<f32>
            41..51 'mat2x2f(y)': mat2x2<f32>
            49..50 'y': [error]
            23..27 '&0.0': cannot use unary operator `&` on type `AbstractFloat`
        "#]],
    );
}

#[test]
fn struct_constructor_error_argument() {
    check_infer(
        "
struct Foo { foo: u32 }
fn foo() {
    let y = &0;
    let x = Foo(y);
}
        ",
        expect![[r#"
            43..44 'y': [error]
            47..49 '&0': [error]
            48..49 '0': integer
            59..60 'x': Foo
            63..69 'Foo(y)': Foo
            67..68 'y': [error]
            47..49 '&0': cannot use unary operator `&` on type `AbstractInt`
        "#]],
    );
}
