use std::path::{Path, PathBuf};

use pathdiff::diff_paths;
use turbopack_binding::swc::core::{
    common::{errors::HANDLER, FileName, DUMMY_SP},
    ecma::{
        ast::{
            op, ArrayLit, ArrowExpr, BinExpr, BinaryOp, BlockStmt, BlockStmtOrExpr, Bool, CallExpr,
            Callee, Expr, ExprOrSpread, ExprStmt, Id, Ident, ImportDecl, ImportSpecifier,
            KeyValueProp, Lit, MemberExpr, MemberProp, Null, ObjectLit, Prop, PropName,
            PropOrSpread, Stmt, Str, Tpl, UnaryExpr, UnaryOp,
        },
        atoms::js_word,
        utils::ExprFactory,
        visit::{Fold, FoldWith},
    },
};

pub fn next_dynamic(
    is_development: bool,
    is_server: bool,
    is_server_components: bool,
    filename: FileName,
    pages_dir: Option<PathBuf>,
) -> impl Fold {
    NextDynamicPatcher {
        is_development,
        is_server,
        is_server_components,
        pages_dir,
        filename,
        dynamic_bindings: vec![],
        is_next_dynamic_first_arg: false,
        dynamically_imported_specifier: None,
    }
}

#[derive(Debug)]
struct NextDynamicPatcher {
    is_development: bool,
    is_server: bool,
    is_server_components: bool,
    pages_dir: Option<PathBuf>,
    filename: FileName,
    dynamic_bindings: Vec<Id>,
    is_next_dynamic_first_arg: bool,
    dynamically_imported_specifier: Option<String>,
}

impl Fold for NextDynamicPatcher {
    fn fold_import_decl(&mut self, decl: ImportDecl) -> ImportDecl {
        let ImportDecl {
            ref src,
            ref specifiers,
            ..
        } = decl;
        if &src.value == "next/dynamic" {
            for specifier in specifiers {
                if let ImportSpecifier::Default(default_specifier) = specifier {
                    self.dynamic_bindings.push(default_specifier.local.to_id());
                }
            }
        }

        decl
    }

