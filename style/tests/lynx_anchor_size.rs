//! css-anchor-position-1's `anchor-name` (§2.1) and `anchor-size()` (§5.1)
//! under the `lynx` feature: lynx-vello's anchor-positioning subset.
//!
//! The rest of the module stays out: `anchor()` (§3.2), `anchor-scope`,
//! `position-anchor`, `position-area`, `position-try-*`,
//! `position-visibility` and `@position-try`. Resolving an `anchor-size()`
//! (§5.1.1: only on an absolutely positioned box, otherwise the fallback or
//! invalid at computed-value time) is the layout engine's job and is covered
//! by the outer repository's `crates/dom/tests/anchor_size.rs`.
#![cfg(feature = "lynx")]

use euclid::{Scale, Size2D};
use servo_arc::Arc;
use style::context::QuirksMode;
use style::device::servo::FontMetricsProvider;
use style::device::Device;
use style::dom::DummyElementContext;
use style::font_metrics::FontMetrics;
use style::media_queries::{MediaList, MediaType};
use style::properties::declaration_block::{parse_one_declaration_into, parse_style_attribute};
use style::properties::{
    style_structs, ComputedValues, LonghandId, PropertyDeclaration, PropertyId,
    SourcePropertyDeclaration,
};
use style::queries::values::PrefersColorScheme;
use style::servo::media_features::PointerCapabilities;
use style::shared_lock::SharedRwLock;
use style::stylesheets::container_rule::ContainerSizeQuery;
use style::stylesheets::{
    AllowImportRules, CssRuleType, Origin, Stylesheet, StylesheetInDocument, UrlExtraData,
};
use style::stylist::Stylist;
use style::values::computed::font::GenericFontFamily;
use style::values::computed::{CSSPixelLength, Context, ToComputedValue};
use style::values::specified::font::QueryFontMetricsFlags;
use style_traits::{CSSPixel, DevicePixel, ParsingMode, ToCss};

fn url_data() -> UrlExtraData {
    UrlExtraData::from(::url::Url::parse("https://example.com/").unwrap())
}

fn parse_declaration(name: &str, value: &str) -> Result<SourcePropertyDeclaration, ()> {
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
    Ok(declarations)
}

fn declaration(name: &str, value: &str) -> PropertyDeclaration {
    parse_declaration(name, value)
        .unwrap_or_else(|_| panic!("`{name}: {value}` must parse"))
        .declarations
        .remove(0)
}

fn assert_parses(name: &str, value: &str) {
    assert!(
        parse_declaration(name, value).is_ok(),
        "`{name}: {value}` must parse"
    );
}

fn assert_rejects(name: &str, value: &str) {
    assert!(
        parse_declaration(name, value).is_err(),
        "`{name}: {value}` must not parse"
    );
}

/// The specified value of `name` serialized from an inline style block.
fn serialized(name: &str, value: &str) -> String {
    let block = parse_style_attribute(
        &format!("{name}: {value}"),
        &url_data(),
        None,
        QuirksMode::NoQuirks,
        CssRuleType::Style,
    );
    let mut out = String::new();
    block
        .property_value_to_css(
            &PropertyId::parse_enabled_for_all_content(name).unwrap(),
            &mut out,
        )
        .unwrap();
    out
}

fn assert_round_trips(name: &str, value: &str) {
    assert_eq!(serialized(name, value), value, "`{name}: {value}`");
}

// ---------------------------------------------------------------------------
// `anchor-name`

#[test]
fn anchor_name_is_an_author_property() {
    for value in [
        "none",
        "--a",
        "--a, --b",
        "--lynx-scroll-coordinator-header",
    ] {
        let ids: Vec<_> = parse_declaration("anchor-name", value)
            .unwrap_or_else(|_| panic!("`anchor-name: {value}` must parse"))
            .declarations
            .iter()
            .map(|d| d.id().as_longhand().unwrap())
            .collect();
        assert_eq!(ids, [LonghandId::AnchorName]);
        assert_round_trips("anchor-name", value);
    }
    // `none | <dashed-ident>#`
    assert_rejects("anchor-name", "a");
    assert_rejects("anchor-name", "--a --b");
    assert_rejects("anchor-name", "none, --a");
    assert_rejects("anchor-name", "--a, none");
    assert_rejects("anchor-name", "10px");
}

#[test]
fn anchor_name_computes_to_its_idents() {
    let PropertyDeclaration::AnchorName(ref specified) = declaration("anchor-name", "--a, --b")
    else {
        panic!("anchor-name")
    };
    let computed = with_context(|context| specified.to_computed_value(context));
    assert_eq!(computed.value.to_css_string(), "--a, --b");
    assert_eq!(computed.value.0.len(), 2);
}

