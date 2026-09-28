//! css-values-5 §8.3 `if()` and §10 `sibling-index()` / `sibling-count()`
//! under the `lynx` feature: the argument grammar a declaration is checked
//! against at parse time, the `<if-condition>` grammar a branch is parsed
//! with after substitution, and the evaluation of conditions that need no
//! element.
//!
//! Evaluation against an element (the cascade, the cycle rule, invalidation)
//! is exercised through a real document by the outer repository's
//! `crates/dom/tests/if_function.rs`.
#![cfg(feature = "lynx")]

use cssparser::Parser as CssParser;
use euclid::{Scale, Size2D};
use servo_arc::Arc;
use style::context::QuirksMode;
use style::custom_properties::{
    AttrTaint, ComputedSubstitutionFunctions, Name, SubstitutionFunctionKind, VariableValue,
};
use style::custom_properties_map::CustomPropertiesMap;
use style::device::servo::FontMetricsProvider;
use style::device::Device;
use style::dom::{AttributeTracker, DummyElementContext};
use style::font_metrics::FontMetrics;
use style::media_queries::MediaType;
use style::parser::{Parse, ParserContext};
use style::properties::declaration_block::parse_one_declaration_into;
use style::properties::{
    style_structs, ComputedValues, PropertyDeclaration, PropertyId, SourcePropertyDeclaration,
};
use style::properties_and_values::rule::Descriptors as PropertyDescriptors;
use style::properties_and_values::value::ComputedValue as ComputedRegisteredValue;
use style::queries::condition::StyleQuerySubject;
use style::queries::if_condition::IfCondition;
use style::queries::values::PrefersColorScheme;
use style::servo::media_features::PointerCapabilities;
use style::stylesheets::container_rule::ContainerSizeQuery;
use style::stylesheets::{CssRuleType, Origin, UrlExtraData};
use style::stylist::Stylist;
use style::values::computed::font::GenericFontFamily;
use style::values::computed::{CSSPixelLength, Context};
use style::values::specified::font::QueryFontMetricsFlags;
use style::values::specified::{Integer, Length};
use style_traits::{CSSPixel, DevicePixel, ParsingMode};

fn url_data() -> UrlExtraData {
    UrlExtraData::from(::url::Url::parse("https://example.com/").unwrap())
}

fn declaration(name: &str, value: &str) -> Result<PropertyDeclaration, ()> {
    let id = PropertyId::parse_enabled_for_all_content(name).map_err(|_| ())?;
    let mut declarations = SourcePropertyDeclaration::default();
    parse_one_declaration_into(
        &mut declarations,
        id,
        value,
        Origin::Author,
        &url_data(),
        None,
        ParsingMode::DEFAULT,
        QuirksMode::NoQuirks,
        CssRuleType::Style,
    )
    .map_err(|_| ())?;
    Ok(declarations.declarations.remove(0))
}

/// Whether `value` parses for `name` as a value that substitutes later.
fn substitutes_later(name: &str, value: &str) -> bool {
    matches!(
        declaration(name, value),
        Ok(PropertyDeclaration::WithVariables(..) | PropertyDeclaration::Custom(..))
    )
}

fn variable_value(css: &str) -> VariableValue {
    VariableValue::parse(&mut CssParser::new(css), None, &url_data()).expect("a declaration value")
}

// ---------------------------------------------------------------------------
// The argument grammar, checked when the declaration is parsed.

#[test]
fn if_is_an_arbitrary_substitution_function() {
    for value in [
        "if(style(--x): red)",
        "if(style(--x): red;)",
        "if(style(--x): red; else: blue)",
        "if(style(--x): red; else: blue;)",
        "if( style( --x : 3 ) : red )",
        "if(style(--x):)",
        "if(style(--x): ;)",
        "if(else: red; style(--x): blue; else: green)",
        "if(media(width > 1px): red)",
        "if(supports(display: flex): red)",
        // Conditions are parsed only after substitution: anything with a
        // top-level `:` is a well-formed branch.
        "if(anything at all: red)",
        "if(var(--cond): red)",
        "if(style(--x): if(style(--y): a; else: b); else: c)",
        "if(if(else: else; else: x): red)",
        "var(--missing, if(else: red))",
        // Values may hold commas, colons and `{}` blocks.
        "if(else: rgb(0, 128, 0))",
        "if(else: a, b: c)",
        "if(else: {a})",
        // Unclosed at the end of the value, closed implicitly.
        "if(style(--x): red",
    ] {
        assert!(
            substitutes_later("color", value),
            "`color: {value}` must parse"
        );
        assert!(substitutes_later("--p", value), "`--p: {value}` must parse");
    }
}

