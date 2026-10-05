/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! A query condition:
//!
//! https://drafts.csswg.org/mediaqueries-4/#typedef-media-condition
//! https://drafts.csswg.org/css-contain-3/#typedef-container-condition

use super::{FeatureFlags, FeatureType, QueryFeatureExpression, QueryStyleRange};
use crate::computed_value_flags::ComputedValueFlags;
use crate::context::QuirksMode;
use crate::custom_properties;
use crate::derives::*;
use crate::dom::AttributeTracker;
use crate::properties::CSSWideKeyword;
use crate::properties_and_values::rule::Descriptors as PropertyDescriptors;
use crate::properties_and_values::value::{
    AllowComputationallyDependent, ComputedValue as ComputedRegisteredValue,
    SpecifiedValue as SpecifiedRegisteredValue,
};
use crate::stylesheets::container_rule::AttrReferenceSet;
use crate::stylesheets::{CssRuleType, CustomMediaEvaluator, Origin, UrlExtraData};
use crate::stylist::Stylist;
use crate::values::{AtomString, DashedIdent, computed};
use crate::{error_reporting::ContextualParseError, parser::Parse, parser::ParserContext};
use cssparser::{
    Parser, SourceLocation, SourcePosition, Token, match_ignore_ascii_case, parse_important,
};
use selectors::kleene_value::KleeneValue;
use servo_arc::Arc;
use std::fmt::{self, Write};
use style_traits::{CssWriter, ParseError, ParsingMode, StyleParseErrorKind, ToCss};

/// A binary `and` or `or` operator.
#[derive(Clone, Copy, Debug, Eq, MallocSizeOf, Parse, PartialEq, ToCss, ToShmem)]
#[allow(missing_docs)]
pub enum Operator {
    And,
    Or,
}

/// Whether to allow an `or` condition or not during parsing.
#[derive(Clone, Copy, Debug, Eq, MallocSizeOf, PartialEq, ToCss)]
pub(crate) enum AllowOr {
    Yes,
    No,
}

#[derive(Clone, Debug, PartialEq, ToShmem)]
enum StyleFeatureValue {
    Value(Option<Arc<custom_properties::SpecifiedValue>>),
    Keyword(CSSWideKeyword),
}

/// Where a `style()` query reads the custom properties it tests.
///
/// `@container style()` reads the query container, whose computed style the
/// container-query context carries as its inherited style
/// ([`ContainerStyleQuery`]). css-values-5's `if()` reads the element whose
/// declaration is being substituted.
pub trait StyleQuerySubject {
    /// Whether there is a subject at all. Without one, a feature with a value
    /// is false.
    fn exists(&self, ctx: &computed::Context) -> bool;

    /// The subject's value of `name`; `None` is the guaranteed-invalid value.
    fn custom_property<'a>(
        &'a self,
        ctx: &'a computed::Context,
        registration: &PropertyDescriptors,
        name: &custom_properties::Name,
    ) -> Option<&'a ComputedRegisteredValue>;

    /// The value of `name` on the subject's parent, which a feature value of
    /// `inherit` names. `None` when there is no parent value to compare with,
    /// which makes the feature false.
    fn inherited_custom_property<'a>(
        &'a self,
        ctx: &'a computed::Context,
        registration: &PropertyDescriptors,
        name: &custom_properties::Name,
    ) -> Option<Option<&'a ComputedRegisteredValue>>;

    /// The substitution functions `var()` / `attr()` in a feature value
    /// substitute against.
    fn substitution_functions(
        &self,
        ctx: &computed::Context,
    ) -> custom_properties::ComputedSubstitutionFunctions;

    /// Whether testing `name` would re-enter a substitution already in
    /// progress, which makes the test false.
    fn is_cyclic(&self, _name: &custom_properties::Name) -> bool {
        false
    }

    /// Whether a feature value of `unset` means `inherit` for an inherited
    /// property and `initial` otherwise, as it does in a declaration. When
    /// false, `unset` matches only the guaranteed-invalid value.
    fn unset_follows_inheritance(&self) -> bool {
        false
    }

    /// Whether a feature without a value is true only for a value that differs
    /// from the property's initial value. When false, it is true for any value
    /// but the guaranteed-invalid value.
    fn boolean_compares_with_initial(&self) -> bool {
        false
    }

    /// Records on the style being computed that it depends on this query.
    fn note_dependency(&self, _ctx: &computed::Context) {}

    /// Records that a value substituted while evaluating the query was
    /// attr()-tainted.
    fn note_attr_taint(&self) {}

    /// Whether a feature value that substitutes to the guaranteed-invalid value
    /// matches the subject's value `current`.
    fn invalid_value_matches(&self, current: Option<&ComputedRegisteredValue>) -> bool {
        current.is_none()
    }

    /// Whether a feature value equals the subject's value.
    fn same_value(
        &self,
        a: Option<&ComputedRegisteredValue>,
        b: Option<&ComputedRegisteredValue>,
    ) -> bool {
        a == b
    }
}