// ---------------------------------------------------------------------------
// `anchor-size()` in the properties §5.1 lists.

/// Every author-facing property the spec admits `anchor-size()` in (sizes,
/// min/max sizes, insets, margins; the block-logical ones are not part of the
/// Lynx surface).
const ANCHOR_SIZE_PROPERTIES: &[&str] = &[
    "width",
    "height",
    "min-width",
    "min-height",
    "max-width",
    "max-height",
    "top",
    "right",
    "bottom",
    "left",
    "inset-inline-start",
    "inset-inline-end",
    "margin-top",
    "margin-right",
    "margin-bottom",
    "margin-left",
    "margin-inline-start",
    "margin-inline-end",
];

#[test]
fn anchor_size_parses_and_round_trips_in_every_listed_property() {
    for &name in ANCHOR_SIZE_PROPERTIES {
        for value in [
            "anchor-size(--a height, 10px)",
            "anchor-size(height)",
            "anchor-size(--a)",
            "anchor-size()",
            "anchor-size(--a width)",
            "anchor-size(--a block)",
            "anchor-size(inline)",
            "anchor-size(self-block, 5%)",
            "anchor-size(--a self-inline)",
            "anchor-size(--a height, calc(5% + 10px))",
            "calc(100% - anchor-size(--a height, 0px))",
            "calc(0.5 * anchor-size(--a width))",
            "max(10px, anchor-size(height))",
            "calc(2 * anchor-size(--a height, anchor-size(--b height)))",
        ] {
            assert_round_trips(name, value);
        }
        // Math functions serialize in their simplified form.
        assert_eq!(
            serialized(name, "calc(anchor-size(--a width) / 2)"),
            "calc(0.5 * anchor-size(--a width))",
            "{name}"
        );
        // `[ <anchor-name> || <anchor-size> ]` in either order.
        assert_eq!(
            serialized(name, "anchor-size(height --a, 10px)"),
            "anchor-size(--a height, 10px)",
            "{name}"
        );
        // A lone fallback needs no comma.
        assert_eq!(serialized(name, "anchor-size(10px)"), "anchor-size(10px)");
    }
}

#[test]
fn anchor_size_keeps_the_spec_grammar() {
    for &name in ANCHOR_SIZE_PROPERTIES {
        for value in [
            // Not an `<anchor-size>` keyword.
            "anchor-size(--a top)",
            "anchor-size(--a size)",
            // Two names or two sizes.
            "anchor-size(--a --b)",
            "anchor-size(width height)",
            // Non-dashed anchor name.
            "anchor-size(a height)",
            // The fallback follows a comma after a name or size.
            "anchor-size(--a height 10px)",
            "anchor-size(--a, 10px, 20px)",
            // The fallback is a `<length-percentage>`: not the property's own
            // keywords, and not `anchor()`.
            "anchor-size(--a, auto)",
            "anchor-size(--a, none)",
            "anchor-size(--a, min-content)",
            "anchor-size(--a, anchor(--b top))",
        ] {
            assert_rejects(name, value);
        }
    }
}

#[test]
fn anchor_size_reaches_the_margin_and_inset_shorthands() {
    assert_parses("margin", "anchor-size(--a height) 0");
    assert_parses("margin", "calc(anchor-size(width) / 2) auto");
    assert_rejects("padding", "anchor-size(--a height)");
    assert_parses("inset", "anchor-size(--a height) auto auto 0");
    assert_rejects("inset", "anchor(--a top) 0");
}

#[test]
fn anchor_size_computes_on_the_servo_build() {
    // The computed value keeps the function: resolution needs the anchor's
    // layout, which the engine owns.
    for (name, value, expected) in [
        (
            "height",
            "anchor-size(--a height, 10px)",
            "anchor-size(--a height, 10px)",
        ),
        (
            "height",
            "calc(100% - anchor-size(--a height, 0px))",
            "calc(100% - anchor-size(--a height, 0px))",
        ),
        (
            "max-height",
            "anchor-size(--a height)",
            "anchor-size(--a height)",
        ),
        (
            "max-width",
            "calc(anchor-size(width) + 1px)",
            "calc(anchor-size(width) + 1px)",
        ),
        (
            "min-width",
            "anchor-size(--a self-inline, 5px)",
            "anchor-size(--a self-inline, 5px)",
        ),
        (
            "top",
            "anchor-size(--a height, 0px)",
            "anchor-size(--a height, 0px)",
        ),
        (
            "left",
            "calc(anchor-size(--a width) * 2)",
            "calc(2 * anchor-size(--a width))",
        ),
        (
            "margin-top",
            "anchor-size(--a height)",
            "anchor-size(--a height)",
        ),
        (
            "margin-left",
            "calc(anchor-size(--a width, 1px) - 1px)",
            "calc(anchor-size(--a width, 1px) - 1px)",
        ),
    ] {
        let declaration = declaration(name, value);
        let computed = with_context(|context| match declaration {
            PropertyDeclaration::Height(ref v)
            | PropertyDeclaration::MinWidth(ref v)
            | PropertyDeclaration::Width(ref v) => v.to_computed_value(context).to_css_string(),
            PropertyDeclaration::MaxHeight(ref v) | PropertyDeclaration::MaxWidth(ref v) => {
                v.to_computed_value(context).to_css_string()
            },
            PropertyDeclaration::Top(ref v) | PropertyDeclaration::Left(ref v) => {
                v.to_computed_value(context).to_css_string()
            },
            PropertyDeclaration::MarginTop(ref v) | PropertyDeclaration::MarginLeft(ref v) => {
                v.to_computed_value(context).to_css_string()
            },
            ref other => panic!("unexpected declaration {other:?}"),
        });
        assert_eq!(computed, expected, "`{name}: {value}`");
    }
}

