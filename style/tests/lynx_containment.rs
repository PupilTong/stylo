// CSS containment (css-contain-2) is a deliberate lynx-vello extension beyond
// Lynx's own property set: `contain`, `content-visibility`, and the
// `contain-intrinsic-size` shorthand / `contain-intrinsic-width|height`
// longhands are force-enabled under the `lynx` feature (see the containment note
// in properties/lynx_properties.txt). This suite covers the parts NOT already
// exercised elsewhere: `content-visibility` / `contain-intrinsic-size` value
// grammar, the computed-value accessors the downstream stylo-dom helper depends
// on, `contain`-change restyle damage, and the negative gating that keeps the
// logical contain-intrinsic pair and the css-contain-3 container-query surface
// disabled-for-content. (`contain`'s structural bit layout lives in
// lynx_containment_bits.rs; its value grammar and content-enablement live in
// lynx_value_grammar.rs / lynx_supported_properties.rs.)
#![cfg(feature = "lynx")]

use cssparser::{Parser as CssParser, ParserInput};
use style::context::QuirksMode;
use style::custom_properties::AttrTaint;
use style::parser::{Parse, ParserContext};
use style::properties::declaration_block::parse_one_declaration_into;
use style::properties::{
    style_structs, ComputedValues, LonghandId, PropertyDeclarationId, PropertyId,
    SourcePropertyDeclaration,
};
use style::servo::restyle_damage::ServoRestyleDamage;
use style::stylesheets::{CssRuleType, Origin, UrlExtraData};
use style::values::computed::ContainIntrinsicSize as ComputedContainIntrinsicSize;
use style::values::specified::box_::{Contain, ContainIntrinsicSize, ContentVisibility};
use style_traits::ParsingMode;

fn url_data() -> UrlExtraData {
    UrlExtraData::from(::url::Url::parse("https://example.com/").unwrap())
}

/// Parse a specified value `T` from `css` via its `Parse` impl.
fn parse<T: Parse>(css: &str) -> Result<T, ()> {
    let url_data = url_data();
    let context = ParserContext::new(
        Origin::Author,
        &url_data,
        None,
        ParsingMode::DEFAULT,
        QuirksMode::NoQuirks,
        Default::default(),
        None,
        None,
        AttrTaint::default(),
    );
    let mut input = ParserInput::new(css);
    let mut parser = CssParser::new(&mut input);
    parser
        .parse_entirely(|input| T::parse(&context, input))
        .map_err(|_| ())
}

/// Parse `name: value` through the real content path (the property must be
/// content-enabled under `lynx`, or this returns `Err`). Returns the single
/// resulting longhand id on success.
fn parse_declaration_longhand(name: &str, value: &str) -> Result<LonghandId, ()> {
    let url_data = url_data();
    let id = PropertyId::parse_enabled_for_all_content(name)?;
    let mut declarations = SourcePropertyDeclaration::default();
    parse_one_declaration_into(
        &mut declarations,
        id,
        value,
        Origin::Author,
        &url_data,
        None,
        ParsingMode::DEFAULT,
        QuirksMode::NoQuirks,
        CssRuleType::Style,
    )
    .map_err(|_| ())?;
    match declarations.declarations.first().map(|d| d.id()) {
        Some(PropertyDeclarationId::Longhand(id)) => Ok(id),
        _ => Err(()),
    }
}

// ------------------------------------------------------------------------
// Negative gating: only the physical contain-intrinsic-* are authorable, and
// css-contain-3 container queries stay out of scope.
// ------------------------------------------------------------------------

#[test]
fn logical_contain_intrinsic_longhands_stay_disabled() {
    for name in [
        "contain-intrinsic-block-size",
        "contain-intrinsic-inline-size",
    ] {
        assert!(
            PropertyId::parse_enabled_for_all_content(name).is_err(),
            "`{name}` is only un-gecko'd for logical-group balance and must stay \
             disabled-for-content under the `lynx` feature",
        );
    }
}

#[test]
fn container_query_properties_stay_disabled() {
    // `container-type` / `container-name` (and their `container` shorthand) are
    // css-contain-3 (container queries), explicitly OUT OF SCOPE for the
    // lynx-vello containment extension — see the containment note in
    // properties/lynx_properties.txt. Enabling css-contain-2 containment must not
    // leak the container-query surface, so these stay disabled-for-content under
    // `lynx`.
    for name in ["container-type", "container-name", "container"] {
        assert!(
            PropertyId::parse_enabled_for_all_content(name).is_err(),
            "`{name}` is css-contain-3 (out of scope) and must stay \
             disabled-for-content under the `lynx` feature",
        );
    }
}

// ------------------------------------------------------------------------
// `content-visibility`: parse + compute (identity computed value).
// ------------------------------------------------------------------------

#[test]
fn content_visibility_keywords_parse() {
    assert_eq!(
        parse::<ContentVisibility>("hidden").unwrap(),
        ContentVisibility::Hidden,
    );
    assert_eq!(
        parse::<ContentVisibility>("auto").unwrap(),
        ContentVisibility::Auto,
    );
    assert_eq!(
        parse::<ContentVisibility>("visible").unwrap(),
        ContentVisibility::Visible,
    );
    assert!(parse::<ContentVisibility>("collapse").is_err());
}

#[test]
fn content_visibility_hidden_parses_from_author_css() {
    assert_eq!(
        parse_declaration_longhand("content-visibility", "hidden"),
        Ok(LonghandId::ContentVisibility),
    );
}