/// The subject of `@container style()`: the query container.
pub struct ContainerStyleQuery;

impl StyleQuerySubject for ContainerStyleQuery {
    fn exists(&self, ctx: &computed::Context) -> bool {
        // If no container, custom props are guaranteed-unknown.
        ctx.container_info.is_some()
    }

    fn custom_property<'a>(
        &'a self,
        ctx: &'a computed::Context,
        registration: &PropertyDescriptors,
        name: &custom_properties::Name,
    ) -> Option<&'a ComputedRegisteredValue> {
        ctx.inherited_custom_properties().get(registration, name)
    }

    fn inherited_custom_property<'a>(
        &'a self,
        ctx: &'a computed::Context,
        registration: &PropertyDescriptors,
        name: &custom_properties::Name,
    ) -> Option<Option<&'a ComputedRegisteredValue>> {
        let inherited = ctx
            .container_info
            .as_ref()
            .expect("queries should provide container info")
            .inherited_style()?;
        Some(inherited.custom_properties().get(registration, name))
    }

    fn substitution_functions(
        &self,
        ctx: &computed::Context,
    ) -> custom_properties::ComputedSubstitutionFunctions {
        custom_properties::ComputedSubstitutionFunctions::new(
            Some(ctx.inherited_custom_properties().clone()),
            None,
        )
    }

    fn note_dependency(&self, ctx: &computed::Context) {
        ctx.builder
            .add_flags(ComputedValueFlags::DEPENDS_ON_CONTAINER_STYLE_QUERY);
    }
}

/// Trait for query elements that parse a series of conditions separated by
/// AND or OR operators, or prefixed with NOT.
///
/// This is used by both QueryCondition and StyleQuery as they support similar
/// syntax for combining multiple conditions with a boolean operator.
pub(crate) trait OperationParser: Sized {
    /// https://drafts.csswg.org/mediaqueries-5/#typedef-media-condition or
    /// https://drafts.csswg.org/mediaqueries-5/#typedef-media-condition-without-or
    /// (depending on `allow_or`).
    fn parse_internal(
        context: &ParserContext,
        input: &mut Parser,
        feature_type: FeatureType,
        allow_or: AllowOr,
    ) -> Result<Self, ParseError> {
        if input.try_parse(|i| i.expect_ident_matching("not")).is_ok() {
            let inner_condition = Self::parse_in_parens(context, input, feature_type)?;
            return Ok(Self::new_not(Box::new(inner_condition)));
        }

        let first_condition = Self::parse_in_parens(context, input, feature_type)?;
        let operator = match input.try_parse(Operator::parse) {
            Ok(op) => op,
            Err(..) => return Ok(first_condition),
        };

        if allow_or == AllowOr::No && operator == Operator::Or {
            return Err(ParseError::custom(StyleParseErrorKind::UnspecifiedError));
        }

        let mut conditions = vec![];
        conditions.push(first_condition);
        conditions.push(Self::parse_in_parens(context, input, feature_type)?);

        let delim = match operator {
            Operator::And => "and",
            Operator::Or => "or",
        };

        loop {
            if input.try_parse(|i| i.expect_ident_matching(delim)).is_err() {
                return Ok(Self::new_operation(conditions.into_boxed_slice(), operator));
            }

            conditions.push(Self::parse_in_parens(context, input, feature_type)?);
        }
    }

    // Parse a condition in parentheses, or `<general-enclosed>`.
    fn parse_in_parens(
        context: &ParserContext,
        input: &mut Parser,
        feature_type: FeatureType,
    ) -> Result<Self, ParseError>;

    // Helpers to create the appropriate enum variant of the implementing type:
    // Create a Not result that encapsulates the `inner` condition.
    fn new_not(inner: Box<Self>) -> Self;

    // Create an Operation result with the given list of `conditions` using `operator`.
    fn new_operation(conditions: Box<[Self]>, operator: Operator) -> Self;
}

