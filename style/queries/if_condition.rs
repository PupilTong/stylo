/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! The `<if-condition>` of css-values-5's `if()` notation:
//! <https://drafts.csswg.org/css-values-5/#if-notation>
//!
//! ```text
//! <if-condition> = <boolean-expr[ <if-test> ]> | else
//! <if-test> = supports( [ <ident> : <declaration-value> ] | <supports-condition> ) |
//!             media( <media-feature> | <media-condition> ) |
//!             style( <style-query> )
//! ```
//!
//! A condition is parsed from its text after the arbitrary substitution
//! functions in it were substituted ("replace an if() function", step 1), so
//! `else` and every test are recognized in the substituted text. The three
//! tests reuse the `@container style()`, media-query and `@supports` parsers
//! and evaluators; only the boolean combination over them lives here.

use super::condition::{
    consume_any_value, AllowOr, OperationParser, Operator, StyleQuery, StyleQuerySubject,
};
use super::{FeatureType, QueryCondition, QueryFeatureExpression};
use crate::context::QuirksMode;
use crate::custom_properties::Name;
use crate::dom::AttributeTracker;
use crate::parser::ParserContext;
use crate::stylesheets::supports_rule::{parse_condition_or_declaration, SupportsCondition};
use crate::stylesheets::{CssRuleType, CustomMediaEvaluator, Origin, UrlExtraData};
use crate::values::computed;
use cssparser::{match_ignore_ascii_case, Parser, Token};
use selectors::kleene_value::KleeneValue;
use style_traits::{ParseError, ParsingMode, StyleParseErrorKind};

/// A parsed `<if-condition>`.
pub enum IfCondition {
    /// `else`, which is always true.
    Else,
    /// A `<boolean-expr[ <if-test> ]>`.
    Expression(IfExpression),
}

/// `<boolean-expr[ <if-test> ]>`: css-values-5 §3.2, evaluated with the
/// three-valued logic of Appendix B.
pub enum IfExpression {
    /// `not <boolean-expr-group>`
    Not(Box<IfExpression>),
    /// Groups joined by `and`, or by `or`.
    Operation(Box<[IfExpression]>, Operator),
    /// `( <boolean-expr[ <if-test> ]> )`
    InParens(Box<IfExpression>),
    /// `style( <style-query> )`
    Style(StyleQuery),
    /// `media( <media-feature> | <media-condition> )`
    Media(QueryCondition),
    /// `supports( [ <ident> : <declaration-value> ] | <supports-condition> )`
    Supports(SupportsCondition),
    /// `<general-enclosed>`, which is unknown.
    GeneralEnclosed,
}

impl IfCondition {
    /// Parses all of `css`, a condition whose substitution functions were
    /// already substituted, as an `<if-condition>`. `None` is a parse
    /// failure, which skips the branch.
    pub fn parse(css: &str, url_data: &UrlExtraData, quirks_mode: QuirksMode) -> Option<Self> {
        let context = ParserContext::new(
            Origin::Author,
            url_data,
            Some(CssRuleType::Style),
            ParsingMode::DEFAULT,
            quirks_mode,
            /* namespaces = */ Default::default(),
            /* error_reporter = */ None,
            /* use_counters = */ None,
            /* attr_taint = */ Default::default(),
        );
        Parser::new(css)
            .parse_entirely(|input| {
                if input.try_parse(|i| i.expect_ident_matching("else")).is_ok() {
                    return Ok(Self::Else);
                }
                IfExpression::parse_internal(&context, input, FeatureType::Media, AllowOr::Yes)
                    .map(Self::Expression)
            })
            .ok()
    }

    /// Evaluates the condition; an unknown result is false (Appendix B).
    ///
    /// `style()` reads `subject`, `media()` the device, and `supports()` this
    /// engine's own grammar, with `url_data` resolving any URL it parses.
    pub fn matches(
        &self,
        ctx: &computed::Context,
        subject: &dyn StyleQuerySubject,
        url_data: &UrlExtraData,
        attribute_tracker: &mut AttributeTracker,
    ) -> bool {
        match self {
            Self::Else => true,
            Self::Expression(expression) => expression
                .matches(ctx, subject, url_data, attribute_tracker)
                .to_bool(/* unknown = */ false),
        }
    }

    /// Calls `f` with the name of every custom property a `style()` test in
    /// this condition reads by name.
    pub fn for_each_queried_property(&self, f: &mut dyn FnMut(&Name)) {
        if let Self::Expression(expression) = self {
            expression.for_each_queried_property(f);
        }
    }
}