    fn fold_call_expr(&mut self, expr: CallExpr) -> CallExpr {
        if self.is_next_dynamic_first_arg {
            if let Callee::Import(..) = &expr.callee {
                match &*expr.args[0].expr {
                    Expr::Lit(Lit::Str(Str { value, .. })) => {
                        self.dynamically_imported_specifier = Some(value.to_string());
                    }
                    Expr::Tpl(Tpl { exprs, quasis, .. }) if exprs.is_empty() => {
                        self.dynamically_imported_specifier = Some(quasis[0].raw.to_string());
                    }
                    _ => {}
                }
            }
            return expr.fold_children_with(self);
        }
        let mut expr = expr.fold_children_with(self);
        if let Callee::Expr(i) = &expr.callee {
            if let Expr::Ident(identifier) = &**i {
                if self.dynamic_bindings.contains(&identifier.to_id()) {
                    if expr.args.is_empty() {
                        HANDLER.with(|handler| {
                            handler
                                .struct_span_err(
                                    identifier.span,
                                    "next/dynamic requires at least one argument",
                                )
                                .emit()
                        });
                        return expr;
                    } else if expr.args.len() > 2 {
                        HANDLER.with(|handler| {
                            handler
                                .struct_span_err(
                                    identifier.span,
                                    "next/dynamic only accepts 2 arguments",
                                )
                                .emit()
                        });
                        return expr;
                    }
                    if expr.args.len() == 2 {
                        match &*expr.args[1].expr {
                            Expr::Object(_) => {}
                            _ => {
                                HANDLER.with(|handler| {
                          handler
                              .struct_span_err(
                                  identifier.span,
                                  "next/dynamic options must be an object literal.\nRead more: https://nextjs.org/docs/messages/invalid-dynamic-options-type",
                              )
                              .emit();
                      });
                                return expr;
                            }
                        }
                    }

                    self.is_next_dynamic_first_arg = true;
                    expr.args[0].expr = expr.args[0].expr.clone().fold_with(self);
                    self.is_next_dynamic_first_arg = false;

                    if self.dynamically_imported_specifier.is_none() {
                        return expr;
                    }

                    // dev client or server:
                    // loadableGenerated: {
                    //   modules:
                    // ["/project/src/file-being-transformed.js -> " + '../components/hello'] }

                    // prod client
                    // loadableGenerated: {
                    //   webpack: () => [require.resolveWeak('../components/hello')],
                    let generated = Box::new(Expr::Object(ObjectLit {
                        span: DUMMY_SP,
                        props: if self.is_development || self.is_server {
                            vec![PropOrSpread::Prop(Box::new(Prop::KeyValue(KeyValueProp {
                                key: PropName::Ident(Ident::new("modules".into(), DUMMY_SP)),
                                value: Box::new(Expr::Array(ArrayLit {
                                    elems: vec![Some(ExprOrSpread {
                                        expr: Box::new(Expr::Bin(BinExpr {
                                            span: DUMMY_SP,
                                            op: BinaryOp::Add,
                                            left: Box::new(Expr::Lit(Lit::Str(Str {
                                                value: format!(
                                                    "{} -> ",
                                                    rel_filename(
                                                        self.pages_dir.as_deref(),
                                                        &self.filename
                                                    )
                                                )
                                                .into(),
                                                span: DUMMY_SP,
                                                raw: None,
                                            }))),
                                            right: Box::new(Expr::Lit(Lit::Str(Str {
                                                value: self
                                                    .dynamically_imported_specifier
                                                    .as_ref()
                                                    .unwrap()
                                                    .clone()
                                                    .into(),
                                                span: DUMMY_SP,
                                                raw: None,
                                            }))),
                                        })),
                                        spread: None,
                                    })],
                                    span: DUMMY_SP,
                                })),
                            })))]
                        } else {
                            vec![PropOrSpread::Prop(Box::new(Prop::KeyValue(KeyValueProp {
                                key: PropName::Ident(Ident::new("webpack".into(), DUMMY_SP)),
                                value: Box::new(Expr::Arrow(ArrowExpr {
                                    params: vec![],
                                    body: Box::new(BlockStmtOrExpr::Expr(Box::new(Expr::Array(
                                        ArrayLit {
                                            elems: vec![Some(ExprOrSpread {
                                                expr: Box::new(Expr::Call(CallExpr {
                                                    callee: Callee::Expr(Box::new(Expr::Member(
                                                        MemberExpr {
                                                            obj: Box::new(Expr::Ident(Ident {
                                                                sym: js_word!("require"),
                                                                span: DUMMY_SP,
                                                                optional: false,
                                                            })),
                                                            prop: MemberProp::Ident(Ident {
                                                                sym: "resolveWeak".into(),
                                                                span: DUMMY_SP,
                                                                optional: false,
                                                            }),
                                                            span: DUMMY_SP,
                                                        },
                                                    ))),
                                                    args: vec![ExprOrSpread {
                                                        expr: Box::new(Expr::Lit(Lit::Str(Str {
                                                            value: self
                                                                .dynamically_imported_specifier
                                                                .as_ref()
                                                                .unwrap()
                                                                .clone()
                                                                .into(),
                                                            span: DUMMY_SP,
                                                            raw: None,
                                                        }))),
                                                        spread: None,
                                                    }],
                                                    span: DUMMY_SP,
                                                    type_args: None,
                                                })),
                                                spread: None,
                                            })],
                                            span: DUMMY_SP,
                                        },
                                    )))),
                                    is_async: false,
                                    is_generator: false,
                                    span: DUMMY_SP,
                                    return_type: None,
                                    type_params: None,
                                })),
                            })))]
                        },
                    }));

                    let mut props =
                        vec![PropOrSpread::Prop(Box::new(Prop::KeyValue(KeyValueProp {
                            key: PropName::Ident(Ident::new("loadableGenerated".into(), DUMMY_SP)),
                            value: generated,
                        })))];

                    let mut has_ssr_false = false;

                    if expr.args.len() == 2 {
                        if let Expr::Object(ObjectLit {
                            props: options_props,
                            ..
                        }) = &*expr.args[1].expr
                        {
                            for prop in options_props.iter() {
                                if let Some(KeyValueProp { key, value }) = match prop {
                                    PropOrSpread::Prop(prop) => match &**prop {
                                        Prop::KeyValue(key_value_prop) => Some(key_value_prop),
                                        _ => None,
                                    },
                                    _ => None,
                                } {
                                    if let Some(Ident {
                                        sym,
                                        span: _,
                                        optional: _,
                                    }) = match key {
                                        PropName::Ident(ident) => Some(ident),
                                        _ => None,
                                    } {
                                        if sym == "ssr" {
                                            if let Some(Lit::Bool(Bool {
                                                value: false,
                                                span: _,
                                            })) = value.as_lit()
                                            {
                                                has_ssr_false = true
                                            }
                                        }
                                    }
                                }
                            }
                            props.extend(options_props.iter().cloned());
                        }
                    }

                    if has_ssr_false && self.is_server {
                        if self.is_server_components {
                            // Transform 1st argument `expr.args[0]` aka the module loader to:
                            // `typeof window !== 'window' && (() => {
                            //    expr.args[0]
                            // })`
                            // For instance:
                            // dynamic(`typeof window !== 'window' && (() => {
                            //   /**
                            //    * this will make sure we can traverse the module first but will be
                            //    * tree-shake out in server bundle */
                            //   () => import('./client-mod')
                            // }), { ssr: false })
                            let side_effect_free_loader_arg = Expr::Arrow(ArrowExpr {
                                span: DUMMY_SP,
                                params: vec![],
                                body: Box::new(BlockStmtOrExpr::BlockStmt(BlockStmt {
                                    span: DUMMY_SP,
                                    stmts: vec![
                                        // loader is still inside the module but not executed,
                                        // then it will be removed by tree-shaking.
                                        Stmt::Expr(ExprStmt {
                                            span: DUMMY_SP,
                                            expr: expr.args[0].expr.clone(),
                                        }),
                                    ],
                                })),
                                is_async: true,
                                is_generator: false,
                                type_params: None,
                                return_type: None,
                            });

                            expr.args[0] =
                                wrap_expr_with_client_only_cond(&side_effect_free_loader_arg)
                                    .as_arg();
                        } else {
                            expr.args[0] = Lit::Null(Null { span: DUMMY_SP }).as_arg();
                        }
                    }

                    let second_arg = ExprOrSpread {
                        spread: None,
                        expr: Box::new(Expr::Object(ObjectLit {
                            span: DUMMY_SP,
                            props,
                        })),
                    };

                    if expr.args.len() == 2 {
                        expr.args[1] = second_arg;
                    } else {
                        expr.args.push(second_arg)
                    }
                    self.dynamically_imported_specifier = None;
                }
            }
        }
        expr
    }
}

