// The scrolling properties lynx-vello's scroll chain and snapping read:
// css-overscroll-1's `overscroll-behavior` and css-scroll-snap-1 (both W3C
// extensions beyond Lynx's own property index) and the project-only
// per-axis `scroll-capture-x`/`-y` and their `scroll-capture` shorthand. All
// of them exist only under the `lynx` feature.
#![cfg(feature = "lynx")]

use style::context::QuirksMode;
use style::properties::declaration_block::{parse_one_declaration_into, parse_style_attribute};
use style::properties::{
    longhands, style_structs, ComputedValues, LonghandId, PropertyDeclaration,
    PropertyDeclarationId, PropertyId, SourcePropertyDeclaration,
};
use style::stylesheets::{CssRuleType, Origin, UrlExtraData};
use style::values::computed::ScrollCapture;
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
fn overscroll_behavior_longhands_parse_the_standard_keywords_contain_bounce_and_circular() {
    for value in ["auto", "contain", "none", "contain-bounce", "circular"] {
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
    assert_rejects("overscroll-behavior-y", "bounce");
    assert_rejects("overscroll-behavior-x", "circle");
    assert_rejects("overscroll-behavior-y", "loop");
}

#[test]
fn overscroll_behavior_shorthand_takes_contain_bounce_per_axis() {
    let ids = longhand_ids("overscroll-behavior", "contain-bounce");
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&LonghandId::OverscrollBehaviorX));
    assert!(ids.contains(&LonghandId::OverscrollBehaviorY));

    let ids = longhand_ids("overscroll-behavior", "auto contain-bounce");
    assert_eq!(ids.len(), 2);
}

#[test]
fn overscroll_behavior_shorthand_takes_circular_per_axis() {
    let ids = longhand_ids("overscroll-behavior", "circular");
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&LonghandId::OverscrollBehaviorX));
    assert!(ids.contains(&LonghandId::OverscrollBehaviorY));

    let ids = longhand_ids("overscroll-behavior", "contain-bounce circular");
    assert_eq!(ids.len(), 2);
    let ids = longhand_ids("overscroll-behavior", "auto circular");
    assert_eq!(ids.len(), 2);
    assert_rejects("overscroll-behavior", "circle");
    assert_rejects("overscroll-behavior", "loop");
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

/// `name` serialized from an inline style block declaring `name: value`.
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

/// The (x, y) values `scroll-capture: value` expands to.
fn scroll_capture_axes(value: &str) -> (ScrollCapture, ScrollCapture) {
    let declarations = parse_declaration("scroll-capture", value)
        .unwrap_or_else(|_| panic!("`scroll-capture: {value}` must parse"))
        .declarations;
    assert_eq!(declarations.len(), 2, "`{value}`");
    let (mut x, mut y) = (None, None);
    for declaration in declarations.iter() {
        match *declaration {
            PropertyDeclaration::ScrollCaptureX(v) => x = Some(v),
            PropertyDeclaration::ScrollCaptureY(v) => y = Some(v),
            ref other => panic!("`{value}` expanded to {other:?}"),
        }
    }
    (x.unwrap(), y.unwrap())
}

/// One per-axis value, its parsed form and its serialization.
const SCROLL_CAPTURE_VALUES: &[(&str, ScrollCapture, &str)] = &[
    ("auto", ScrollCapture::Auto, "auto"),
    ("nearest", ScrollCapture::Nearest, "nearest"),
    (
        "nearest forward",
        ScrollCapture::NearestForward,
        "nearest forward",
    ),
    (
        "nearest backward",
        ScrollCapture::NearestBackward,
        "nearest backward",
    ),
    (
        "NEAREST Forward",
        ScrollCapture::NearestForward,
        "nearest forward",
    ),
    ("AUTO", ScrollCapture::Auto, "auto"),
];

/// Values neither a longhand nor the shorthand accepts.
const SCROLL_CAPTURE_REJECTED: &[&str] = &[
    "parent",
    "self",
    "auto forward",
    "auto backward",
    "forward",
    "backward",
    "forward nearest",
    "nearest both",
    "nearest forward backward",
    "nearest, forward",
    "nearest x",
    "none",
    "1",
];

#[test]
fn scroll_capture_longhands_parse_auto_and_nearest_with_an_optional_direction() {
    for &(name, id) in &[
        ("scroll-capture-x", LonghandId::ScrollCaptureX),
        ("scroll-capture-y", LonghandId::ScrollCaptureY),
    ] {
        for &(value, expected, serialization) in SCROLL_CAPTURE_VALUES {
            let mut declarations = parse_declaration(name, value)
                .unwrap_or_else(|_| panic!("`{name}: {value}` must parse"))
                .declarations;
            assert_eq!(declarations.len(), 1);
            let declaration = declarations.remove(0);
            assert_eq!(declaration.id(), PropertyDeclarationId::Longhand(id));
            match declaration {
                PropertyDeclaration::ScrollCaptureX(parsed)
                | PropertyDeclaration::ScrollCaptureY(parsed) => {
                    assert_eq!(parsed, expected, "`{name}: {value}`")
                },
                other => panic!("`{name}: {value}` parsed to {other:?}"),
            }
            assert_eq!(serialized(name, value), serialization, "`{name}: {value}`");
        }
        for value in SCROLL_CAPTURE_REJECTED.iter().chain(&[
            "auto nearest",
            "nearest nearest",
            "nearest forward auto",
        ]) {
            assert_rejects(name, value);
        }
    }
}