impl IfExpression {
    fn parse_test(
        context: &ParserContext,
        name: &str,
        input: &mut Parser,
    ) -> Result<Self, ParseError> {
        match_ignore_ascii_case! { name,
            "style" => StyleQuery::parse_query(context, input).map(Self::Style),
            "media" => {
                // Media queries parse without element context, so the
                // tree-counting functions stay out of them.
                let context = ParserContext::new(
                    context.stylesheet_origin,
                    context.url_data,
                    Some(CssRuleType::Style),
                    context.parsing_mode | ParsingMode::MEDIA_QUERY_CONDITION,
                    context.quirks_mode,
                    /* namespaces = */ Default::default(),
                    /* error_reporter = */ None,
                    /* use_counters = */ None,
                    /* attr_taint = */ Default::default(),
                );
                if let Ok(feature) = input.try_parse(|input| {
                    QueryFeatureExpression::parse_in_parenthesis_block(
                        &context,
                        input,
                        FeatureType::Media,
                    )
                }) {
                    return Ok(Self::Media(QueryCondition::Feature(feature)));
                }
                QueryCondition::parse(&context, input, FeatureType::Media).map(Self::Media)
            },
            "supports" => parse_condition_or_declaration(input).map(Self::Supports),
            _ => Err(ParseError::custom(StyleParseErrorKind::UnspecifiedError)),
        }
    }

    fn matches(
        &self,
        ctx: &computed::Context,
        subject: &dyn StyleQuerySubject,
        url_data: &UrlExtraData,
        attribute_tracker: &mut AttributeTracker,
    ) -> KleeneValue {
        match self {
            Self::Not(c) => !c.matches(ctx, subject, url_data, attribute_tracker),
            Self::InParens(c) => c.matches(ctx, subject, url_data, attribute_tracker),
            Self::Operation(conditions, Operator::And) => {
                KleeneValue::any_false(conditions.iter(), |c| {
                    c.matches(ctx, subject, url_data, attribute_tracker)
                })
            },
            Self::Operation(conditions, Operator::Or) => KleeneValue::any(conditions.iter(), |c| {
                c.matches(ctx, subject, url_data, attribute_tracker)
            }),
            Self::Style(query) => query.matches(ctx, subject, attribute_tracker),
            Self::Media(condition) => computed::Context::for_media_query_evaluation(
                ctx.device(),
                ctx.quirks_mode,
                |media_context| {
                    condition.matches(
                        media_context,
                        &mut CustomMediaEvaluator::none(),
                        attribute_tracker,
                    )
                },
            ),
            Self::Supports(condition) => {
                let context = ParserContext::new(
                    Origin::Author,
                    url_data,
                    Some(CssRuleType::Style),
                    ParsingMode::DEFAULT,
                    ctx.quirks_mode,
                    /* namespaces = */ Default::default(),
                    /* error_reporter = */ None,
                    /* use_counters = */ None,
                    /* attr_taint = */ Default::default(),
                );
                KleeneValue::from(condition.eval(&context))
            },
            Self::GeneralEnclosed => KleeneValue::Unknown,
        }
    }

    fn for_each_queried_property(&self, f: &mut dyn FnMut(&Name)) {
        match self {
            Self::Not(c) | Self::InParens(c) => c.for_each_queried_property(f),
            Self::Operation(conditions, _) => conditions
                .iter()
                .for_each(|c| c.for_each_queried_property(f)),
            Self::Style(query) => query.for_each_queried_property(f),
            Self::Media(..) | Self::Supports(..) | Self::GeneralEnclosed => {},
        }
    }
}

impl OperationParser for IfExpression {
    /// `<boolean-expr-group> = <if-test> | ( <boolean-expr[ <if-test> ]> ) | <general-enclosed>`
    fn parse_in_parens(
        context: &ParserContext,
        input: &mut Parser,
        feature_type: FeatureType,
    ) -> Result<Self, ParseError> {
        input.skip_whitespace();
        match *input.next()? {
            Token::ParenthesisBlock => {
                if let Ok(inner) = input.try_parse(|input| {
                    input.parse_nested_block(|input| {
                        Self::parse_internal(context, input, feature_type, AllowOr::Yes)
                    })
                }) {
                    return Ok(Self::InParens(Box::new(inner)));
                }
            },
            Token::Function(ref name) => {
                let name = name.clone();
                if let Ok(test) = input.try_parse(|input| {
                    input.parse_nested_block(|input| Self::parse_test(context, &name, input))
                }) {
                    return Ok(test);
                }
            },
            _ => return Err(ParseError::unexpected_token()),
        }
        input.parse_nested_block(consume_any_value)?;
        Ok(Self::GeneralEnclosed)
    }

    fn new_not(inner: Box<Self>) -> Self {
        Self::Not(inner)
    }

    fn new_operation(conditions: Box<[Self]>, operator: Operator) -> Self {
        Self::Operation(conditions, operator)
    }
}
