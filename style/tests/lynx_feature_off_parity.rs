//! Guards the upstream Servo author grammar when the `lynx` feature is off.
//!
//! Lynx's property/type trimming is compile-time code generation. These
//! representative assertions make accidental leakage into a normal Stylo
//! build visible immediately.
#![cfg(not(feature = "lynx"))]

use style::context::QuirksMode;
use style::properties::declaration_block::parse_one_declaration_into;
use style::properties::{
    longhands, style_structs, ComputedValues, PropertyId, SourcePropertyDeclaration,
};
use style::stylesheets::{CssRuleType, Origin, UrlExtraData};
use style::values::specified::box_::Display;
use style_traits::ParsingMode;

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
        ("width", "1rpx"),
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