// Receive an expression and return `typeof window !== 'undefined' &&
// <expression>`, to make the expression is tree-shakable on server side but
// still remain in module graph.
fn wrap_expr_with_client_only_cond(wrapped_expr: &Expr) -> Expr {
    let typeof_expr = Expr::Unary(UnaryExpr {
        span: DUMMY_SP,
        op: UnaryOp::TypeOf, // 'typeof' operator
        arg: Box::new(Expr::Ident(Ident {
            span: DUMMY_SP,
            sym: "window".into(),
            optional: false,
        })),
    });
    let undefined_literal = Expr::Lit(Lit::Str(Str {
        span: DUMMY_SP,
        value: "undefined".into(),
        raw: None,
    }));
    let inequality_expr = Expr::Bin(BinExpr {
        span: DUMMY_SP,
        left: Box::new(typeof_expr),
        op: BinaryOp::NotEq, // '!=='
        right: Box::new(undefined_literal),
    });

    // Create the LogicalExpr 'typeof window !== "undefined" && x'
    let logical_expr = Expr::Bin(BinExpr {
        span: DUMMY_SP,
        op: op!("&&"), // '&&' operator
        left: Box::new(inequality_expr),
        right: Box::new(wrapped_expr.clone()),
    });

    logical_expr
}

fn rel_filename(base: Option<&Path>, file: &FileName) -> String {
    let base = match base {
        Some(v) => v,
        None => return file.to_string(),
    };

    let file = match file {
        FileName::Real(v) => v,
        _ => {
            return file.to_string();
        }
    };

    let rel_path = diff_paths(file, base);

    let rel_path = match rel_path {
        Some(v) => v,
        None => return file.display().to_string(),
    };

    rel_path.display().to_string()
}
