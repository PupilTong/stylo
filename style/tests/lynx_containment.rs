// CSS containment (css-contain-2) is a deliberate lynx-vello extension beyond
// Lynx's own property set: `contain`, `content-visibility`, and the
// `contain-intrinsic-size` shorthand / `contain-intrinsic-width|height`
// longhands are force-enabled under the `lynx` feature (see the containment note
// in properties/lynx_properties.txt). This suite covers the parts NOT already
// exercised elsewhere: `content-visibility` / `contain-intrinsic-size` value
// grammar, the computed-value accessors the downstream stylo-dom helper depends
// on, `contain`-change restyle damage, and the negative gating that keeps the
// logical contain-intrinsic pair disabled-for-content. It also covers the
// css-contain-3 container-query *properties* (`container-type`,
// `container-name`, and the `container` shorthand), which are exposed so that
// `cqw`/`cqh` are a standard implementation -- an element with
// `container-type: size | inline-size` really is a size query container. The
// `@container` *rule* stays gecko-only and out of scope. (`contain`'s
// structural bit layout lives in lynx_containment_bits.rs; its value grammar
// and content-enablement live in lynx_value_grammar.rs /
// lynx_supported_properties.rs.)
#![cfg(feature = "lynx")]

use cssparser::Parser as CssParser;
use style::context::QuirksMode;
use style::custom_properties::AttrTaint;
use style::parser::{Parse, ParserContext};
use style::properties::declaration_block::{parse_one_declaration_into, parse_style_attribute};
use style::properties::{
    style_structs, ComputedValues, LonghandId, PropertyDeclarationBlock, PropertyDeclarationId,
    PropertyId, SourcePropertyDeclaration,
};
use style::servo::restyle_damage::ServoRestyleDamage;
use style::stylesheets::{CssRuleType, Origin, UrlExtraData};
use style::values::computed::ContainIntrinsicSize as ComputedContainIntrinsicSize;
use style::values::specified::box_::{
    Contain, ContainIntrinsicSize, ContainerName, ContainerType, ContentVisibility,
};
use style_traits::{CssWriter, ParsingMode, ToCss};

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
    let mut parser = CssParser::new(css);
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
// Negative gating: only the physical contain-intrinsic-* are authorable.
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

// ------------------------------------------------------------------------
// css-contain-3 container-query properties. They exist so that `cqw`/`cqh` are
// a standard implementation: without a content-enabled `container-type`, no
// element could ever be a size query container and the units would be
// permanent `vw`/`vh` aliases. Only the properties are in scope; the
// `@container` rule stays gecko-only (stylesheets/rule_parser.rs), so
// `container-name` is parsed and cascaded but nothing matches on it yet.
// ------------------------------------------------------------------------

/// Serialize a specified value through `ToCss`.
fn to_css<T: ToCss>(value: &T) -> String {
    let mut out = String::new();
    value
        .to_css(&mut CssWriter::new(&mut out))
        .expect("serialization must not fail");
    out
}

/// Serialize a declaration block the way CSSOM's `cssText` does (shorthands
/// restored where every longhand is present).
fn block_to_css(block: &PropertyDeclarationBlock) -> String {
    let mut out = String::new();
    block.to_css(&mut out).expect("serialization must not fail");
    out
}

#[test]
fn container_query_properties_are_content_enabled() {
    for name in ["container-type", "container-name", "container"] {
        assert!(
            PropertyId::parse_enabled_for_all_content(name).is_ok(),
            "`{name}` must be content-enabled under the `lynx` feature so that \
             an element can be a size query container for `cqw`/`cqh`",
        );
    }
}

#[test]
fn container_type_keywords_parse_and_serialize() {
    assert_eq!(
        parse::<ContainerType>("normal").unwrap(),
        ContainerType::NORMAL
    );
    assert_eq!(parse::<ContainerType>("size").unwrap(), ContainerType::SIZE);
    assert_eq!(
        parse::<ContainerType>("inline-size").unwrap(),
        ContainerType::INLINE_SIZE,
    );

    assert_eq!(to_css(&ContainerType::NORMAL), "normal");
    assert_eq!(to_css(&ContainerType::SIZE), "size");
    assert_eq!(to_css(&ContainerType::INLINE_SIZE), "inline-size");

    // `size` and `inline-size` are mutually exclusive.
    assert!(parse::<ContainerType>("size inline-size").is_err());
    assert!(parse::<ContainerType>("block-size").is_err());
}