fn try_parse_block<'i, T, F>(
    context: &ParserContext,
    input: &mut Parser<'i>,
    start: SourcePosition,
    start_location: SourceLocation,
    parse: F,
) -> Option<T>
where
    F: FnOnce(&mut Parser<'i>) -> Result<T, ParseError>,
{
    input
        .try_parse(|input| {
            let result = input.parse_nested_block(parse);
            if let Err(ref e) = result
                && context.error_reporting_enabled()
            {
                // We're about to swallow the error in a `<general-enclosed>` condition, so report
                // it while we can.
                let error =
                    ContextualParseError::InvalidMediaRule(input.slice_from(start), e.clone());
                context.log_css_error(start_location, error);
            }
            result
        })
        .ok()
}

/// https://drafts.csswg.org/css-conditional-5/#typedef-style-query
#[derive(Clone, Debug, MallocSizeOf, PartialEq, ToShmem)]
pub enum StyleQuery {
    /// A negation of a condition.
    Not(Box<StyleQuery>),
    /// A set of joint operations.
    Operation(Box<[StyleQuery]>, Operator),
    /// A condition wrapped in parenthesis.
    InParens(Box<StyleQuery>),
    /// A feature query (`--foo: bar` or just `--foo`).
    Feature(StyleFeature),
    /// An unknown "general-enclosed" term.
    GeneralEnclosed(String),
}

impl ToCss for StyleQuery {
    fn to_css<W>(&self, dest: &mut CssWriter<W>) -> fmt::Result
    where
        W: fmt::Write,
    {
        match *self {
            StyleQuery::Not(ref c) => {
                dest.write_str("not ")?;
                c.maybe_parenthesized(dest)
            },
            StyleQuery::Operation(ref list, op) => {
                let mut iter = list.iter();
                let item = iter.next().unwrap();
                item.maybe_parenthesized(dest)?;
                for item in iter {
                    dest.write_char(' ')?;
                    op.to_css(dest)?;
                    dest.write_char(' ')?;
                    item.maybe_parenthesized(dest)?;
                }
                Ok(())
            },
            StyleQuery::InParens(ref c) => match &**c {
                StyleQuery::Feature(_) | StyleQuery::InParens(_) => {
                    dest.write_char('(')?;
                    c.to_css(dest)?;
                    dest.write_char(')')
                },
                _ => c.to_css(dest),
            },
            StyleQuery::Feature(ref f) => f.to_css(dest),
            StyleQuery::GeneralEnclosed(ref s) => dest.write_str(s),
        }
    }
}

impl StyleQuery {
    // Helper for to_css when handling values within boolean operators:
    // GeneralEnclosed includes its parens in the string, so we don't need to
    // wrap the value with an additional set here.
    fn maybe_parenthesized<W>(&self, dest: &mut CssWriter<W>) -> fmt::Result
    where
        W: fmt::Write,
    {
        if let StyleQuery::GeneralEnclosed(s) = self {
            dest.write_str(s)
        } else {
            dest.write_char('(')?;
            self.to_css(dest)?;
            dest.write_char(')')
        }
    }

    fn enabled(feature_type: FeatureType) -> bool {
        crate::pref!("layout.css.style-queries.enabled") && feature_type == FeatureType::Container
    }

    fn parse(
        context: &ParserContext,
        input: &mut Parser,
        feature_type: FeatureType,
    ) -> Result<Self, ParseError> {
        if let Ok(feature) = input.try_parse(|input| StyleFeature::parse(context, input)) {
            return Ok(Self::Feature(feature));
        }

        let inner = Self::parse_internal(context, input, feature_type, AllowOr::Yes)?;
        Ok(Self::InParens(Box::new(inner)))
    }

    /// Parses a `<style-query>`: the contents of a `style()` function.
    #[cfg(feature = "lynx")]
    pub(crate) fn parse_query(
        context: &ParserContext,
        input: &mut Parser,
    ) -> Result<Self, ParseError> {
        Self::parse(context, input, FeatureType::Container)
    }

    fn parse_in_parenthesis_block(
        context: &ParserContext,
        input: &mut Parser,
    ) -> Result<Self, ParseError> {
        // Base case. Make sure to preserve this error as it's more generally
        // relevant.
        let feature_error = match input.try_parse(|input| StyleFeature::parse(context, input)) {
            Ok(feature) => return Ok(Self::Feature(feature)),
            Err(e) => e,
        };

        if let Ok(inner) = Self::parse(context, input, FeatureType::Container) {
            return Ok(inner);
        }

        Err(feature_error)
    }

    /// Evaluates this query against `subject`.
    pub(crate) fn matches(
        &self,
        ctx: &computed::Context,
        subject: &dyn StyleQuerySubject,
        attribute_tracker: &mut AttributeTracker,
    ) -> KleeneValue {
        subject.note_dependency(ctx);
        match *self {
            StyleQuery::Feature(ref f) => f.matches(ctx, subject, attribute_tracker),
            StyleQuery::Not(ref c) => !c.matches(ctx, subject, attribute_tracker),
            StyleQuery::InParens(ref c) => c.matches(ctx, subject, attribute_tracker),
            StyleQuery::Operation(ref conditions, op) => {
                debug_assert!(!conditions.is_empty(), "We never create an empty op");
                match op {
                    Operator::And => KleeneValue::any_false(conditions.iter(), |c| {
                        c.matches(ctx, subject, attribute_tracker)
                    }),
                    Operator::Or => KleeneValue::any(conditions.iter(), |c| {
                        c.matches(ctx, subject, attribute_tracker)
                    }),
                }
            },
            StyleQuery::GeneralEnclosed(_) => KleeneValue::Unknown,
        }
    }

    /// Calls `f` with the name of every custom property this query tests by
    /// name.
    #[cfg(feature = "lynx")]
    pub(crate) fn for_each_queried_property(&self, f: &mut dyn FnMut(&custom_properties::Name)) {
        match self {
            Self::Feature(feature) => feature.for_each_queried_property(f),
            Self::GeneralEnclosed(_) => {},
            Self::InParens(c) | Self::Not(c) => c.for_each_queried_property(f),
            Self::Operation(c, _) => c.iter().for_each(|c| c.for_each_queried_property(f)),
        }
    }

    fn collect_attribute_references(&self, references: &mut AttrReferenceSet) {
        match self {
            Self::Feature(c) => c.collect_attribute_references(references),
            Self::GeneralEnclosed(_) => {},
            Self::InParens(c) => c.collect_attribute_references(references),
            Self::Not(c) => c.collect_attribute_references(references),
            Self::Operation(c, _) => c
                .iter()
                .for_each(|c| c.collect_attribute_references(references)),
        }
    }
}

impl OperationParser for StyleQuery {
    fn parse_in_parens(
        context: &ParserContext,
        input: &mut Parser,
        feature_type: FeatureType,
    ) -> Result<Self, ParseError> {
        assert!(feature_type == FeatureType::Container);
        input.skip_whitespace();
        let start = input.position();
        let start_location = input.current_source_location();
        match *input.next()? {
            Token::ParenthesisBlock => {
                if let Some(nested) = try_parse_block(context, input, start, start_location, |i| {
                    Self::parse_in_parenthesis_block(context, i)
                }) {
                    return Ok(nested);
                }
                // Accept <ident>: <any-value> as a GeneralEnclosed (which evaluates
                // to false, but does not invalidate the query as a whole).
                input.parse_nested_block(|i| {
                    i.expect_ident()?;
                    i.expect_colon()?;
                    consume_any_value(i)
                })?;
                Ok(Self::GeneralEnclosed(input.slice_from(start).to_owned()))
            },
            _ => Err(ParseError::unexpected_token()),
        }
    }

    fn new_not(inner: Box<Self>) -> Self {
        Self::Not(inner)
    }

    fn new_operation(conditions: Box<[Self]>, operator: Operator) -> Self {
        Self::Operation(conditions, operator)
    }
}

/// A style query feature:
/// https://drafts.csswg.org/css-conditional-5/#typedef-style-feature
#[derive(Clone, Debug, MallocSizeOf, PartialEq, ToCss, ToShmem)]
pub enum StyleFeature {
    /// A property name and optional value to match.
    Plain(StyleFeaturePlain),
    /// A style query range expression.
    Range(QueryStyleRange),
}

impl StyleFeature {
    fn parse(context: &ParserContext, input: &mut Parser) -> Result<Self, ParseError> {
        if let Ok(range) = input.try_parse(|i| QueryStyleRange::parse(context, i)) {
            return Ok(Self::Range(range));
        }

        Ok(Self::Plain(StyleFeaturePlain::parse(context, input)?))
    }

    fn matches(
        &self,
        ctx: &computed::Context,
        subject: &dyn StyleQuerySubject,
        attribute_tracker: &mut AttributeTracker,
    ) -> KleeneValue {
        match self {
            Self::Plain(plain) => plain.matches(ctx, subject, attribute_tracker),
            Self::Range(range) => range.evaluate(ctx, subject, attribute_tracker),
        }
    }

    #[cfg(feature = "lynx")]
    fn for_each_queried_property(&self, f: &mut dyn FnMut(&custom_properties::Name)) {
        match self {
            Self::Plain(plain) => f(&plain.name),
            Self::Range(range) => range.for_each_queried_property(f),
        }
    }

    fn collect_attribute_references(&self, references: &mut AttrReferenceSet) {
        match self {
            Self::Plain(plain) => plain.collect_attribute_references(references),
            Self::Range(range) => range.collect_attribute_references(references),
        }
    }
}

/// A style feature consisting of a custom property name and (optionally) value.
#[derive(Clone, Debug, MallocSizeOf, PartialEq, ToShmem)]
pub struct StyleFeaturePlain {
    name: custom_properties::Name,
    #[ignore_malloc_size_of = "StyleFeatureValue has an Arc variant"]
    value: StyleFeatureValue,
}

impl ToCss for StyleFeaturePlain {
    fn to_css<W>(&self, dest: &mut CssWriter<W>) -> fmt::Result
    where
        W: fmt::Write,
    {
        dest.write_str("--")?;
        crate::values::serialize_atom_identifier(&self.name, dest)?;
        match self.value {
            StyleFeatureValue::Keyword(k) => {
                dest.write_str(": ")?;
                k.to_css(dest)?;
            },
            StyleFeatureValue::Value(Some(ref v)) => {
                dest.write_str(": ")?;
                v.to_css(dest)?;
            },
            StyleFeatureValue::Value(None) => (),
        }
        Ok(())
    }
}

impl StyleFeaturePlain {
    fn parse(context: &ParserContext, input: &mut Parser) -> Result<Self, ParseError> {
        let ident = input.expect_ident()?;
        // TODO(emilio): Maybe support non-custom properties?
        let name = match custom_properties::parse_name(ident.as_ref()) {
            Ok(name) => custom_properties::Name::from(name),
            Err(()) => return Err(ParseError::custom(StyleParseErrorKind::UnspecifiedError)),
        };
        let value = if input.try_parse(|i| i.expect_colon()).is_ok() {
            input.skip_whitespace();
            if let Ok(keyword) = input.try_parse(|i| CSSWideKeyword::parse(i)) {
                StyleFeatureValue::Keyword(keyword)
            } else {
                let value = custom_properties::SpecifiedValue::parse(
                    input,
                    Some(&context.namespaces.prefixes),
                    context.url_data,
                )?;
                // `!important` is allowed (but ignored) after the value.
                let _ = input.try_parse(parse_important);
                StyleFeatureValue::Value(Some(Arc::new(value)))
            }
        } else {
            StyleFeatureValue::Value(None)
        };
        Ok(Self { name, value })
    }

    // Substitute custom-property references in `value`, then re-parse and compute it,
    // and compare against `current_value`.
    fn substitute_and_compare(
        value: &Arc<custom_properties::SpecifiedValue>,
        registration: &PropertyDescriptors,
        stylist: &Stylist,
        ctx: &computed::Context,
        subject: &dyn StyleQuerySubject,
        attribute_tracker: &mut AttributeTracker,
        current_value: Option<&ComputedRegisteredValue>,
    ) -> bool {
        let substitution_functions = subject.substitution_functions(ctx);
        let custom_properties::SubstitutionResult { css, attr_taint } =
            match custom_properties::substitute(
                value,
                /* property_id */ None,
                &substitution_functions,
                stylist,
                ctx,
                attribute_tracker,
            ) {
                Ok(sub) => sub,
                Err(_) => return subject.invalid_value_matches(current_value),
            };
        if !attr_taint.is_empty() {
            subject.note_attr_taint();
        }
        if registration.is_universal() {
            return match current_value {
                Some(v) => v.as_universal().is_some_and(|v| v.css == css),
                None => css.is_empty(),
            };
        }
        let mut parser = Parser::new(&css);
        let computed = SpecifiedRegisteredValue::compute(
            &mut parser,
            registration,
            None,
            &value.url_data,
            ctx,
            AllowComputationallyDependent::Yes,
            attr_taint,
            /* property_id */ None,
        )
        .ok();
        subject.same_value(computed.as_ref(), current_value)
    }

    fn matches(
        &self,
        ctx: &computed::Context,
        subject: &dyn StyleQuerySubject,
        attribute_tracker: &mut AttributeTracker,
    ) -> KleeneValue {
        if subject.is_cyclic(&self.name) {
            return KleeneValue::False;
        }
        // FIXME(emilio): Confirm this is the right style to query.
        let stylist = ctx
            .builder
            .stylist
            .expect("container queries should have a stylist around");
        let registration = stylist.get_custom_property_registration(&self.name);
        let current_value = subject.custom_property(ctx, registration, &self.name);
        KleeneValue::from(match self.value {
            StyleFeatureValue::Value(Some(ref v)) => {
                if !subject.exists(ctx) {
                    false
                } else if v.has_references() {
                    // If there are --var() references in the query value,
                    // try to substitute them before comparing to current.
                    Self::substitute_and_compare(
                        v,
                        registration,
                        stylist,
                        ctx,
                        subject,
                        attribute_tracker,
                        current_value,
                    )
                } else {
                    subject.same_value(
                        custom_properties::compute_variable_value(
                            v,
                            registration,
                            ctx,
                            /* property_id */ None,
                        )
                        .as_ref(),
                        current_value,
                    )
                }
            },
            StyleFeatureValue::Value(None) => match registration.initial_value {
                Some(ref initial) if subject.boolean_compares_with_initial() => !subject
                    .same_value(
                        custom_properties::compute_variable_value(
                            initial,
                            registration,
                            ctx,
                            /* property_id */ None,
                        )
                        .as_ref(),
                        current_value,
                    ),
                _ => current_value.is_some(),
            },
            StyleFeatureValue::Keyword(kw) => {
                let kw = match kw {
                    CSSWideKeyword::Unset if subject.unset_follows_inheritance() => {
                        if registration.inherits() {
                            CSSWideKeyword::Inherit
                        } else {
                            CSSWideKeyword::Initial
                        }
                    },
                    kw => kw,
                };
                match kw {
                    CSSWideKeyword::Unset => current_value.is_none(),
                    CSSWideKeyword::Initial => {
                        if let Some(initial) = &registration.initial_value {
                            let v = custom_properties::compute_variable_value(
                                initial,
                                registration,
                                ctx,
                                /* property_id */ None,
                            );
                            subject.same_value(v.as_ref(), current_value)
                        } else {
                            current_value.is_none()
                        }
                    },
                    CSSWideKeyword::Inherit => {
                        match subject.inherited_custom_property(ctx, registration, &self.name) {
                            Some(inherited) => subject.same_value(inherited, current_value),
                            None => false,
                        }
                    },
                    // Cascade-dependent keywords, such as revert and revert-layer,
                    // are invalid as values in a style feature, and cause the
                    // container style query to be false.
                    // https://drafts.csswg.org/css-conditional-5/#evaluate-a-style-range
                    CSSWideKeyword::Revert
                    | CSSWideKeyword::RevertLayer
                    | CSSWideKeyword::RevertRule => false,
                }
            },
        })
    }

    fn collect_attribute_references(&self, references: &mut AttrReferenceSet) {
        if let StyleFeatureValue::Value(Some(v)) = &self.value {
            v.collect_attribute_references(references)
        }
    }
}

/// A boolean value for a pref query.
#[derive(
    Clone,
    Debug,
    MallocSizeOf,
    PartialEq,
    Eq,
    Parse,
    SpecifiedValueInfo,
    ToComputedValue,
    ToCss,
    ToShmem,
)]
#[repr(u8)]
#[allow(missing_docs)]
pub enum BoolValue {
    False,
    True,
}

