use base_db::EditionedFileId;
use hir_def::{
    expression_store::path::Path, item_tree::Name, mod_path::ModPath, resolver::ResolveKind,
};
use syntax::{AstNode as _, SyntaxNode, SyntaxToken, ast, match_ast};

use crate::{
    Field, Function, GlobalConstant, GlobalVariable, Local, ModuleDef, Override, Semantics, Struct,
    TypeAlias,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Definition {
    Local(Local),
    Field(Field),
    ModuleDef(ModuleDef),
    BuiltinFunction(Name),
    BuiltinType(Name),
    BuiltinTypeGenerator(Name),
    BuiltinTypeConstructor(Name),
    BuiltinEnumerant(Name),
    BuiltinDeclaration(Name),
}

impl Definition {
    #[must_use]
    pub fn from_token(
        semantics: &Semantics<'_>,
        file_id: EditionedFileId,
        token: &SyntaxToken,
    ) -> Option<Self> {
        let parent = token.parent()?;
        // A qualified path denotes its item only at its final segment.
        if let Some(path) = ast::Path::cast(parent.clone())
            && path.segments().last().as_ref() != Some(token)
        {
            return None;
        }
        Self::from_node(semantics, file_id, &parent)
    }

    #[must_use]
    pub fn from_node(
        semantics: &Semantics<'_>,
        file_id: EditionedFileId,
        node: &SyntaxNode,
    ) -> Option<Self> {
        match_ast! {
            match node {
                ast::Name(name) => {
                    resolve_name(semantics, file_id, &name)
                },
                ast::Path(name_ref) => {
                    resolve_path(semantics, file_id, &name_ref)
                },
                ast::FieldExpression(field_expression) => {
                    resolve_field(semantics, file_id, field_expression)
                },
                _ => {
                    None
                }
            }
        }
    }
}

fn resolve_name(
    semantics: &Semantics<'_>,
    file_id: EditionedFileId,
    name: &ast::Name,
) -> Option<Definition> {
    use crate::HasSource as _;
    use hir_def::{
        InFile,
        db::DefinitionWithBodyId,
        signature::{FieldId, StructSignature},
    };

    let parent = name.syntax().parent()?;
    if let Some(item) = ast::ImportItem::cast(parent.clone()) {
        let import = parent.ancestors().find_map(ast::ImportStatement::cast)?;
        let mut segments: Vec<_> = parent
            .ancestors()
            .filter_map(ast::ImportPath::cast)
            .map(|path| Some(Name::from(path.name()?.ident_token()?.text())))
            .collect::<Option<_>>()?;
        segments.reverse();
        segments.push(Name::from(item.name()?.ident_token()?.text()));
        let path = ModPath::from_segments(
            hir_def::mod_path::PathKind::from_src(import.relative()),
            segments,
        );
        return semantics
            .resolver(file_id, import.syntax())
            .resolve(semantics.db, &Path(path))
            .ok()
            .map(Definition::from);
    }
    // Resolve bindings by their source map, not by lookup in the surrounding scope.
    if let Some(function) = parent.ancestors().find_map(ast::FunctionDeclaration::cast)
        && function.syntax() != &parent
    {
        let id = semantics.function_to_def(&InFile::new(file_id, function))?;
        if let Some(binding) = semantics
            .analyze(DefinitionWithBodyId::Function(id))
            .binding_id(name)
        {
            return Some(Definition::Local(Local {
                parent: id,
                binding,
            }));
        }
    }
    if let Some(member) = ast::StructMember::cast(parent.clone()) {
        let declaration = parent.ancestors().find_map(ast::StructDeclaration::cast)?;
        let id = semantics.global_struct_to_def(&InFile::new(file_id, declaration))?;
        return StructSignature::of(semantics.db, id)
            .fields()
            .iter()
            .find_map(|(field, _)| {
                let field = Field {
                    id: FieldId {
                        r#struct: id,
                        field,
                    },
                };
                (field.source(semantics.db)?.value.syntax() == member.syntax())
                    .then_some(Definition::Field(field))
            });
    }
    let definition = match_ast! {
        match parent {
            ast::FunctionDeclaration(node) => {
                Some(ModuleDef::Function(Function {
                    id: semantics.function_to_def(&InFile::new(file_id, node))?,
                }))
            },
            ast::VariableDeclaration(node) => {
                Some(ModuleDef::GlobalVariable(GlobalVariable {
                    id: semantics.global_variable_to_def(&InFile::new(file_id, node))?,
                }))
            },
            ast::ConstantDeclaration(node) => {
                Some(ModuleDef::GlobalConstant(GlobalConstant {
                    id: semantics.global_constant_to_def(&InFile::new(file_id, node))?,
                }))
            },
            ast::OverrideDeclaration(node) => {
                Some(ModuleDef::Override(Override {
                    id: semantics.global_override_to_def(&InFile::new(file_id, node))?,
                }))
            },
            ast::StructDeclaration(node) => {
                Some(ModuleDef::Struct(Struct {
                    id: semantics.global_struct_to_def(&InFile::new(file_id, node))?,
                }))
            },
            ast::TypeAliasDeclaration(node) => {
                Some(ModuleDef::TypeAlias(TypeAlias {
                    id: semantics.global_type_alias_to_def(&InFile::new(file_id, node))?,
                }))
            },
            _ => None,
        }
    };
    definition.map(Definition::ModuleDef)
}

impl From<ResolveKind> for Definition {
    fn from(value: ResolveKind) -> Self {
        match value {
            ResolveKind::Local(binding, parent) => Self::Local(Local { parent, binding }),
            ResolveKind::GlobalVariable(id) => {
                Self::ModuleDef(ModuleDef::GlobalVariable(GlobalVariable { id }))
            },
            ResolveKind::GlobalConstant(id) => {
                Self::ModuleDef(ModuleDef::GlobalConstant(GlobalConstant { id }))
            },
            ResolveKind::Override(id) => Self::ModuleDef(ModuleDef::Override(Override { id })),
            ResolveKind::Struct(id) => Self::ModuleDef(ModuleDef::Struct(Struct { id })),
            ResolveKind::TypeAlias(id) => Self::ModuleDef(ModuleDef::TypeAlias(TypeAlias { id })),
            ResolveKind::Function(id) => Self::ModuleDef(ModuleDef::Function(Function { id })),
            ResolveKind::BuiltinFunction(name) => Self::BuiltinFunction(name),
            ResolveKind::BuiltinType(name) => Self::BuiltinType(name),
            ResolveKind::BuiltinTypeGenerator(name) => Self::BuiltinTypeGenerator(name),
            ResolveKind::BuiltinTypeConstructor(name) => Self::BuiltinTypeConstructor(name),
            ResolveKind::BuiltinEnumerant(name) => Self::BuiltinEnumerant(name),
            ResolveKind::BuiltinDeclaration(name) => Self::BuiltinDeclaration(name),
        }
    }
}

fn resolve_path(
    semantics: &Semantics<'_>,
    file_id: EditionedFileId,
    path: &ast::Path,
) -> Option<Definition> {
    let parent = path.syntax().parent()?;

    if ast::IdentExpression::can_cast(parent.kind()) || ast::TypeSpecifier::can_cast(parent.kind())
    {
        let resolver = semantics.resolver(file_id, path.syntax());
        resolver
            .resolve(semantics.db, &Path(ModPath::from_src(path)))
            .ok()
            .map(Definition::from)
    } else if let Some(expression) = ast::FieldExpression::cast(parent) {
        resolve_field(semantics, file_id, expression)
    } else {
        None
    }
}

fn resolve_field(
    semantics: &Semantics<'_>,
    file_id: EditionedFileId,
    field_expression: ast::FieldExpression,
) -> Option<Definition> {
    let definition = semantics.find_container(file_id, field_expression.syntax())?;
    let field = semantics
        .analyze(definition.as_def_with_body_id()?)
        .resolve_field(field_expression)?;
    Some(Definition::Field(field))
}
