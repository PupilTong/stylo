//! Guards the upstream Servo author grammar when the `lynx` feature is off.
//!
//! Lynx's property/type trimming is compile-time code generation. These
//! representative assertions make accidental leakage into a normal Stylo
//! build visible immediately.
#![cfg(not(feature = "lynx"))]

use style::context::QuirksMode;
use style::properties::declaration_block::{parse_one_declaration_into, parse_style_attribute};
use style::properties::{
    longhands, style_structs, ComputedValues, PropertyId, SourcePropertyDeclaration,
};
use style::stylesheets::{CssRuleType, Origin, UrlExtraData};
use style::values::specified::box_::Display;
use style::values::specified::AnimationDuration;
use style_traits::{ParsingMode, ToCss};

fn url_data() -> UrlExtraData {
    UrlExtraData::from(::url::Url::parse("https://example.com/").unwrap())
}

fn parses(name: &str, value: &str) -> bool {
    let Ok(id) = PropertyId::parse_enabled_for_all_content(name) else {
        return false;
    };
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
    .is_ok()
}

#[test]
fn upstream_display_initial_value_remains_inline() {
    assert_eq!(Display::initial(), Display::Inline);
    assert_eq!(longhands::display::get_initial_value(), Display::Inline);
    assert_eq!(
        longhands::display::get_initial_specified_value(),
        Display::Inline
    );

    let values =
        ComputedValues::initial_values_with_font_override(style_structs::Font::initial_values());
    assert_eq!(values.clone_display(), Display::Inline);
    assert_eq!(values.get_box().original_display, Display::Inline);
}

#[test]
fn standard_property_and_value_grammar_is_unchanged() {
    for (name, value) in [
        ("all", "initial"),
        ("display", "block"),
        ("display", "inline-flex"),
        ("overflow", "scroll"),
        ("overflow-x", "auto"),
        ("overflow-y", "clip"),
        ("position", "static"),
        ("visibility", "collapse"),
        ("white-space", "pre-wrap"),
        ("font-size", "medium"),
        ("font-weight", "bolder"),
        ("width", "1cm"),
        ("transform", "rotate(1grad)"),
        ("color", "currentcolor"),
        ("display", "inherit"),
    ] {
        assert!(parses(name, value), "upstream `{name}: {value}` must parse");
    }
}

#[test]
fn lynx_only_names_and_values_do_not_exist() {
    for (name, value) in [
        ("linear-weight", "1"),
        ("linear-direction", "row"),
        ("relative-id", "1"),
        ("relative-center", "both"),
        ("display", "linear"),
        ("display", "relative"),
        ("display", "-lynx-text"),
        ("display", "grid-lanes"),
        ("flow-tolerance", "0"),
        ("width", "1rpx"),
        ("overscroll-behavior-x", "contain-bounce"),
    ] {
        assert!(
            !parses(name, value),
            "Lynx-only `{name}: {value}` leaked upstream"
        );
    }
}

#[test]
fn ported_containment_surface_stays_pref_gated() {
    // The css-contain-2 family is ported to servo for the `lynx` feature's
    // benefit, but a stock Servo build keeps it behind the experimental
    // `layout.unimplemented` pref (the -webkit-text-stroke*/offset-distance
    // pattern): none of it may reach the author surface — or `@supports` —
    // without the pref.
    for (name, value) in [
        ("contain", "strict"),
        ("content-visibility", "hidden"),
        ("contain-intrinsic-size", "auto 100px"),
        ("contain-intrinsic-width", "50px"),
        ("contain-intrinsic-height", "none"),
    ] {
        assert!(
            !parses(name, value),
            "stock servo must keep `{name}: {value}` pref-gated"
        );
    }
}

#[test]
fn container_query_properties_stay_pref_gated() {
    // `container-type` / `container-name` are upstream servo longhands behind
    // `layout.container-queries.enabled` (false), and the `container`
    // shorthand is un-gecko'd for the `lynx` feature's benefit while carrying
    // that same pref. A stock Servo build must still refuse all three, and
    // `PropertyId` must not resolve them for `@supports` either.
    for (name, value) in [
        ("container-type", "size"),
        ("container-type", "inline-size"),
        ("container-type", "normal"),
        ("container-name", "foo"),
        ("container-name", "none"),
        ("container", "foo / size"),
    ] {
        assert!(
            !parses(name, value),
            "stock servo must keep `{name}: {value}` pref-gated"
        );
    }
    for name in ["container", "container-name", "container-type"] {
        assert!(
            PropertyId::parse_enabled_for_all_content(name).is_err(),
            "stock servo must not expose `{name}` to the author surface"
        );
    }
}