/// Simple values we support for -moz-pref(). We don't want to deal with calc() and other
/// shenanigans for now.
#[derive(
    Clone,
    Debug,
    Eq,
    MallocSizeOf,
    Parse,
    PartialEq,
    SpecifiedValueInfo,
    ToComputedValue,
    ToCss,
    ToShmem,
)]
#[repr(u8)]
pub enum MozPrefFeatureValue<I> {
    /// No pref value, implicitly bool, but also used to represent missing prefs.
    #[css(skip)]
    None,
    /// A bool value.
    Boolean(BoolValue),
    /// An integer value, useful for int prefs.
    Integer(I),
    /// A string pref value.
    String(crate::values::AtomString),
}

type SpecifiedMozPrefFeatureValue = MozPrefFeatureValue<crate::values::specified::Integer>;
/// The computed -moz-pref() value.
pub type ComputedMozPrefFeatureValue = MozPrefFeatureValue<crate::values::computed::Integer>;

/// A custom -moz-pref(<name>, <value>) query feature.
#[derive(Clone, Debug, MallocSizeOf, PartialEq, ToShmem)]
pub struct MozPrefFeature {
    name: crate::values::AtomString,
    value: SpecifiedMozPrefFeatureValue,
}

impl MozPrefFeature {
    fn parse(
        context: &ParserContext,
        input: &mut Parser,
        feature_type: FeatureType,
    ) -> Result<Self, ParseError> {
        use crate::parser::Parse;
        if !context.chrome_rules_enabled() || feature_type != FeatureType::Media {
            return Err(ParseError::custom(StyleParseErrorKind::UnspecifiedError));
        }
        let name = AtomString::parse(context, input)?;
        let value = if input.try_parse(|i| i.expect_comma()).is_ok() {
            SpecifiedMozPrefFeatureValue::parse(context, input)?
        } else {
            SpecifiedMozPrefFeatureValue::None
        };
        Ok(Self { name, value })
    }

