// scroll-animations-1's timeline declarations (`scroll-timeline-*`,
// `view-timeline-*`, `timeline-scope`, the `animation-range` shorthand) and
// css-animations-2's `animation-duration: auto`: a user-directed W3C extension
// beyond Lynx's own property index, exposed only under the `lynx` feature.
// `animation-timeline` and `animation-range-start`/`-end` were already
// content-enabled; they are covered here for the `scroll()`/`view()` surface.
#![cfg(feature = "lynx")]

use cssparser::Parser as CssParser;
use style::context::QuirksMode;
use style::custom_properties::AttrTaint;
use style::parser::{Parse, ParserContext};
use style::properties::declaration_block::{parse_one_declaration_into, parse_style_attribute};
use style::properties::{
    longhands, style_structs, ComputedValues, LonghandId, PropertyDeclaration,
    PropertyDeclarationBlock, PropertyDeclarationId, PropertyId, SourcePropertyDeclaration,
};
use style::stylesheets::{CssRuleType, Origin, UrlExtraData};
use style::values::generics::position::TreeScoped;
use style::values::specified::animation::TimelineIdent;
use style::values::specified::position::{ScopedName, ScopedNameList};
use style::Atom;
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
    let mut parser = CssParser::new(css);
    parser
        .parse_entirely(|input| T::parse(&context, input))
        .map_err(|_| ())
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
        .unwrap_or_else(|()| panic!("`{name}: {value}` must parse"))
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

/// `css` parsed as a style attribute and serialized back the way CSSOM's
/// `cssText` does (shorthands restored where every longhand is present).
fn round_trip(css: &str) -> String {
    let block: PropertyDeclarationBlock = parse_style_attribute(
        css,
        &url_data(),
        None,
        QuirksMode::NoQuirks,
        CssRuleType::Style,
    );
    let mut out = String::new();
    block.to_css(&mut out).expect("serialization must not fail");
    out
}

/// The value `css` gives the longhand `name`, serialized.
fn longhand_value(css: &str, name: &str) -> String {
    let block: PropertyDeclarationBlock = parse_style_attribute(
        css,
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
        .expect("serialization must not fail");
    out
}

fn declarations(css: &str) -> Vec<PropertyDeclaration> {
    parse_style_attribute(
        css,
        &url_data(),
        None,
        QuirksMode::NoQuirks,
        CssRuleType::Style,
    )
    .declarations()
    .to_vec()
}

#[test]
fn timeline_properties_are_content_enabled() {
    for name in [
        "scroll-timeline",
        "scroll-timeline-name",
        "scroll-timeline-axis",
        "view-timeline",
        "view-timeline-name",
        "view-timeline-axis",
        "view-timeline-inset",
        "timeline-scope",
        "animation-timeline",
        "animation-range",
        "animation-range-start",
        "animation-range-end",
    ] {
        assert!(
            PropertyId::parse_enabled_for_all_content(name).is_ok(),
            "`{name}` must be content-enabled under `lynx`"
        );
    }
}

#[test]
fn scroll_timeline_longhands_parse_names_and_axes() {
    for value in ["none", "--a", "--a, none, --b"] {
        assert_eq!(
            longhand_ids("scroll-timeline-name", value),
            [LonghandId::ScrollTimelineName]
        );
    }
    for value in ["block", "inline", "x", "y", "x, block"] {
        assert_eq!(
            longhand_ids("scroll-timeline-axis", value),
            [LonghandId::ScrollTimelineAxis]
        );
    }
    // Timeline names are `<dashed-ident>`s.
    assert_rejects("scroll-timeline-name", "foo");
    assert_rejects("scroll-timeline-name", "--a --b");
    assert_rejects("scroll-timeline-axis", "vertical");
    assert_rejects("scroll-timeline-axis", "x y");
}

#[test]
fn view_timeline_longhands_parse_names_axes_and_insets() {
    for value in ["none", "--v", "--v, --w"] {
        assert_eq!(
            longhand_ids("view-timeline-name", value),
            [LonghandId::ViewTimelineName]
        );
    }
    for value in ["block", "inline", "x", "y"] {
        assert_eq!(
            longhand_ids("view-timeline-axis", value),
            [LonghandId::ViewTimelineAxis]
        );
    }
    for value in [
        "auto",
        "10px",
        "10% 20px",
        "auto 5px",
        "-5px",
        "10px, auto",
        "calc(10% + 5px) -2px",
    ] {
        assert_eq!(
            longhand_ids("view-timeline-inset", value),
            [LonghandId::ViewTimelineInset]
        );
    }
    assert_rejects("view-timeline-name", "foo");
    assert_rejects("view-timeline-inset", "1px 2px 3px");
    assert_rejects("view-timeline-inset", "none");
}

#[test]
fn timeline_scope_parses_none_all_and_dashed_idents() {
    for value in ["none", "all", "--a", "--a, --b"] {
        assert_eq!(
            longhand_ids("timeline-scope", value),
            [LonghandId::TimelineScope]
        );
    }
    assert_rejects("timeline-scope", "foo");
    assert_rejects("timeline-scope", "all, --a");
    assert_rejects("timeline-scope", "--a, none");
    assert_rejects("timeline-scope", "--a --b");
}

#[test]
fn timeline_shorthands_expand_to_their_longhands() {
    let ids = longhand_ids("scroll-timeline", "--a x");
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&LonghandId::ScrollTimelineName));
    assert!(ids.contains(&LonghandId::ScrollTimelineAxis));

    let ids = longhand_ids("view-timeline", "--v block 10% 20px");
    assert_eq!(ids.len(), 3);
    assert!(ids.contains(&LonghandId::ViewTimelineName));
    assert!(ids.contains(&LonghandId::ViewTimelineAxis));
    assert!(ids.contains(&LonghandId::ViewTimelineInset));

    // The inset may come before the axis.
    let ids = longhand_ids("view-timeline", "--v 10px inline");
    assert_eq!(ids.len(), 3);
    assert!(ids.contains(&LonghandId::ViewTimelineAxis));
    assert!(ids.contains(&LonghandId::ViewTimelineInset));

    for value in ["entry 10% exit 90%", "entry, exit 10%", "normal"] {
        let ids = longhand_ids("animation-range", value);
        assert_eq!(ids.len(), 2, "`animation-range: {value}`");
        assert!(ids.contains(&LonghandId::AnimationRangeStart));
        assert!(ids.contains(&LonghandId::AnimationRangeEnd));
    }
    assert_eq!(
        longhand_ids("animation-range-start", "entry calc(10% + 5px)"),
        [LonghandId::AnimationRangeStart]
    );

    // The name comes first.
    assert_rejects("scroll-timeline", "x --a");
    assert_rejects("view-timeline", "block --v");
    assert_rejects("animation-range", "foo");
}