#[test]
fn anchor_size_substitutes_through_var() {
    // `var()` defers the check to computed-value time; the same grammar
    // applies after substitution.
    assert!(matches!(
        declaration("top", "var(--offset, anchor-size(--a height))"),
        PropertyDeclaration::WithVariables(..)
    ));
    assert!(matches!(
        declaration("--offset", "anchor-size(--a height)"),
        PropertyDeclaration::Custom(..)
    ));
}

// ---------------------------------------------------------------------------
// What stays out.

#[test]
fn anchor_function_does_not_parse() {
    for name in ["top", "right", "bottom", "left", "inset-inline-start"] {
        for value in [
            "anchor(top)",
            "anchor(--a top)",
            "anchor(--a bottom, 10px)",
            "calc(anchor(--a top) + 10px)",
            "anchor-size(--a height, anchor(--b top))",
            "calc(anchor-size(--a height, anchor(--b top)))",
        ] {
            assert_rejects(name, value);
        }
    }
    assert_rejects("width", "anchor(--a width)");
    assert_rejects("margin-top", "anchor(--a top)");
    // A top declaration with `anchor()` is dropped from the block, so the
    // earlier declaration wins.
    assert_eq!(serialized("top", "anchor(--a top)"), "");
}

#[test]
fn anchor_size_stays_out_of_other_properties() {
    for (name, value) in [
        ("padding-top", "anchor-size(--a height)"),
        ("padding-top", "calc(anchor-size(--a height) + 1px)"),
        ("font-size", "anchor-size(--a height)"),
        ("font-size", "calc(anchor-size(--a height))"),
        ("flex-basis", "anchor-size(--a width)"),
        ("border-top-width", "anchor-size(--a height)"),
        ("line-height", "anchor-size(--a height)"),
        ("translate", "anchor-size(--a width)"),
        ("transform", "translateX(anchor-size(--a width))"),
        ("border-top-left-radius", "anchor-size(--a height)"),
        ("gap", "anchor-size(--a height)"),
    ] {
        assert_rejects(name, value);
    }
}

#[test]
fn the_rest_of_anchor_positioning_is_unknown() {
    for name in [
        "anchor-scope",
        "position-anchor",
        "position-area",
        "position-try",
        "position-try-fallbacks",
        "position-try-order",
        "position-visibility",
        "inset-area",
    ] {
        assert!(
            PropertyId::parse_enabled_for_all_content(name).is_err(),
            "`{name}` must not be an author property"
        );
    }
    // `anchor-center` is alignment's anchor keyword.
    assert_rejects("align-self", "anchor-center");
    assert_rejects("justify-self", "anchor-center");
}

#[test]
fn position_try_rule_is_not_parsed() {
    let lock = SharedRwLock::new();
    let sheet = Stylesheet::from_str(
        "@position-try --flip { top: 0 } a { color: red }",
        url_data(),
        Origin::Author,
        Arc::new(lock.wrap(MediaList::empty())),
        lock.clone(),
        None,
        None,
        QuirksMode::NoQuirks,
        AllowImportRules::Yes,
    );
    let guard = lock.read();
    assert_eq!(sheet.contents(&guard).rules(&guard).len(), 1);
}

// ---------------------------------------------------------------------------
// A computed-value context on an 800 × 600 screen.

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

fn with_context<R>(f: impl FnOnce(&Context) -> R) -> R {
    let stylist = Stylist::new(device(), QuirksMode::NoQuirks);
    Context::for_container_query_evaluation(
        stylist.device(),
        Some(&stylist),
        None,
        ContainerSizeQuery::none(),
        &DummyElementContext,
        f,
    )
}