#[test]
fn if_arguments_that_break_the_argument_grammar_do_not_parse() {
    for value in [
        "if()",
        "if( )",
        "if(;)",
        "if(style(--x))",
        "if(style(--x) red)",
        "if(: red)",
        "if(  : red)",
        "if(/* */: red)",
        "if(style(--x): red;;)",
        "if(style(--x): red; ;)",
        "if(style(--x): red; else)",
        "if(style(--x): red!)",
        "if(style(--x): red !important)",
        "if(!style(--x): red)",
        "if(style(--x) !: red)",
    ] {
        // A custom property takes any token stream, but not a broken if().
        assert!(
            declaration("color", value).is_err(),
            "`color: {value}` must not parse"
        );
        assert!(
            declaration("--p", value).is_err(),
            "`--p: {value}` must not parse"
        );
    }
}

#[test]
fn an_if_reference_records_its_branches() {
    let value = variable_value("a if(style(--x): b; media(width > 1px): c var(--d); else:) e");
    let refs = &value.references.refs;
    assert_eq!(refs.len(), 1, "nested references belong to their branch");
    let reference = &refs[0];
    assert_eq!(reference.substitution_kind, SubstitutionFunctionKind::If);
    assert_eq!(reference.if_branches.len(), 3);
    let value_references = &reference.if_branches[1].value.references.refs;
    assert_eq!(value_references.len(), 1);
    assert_eq!(
        value_references[0].substitution_kind,
        SubstitutionFunctionKind::Var
    );
    assert_eq!(value_references[0].name, Name::from("d"));
    assert!(value.has_references());
}

#[test]
fn nested_references_raise_the_outer_flags() {
    use style::custom_properties::ReferenceFlags;
    let value = variable_value("if(else: attr(data-x))");
    assert!(value.references.flags.contains(ReferenceFlags::ATTR));
    // style() reads custom properties, which is what VAR stands for.
    assert!(value.references.flags.contains(ReferenceFlags::VAR));
    assert!(value.is_attr_tainted());
}

#[test]
fn if_is_case_insensitive_like_other_functions() {
    let value = variable_value("IF(else: a)");
    assert_eq!(
        value.references.refs[0].substitution_kind,
        SubstitutionFunctionKind::If
    );
}

// ---------------------------------------------------------------------------
// The `<if-condition>` grammar, applied to substituted text.

fn condition(css: &str) -> Option<IfCondition> {
    IfCondition::parse(css, &url_data(), QuirksMode::NoQuirks)
}

#[test]
fn if_conditions_parse() {
    for css in [
        "else",
        "ELSE",
        "style(--x)",
        "style(--x: 3)",
        "style(--x: 3 !important)",
        "style(not (--x))",
        "style((--x) and (--y: 1))",
        "style((--x) or ((--y) and (--z)))",
        "style(--x > 3)",
        "style(1 < --x <= 5)",
        "style(sibling-index() = 2)",
        "media(width > 1px)",
        "media(color)",
        "media((width > 1px) and (height < 1px))",
        "supports(display: flex)",
        "supports((display: flex) or (display: grid))",
        "supports(selector(a > b))",
        "not style(--x)",
        "style(--x) and media(width > 1px) and supports(display: flex)",
        "style(--x) or (media(width > 1px) and supports(display: flex))",
        // <general-enclosed> parses, as unknown.
        "unknown(1)",
        "(unknown)",
        "style(style(--x))",
        "style(color: red)",
        "style()",
        "supports(display)",
        "media(screen)",
    ] {
        assert!(
            condition(css).is_some(),
            "`{css}` must parse as an <if-condition>"
        );
    }
}