#[test]
fn backdrop_filter_stays_pref_gated() {
    // `backdrop-filter` is exposed to author CSS only under the `lynx`
    // feature. Upstream it carries `servo_pref = "layout.unimplemented"`, and
    // that pref is untouched, so a stock Servo build must still refuse it.
    for value in ["none", "blur(4px)", "blur(4px) brightness(0.5)"] {
        assert!(
            !parses("backdrop-filter", value),
            "stock servo must keep `backdrop-filter: {value}` pref-gated"
        );
    }
    // `filter` is not pref-gated upstream and must keep parsing.
    assert!(parses("filter", "blur(4px) sepia(1)"));
}

#[test]
fn background_clip_text_stays_pref_gated() {
    // `background-clip: text` is un-gated from gecko for the `lynx`
    // feature's benefit (Lynx supports it as a Core value); a stock Servo
    // build keeps it behind gecko's backgrounds-4 pref name
    // (`layout.css.background-clip-text.enabled`), like `border-area`.
    assert!(
        !parses("background-clip", "text"),
        "stock servo must keep `background-clip: text` pref-gated"
    );
    assert!(
        !parses("mask-clip", "text"),
        "`text` is background-only in the shared clip grammar"
    );
}

#[test]
fn container_units_stay_gecko_only() {
    // `cqw`/`cqh` are admitted under the `lynx` feature, and the rest of the
    // container family only under gecko. A stock Servo build parses none of
    // them.
    for value in ["1cqw", "1cqh", "1cqi", "1cqb", "1cqmin", "1cqmax"] {
        assert!(
            !parses("width", value),
            "stock servo must not parse container unit `{value}`"
        );
    }
}

#[test]
fn scroll_timeline_surface_stays_pref_gated() {
    // scroll-animations-1's timeline declarations are ported to servo for the
    // `lynx` feature's benefit behind `layout.unimplemented`, and
    // `animation-duration: auto` stays behind gecko's scroll-driven-animations
    // pref (false). A stock Servo build refuses all of them.
    for (name, value) in [
        ("scroll-timeline", "--a x"),
        ("scroll-timeline-name", "--a"),
        ("scroll-timeline-axis", "inline"),
        ("view-timeline", "--v block 10% 20px"),
        ("view-timeline-name", "--v"),
        ("view-timeline-axis", "y"),
        ("view-timeline-inset", "auto 5px"),
        ("timeline-scope", "all"),
        ("timeline-scope", "--a, --b"),
        ("animation-range", "entry 10% exit 90%"),
        ("animation-timeline", "scroll()"),
        ("animation-duration", "auto"),
    ] {
        assert!(
            !parses(name, value),
            "stock servo must keep `{name}: {value}` pref-gated"
        );
    }
    assert!(parses("animation-duration", "1s"));
}

#[test]
fn animation_duration_auto_still_serializes_as_zero() {
    // The initial `auto` keeps serializing as `0s` in a stock Servo build,
    // both on its own and as the duration an `animation` shorthand leaves
    // unset.
    assert_eq!(AnimationDuration::auto().to_css_string(), "0s");

    let block = parse_style_attribute(
        "animation: fade",
        &url_data(),
        None,
        QuirksMode::NoQuirks,
        CssRuleType::Style,
    );
    let mut duration = String::new();
    block
        .property_value_to_css(
            &PropertyId::parse_enabled_for_all_content("animation-duration").unwrap(),
            &mut duration,
        )
        .unwrap();
    assert_eq!(duration, "0s");
}