#[test]
fn animation_timeline_parses_scroll_and_view_notations() {
    for value in [
        "auto",
        "none",
        "--a",
        "scroll()",
        "scroll(nearest inline)",
        "scroll(root)",
        "scroll(self x)",
        "view()",
        "view(block 10% 20px)",
        "view(auto 5px inline)",
        "scroll(y root), view(), --b",
    ] {
        assert_eq!(
            longhand_ids("animation-timeline", value),
            [LonghandId::AnimationTimeline]
        );
    }
    assert_rejects("animation-timeline", "scroll(foo)");
    assert_rejects("animation-timeline", "scroll(nearest root)");
    assert_rejects("animation-timeline", "view(nearest)");
    assert_rejects("animation-timeline", "view(1px, 2px)");
    assert_rejects("animation-timeline", "view(1px 2px 3px)");
    assert_rejects("animation-timeline", "foo");
}

#[test]
fn timeline_values_serialize_in_their_shortest_form() {
    for (css, expected) in [
        ("scroll-timeline: --a;", "scroll-timeline: --a;"),
        ("scroll-timeline: --a block;", "scroll-timeline: --a;"),
        (
            "scroll-timeline: --a x, --b;",
            "scroll-timeline: --a x, --b;",
        ),
        (
            "view-timeline: --v inline 10% 20px;",
            "view-timeline: --v inline 10% 20px;",
        ),
        ("view-timeline: --v 10px 10px;", "view-timeline: --v 10px;"),
        ("view-timeline: --v auto;", "view-timeline: --v;"),
        ("timeline-scope: none;", "timeline-scope: none;"),
        ("timeline-scope: all;", "timeline-scope: all;"),
        ("timeline-scope: --a, --b;", "timeline-scope: --a, --b;"),
        (
            "animation-timeline: scroll(nearest block);",
            "animation-timeline: scroll();",
        ),
        (
            "animation-timeline: scroll(inline root);",
            "animation-timeline: scroll(root inline);",
        ),
        (
            "animation-timeline: view(block 10% 20px);",
            "animation-timeline: view(10% 20px);",
        ),
        ("animation-range: entry;", "animation-range: entry;"),
        (
            "animation-range: entry 10% exit 90%;",
            "animation-range: entry 10% exit 90%;",
        ),
        ("animation-range: 10%;", "animation-range: 10%;"),
        (
            "animation-range: entry exit;",
            "animation-range: entry exit;",
        ),
        ("animation-duration: auto;", "animation-duration: auto;"),
    ] {
        assert_eq!(round_trip(css), expected, "round trip of `{css}`");
    }
}