    #[cfg(feature = "gecko")]
    fn matches(&self, ctx: &computed::Context) -> KleeneValue {
        use crate::values::computed::ToComputedValue;
        let value = self.value.to_computed_value(ctx);
        KleeneValue::from(unsafe {
            crate::gecko_bindings::bindings::Gecko_EvalMozPrefFeature(self.name.as_ptr(), &value)
        })
    }

    #[cfg(feature = "servo")]
    fn matches(&self, _: &computed::Context) -> KleeneValue {
        KleeneValue::Unknown
    }
}

impl ToCss for MozPrefFeature {
    fn to_css<W>(&self, dest: &mut CssWriter<W>) -> fmt::Result
    where
        W: fmt::Write,
    {
        self.name.to_css(dest)?;
        if !matches!(self.value, MozPrefFeatureValue::None) {
            dest.write_str(", ")?;
            self.value.to_css(dest)?;
        }
        Ok(())
    }
}

/// Represents a condition.
#[derive(Clone, Debug, MallocSizeOf, PartialEq, ToShmem)]
pub enum QueryCondition {
    /// A simple feature expression, implicitly parenthesized.
    Feature(QueryFeatureExpression),
    /// A custom media query reference in a boolean context, implicitly parenthesized.
    Custom(DashedIdent),
    /// A negation of a condition.
    Not(Box<QueryCondition>),
    /// A set of joint operations.
    Operation(Box<[QueryCondition]>, Operator),
    /// A condition wrapped in parenthesis.
    InParens(Box<QueryCondition>),
    /// A <style> query.
    Style(StyleQuery),
    /// A -moz-pref() query.
    MozPref(MozPrefFeature),
    /// [ <function-token> <any-value>? ) ] | [ ( <any-value>? ) ]
    GeneralEnclosed(String, UrlExtraData, FeatureFlags),
}

