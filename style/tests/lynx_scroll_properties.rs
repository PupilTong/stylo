// The scrolling properties lynx-vello's scroll chain and snapping read:
// css-overscroll-1's `overscroll-behavior` and css-scroll-snap-1 (both W3C
// extensions beyond Lynx's own property index) and the project-only
// `scroll-capture`. All of them exist only under the `lynx` feature.
#![cfg(feature = "lynx")]

use style::context::QuirksMode;
use style::properties::declaration_block::parse_one_declaration_into;
use style::properties::{
    LonghandId, PropertyDeclarationId, PropertyId, SourcePropertyDeclaration,
};
use style::stylesheets::{CssRuleType, Origin, UrlExtraData};
use style_traits::ParsingMode;

fn url_data() -> UrlExtraData {
    UrlExtraData::from(::url::Url::parse("https://example.com/").unwrap())
}

fn parse_declaration(name: &str, value: &str) -> Result<SourcePropertyDeclaration, ()> {
    let url_data = url_data();
    let id = PropertyId::parse_enabled_for_all_content(name).map_err(|_| ())?;
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
    )?;
    Ok(declarations)
}

fn longhand_ids(name: &str, value: &str) -> Vec<LonghandId> {
    parse_declaration(name, value)
        .unwrap()
        .declarations
        .iter()
        .map(|declaration| match declaration.id() {
            PropertyDeclarationId::Longhand(id) => id,
            PropertyDeclarationId::Custom(_) => panic!("{name} is not a custom property"),
        })
        .collect()
}

fn assert_rejects(name: &str, value: &str) {
    assert!(
        parse_declaration(name, value).is_err(),
        "`{name}: {value}` must not parse"
    );
}

#[test]
fn overscroll_behavior_longhands_parse_the_three_keywords() {
    for value in ["auto", "contain", "none"] {
        assert_eq!(
            longhand_ids("overscroll-behavior-x", value),
            [LonghandId::OverscrollBehaviorX]
        );
        assert_eq!(
            longhand_ids("overscroll-behavior-y", value),
            [LonghandId::OverscrollBehaviorY]
        );
    }
    assert_rejects("overscroll-behavior-x", "scroll");
    assert_rejects("overscroll-behavior-y", "nearest");
}

#[test]
fn overscroll_behavior_shorthand_expands_to_the_physical_pair() {
    let ids = longhand_ids("overscroll-behavior", "contain");
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&LonghandId::OverscrollBehaviorX));
    assert!(ids.contains(&LonghandId::OverscrollBehaviorY));

    let ids = longhand_ids("overscroll-behavior", "auto none");
    assert_eq!(ids.len(), 2);
    assert_rejects("overscroll-behavior", "contain contain contain");
}

#[test]
fn the_logical_overscroll_behavior_pair_stays_out_of_author_reach() {
    // Compiled so the logical group stays balanced, never author-facing:
    // Lynx has no writing-mode-relative scrolling surface to hang it on.
    assert!(PropertyId::parse_enabled_for_all_content("overscroll-behavior-block").is_err());
    assert!(PropertyId::parse_enabled_for_all_content("overscroll-behavior-inline").is_err());
}

#[test]
fn scroll_capture_parses_auto_and_nearest_only() {
    for value in ["auto", "nearest"] {
        assert_eq!(
            longhand_ids("scroll-capture", value),
            [LonghandId::ScrollCapture]
        );
    }
    assert_rejects("scroll-capture", "parent");
    assert_rejects("scroll-capture", "self");
    assert_rejects("scroll-capture", "auto nearest");
}

#[test]
fn scroll_snap_type_parses_axis_and_strictness() {
    for value in ["none", "x", "y", "block", "inline", "both", "y mandatory", "x proximity"] {
        assert_eq!(
            longhand_ids("scroll-snap-type", value),
            [LonghandId::ScrollSnapType]
        );
    }
    assert_rejects("scroll-snap-type", "mandatory");
    assert_rejects("scroll-snap-type", "y always");
}

#[test]
fn scroll_snap_align_and_stop_parse_their_keywords() {
    for value in ["none", "start", "end", "center", "start end", "center none"] {
        assert_eq!(
            longhand_ids("scroll-snap-align", value),
            [LonghandId::ScrollSnapAlign]
        );
    }
    assert_rejects("scroll-snap-align", "middle");
    assert_rejects("scroll-snap-align", "start end center");
    for value in ["normal", "always"] {
        assert_eq!(
            longhand_ids("scroll-snap-stop", value),
            [LonghandId::ScrollSnapStop]
        );
    }
    assert_rejects("scroll-snap-stop", "never");
}

#[test]
fn scroll_margin_and_padding_shorthands_expand_to_the_physical_sides() {
    let ids = longhand_ids("scroll-margin", "10px 20px");
    assert_eq!(ids.len(), 4);
    assert!(ids.contains(&LonghandId::ScrollMarginTop));
    assert!(ids.contains(&LonghandId::ScrollMarginLeft));
    assert_rejects("scroll-margin", "10%");

    let ids = longhand_ids("scroll-padding", "auto 10% 5px");
    assert_eq!(ids.len(), 4);
    assert!(ids.contains(&LonghandId::ScrollPaddingBottom));
    assert!(ids.contains(&LonghandId::ScrollPaddingRight));
    assert_rejects("scroll-padding", "-5px");
    assert!(PropertyId::parse_enabled_for_all_content("scroll-margin-block").is_err());
    assert!(PropertyId::parse_enabled_for_all_content("scroll-padding-inline-start").is_err());
}