#[test]
fn container_type_scroll_state_stays_rejected() {
    // `scroll-state` is guarded by `layout.css.scroll-state.enabled`, which is
    // false and deliberately left alone: lynx-vello implements size query
    // containers only.
    for css in ["scroll-state", "size scroll-state", "scroll-state size"] {
        assert!(
            parse::<ContainerType>(css).is_err(),
            "`container-type: {css}` must stay rejected",
        );
    }
}

#[test]
fn container_type_parses_from_author_css() {
    assert_eq!(
        parse_declaration_longhand("container-type", "size"),
        Ok(LonghandId::ContainerType),
    );
    assert_eq!(
        parse_declaration_longhand("container-type", "inline-size"),
        Ok(LonghandId::ContainerType),
    );
}

#[test]
fn container_name_round_trips() {
    assert!(parse::<ContainerName>("none").unwrap().is_none());
    assert_eq!(to_css(&ContainerName::none()), "none");

    let single = parse::<ContainerName>("foo").unwrap();
    assert!(!single.is_none());
    assert_eq!(to_css(&single), "foo");

    let multiple = parse::<ContainerName>("foo bar").unwrap();
    assert_eq!(multiple.0.len(), 2);
    assert_eq!(to_css(&multiple), "foo bar");

    // `none`/`not`/`or`/`and` are disallowed idents in a name list.
    assert!(parse::<ContainerName>("foo none").is_err());
    assert!(parse::<ContainerName>("and").is_err());

    assert_eq!(
        parse_declaration_longhand("container-name", "foo bar"),
        Ok(LonghandId::ContainerName),
    );
}

#[test]
fn container_shorthand_expands_and_serializes() {
    let url_data = url_data();
    let block = parse_style_attribute(
        "container: foo / size;",
        &url_data,
        None,
        QuirksMode::NoQuirks,
        CssRuleType::Style,
    );
    let ids: Vec<_> = block.declarations().iter().map(|d| d.id()).collect();
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&PropertyDeclarationId::Longhand(LonghandId::ContainerName)));
    assert!(ids.contains(&PropertyDeclarationId::Longhand(LonghandId::ContainerType)));
    // Both longhands present and non-initial-only => the shorthand is restored.
    assert_eq!(block_to_css(&block), "container: foo / size;");
}

#[test]
fn container_shorthand_is_name_first() {
    // The hand-written parser is `<container-name> [ / <container-type> ]?`,
    // deliberately NOT the spec grammar, because the two sides are
    // ambiguous when the type keyword is omitted: see
    // https://github.com/w3c/csswg-drafts/issues/7180. So a bare
    // `container: size` sets container-name to the custom ident `size` and
    // leaves container-type at `normal`; it does NOT make the element a size
    // query container. Authors who want that must write `container: / size`
    // ... which the name-first parser rejects, so `container-type: size` is
    // the only way to spell it.
    let url_data = url_data();
    let block = parse_style_attribute(
        "container: size;",
        &url_data,
        None,
        QuirksMode::NoQuirks,
        CssRuleType::Style,
    );
    assert_eq!(block.len(), 2);
    assert_eq!(block_to_css(&block), "container: size;");

    let empty_name = parse_style_attribute(
        "container: / size;",
        &url_data,
        None,
        QuirksMode::NoQuirks,
        CssRuleType::Style,
    );
    assert_eq!(empty_name.len(), 0);

    // `none` is a valid container-name, so `container: none` resets both.
    let none = parse_style_attribute(
        "container: none;",
        &url_data,
        None,
        QuirksMode::NoQuirks,
        CssRuleType::Style,
    );
    assert_eq!(none.len(), 2);
    assert_eq!(block_to_css(&none), "container: none;");
}

#[test]
fn container_type_is_a_size_container_type() {
    // The accessor `style_adjuster` / `matching` use to set
    // SELF_OR_ANCESTOR_HAS_SIZE_CONTAINER_TYPE, which is what makes `cqw`/`cqh`
    // resolve against this element instead of the small viewport.
    assert!(ContainerType::SIZE.is_size_container_type());
    assert!(ContainerType::INLINE_SIZE.is_size_container_type());
    assert!(!ContainerType::NORMAL.is_size_container_type());
    assert!(ContainerType::NORMAL.is_normal());
}

#[test]
fn container_computed_accessors_round_trip() {
    let initial =
        ComputedValues::initial_values_with_font_override(style_structs::Font::initial_values());
    assert_eq!(initial.clone_container_type(), ContainerType::NORMAL);
    assert!(initial.clone_container_name().is_none());

    let mut values: ComputedValues = (*initial).clone();
    values.mutate_box().set_container_type(ContainerType::SIZE);
    assert_eq!(values.clone_container_type(), ContainerType::SIZE);
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