#[test]
fn named_range_keyframe_selectors_stay_pref_gated() {
    // `<timeline-range-name> <percentage>` keyframe selectors are admitted
    // under the `lynx` feature and otherwise stay behind gecko's
    // scroll-driven-animations pref (false).
    let parses = |css: &str| {
        cssparser::Parser::new(css)
            .parse_entirely(style::stylesheets::keyframes_rule::KeyframeSelectors::parse)
            .is_ok()
    };
    assert!(!parses("entry 0%"));
    assert!(!parses("from, cover 50%"));
    assert!(parses("from, 50%, to"));
}

#[test]
fn if_is_not_a_substitution_function() {
    // css-values-5 `if()` is an arbitrary substitution function only under
    // the `lynx` feature. Otherwise it is an unknown function, which a
    // property that does not accept it rejects at parse time.
    assert!(!parses("color", "if(style(--x): red; else: blue)"));
    assert!(!parses("width", "if(else: 1px)"));
    assert!(style::properties::ARBITRARY_SUBSTITUTION_FUNCTIONS
        .iter()
        .all(|name| *name != "if"));
    // A custom property keeps it as tokens that reference nothing.
    let value = style::custom_properties::VariableValue::parse(
        &mut cssparser::Parser::new("if(style(--x): var(--y)) z"),
        None,
        &url_data(),
    )
    .unwrap();
    assert_eq!(
        value.references.refs.len(),
        1,
        "only the var() is a reference"
    );
}

#[test]
fn tree_counting_functions_stay_pref_gated() {
    // `sibling-index()` / `sibling-count()` are enabled under the `lynx`
    // feature and otherwise stay behind their pref (false).
    assert!(!parses("width", "calc(10px * sibling-index())"));
    assert!(!parses("z-index", "sibling-count()"));
    assert!(parses("width", "calc(10px * 2)"));
}

#[test]
fn anchor_positioning_stays_pref_gated() {
    // css-anchor-position-1 is ported to servo for the `lynx` feature's
    // benefit: its properties behind `layout.unimplemented`, `anchor()`,
    // `anchor-size()`, `anchor-center` and `@position-try` behind
    // `anchor_positioning_enabled()` (the lynx feature or the
    // anchor-positioning pref). A stock Servo build keeps all of it out
    // (both prefs false).
    for name in [
        "anchor-name",
        "anchor-scope",
        "position-anchor",
        "position-area",
        "position-try",
        "position-try-fallbacks",
        "position-try-order",
        "position-visibility",
    ] {
        assert!(
            PropertyId::parse_enabled_for_all_content(name).is_err(),
            "stock servo must keep `{name}` pref-gated"
        );
    }
    for (name, value) in [
        ("width", "anchor-size(--a width)"),
        ("height", "calc(100% - anchor-size(--a height, 0px))"),
        ("max-height", "anchor-size(height)"),
        ("min-width", "anchor-size(--a)"),
        ("top", "anchor-size(--a height, 0px)"),
        ("left", "calc(anchor-size(--a width) * 2)"),
        ("margin-top", "anchor-size(--a height)"),
        ("top", "anchor(--a top)"),
        ("top", "calc(anchor(--a top) + 1px)"),
        ("inset", "anchor(--a top) 0"),
        ("justify-self", "anchor-center"),
        ("align-self", "anchor-center"),
        ("place-self", "anchor-center"),
    ] {
        assert!(
            !parses(name, value),
            "stock servo must keep `{name}: {value}` pref-gated"
        );
    }
    assert!(parses("top", "calc(100% - 10px)"));
    assert!(parses("margin-top", "auto"));
    assert!(parses("max-height", "none"));
    assert!(parses("justify-self", "center"));

    // `@position-try` is an unknown at-rule.
    let lock = style::shared_lock::SharedRwLock::new();
    let sheet = style::stylesheets::Stylesheet::from_str(
        "@position-try --flip { top: 0 } a { color: red }",
        url_data(),
        Origin::Author,
        servo_arc::Arc::new(lock.wrap(style::media_queries::MediaList::empty())),
        lock.clone(),
        None,
        None,
        QuirksMode::NoQuirks,
        style::stylesheets::AllowImportRules::Yes,
    );
    let guard = lock.read();
    use style::stylesheets::StylesheetInDocument;
    assert_eq!(sheet.contents(&guard).rules(&guard).len(), 1);
}