#[test]
fn if_conditions_that_do_not_parse() {
    for css in [
        "",
        "else else",
        "else and style(--x)",
        "style(--x) and else",
        "invalid",
        "style(--x) and invalid",
        "style(--x) and style(--y) or style(--z)",
        "style(--x) and not style(--y)",
        "not not style(--x)",
        "style(--x) style(--y)",
        "3",
    ] {
        assert!(condition(css).is_none(), "`{css}` must not parse");
    }
}

#[test]
fn if_condition_reports_the_properties_it_queries() {
    let parsed =
        condition("style(--a) or (style((--b: 1) and (3 < --c)) and media(width > 1px))").unwrap();
    let mut queried = Vec::new();
    parsed.for_each_queried_property(&mut |name| queried.push(name.to_string()));
    assert_eq!(queried, ["a", "b", "c"]);
}

// ---------------------------------------------------------------------------
// Evaluation that needs no element.

#[derive(Debug)]
struct TestFontMetricsProvider;

impl FontMetricsProvider for TestFontMetricsProvider {
    fn query_font_metrics(
        &self,
        _vertical: bool,
        _font: &style_structs::Font,
        _base_size: CSSPixelLength,
        _flags: QueryFontMetricsFlags,
    ) -> FontMetrics {
        FontMetrics::default()
    }

    fn base_size_for_generic(&self, _: GenericFontFamily) -> style::values::computed::Length {
        style::values::computed::Length::new(16.0)
    }
}

fn device() -> Device {
    Device::new(
        MediaType::screen(),
        QuirksMode::NoQuirks,
        Size2D::<f32, CSSPixel>::new(800.0, 600.0),
        Size2D::<f32, DevicePixel>::new(800.0, 600.0),
        Scale::<f32, CSSPixel, DevicePixel>::new(1.0),
        Box::new(TestFontMetricsProvider),
        ComputedValues::initial_values_with_font_override(style_structs::Font::initial_values()),
        PrefersColorScheme::Light,
        PointerCapabilities::default(),
        PointerCapabilities::default(),
    )
}

/// A `style()` subject holding fixed, unregistered custom properties.
struct Fixed(CustomPropertiesMap);

impl Fixed {
    fn new(properties: &[(&str, &str)]) -> Self {
        let mut map = CustomPropertiesMap::default();
        for (name, value) in properties {
            map.insert(
                &Name::from(*name),
                ComputedRegisteredValue::universal(Arc::new(variable_value(value))),
            );
        }
        Self(map)
    }
}

impl StyleQuerySubject for Fixed {
    fn exists(&self, _: &Context) -> bool {
        true
    }

    fn custom_property<'a>(
        &'a self,
        _: &'a Context,
        _: &PropertyDescriptors,
        name: &Name,
    ) -> Option<&'a ComputedRegisteredValue> {
        self.0.get(name)
    }

    fn inherited_custom_property<'a>(
        &'a self,
        _: &'a Context,
        _: &PropertyDescriptors,
        _: &Name,
    ) -> Option<Option<&'a ComputedRegisteredValue>> {
        Some(None)
    }

    fn substitution_functions(&self, _: &Context) -> ComputedSubstitutionFunctions {
        ComputedSubstitutionFunctions::default()
    }
}

/// Evaluates `css` as an `<if-condition>` against `subject` on an 800 × 600
/// screen.
fn evaluate(css: &str, subject: &Fixed) -> bool {
    let parsed = condition(css).unwrap_or_else(|| panic!("`{css}` must parse"));
    let stylist = Stylist::new(device(), QuirksMode::NoQuirks);
    Context::for_container_query_evaluation(
        stylist.device(),
        Some(&stylist),
        None,
        ContainerSizeQuery::none(),
        &DummyElementContext,
        |context| {
            parsed.matches(
                context,
                subject,
                &url_data(),
                &mut AttributeTracker::new_dummy(),
            )
        },
    )
}