impl ToCss for QueryCondition {
    fn to_css<W>(&self, dest: &mut CssWriter<W>) -> fmt::Result
    where
        W: fmt::Write,
    {
        match *self {
            // NOTE(emilio): QueryFeatureExpression already includes the
            // parenthesis.
            Self::Feature(ref f) => f.to_css(dest),
            Self::Custom(ref name) => {
                dest.write_char('(')?;
                name.to_css(dest)?;
                dest.write_char(')')
            },
            Self::Not(ref c) => {
                dest.write_str("not ")?;
                c.to_css(dest)
            },
            Self::InParens(ref c) => {
                dest.write_char('(')?;
                c.to_css(dest)?;
                dest.write_char(')')
            },
            Self::Style(ref c) => {
                dest.write_str("style(")?;
                c.to_css(dest)?;
                dest.write_char(')')
            },
            Self::MozPref(ref c) => {
                dest.write_str("-moz-pref(")?;
                c.to_css(dest)?;
                dest.write_char(')')
            },
            Self::Operation(ref list, op) => {
                let mut iter = list.iter();
                iter.next().unwrap().to_css(dest)?;
                for item in iter {
                    dest.write_char(' ')?;
                    op.to_css(dest)?;
                    dest.write_char(' ')?;
                    item.to_css(dest)?;
                }
                Ok(())
            },
            Self::GeneralEnclosed(ref s, ..) => dest.write_str(s),
        }
    }
}