#[test]
fn scroll_capture_shorthand_takes_one_or_two_values() {
    use ScrollCapture::{Auto, Nearest, NearestBackward, NearestForward};
    // One value sets both axes.
    for &(value, expected, serialization) in SCROLL_CAPTURE_VALUES {
        assert_eq!(
            scroll_capture_axes(value),
            (expected, expected),
            "`{value}`"
        );
        assert_eq!(
            serialized("scroll-capture", value),
            serialization,
            "`{value}`"
        );
    }
    // Two values are x then y; a value is `auto` or `nearest` with an
    // optional direction, parsed greedily.
    for (value, expected, serialization) in [
        ("auto nearest", (Auto, Nearest), "auto nearest"),
        (
            "nearest forward auto",
            (NearestForward, Auto),
            "nearest forward auto",
        ),
        (
            "auto nearest backward",
            (Auto, NearestBackward),
            "auto nearest backward",
        ),
        (
            "nearest backward nearest forward",
            (NearestBackward, NearestForward),
            "nearest backward nearest forward",
        ),
        ("nearest nearest", (Nearest, Nearest), "nearest"),
        (
            "nearest forward nearest forward",
            (NearestForward, NearestForward),
            "nearest forward",
        ),
        ("auto auto", (Auto, Auto), "auto"),
    ] {
        assert_eq!(scroll_capture_axes(value), expected, "`{value}`");
        assert_eq!(
            serialized("scroll-capture", value),
            serialization,
            "`{value}`"
        );
    }
    for value in SCROLL_CAPTURE_REJECTED.iter().chain(&[
        "auto auto auto",
        "nearest nearest nearest",
        "nearest forward auto auto",
        "auto, nearest",
        "nearest forward backward auto",
    ]) {
        assert_rejects("scroll-capture", value);
    }
}

#[test]
fn scroll_capture_shorthand_serializes_from_its_longhands() {
    let block = parse_style_attribute(
        "scroll-capture-x: auto; scroll-capture-y: nearest forward",
        &url_data(),
        None,
        QuirksMode::NoQuirks,
        CssRuleType::Style,
    );
    let mut out = String::new();
    block
        .property_value_to_css(
            &PropertyId::parse_enabled_for_all_content("scroll-capture").unwrap(),
            &mut out,
        )
        .unwrap();
    assert_eq!(out, "auto nearest forward");
}

#[test]
fn scroll_capture_longhands_are_initially_auto_and_not_inherited() {
    assert_eq!(
        longhands::scroll_capture_x::get_initial_value(),
        ScrollCapture::Auto
    );
    assert_eq!(
        longhands::scroll_capture_y::get_initial_value(),
        ScrollCapture::Auto
    );
    assert!(!LonghandId::ScrollCaptureX.inherited());
    assert!(!LonghandId::ScrollCaptureY.inherited());
}

#[test]
fn scroll_capture_computed_values_read_per_axis() {
    let initial =
        ComputedValues::initial_values_with_font_override(style_structs::Font::initial_values());
    assert_eq!(initial.slow_clone_scroll_capture_x(), ScrollCapture::Auto);
    assert_eq!(initial.slow_clone_scroll_capture_y(), ScrollCapture::Auto);

    // The computed value is the specified value.
    let (x, y) = scroll_capture_axes("auto nearest forward");
    let mut values: ComputedValues = (*initial).clone();
    values.mutate_box().set_scroll_capture_x(x);
    values.mutate_box().set_scroll_capture_y(y);
    assert_eq!(values.slow_clone_scroll_capture_x(), ScrollCapture::Auto);
    assert_eq!(
        values.slow_clone_scroll_capture_y(),
        ScrollCapture::NearestForward
    );
    assert!(!values.slow_clone_scroll_capture_x().covers_delta(10.));
    assert!(values.slow_clone_scroll_capture_y().covers_delta(10.));
    assert!(!values.slow_clone_scroll_capture_y().covers_delta(-10.));
}

#[test]
fn scroll_capture_has_no_logical_axes() {
    for name in [
        "scroll-capture-block",
        "scroll-capture-inline",
        "scroll-capture-descendants",
    ] {
        assert!(PropertyId::parse_enabled_for_all_content(name).is_err());
    }
}

#[test]
fn scroll_capture_reports_direction_coverage() {
    use ScrollCapture::{Auto, Nearest, NearestBackward, NearestForward};
    for (value, nearest, forward, backward) in [
        (Auto, false, false, false),
        (Nearest, true, true, true),
        (NearestForward, true, true, false),
        (NearestBackward, true, false, true),
    ] {
        assert_eq!(value.is_nearest(), nearest, "{value:?}");
        assert_eq!(value.covers_forward(), forward, "{value:?}");
        assert_eq!(value.covers_backward(), backward, "{value:?}");
        assert_eq!(value.covers_delta(10.), forward, "{value:?}");
        assert_eq!(value.covers_delta(-0.5), backward, "{value:?}");
        assert!(!value.covers_delta(0.), "{value:?}");
        assert!(!value.covers_delta(-0.), "{value:?}");
        assert!(!value.covers_delta(f32::NAN), "{value:?}");
    }
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

#[test]
fn scroll_initial_target_parses_none_and_nearest_only() {
    for value in ["none", "nearest"] {
        assert_eq!(
            longhand_ids("scroll-initial-target", value),
            [LonghandId::ScrollInitialTarget]
        );
    }
    assert_rejects("scroll-initial-target", "auto");
    assert_rejects("scroll-initial-target", "start");
}
