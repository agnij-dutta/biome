//! Class context detection shared by parsed and unparsed Tailwind queries.

use std::marker::PhantomData;

use biome_analyze::{
    AddVisitor, Phases, QueryMatch, Queryable, ServiceBag, Visitor, VisitorContext,
    options::TailwindOptions,
};
use biome_html_syntax::HtmlAttribute;
use biome_js_syntax::{AnyJsExpression, JsCallExpression, JsxAttribute};
use biome_rowan::{
    AstNode, Language, SyntaxNode, TextRange, TokenText, WalkEvent, declare_node_union,
};

/// Syntax nodes that identify where Tailwind classes are supplied.
/// Recognition uses attribute and function names without resolving bindings.
pub trait TailwindClassContextNode: AstNode {
    fn is_tailwind_class_context(&self, options: &TailwindOptions) -> bool;
}

declare_node_union! {
    pub AnyJsClassContext = JsxAttribute | JsCallExpression
}

impl TailwindClassContextNode for AnyJsClassContext {
    fn is_tailwind_class_context(&self, options: &TailwindOptions) -> bool {
        match self {
            Self::JsxAttribute(attribute) => attribute.is_tailwind_class_context(options),
            Self::JsCallExpression(call) => call.is_tailwind_class_context(options),
        }
    }
}

impl TailwindClassContextNode for JsxAttribute {
    fn is_tailwind_class_context(&self, options: &TailwindOptions) -> bool {
        get_jsx_attribute_name(self)
            .is_some_and(|name| is_configured_attribute(options, name.text()))
    }
}

impl TailwindClassContextNode for JsCallExpression {
    fn is_tailwind_class_context(&self, options: &TailwindOptions) -> bool {
        self.callee()
            .ok()
            .and_then(get_root_name)
            .is_some_and(|name| {
                is_merge_function(options, name.text()) || is_variant_function(options, name.text())
            })
    }
}

impl TailwindClassContextNode for HtmlAttribute {
    fn is_tailwind_class_context(&self, options: &TailwindOptions) -> bool {
        let Some(name) = self.name().ok().and_then(|name| name.value_token().ok()) else {
            return false;
        };
        let name = name.text_trimmed();
        options.attributes().map_or_else(
            || {
                DEFAULT_ATTRIBUTES
                    .iter()
                    .any(|attribute| attribute.eq_ignore_ascii_case(name))
            },
            |attributes| {
                attributes
                    .iter()
                    .any(|attribute| attribute.as_ref().eq_ignore_ascii_case(name))
            },
        )
    }
}

/// Matches class attributes and utility calls without parsing their class text.
/// Rules receive the context node and inspect its values or arguments.
#[derive(Clone)]
pub struct TailwindClassContext<N>(N);

impl<N: AstNode + 'static> QueryMatch for TailwindClassContext<N> {
    fn text_range(&self) -> TextRange {
        self.0.range()
    }
}

impl<N: TailwindClassContextNode + 'static> Queryable for TailwindClassContext<N> {
    type Input = Self;
    type Output = N;
    type Language = N::Language;
    type Services = ();

    fn build_visitor(
        analyzer: &mut impl AddVisitor<Self::Language>,
        _: &<Self::Language as Language>::Root,
    ) {
        analyzer.add_visitor(Phases::Syntax, || ClassContextVisitor::<N>(PhantomData));
    }

    fn unwrap_match(_: &ServiceBag, context: &Self::Input) -> Self::Output {
        context.0.clone()
    }
}

struct ClassContextVisitor<N>(PhantomData<N>);

impl<N: TailwindClassContextNode + 'static> Visitor for ClassContextVisitor<N> {
    type Language = N::Language;

    fn visit(
        &mut self,
        event: &WalkEvent<SyntaxNode<Self::Language>>,
        mut ctx: VisitorContext<Self::Language>,
    ) {
        if let WalkEvent::Enter(node) = event
            && let Some(context) = N::cast_ref(node)
            && context.is_tailwind_class_context(ctx.options.tailwind())
        {
            ctx.match_query(TailwindClassContext(context));
        }
    }
}

const DEFAULT_MERGE_FUNCTIONS: [&str; 8] =
    ["clsx", "tw", "twMerge", "twJoin", "cn", "cc", "cnb", "ctl"];

const DEFAULT_VARIANT_FUNCTIONS: [&str; 2] = ["cva", "tv"];

pub(crate) fn is_merge_function(options: &TailwindOptions, name: &str) -> bool {
    options.merge_functions().map_or_else(
        || DEFAULT_MERGE_FUNCTIONS.contains(&name),
        |functions| functions.iter().any(|function| function.as_ref() == name),
    )
}

pub(crate) fn is_variant_function(options: &TailwindOptions, name: &str) -> bool {
    options.variant_functions().map_or_else(
        || DEFAULT_VARIANT_FUNCTIONS.contains(&name),
        |functions| functions.iter().any(|function| function.as_ref() == name),
    )
}

/// Returns the identifier a callee or template tag is rooted at, so both `tw`
/// and `tw.div.span` yield `tw`.
pub(crate) fn get_root_name(expression: AnyJsExpression) -> Option<TokenText> {
    let mut current = expression;
    loop {
        match current {
            AnyJsExpression::JsIdentifierExpression(identifier) => {
                return identifier.name().ok()?.name().ok();
            }
            AnyJsExpression::JsStaticMemberExpression(member) => {
                current = member.object().ok()?;
            }
            _ => return None,
        }
    }
}

fn get_jsx_attribute_name(attribute: &JsxAttribute) -> Option<TokenText> {
    Some(
        attribute
            .name()
            .ok()?
            .as_jsx_name()?
            .value_token()
            .ok()?
            .token_text_trimmed(),
    )
}

const DEFAULT_ATTRIBUTES: [&str; 2] = ["class", "className"];

fn is_configured_attribute(options: &TailwindOptions, name: &str) -> bool {
    options.attributes().map_or_else(
        || DEFAULT_ATTRIBUTES.contains(&name),
        |attributes| {
            attributes
                .iter()
                .any(|attribute| attribute.as_ref() == name)
        },
    )
}