/// <https://drafts.csswg.org/css-syntax-3/#typedef-any-value>
pub(crate) fn consume_any_value(input: &mut Parser) -> Result<(), ParseError> {
    input.expect_no_error_token().map_err(Into::into)
}

impl QueryCondition {
    /// Parse a single condition.
    pub fn parse(
        context: &ParserContext,
        input: &mut Parser,
        feature_type: FeatureType,
    ) -> Result<Self, ParseError> {
        Self::parse_internal(context, input, feature_type, AllowOr::Yes)
    }

    fn visit<F>(&self, visitor: &mut F)
    where
        F: FnMut(&Self),
    {
        visitor(self);
        match *self {
            Self::Custom(..)
            | Self::Feature(..)
            | Self::GeneralEnclosed(..)
            | Self::Style(..)
            | Self::MozPref(..) => {},
            Self::Not(ref cond) => cond.visit(visitor),
            Self::Operation(ref conds, _op) => {
                for cond in conds.iter() {
                    cond.visit(visitor);
                }
            },
            Self::InParens(ref cond) => cond.visit(visitor),
        }
    }

    /// Returns the union of all flags in the expression. This is useful for
    /// container queries.
    pub fn cumulative_flags(&self) -> FeatureFlags {
        let mut result = FeatureFlags::empty();
        self.visit(&mut |condition| match condition {
            Self::Style(..) => result.insert(FeatureFlags::STYLE),
            Self::Feature(f) => result.insert(f.feature_flags()),
            Self::GeneralEnclosed(_, _, flags) => result.insert(*flags),
            _ => {},
        });
        result
    }

    /// Parse a single condition, disallowing `or` expressions.
    ///
    /// To be used from the legacy query syntax.
    pub fn parse_disallow_or(
        context: &ParserContext,
        input: &mut Parser,
        feature_type: FeatureType,
    ) -> Result<Self, ParseError> {
        Self::parse_internal(context, input, feature_type, AllowOr::No)
    }

    fn parse_in_parenthesis_block(
        context: &ParserContext,
        input: &mut Parser,
        feature_type: FeatureType,
    ) -> Result<Self, ParseError> {
        // Base case. Make sure to preserve this error as it's more generally
        // relevant.
        let feature_error = match input.try_parse(|input| {
            QueryFeatureExpression::parse_in_parenthesis_block(context, input, feature_type)
        }) {
            Ok(expr) => return Ok(Self::Feature(expr)),
            Err(e) => e,
        };
        if crate::pref!("layout.css.custom-media.enabled")
            && let Ok(custom) = input.try_parse(|input| DashedIdent::parse(context, input))
        {
            return Ok(Self::Custom(custom));
        }
        if let Ok(inner) = Self::parse(context, input, feature_type) {
            return Ok(Self::InParens(Box::new(inner)));
        }
        Err(feature_error)
    }

    /// Whether this condition matches the device and quirks mode.
    /// https://drafts.csswg.org/mediaqueries/#evaluating
    /// https://drafts.csswg.org/mediaqueries/#typedef-general-enclosed
    /// Kleene 3-valued logic is adopted here due to the introduction of
    /// <general-enclosed>.
    pub fn matches(
        &self,
        context: &computed::Context,
        custom: &mut CustomMediaEvaluator,
        attribute_tracker: &mut AttributeTracker,
    ) -> KleeneValue {
        match *self {
            Self::Custom(ref f) => custom.matches(f, context),
            Self::Feature(ref f) => f.matches(context),
            Self::GeneralEnclosed(ref str, ref url_data, _) => {
                self.matches_general(str, url_data, context, custom, attribute_tracker)
            },
            Self::InParens(ref c) => c.matches(context, custom, attribute_tracker),
            Self::Not(ref c) => !c.matches(context, custom, attribute_tracker),
            Self::Style(ref c) => c.matches(context, &ContainerStyleQuery, attribute_tracker),
            Self::MozPref(ref c) => c.matches(context),
            Self::Operation(ref conditions, op) => {
                debug_assert!(!conditions.is_empty(), "We never create an empty op");
                match op {
                    Operator::And => KleeneValue::any_false(conditions.iter(), |c| {
                        c.matches(context, custom, attribute_tracker)
                    }),
                    Operator::Or => KleeneValue::any(conditions.iter(), |c| {
                        c.matches(context, custom, attribute_tracker)
                    }),
                }
            },
        }
    }