#[test]
fn media_and_supports_tests_evaluate_without_an_element() {
    let none = Fixed::new(&[]);
    for (css, expected) in [
        ("else", true),
        ("media(width > 700px)", true),
        ("media(width > 900px)", false),
        ("media(max-width: 1px)", false),
        ("media((orientation: landscape) and (height = 600px))", true),
        ("media(screen)", false),
        ("supports(display: flex)", true),
        ("supports(display: linear)", true),
        ("supports(display: table-cell)", false),
        ("supports(not (display: nope))", true),
        ("supports(selector(a > b))", true),
        ("supports(display)", false),
    ] {
        assert_eq!(evaluate(css, &none), expected, "`{css}`");
    }
}

#[test]
fn boolean_logic_is_three_valued() {
    let none = Fixed::new(&[]);
    for (css, expected) in [
        // unknown(…) is <general-enclosed>: unknown, and false at the top.
        ("unknown(1)", false),
        ("not unknown(1)", false),
        ("unknown(1) or media(width > 1px)", true),
        ("unknown(1) or media(width < 1px)", false),
        ("unknown(1) and media(width < 1px)", false),
        ("unknown(1) and media(width > 1px)", false),
        ("not (unknown(1) and media(width < 1px))", true),
        ("not (unknown(1) or media(width > 1px))", false),
        ("(media(width > 1px))", true),
        ("not media(width > 1px)", false),
    ] {
        assert_eq!(evaluate(css, &none), expected, "`{css}`");
    }
}

#[test]
fn style_tests_evaluate_against_the_subject() {
    let subject = Fixed::new(&[("x", "3"), ("y", "blue"), ("n", "5")]);
    for (css, expected) in [
        ("style(--x)", true),
        ("style(--missing)", false),
        ("style(--x: 3)", true),
        ("style(--x: 4)", false),
        ("style(--x: calc(1 + 2))", false),
        ("style(--y: blue)", true),
        ("style(not (--y: red))", true),
        ("style((--x: 3) and (--y: blue))", true),
        ("style((--x: 4) or (--y: blue))", true),
        ("style(--missing: initial)", true),
        ("style(--x: initial)", false),
        ("style(--x: revert)", false),
        ("style(--n > 4)", true),
        ("style(4 < --n <= 5)", true),
        ("style(--n = --x)", false),
        ("style(10px > 3px)", true),
        ("style(3px > 3)", false),
        ("style(color: red)", false),
    ] {
        assert_eq!(evaluate(css, &subject), expected, "`{css}`");
    }
}

// ---------------------------------------------------------------------------
// Tree-counting functions.

fn parse<T: Parse>(css: &str, parsing_mode: ParsingMode) -> Result<T, ()> {
    let url_data = url_data();
    let context = ParserContext::new(
        Origin::Author,
        &url_data,
        Some(CssRuleType::Style),
        parsing_mode,
        QuirksMode::NoQuirks,
        Default::default(),
        None,
        None,
        AttrTaint::default(),
    );
    CssParser::new(css)
        .parse_entirely(|input| T::parse(&context, input))
        .map_err(|_| ())
}

#[test]
fn tree_counting_functions_are_enabled() {
    for css in [
        "sibling-index()",
        "sibling-count()",
        "calc(sibling-index() - 1)",
    ] {
        assert!(
            parse::<Integer>(css, ParsingMode::DEFAULT).is_ok(),
            "<integer> `{css}`"
        );
    }
    for css in [
        "calc(10px * sibling-index())",
        "calc(1px * sibling-count())",
    ] {
        assert!(
            parse::<Length>(css, ParsingMode::DEFAULT).is_ok(),
            "<length> `{css}`"
        );
    }
    for (name, value) in [
        ("width", "calc(10px * sibling-index())"),
        ("opacity", "calc(1 / sibling-count())"),
        ("z-index", "sibling-index()"),
    ] {
        assert!(declaration(name, value).is_ok(), "`{name}: {value}`");
    }
    // Arguments are not accepted.
    assert!(parse::<Integer>("sibling-index(1)", ParsingMode::DEFAULT).is_err());
}

#[test]
fn tree_counting_functions_stay_out_of_media_queries() {
    assert!(parse::<Length>(
        "calc(10px * sibling-index())",
        ParsingMode::MEDIA_QUERY_CONDITION
    )
    .is_err());
}
