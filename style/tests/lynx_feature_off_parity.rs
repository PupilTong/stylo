//! Guards the upstream Servo author grammar when the `lynx` feature is off.
//!
//! Lynx's property/type trimming is compile-time code generation. These
//! representative assertions make accidental leakage into a normal Stylo
//! build visible immediately.
#![cfg(not(feature = "lynx"))]

use style::context::QuirksMode;
use style::properties::declaration_block::parse_one_declaration_into;
use style::properties::{PropertyId, SourcePropertyDeclaration};
use style::stylesheets::{CssRuleType, Origin, UrlExtraData};
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
        ("width", "1rpx"),
    ] {
        assert!(
            !parses(name, value),
            "Lynx-only `{name}: {value}` leaked upstream"
        );
    }
}