    /// For a condition that was parsed as GeneralEnclosed, try applying custom-property
    /// substitution and re-parse the result.
    fn matches_general(
        &self,
        css_text: &str,
        url_data: &UrlExtraData,
        context: &computed::Context,
        custom: &mut CustomMediaEvaluator,
        attribute_tracker: &mut AttributeTracker,
    ) -> KleeneValue {
        // This only applies (currently, at least) to container queries.
        if !context.in_container_query {
            return KleeneValue::Unknown;
        }

        let stylist = context
            .builder
            .stylist
            .expect("container query should provide a Stylist");

        // Parse the text as a custom-property value to identify references.
        let value = match custom_properties::SpecifiedValue::parse(
            &mut Parser::new(css_text),
            None, // TODO: what Namespaces should we pass here?
            url_data,
        ) {
            Ok(val) => val,
            Err(_) => return KleeneValue::Unknown,
        };

        // If no references, we're not going to end up with a new result, just bail out.
        if !value.has_references() {
            return KleeneValue::Unknown;
        }

        // Substitute var() functions if possible.
        let substitution_functions = custom_properties::ComputedSubstitutionFunctions::new(
            Some(context.inherited_custom_properties().clone()),
            None,
        );
        let custom_properties::SubstitutionResult { css, attr_taint } =
            match custom_properties::substitute(
                &value,
                /* property_id */ None,
                &substitution_functions,
                stylist,
                context,
                attribute_tracker,
            ) {
                Ok(sub) => sub,
                Err(_) => return KleeneValue::Unknown,
            };

        // Re-parse the result as a query-condition, and evaluate it.
        let parser_context = ParserContext::new(
            Origin::Author,
            url_data,
            Some(CssRuleType::Container),
            ParsingMode::DEFAULT,
            QuirksMode::NoQuirks,
            /* namespaces = */ Default::default(),
            /* error_reporter = */ None,
            /* use_counters = */ None,
            attr_taint,
        );

        match Self::parse(
            &parser_context,
            &mut Parser::new(&css),
            FeatureType::Container,
        ) {
            Ok(Self::GeneralEnclosed(..)) => {
                // If the result is still GeneralEnclosed, the query is unknown.
                KleeneValue::Unknown
            },
            Ok(query) => query.matches(context, custom, attribute_tracker),
            Err(_) => KleeneValue::Unknown,
        }
    }

    /// Collect the attribute references in this query condition, if any.
    pub fn collect_attribute_references(&self, references: &mut AttrReferenceSet) {
        if let QueryCondition::Style(c) = self {
            c.collect_attribute_references(references)
        }
    }
}

impl OperationParser for QueryCondition {
    /// Parse a condition in parentheses, or `<general-enclosed>`.
    ///
    /// https://drafts.csswg.org/mediaqueries/#typedef-media-in-parens
    fn parse_in_parens(
        context: &ParserContext,
        input: &mut Parser,
        feature_type: FeatureType,
    ) -> Result<Self, ParseError> {
        input.skip_whitespace();
        let start = input.position();
        let start_location = input.current_source_location();
        let mut flags = FeatureFlags::empty();
        match *input.next()? {
            Token::ParenthesisBlock => {
                let nested = try_parse_block(context, input, start, start_location, |input| {
                    Self::parse_in_parenthesis_block(context, input, feature_type)
                });
                if let Some(nested) = nested {
                    return Ok(nested);
                }
            },
            Token::Function(ref name) => {
                match_ignore_ascii_case! { name,
                    "style" if StyleQuery::enabled(feature_type) => {
                        let query = try_parse_block(context, input, start, start_location, |input| {
                            StyleQuery::parse(context, input, feature_type)
                        });
                        if let Some(query) = query {
                            return Ok(Self::Style(query));
                        }
                        flags.insert(FeatureFlags::STYLE);
                    },
                    "-moz-pref" => {
                        let feature = try_parse_block(context, input, start, start_location, |input| {
                            MozPrefFeature::parse(context, input, feature_type)
                        });
                        if let Some(feature) = feature {
                            return Ok(Self::MozPref(feature));
                        }
                    },
                    _ => {},
                }
            },
            _ => return Err(ParseError::unexpected_token()),
        }
        input.parse_nested_block(consume_any_value)?;
        Ok(Self::GeneralEnclosed(
            input.slice_from(start).to_owned(),
            context.url_data.clone(),
            flags,
        ))
    }

    fn new_not(inner: Box<Self>) -> Self {
        Self::Not(inner)
    }

    fn new_operation(conditions: Box<[Self]>, operator: Operator) -> Self {
        Self::Operation(conditions, operator)
    }
}