// ------------------------------------------------------------------------
// `contain-intrinsic-size`: `none | <length> | auto <length>` (plus `auto none`).
// ------------------------------------------------------------------------

#[test]
fn contain_intrinsic_size_values_parse() {
    assert!(matches!(
        parse::<ContainIntrinsicSize>("none").unwrap(),
        ContainIntrinsicSize::None,
    ));
    assert!(matches!(
        parse::<ContainIntrinsicSize>("300px").unwrap(),
        ContainIntrinsicSize::Length(_),
    ));
    assert!(matches!(
        parse::<ContainIntrinsicSize>("auto 300px").unwrap(),
        ContainIntrinsicSize::AutoLength(_),
    ));
    assert!(matches!(
        parse::<ContainIntrinsicSize>("auto none").unwrap(),
        ContainIntrinsicSize::AutoNone,
    ));
    // Negative lengths are rejected (non-negative grammar).
    assert!(parse::<ContainIntrinsicSize>("-10px").is_err());
}

#[test]
fn contain_intrinsic_size_parses_from_author_css() {
    // The shorthand expands into the two physical longhands.
    assert_eq!(
        parse_declaration_longhand("contain-intrinsic-width", "auto 300px"),
        Ok(LonghandId::ContainIntrinsicWidth),
    );
    assert_eq!(
        parse_declaration_longhand("contain-intrinsic-height", "none"),
        Ok(LonghandId::ContainIntrinsicHeight),
    );

    let url_data = url_data();
    let id = PropertyId::parse_enabled_for_all_content("contain-intrinsic-size").unwrap();
    let mut declarations = SourcePropertyDeclaration::default();
    parse_one_declaration_into(
        &mut declarations,
        id,
        "auto 300px",
        Origin::Author,
        &url_data,
        None,
        ParsingMode::DEFAULT,
        QuirksMode::NoQuirks,
        CssRuleType::Style,
    )
    .expect("`contain-intrinsic-size: auto 300px` must parse");
    let ids: Vec<_> = declarations.declarations.iter().map(|d| d.id()).collect();
    assert!(ids.contains(&PropertyDeclarationId::Longhand(
        LonghandId::ContainIntrinsicWidth
    )));
    assert!(ids.contains(&PropertyDeclarationId::Longhand(
        LonghandId::ContainIntrinsicHeight
    )));
}

// ------------------------------------------------------------------------
// The computed-value accessors that the downstream stylo-dom containment helper
// (Wave 2) depends on exist and round-trip: clone_contain,
// clone_content_visibility, clone_contain_intrinsic_width/height.
// ------------------------------------------------------------------------

#[test]
fn computed_accessors_round_trip() {
    let initial =
        ComputedValues::initial_values_with_font_override(style_structs::Font::initial_values());

    // Initial values match the CSS-initial defaults.
    assert_eq!(initial.clone_contain(), Contain::empty());
    assert_eq!(
        initial.clone_content_visibility(),
        ContentVisibility::Visible
    );
    assert!(matches!(
        initial.clone_contain_intrinsic_width(),
        ComputedContainIntrinsicSize::None
    ));
    assert!(matches!(
        initial.clone_contain_intrinsic_height(),
        ComputedContainIntrinsicSize::None
    ));

    let mut values: ComputedValues = (*initial).clone();
    values.mutate_box().set_contain(Contain::STRICT);
    values
        .mutate_box()
        .set_content_visibility(ContentVisibility::Hidden);
    values
        .mutate_position()
        .set_contain_intrinsic_width(ComputedContainIntrinsicSize::AutoNone);
    values
        .mutate_position()
        .set_contain_intrinsic_height(ComputedContainIntrinsicSize::AutoNone);

    assert_eq!(values.clone_contain(), Contain::STRICT);
    assert_eq!(values.clone_content_visibility(), ContentVisibility::Hidden);
    assert!(matches!(
        values.clone_contain_intrinsic_width(),
        ComputedContainIntrinsicSize::AutoNone
    ));
    assert!(matches!(
        values.clone_contain_intrinsic_height(),
        ComputedContainIntrinsicSize::AutoNone
    ));
}

// ------------------------------------------------------------------------
// A `contain` change between two ComputedValues must produce RELAYOUT damage.
// `contain` carries an explicit `servo_restyle_damage = "rebuild_box"` (see
// longhands.toml); without it a containment change would produce no relayout
// signal for the downstream damage harvest (Wave 2). `compute_base_damage` is
// the servo damage classifier without the (default-empty) custom-layout hook.
// ------------------------------------------------------------------------

#[test]
fn contain_change_produces_relayout_damage() {
    let old =
        ComputedValues::initial_values_with_font_override(style_structs::Font::initial_values());

    let mut new: ComputedValues = (*old).clone();
    new.mutate_box().set_contain(Contain::STRICT);
    assert_ne!(old.clone_contain(), new.clone_contain());

    let damage = ServoRestyleDamage::compute_base_damage(&old, &new);
    assert!(
        damage.contains(ServoRestyleDamage::RELAYOUT),
        "a `contain` change must trip rebuild_box -> RELAYOUT damage, got {damage:?}",
    );

    // An unchanged style produces no damage (the harvest must stay quiet).
    let same: ComputedValues = (*old).clone();
    assert!(ServoRestyleDamage::compute_base_damage(&old, &same).is_empty());
}