#[test]
fn animation_range_shorthand_fills_an_omitted_end_from_the_start_name() {
    // `entry 10%` alone ends at `entry 100%`; a bare offset ends at `normal`.
    for (shorthand, longhands) in [
        (
            "animation-range: entry 10%;",
            "animation-range-start: entry 10%; animation-range-end: entry 100%;",
        ),
        (
            "animation-range: entry 10% exit 90%;",
            "animation-range-start: entry 10%; animation-range-end: exit 90%;",
        ),
        (
            "animation-range: 10%;",
            "animation-range-start: 10%; animation-range-end: normal;",
        ),
        (
            "animation-range: entry exit;",
            "animation-range-start: entry 0%; animation-range-end: exit 100%;",
        ),
        (
            "animation-range: entry 10% 90%;",
            "animation-range-start: entry 10%; animation-range-end: 90%;",
        ),
    ] {
        assert_eq!(
            declarations(shorthand).len(),
            2,
            "`{shorthand}` sets two longhands"
        );
        assert_eq!(
            declarations(shorthand),
            declarations(longhands),
            "`{shorthand}` expands to `{longhands}`"
        );
    }
}

#[test]
fn timeline_ident_exposes_its_name() {
    let name: TimelineIdent = parse("--a").unwrap();
    assert_eq!(name.as_atom(), Some(&Atom::from("--a")));
    let none: TimelineIdent = parse("none").unwrap();
    assert_eq!(none.as_atom(), None);
}

#[test]
fn scoped_name_list_exposes_all_and_its_names() {
    let none: ScopedNameList = parse("none").unwrap();
    assert!(none.is_none() && !none.is_all());
    assert_eq!(none.iter().count(), 0);

    let all: ScopedNameList = parse("all").unwrap();
    assert!(all.is_all() && !all.is_none());
    assert_eq!(all.iter().count(), 0);

    let names: ScopedNameList = parse("--a, --b").unwrap();
    assert!(!names.is_all() && !names.is_none());
    assert_eq!(
        names.iter().cloned().collect::<Vec<_>>(),
        [Atom::from("--a"), Atom::from("--b")]
    );
}

#[test]
fn ui_struct_reports_declared_timelines_and_scopes() {
    let initial =
        ComputedValues::initial_values_with_font_override(style_structs::Font::initial_values());
    let ui = initial.get_ui();
    assert!(!ui.specifies_scroll_timelines());
    assert!(!ui.specifies_view_timelines());
    assert!(!ui.specifies_timeline_scope());

    let mut values: ComputedValues = (*initial).clone();
    values
        .mutate_ui()
        .set_timeline_scope(ScopedName::with_default_level(ScopedNameList::all()));
    assert!(values.get_ui().specifies_timeline_scope());

    let name = TreeScoped::with_default_level(parse::<TimelineIdent>("--a").unwrap());
    let mut names = longhands::scroll_timeline_name::get_initial_value();
    names.0[0] = name.clone();
    values.mutate_ui().set_scroll_timeline_name(names);
    assert!(values.get_ui().specifies_scroll_timelines());
    assert!(!values.get_ui().specifies_view_timelines());

    let mut names = longhands::view_timeline_name::get_initial_value();
    names.0[0] = name;
    values.mutate_ui().set_view_timeline_name(names);
    assert!(values.get_ui().specifies_view_timelines());
}

#[test]
fn animation_shorthand_reads_auto_as_a_duration() {
    // css-animations-2 makes `auto` a duration keyword, so it is no longer a
    // name in the `animation` shorthand.
    let css = "animation: auto 1s";
    assert_eq!(longhand_value(css, "animation-duration"), "auto");
    assert_eq!(longhand_value(css, "animation-delay"), "1s");
    assert_eq!(longhand_value(css, "animation-name"), "none");

    // An animation named `auto` therefore serializes its duration too.
    let longhands = "animation-name: auto; animation-duration: auto; \
        animation-timing-function: ease; animation-delay: 0s; \
        animation-iteration-count: 1; animation-direction: normal; \
        animation-fill-mode: none; animation-play-state: running; \
        animation-timeline: auto; animation-range-start: normal; \
        animation-range-end: normal;";
    let serialized = round_trip(longhands);
    assert_eq!(serialized, "animation: auto auto;");
    assert_eq!(longhand_value(&serialized, "animation-name"), "auto");
    assert_eq!(longhand_value(&serialized, "animation-duration"), "auto");
}
