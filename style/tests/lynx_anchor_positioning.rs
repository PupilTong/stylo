//! css-anchor-position-1 under the `lynx` feature: lynx-vello implements the
//! module (the Editor's Draft, current spellings only).
//!
//! Parse / serialize / computed-value expectations follow the WPT
//! `css/css-anchor-position` parsing tests (`anchor-parse-*`,
//! `anchor-size-parse-*`, `position-anchor-basics`,
//! `position-anchor-match-parent`, `position-area-parsing`,
//! `position-area-computed`, `parsing/anchor-scope-*`,
//! `parsing/position-try-*`, `parsing/position-visibility-*`,
//! `at-position-try-parse`, `at-position-try-allowed-declarations`) and the
//! `try-tactic-*` tests, adapted where the Lynx surface or the Editor's Draft
//! differs (each adaptation is commented at its test). Layout-time behavior
//! (resolving `anchor()` / `anchor-size()`, `position-area`, `anchor-center`,
//! the fallback loop, `position-visibility`) is the outer repository's.
//!
//! `Stylist::resolve_position_try` runs here on a detached test element
//! (`support/test_element.rs`) against a small stylesheet.
#![cfg(feature = "lynx")]

#[path = "support/test_element.rs"]
mod test_element;

use euclid::{Scale, Size2D};
use servo_arc::Arc;
use style::context::{CascadeInputs, QuirksMode, TreeCountingCaches};
use style::device::servo::FontMetricsProvider;
use style::device::Device;
use style::dom::DummyElementContext;
use style::font_metrics::FontMetrics;
use style::media_queries::{MediaList, MediaType};
use style::properties::declaration_block::{parse_one_declaration_into, parse_style_attribute};
use style::properties::{
    style_structs, ComputedValues, FirstLineReparenting, LonghandId, PropertyDeclaration,
    PropertyId, SourcePropertyDeclaration,
};
use style::queries::values::PrefersColorScheme;
use style::rule_cache::RuleCacheConditions;
use style::rule_tree::{CascadeLevel, RuleCascadeFlags, StyleSource};
use style::servo::media_features::PointerCapabilities;
use style::shared_lock::{SharedRwLock, StylesheetGuards, ToCssWithGuard};
use style::stylesheets::container_rule::ContainerSizeQuery;
use style::stylesheets::layer_rule::LayerOrder;
use style::stylesheets::{
    AllowImportRules, CssRule, CssRuleType, DocumentStyleSheet, Origin, Stylesheet,
    StylesheetInDocument, UrlExtraData,
};
use style::stylist::Stylist;
use style::values::computed::font::GenericFontFamily;
use style::values::computed::position::{
    PositionAnchor, PositionArea, PositionTryFallbacks, PositionVisibility,
};
use style::values::computed::{CSSPixelLength, Context, ToComputedValue};
use style::values::specified::font::QueryFontMetricsFlags;
use style::values::specified::position::{PositionAnchorKeyword, PositionTryFallbacksItem};
use style_traits::{CSSPixel, DevicePixel, ParsingMode, ToCss};
use test_element::TestElement;

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

fn is_author_property(name: &str) -> bool {
    PropertyId::parse_enabled_for_all_content(name).is_ok()
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

/// WPT `test_valid_value(name, value, expected)`.
fn assert_valid(name: &str, value: &str, expected: &str) {
    assert!(
        parse_declaration(name, value).is_ok(),
        "`{name}: {value}` must parse"
    );
    assert_eq!(serialized(name, value), expected, "`{name}: {value}`");
}

// ---------------------------------------------------------------------------
// §2.1 `anchor-name`

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
// §2.2 `anchor-scope` (WPT parsing/anchor-scope-parsing.html,
// parsing/anchor-scope-computed.html)

#[test]
fn anchor_scope_parses_and_computes() {
    for value in [
        "none",
        "all",
        "--a",
        "--a, --b",
        "--a, --b, --c",
        "--foo, --bar",
        "--bar, --foo",
    ] {
        assert_valid("anchor-scope", value, value);
        assert_eq!(
            computed("", &format!("anchor-scope: {value}"), "anchor-scope"),
            value
        );
    }
    for value in ["initial", "inherit", "unset", "revert"] {
        assert_valid("anchor-scope", value, value);
    }
    for value in ["--a none", "none --a", "none all", "--a --b", "a, b, c", ""] {
        assert_rejects("anchor-scope", value);
    }
    assert_eq!(computed("", "", "anchor-scope"), "none");
    assert_eq!(
        computed("", "anchor-scope: initial", "anchor-scope"),
        "none"
    );
}

#[test]
fn anchor_scope_names_are_exposed() {
    let style = style_for("", "anchor-scope: --a, --b");
    let scope = style.clone_anchor_scope();
    assert!(!scope.is_none());
    assert!(!scope.value.is_all());
    let names: Vec<_> = scope.value.iter().map(|a| a.to_string()).collect();
    assert_eq!(names, ["--a", "--b"]);
    let all = style_for("", "anchor-scope: all").clone_anchor_scope();
    assert!(all.value.is_all());
    assert_eq!(all.value.iter().count(), 0);
}

// ---------------------------------------------------------------------------
// §2.4 `position-anchor` (WPT position-anchor-basics.html,
// position-anchor-match-parent.html)

#[test]
fn position_anchor_parses_and_computes() {
    for value in ["normal", "none", "auto", "--foo", "match-parent"] {
        assert_valid("position-anchor", value, value);
        assert_eq!(
            computed("", &format!("position-anchor: {value}"), "position-anchor"),
            value
        );
    }
    for value in [
        "foo-bar",
        "--foo --bar",
        "--foo, --bar",
        "100px",
        "100%",
        "match-parent --foo",
    ] {
        assert_rejects("position-anchor", value);
    }
    // Initial: normal.
    assert_eq!(computed("", "", "position-anchor"), "normal");
    assert!(matches!(
        style_for("", "").clone_position_anchor().value,
        PositionAnchorKeyword::Normal
    ));
    let match_parent: PositionAnchor =
        style_for("", "position-anchor: match-parent").clone_position_anchor();
    assert!(matches!(
        match_parent.value,
        PositionAnchorKeyword::MatchParent
    ));
    let named = style_for("", "position-anchor: --foo").clone_position_anchor();
    let PositionAnchorKeyword::Ident(ref name) = named.value else {
        panic!("a named default anchor")
    };
    assert_eq!(name.0.to_string(), "--foo");
    assert_eq!(named.scope, CascadeLevel::same_tree_author_normal());
}

// ---------------------------------------------------------------------------
// §3.1 `position-area` (WPT position-area-parsing.html,
// position-area-computed.html)

const HORIZONTAL: &[&str] = &[
    "left",
    "right",
    "span-left",
    "span-right",
    "x-start",
    "x-end",
    "span-x-start",
    "span-x-end",
    "self-x-start",
    "self-x-end",
    "span-self-x-start",
    "span-self-x-end",
];
const VERTICAL: &[&str] = &[
    "top",
    "bottom",
    "span-top",
    "span-bottom",
    "y-start",
    "y-end",
    "span-y-start",
    "span-y-end",
    "self-y-start",
    "self-y-end",
    "span-self-y-start",
    "span-self-y-end",
];
const INLINE: &[&str] = &[
    "inline-start",
    "inline-end",
    "span-inline-start",
    "span-inline-end",
];
const BLOCK: &[&str] = &[
    "block-start",
    "block-end",
    "span-block-start",
    "span-block-end",
];
const SELF_INLINE: &[&str] = &[
    "self-inline-start",
    "self-inline-end",
    "span-self-inline-start",
    "span-self-inline-end",
];
const SELF_BLOCK: &[&str] = &[
    "self-block-start",
    "self-block-end",
    "span-self-block-start",
    "span-self-block-end",
];
const START_END: &[&str] = &["start", "end", "span-start", "span-end"];
const SELF_START_END: &[&str] = &["self-start", "self-end", "span-self-start", "span-self-end"];

#[test]
fn position_area_parses_per_wpt() {
    let pa = "position-area";
    assert_valid(pa, "none", "none");
    for value in ["none none", "start none", "none start", "top left top"] {
        assert_rejects(pa, value);
    }
    assert_valid(pa, "center", "center");
    assert_valid(pa, "center center", "center");
    assert_valid(pa, "span-all", "span-all");
    assert_valid(pa, "span-all span-all", "span-all");
    assert_valid(pa, "center span-all", "center span-all");
    assert_valid(pa, "span-all center", "span-all center");
    for group in [
        HORIZONTAL,
        VERTICAL,
        INLINE,
        BLOCK,
        SELF_INLINE,
        SELF_BLOCK,
        START_END,
        SELF_START_END,
    ] {
        for keyword in group {
            assert_valid(pa, keyword, keyword);
        }
    }
    let pairs = |a: &[&str], b: &[&str], flip: bool| {
        for k1 in a {
            for k2 in b {
                let expected = if k1 == k2 {
                    k1.to_string()
                } else if flip {
                    format!("{k2} {k1}")
                } else {
                    format!("{k1} {k2}")
                };
                assert_valid(pa, &format!("{k1} {k2}"), &expected);
            }
        }
    };
    pairs(HORIZONTAL, VERTICAL, false);
    pairs(VERTICAL, HORIZONTAL, true);
    pairs(BLOCK, INLINE, false);
    pairs(INLINE, BLOCK, true);
    pairs(SELF_BLOCK, SELF_INLINE, false);
    pairs(SELF_INLINE, SELF_BLOCK, true);
    pairs(START_END, START_END, false);
    pairs(SELF_START_END, SELF_START_END, false);
    let with_span_all_center = |group: &[&str], flip: bool| {
        for k in group {
            let center = if flip {
                format!("center {k}")
            } else {
                format!("{k} center")
            };
            assert_valid(pa, &format!("{k} center"), &center);
            assert_valid(pa, &format!("center {k}"), &center);
            assert_valid(pa, &format!("{k} span-all"), k);
            assert_valid(pa, &format!("span-all {k}"), k);
        }
    };
    with_span_all_center(HORIZONTAL, false);
    with_span_all_center(VERTICAL, true);
    with_span_all_center(BLOCK, false);
    with_span_all_center(INLINE, true);
    with_span_all_center(SELF_BLOCK, false);
    with_span_all_center(SELF_INLINE, true);
    for group in [START_END, SELF_START_END] {
        for k in group {
            for value in [
                format!("{k} center"),
                format!("center {k}"),
                format!("{k} span-all"),
                format!("span-all {k}"),
            ] {
                assert_valid(pa, &value, &value);
            }
        }
    }
    let invalid_pairs = |a: &[&str], b: &[&str]| {
        for k1 in a {
            for k2 in b {
                assert_rejects(pa, &format!("{k1} {k2}"));
                assert_rejects(pa, &format!("{k2} {k1}"));
            }
        }
    };
    for (a, b) in [
        (HORIZONTAL, INLINE),
        (HORIZONTAL, BLOCK),
        (HORIZONTAL, SELF_INLINE),
        (HORIZONTAL, SELF_BLOCK),
        (HORIZONTAL, START_END),
        (HORIZONTAL, SELF_START_END),
        (VERTICAL, INLINE),
        (VERTICAL, BLOCK),
        (VERTICAL, SELF_INLINE),
        (VERTICAL, SELF_BLOCK),
        (VERTICAL, START_END),
        (VERTICAL, SELF_START_END),
        (INLINE, SELF_INLINE),
        (INLINE, SELF_BLOCK),
        (INLINE, START_END),
        (INLINE, SELF_START_END),
        (BLOCK, SELF_INLINE),
        (BLOCK, SELF_BLOCK),
        (BLOCK, START_END),
        (BLOCK, SELF_START_END),
        (START_END, SELF_START_END),
    ] {
        invalid_pairs(a, b);
    }
    for group in [HORIZONTAL, VERTICAL, INLINE, BLOCK, SELF_INLINE, SELF_BLOCK] {
        for k in group {
            assert_rejects(pa, &format!("{k} {k}"));
        }
    }
    for value in [
        "foobar",
        "visible",
        "hidden",
        "start foobar",
        "end visible",
        "block-start hidden",
        "hidden inline-end",
        "foo bar",
        "visible hidden",
        "hidden visible",
        // The withdrawn function spelling.
        "inset-area(top)",
    ] {
        assert_rejects(pa, value);
    }
    // The withdrawn property spelling.
    assert!(!is_author_property("inset-area"));
}

fn computed_position_area(value: &str) -> String {
    computed("", &format!("position-area: {value}"), "position-area")
}

#[test]
fn position_area_computes_per_wpt() {
    for value in ["none", "span-all", "center"] {
        assert_eq!(computed_position_area(value), value);
    }
    for group in [
        HORIZONTAL,
        VERTICAL,
        INLINE,
        BLOCK,
        SELF_INLINE,
        SELF_BLOCK,
        START_END,
        SELF_START_END,
    ] {
        for k in group {
            assert_eq!(computed_position_area(k), *k);
        }
    }
    for h in HORIZONTAL {
        for v in VERTICAL {
            assert_eq!(
                computed_position_area(&format!("{h} {v}")),
                format!("{h} {v}")
            );
            assert_eq!(
                computed_position_area(&format!("{v} {h}")),
                format!("{h} {v}")
            );
        }
        assert_eq!(computed_position_area(&format!("{h} span-all")), *h);
        assert_eq!(computed_position_area(&format!("span-all {h}")), *h);
        assert_eq!(
            computed_position_area(&format!("{h} center")),
            format!("{h} center")
        );
        assert_eq!(
            computed_position_area(&format!("center {h}")),
            format!("{h} center")
        );
    }
    for v in VERTICAL {
        assert_eq!(computed_position_area(&format!("span-all {v}")), *v);
        assert_eq!(computed_position_area(&format!("{v} span-all")), *v);
        assert_eq!(
            computed_position_area(&format!("center {v}")),
            format!("center {v}")
        );
        assert_eq!(
            computed_position_area(&format!("{v} center")),
            format!("center {v}")
        );
    }
    // Unambiguous logical pairs compute to the short `start` / `end` forms.
    let short = |k: &str| {
        k.replace("block-", "")
            .replace("inline-", "")
            .replace("self-block-", "self-")
            .replace("self-inline-", "self-")
    };
    for (block, inline) in [(BLOCK, INLINE), (SELF_BLOCK, SELF_INLINE)] {
        for b in block {
            for i in inline {
                let (sb, si) = (short(b), short(i));
                let expected = if sb == si {
                    sb.clone()
                } else {
                    format!("{sb} {si}")
                };
                assert_eq!(computed_position_area(&format!("{b} {i}")), expected);
                assert_eq!(computed_position_area(&format!("{i} {b}")), expected);
            }
        }
        for b in block {
            assert_eq!(computed_position_area(&format!("{b} span-all")), *b);
            assert_eq!(computed_position_area(&format!("span-all {b}")), *b);
            let sb = short(b);
            assert_eq!(
                computed_position_area(&format!("{b} center")),
                format!("{sb} center")
            );
            assert_eq!(
                computed_position_area(&format!("center {b}")),
                format!("{sb} center")
            );
        }
        for i in inline {
            assert_eq!(computed_position_area(&format!("{i} span-all")), *i);
            assert_eq!(computed_position_area(&format!("span-all {i}")), *i);
            let si = short(i);
            assert_eq!(
                computed_position_area(&format!("{i} center")),
                format!("center {si}")
            );
            assert_eq!(
                computed_position_area(&format!("center {i}")),
                format!("center {si}")
            );
        }
    }
    for (group, block_prefix, inline_prefix) in [
        (START_END, "block-", "inline-"),
        (SELF_START_END, "self-block-", "self-inline-"),
    ] {
        for k1 in group {
            for k2 in group {
                let expected = if k1 == k2 {
                    k1.to_string()
                } else {
                    format!("{k1} {k2}")
                };
                assert_eq!(computed_position_area(&format!("{k1} {k2}")), expected);
            }
            // `start span-all` → `block-start`, `span-all start` →
            // `inline-start` (and the span / self variants alike).
            let axis_keyword = |prefix: &str| {
                let bare = k1.trim_start_matches("span-").trim_start_matches("self-");
                let span = if k1.starts_with("span-") { "span-" } else { "" };
                format!("{span}{prefix}{bare}")
            };
            assert_eq!(
                computed_position_area(&format!("{k1} span-all")),
                axis_keyword(block_prefix)
            );
            assert_eq!(
                computed_position_area(&format!("span-all {k1}")),
                axis_keyword(inline_prefix)
            );
            assert_eq!(
                computed_position_area(&format!("{k1} center")),
                format!("{k1} center")
            );
            assert_eq!(
                computed_position_area(&format!("center {k1}")),
                format!("center {k1}")
            );
        }
    }
    assert_eq!(computed_position_area("span-all span-all"), "span-all");
    assert_eq!(computed_position_area("span-all center"), "span-all center");
    assert_eq!(computed_position_area("center span-all"), "center span-all");
    assert_eq!(computed_position_area("center center"), "center");
}

#[test]
fn position_area_exposes_its_physical_and_alignment_helpers() {
    use style::logical_geometry::{LogicalAxis, WritingMode};
    use style::values::specified::align::AlignFlags;
    let area: PositionArea =
        style_for("", "position-area: block-end span-inline-end").clone_position_area();
    let wm = WritingMode::horizontal_tb();
    let physical = area.to_physical(wm, wm);
    assert_eq!(physical.to_css_string(), "span-right bottom");
    // §4.1 (on the physical value): the box aligns toward the unselected
    // side (`bottom`, `span-right` → start); a center track centers; all
    // three tracks → anchor-center.
    assert_eq!(
        physical.first.to_self_alignment(LogicalAxis::Inline, &wm),
        Some(AlignFlags::START)
    );
    assert_eq!(
        physical.second.to_self_alignment(LogicalAxis::Block, &wm),
        Some(AlignFlags::START)
    );
    let top_left = style_for("", "position-area: top left")
        .clone_position_area()
        .to_physical(wm, wm);
    assert_eq!(top_left.to_css_string(), "left top");
    assert_eq!(
        top_left.first.to_self_alignment(LogicalAxis::Inline, &wm),
        Some(AlignFlags::END)
    );
    assert_eq!(
        top_left.second.to_self_alignment(LogicalAxis::Block, &wm),
        Some(AlignFlags::END)
    );
    let centered = style_for("", "position-area: center span-all")
        .clone_position_area()
        .to_physical(wm, wm);
    assert_eq!(
        centered.first.to_self_alignment(LogicalAxis::Inline, &wm),
        Some(AlignFlags::ANCHOR_CENTER)
    );
    assert_eq!(
        centered.second.to_self_alignment(LogicalAxis::Block, &wm),
        Some(AlignFlags::CENTER)
    );
    // rtl: the inline axis is reversed.
    let rtl = WritingMode::horizontal_tb() | WritingMode::RTL | WritingMode::INLINE_REVERSED;
    let start = style_for("", "position-area: inline-start")
        .clone_position_area()
        .to_physical(rtl, rtl);
    // The physical form keeps both keywords explicit (the layout engine's
    // input, not a serialization).
    assert_eq!(start.to_css_string(), "right span-all");
}

// ---------------------------------------------------------------------------
// §3.2 `anchor()` (WPT anchor-parse-valid.html, anchor-parse-invalid.html)

/// The inset properties on the Lynx surface. WPT also lists
/// `inset-block-start` / `inset-block-end`, which are not Lynx properties.
const INSET_PROPERTIES: &[&str] = &[
    "left",
    "right",
    "top",
    "bottom",
    "inset-inline-start",
    "inset-inline-end",
];

#[test]
fn anchor_parses_in_every_inset_property_per_wpt() {
    assert!(!is_author_property("inset-block-start"));
    let sides: &[(&str, &str)] = &[
        ("inside", "inside"),
        ("outside", "outside"),
        ("left", "left"),
        ("right", "right"),
        ("top", "top"),
        ("bottom", "bottom"),
        ("start", "start"),
        ("end", "end"),
        ("self-start", "self-start"),
        ("self-end", "self-end"),
        ("center", "center"),
        ("50%", "50%"),
        ("calc(50%)", "calc(50%)"),
        ("min(50%, 100%)", "calc(50%)"),
    ];
    let fallbacks = [
        None,
        Some("1px"),
        Some("50%"),
        Some("calc(50% + 1px)"),
        Some("anchor(left)"),
        Some("anchor(--bar left)"),
        Some("anchor(--bar left, anchor(--baz right))"),
    ];
    for &property in INSET_PROPERTIES {
        for name in ["", "--foo"] {
            for &(side, side_serialized) in sides {
                for fallback in fallbacks {
                    let tail = fallback.map_or(String::new(), |f| format!(", {f}"));
                    let named = |s: &str| {
                        if name.is_empty() {
                            format!("anchor({s}{tail})")
                        } else {
                            format!("anchor({name} {s}{tail})")
                        }
                    };
                    let value = named(side);
                    let expected = named(side_serialized);
                    // `min(50%, 100%)` may serialize either way (WPT accepts
                    // both); ours simplifies.
                    assert_valid(property, &value, &expected);
                    if !name.is_empty() {
                        // The name may follow the side.
                        assert_valid(property, &format!("anchor({side} {name}{tail})"), &expected);
                    }
                }
            }
        }
    }
    let top = "top";
    assert_valid(
        top,
        "calc((anchor(--foo top) + anchor(--bar bottom)) / 2)",
        "calc(0.5 * (anchor(--foo top) + anchor(--bar bottom)))",
    );
    assert_valid(
        top,
        "calc(0.5 * (anchor(--foo top) + anchor(--bar bottom)))",
        "calc(0.5 * (anchor(--foo top) + anchor(--bar bottom)))",
    );
    assert_valid(
        top,
        "anchor(--foo top, calc(0.5 * anchor(--bar bottom)))",
        "anchor(--foo top, calc(0.5 * anchor(--bar bottom)))",
    );
    assert_valid(
        top,
        "min(100px, 10%, anchor(--foo top), anchor(--bar bottom))",
        "min(100px, 10%, anchor(--foo top), anchor(--bar bottom))",
    );
    assert_valid(
        top,
        "calc(anchor(--foo left, 1px) + 10%)",
        "calc(10% + anchor(--foo left, 1px))",
    );
    assert_valid(top, "anchor(--foo left, 0)", "anchor(--foo left, 0px)");
    assert_valid(
        top,
        "calc(anchor(--foo left, 0))",
        "anchor(--foo left, 0px)",
    );
    // An anchor-size() fallback of anchor() is an inset value too.
    assert_valid(
        top,
        "anchor(--foo top, anchor-size(--bar height))",
        "anchor(--foo top, anchor-size(--bar height))",
    );
}

#[test]
fn anchor_reaches_the_inset_shorthand() {
    assert_parses("inset", "anchor(--a top) 0");
    assert_parses(
        "inset",
        "anchor(--a bottom) anchor(--a left) auto calc(anchor(right) + 1px)",
    );
    assert_eq!(
        serialized("top", "anchor(--a top) ; inset: anchor(--b bottom, 5px)"),
        "anchor(--b bottom, 5px)"
    );
    assert_parses("inset-inline", "anchor(--a start) anchor(--a end)");
}

#[test]
fn anchor_keeps_the_spec_grammar_per_wpt() {
    // Only in the inset properties.
    for (name, value) in [
        ("margin-top", "anchor(--foo top)"),
        ("height", "anchor(--foo top)"),
        ("width", "anchor(--a width)"),
        ("font-size", "anchor(--foo top)"),
        ("padding-top", "anchor(--foo top)"),
        ("translate", "anchor(--foo top)"),
        ("margin-top", "calc(anchor(--foo top) + 1px)"),
        ("height", "calc(anchor(--foo top) + 1px)"),
        ("margin", "anchor(--a top) 0"),
    ] {
        assert_rejects(name, value);
    }
    for value in [
        "anchor(--foo, top)",
        "anchor(--foo top,)",
        "anchor(--foo top bottom)",
        "anchor(--foo top, 10px 20%)",
        "anchor(--foo top, 10px, 20%)",
        "anchor(2 * 20%)",
        "anchor((2 * 20%))",
        "anchor(foo top)",
        "anchor(top foo)",
        "anchor(--foo height)",
        "anchor(--foo 10em)",
        "anchor(--foo 100s)",
        "anchor(--foo top, 1)",
        "anchor(--foo top, 100s)",
        "anchor(--foo top, bottom)",
        "anchor(--foo top, anchor(bar top))",
        "anchor(--foo top, anchor-size(bar height))",
        "anchor(--foo top, auto",
        "calc(anchor(foo top) + 10px + 10%)",
        "calc(10px + 100 * anchor(--foo top, anchor(bar bottom)))",
        "min(anchor(--foo top), anchor(--bar bottom), anchor-size(baz height))",
        "calc(anchor(--foo top, 1) + 1px)",
        // The withdrawn implicit keyword.
        "anchor(implicit top)",
        // `anchor-size()`'s fallback is a plain `<length-percentage>`.
        "anchor-size(--a height, anchor(--b top))",
        "calc(anchor-size(--a height, anchor(--b top)))",
    ] {
        for &name in INSET_PROPERTIES {
            assert_rejects(name, value);
        }
    }
}

#[test]
fn anchor_computes_to_itself() {
    // The computed value keeps the function: resolution needs layout.
    for (name, value, expected) in [
        ("top", "anchor(--a top, 10px)", "anchor(--a top, 10px)"),
        ("left", "anchor(outside)", "anchor(outside)"),
        ("right", "anchor(--a 25%)", "anchor(--a 25%)"),
        (
            "bottom",
            "calc(anchor(--a top) + 10%)",
            "calc(10% + anchor(--a top))",
        ),
        (
            "inset-inline-start",
            "anchor(--a end, anchor(--b start))",
            "anchor(--a end, anchor(--b start))",
        ),
    ] {
        assert_eq!(
            computed("", &format!("position: absolute; {name}: {value}"), name),
            expected,
            "`{name}: {value}`"
        );
    }
}

#[test]
fn anchor_function_accessors_serve_the_layout_engine() {
    use style::logical_geometry::PhysicalSide;
    use style::values::computed::position::{AnchorSide, Inset};
    use style::values::generics::position::{AnchorSideKeyword, GenericAnchorSide};
    use style::values::specified::box_::PositionProperty;
    let style = style_for("", "position: absolute; top: anchor(--a bottom, 4px)");
    let top = style.get_position().clone_top();
    let Inset::AnchorFunction(ref anchor) = top else {
        panic!("an anchor() inset, got {top:?}")
    };
    assert_eq!(anchor.target_element.value.0.to_string(), "--a");
    assert!(matches!(
        anchor.side,
        GenericAnchorSide::Keyword(AnchorSideKeyword::Bottom)
    ));
    assert!(anchor.fallback.is_some());
    // The matching-axis rule (§3.2): `bottom` is usable in the vertical
    // insets only, and only on an absolutely positioned box.
    assert!(anchor.valid_for(PhysicalSide::Top, PositionProperty::Absolute));
    assert!(!anchor.valid_for(PhysicalSide::Left, PositionProperty::Absolute));
    assert!(!anchor.valid_for(PhysicalSide::Top, PositionProperty::Relative));
    let side: AnchorSide = GenericAnchorSide::Keyword(AnchorSideKeyword::Center);
    let (keyword, percentage) = side.keyword_and_percentage();
    assert_eq!(keyword, AnchorSideKeyword::Start);
    assert_eq!(percentage.0, 0.5);
}

// ---------------------------------------------------------------------------
// §5.1 `anchor-size()` in the properties it lists (WPT
// anchor-size-parse-valid.html / -invalid.html).

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
fn anchor_size_fallbacks_nest_anchor_size_per_wpt() {
    // WPT anchor-size-parse-valid.html: the `<length-percentage>` fallback
    // may itself use anchor-size(), plain or in a math function, in every
    // property that accepts anchor-size().
    let sizes = [
        "width",
        "height",
        "block",
        "inline",
        "self-block",
        "self-inline",
    ];
    let fallbacks = [
        None,
        Some("1px"),
        Some("50%"),
        Some("calc(50% + 1px)"),
        Some("anchor-size(block)"),
        Some("anchor-size(--bar block)"),
        Some("anchor-size(--bar block, anchor-size(--baz inline))"),
    ];
    for &property in ANCHOR_SIZE_PROPERTIES {
        for name in ["", "--foo"] {
            for size in sizes {
                for fallback in fallbacks {
                    let tail = fallback.map_or(String::new(), |f| format!(", {f}"));
                    let value = if name.is_empty() {
                        format!("anchor-size({size}{tail})")
                    } else {
                        format!("anchor-size({name} {size}{tail})")
                    };
                    assert_valid(property, &value, &value);
                    if !name.is_empty() {
                        assert_valid(
                            property,
                            &format!("anchor-size({size} {name}{tail})"),
                            &value,
                        );
                    }
                }
            }
        }
        for (value, expected) in [
            (
                "anchor-size(--foo width, calc(0.5 * anchor-size(--bar height)))",
                "anchor-size(--foo width, calc(0.5 * anchor-size(--bar height)))",
            ),
            (
                "anchor-size(--a width, anchor-size(--b width, 10px))",
                "anchor-size(--a width, anchor-size(--b width, 10px))",
            ),
            (
                "anchor-size(--a, anchor-size(--b, anchor-size(--c, 1px)))",
                "anchor-size(--a, anchor-size(--b, anchor-size(--c, 1px)))",
            ),
            (
                "anchor-size(--a, min(10px, anchor-size(--b height)))",
                "anchor-size(--a, min(10px, anchor-size(--b height)))",
            ),
            (
                "calc(anchor-size(--a width, anchor-size(--b width)) + 1px)",
                "calc(anchor-size(--a width, anchor-size(--b width)) + 1px)",
            ),
        ] {
            assert_valid(property, value, expected);
        }
        // The nested function's own fallback keeps the same grammar.
        for value in [
            "anchor-size(--a, anchor-size(--b, auto))",
            "anchor-size(--a, anchor-size(--b, none))",
            "anchor-size(--a, anchor-size(b))",
            "anchor-size(--a, anchor-size(--b top))",
            "anchor-size(--a, anchor-size(--b, anchor(--c top)))",
            "anchor-size(--a, calc(anchor(--c top)))",
        ] {
            assert_rejects(property, value);
        }
    }
}

#[test]
fn nested_anchor_size_fallbacks_compute_to_themselves() {
    for (name, value) in [
        (
            "width",
            "anchor-size(--a width, anchor-size(--b width, 10px))",
        ),
        ("max-height", "anchor-size(--a, anchor-size(--b height))"),
        (
            "min-width",
            "anchor-size(--a, calc(0.5 * anchor-size(--b width)))",
        ),
        (
            "margin-left",
            "anchor-size(--a width, anchor-size(--b width))",
        ),
        (
            "top",
            "anchor-size(--a height, anchor-size(--b height, 5%))",
        ),
    ] {
        assert_eq!(
            computed("", &format!("position: absolute; {name}: {value}"), name),
            value,
            "`{name}: {value}`"
        );
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
    // WPT anchor-size-parse-invalid.html: anchor() is not a size.
    assert_rejects("width", "anchor-size(--foo width, anchor(--bar top))");
    assert_rejects(
        "width",
        "min(anchor-size(--foo width), anchor-size(--bar height), anchor(--baz top))",
    );
}

#[test]
fn anchor_size_reaches_the_margin_and_inset_shorthands() {
    assert_parses("margin", "anchor-size(--a height) 0");
    assert_parses("margin", "calc(anchor-size(width) / 2) auto");
    assert_rejects("padding", "anchor-size(--a height)");
    assert_parses("inset", "anchor-size(--a height) auto auto 0");
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
fn anchor_functions_substitute_through_var() {
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
    assert_eq!(
        computed(
            "",
            "position: absolute; --edge: anchor(--a bottom); top: var(--edge)",
            "top"
        ),
        "anchor(--a bottom)"
    );
    // After substitution anchor() is still invalid outside the insets: the
    // declaration is invalid at computed-value time (`margin-top` → 0px).
    assert_eq!(
        computed(
            "",
            "--edge: anchor(--a bottom); margin-top: var(--edge)",
            "margin-top"
        ),
        "0px"
    );
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

// ---------------------------------------------------------------------------
// §4.2 `anchor-center`

#[test]
fn anchor_center_is_a_self_alignment_keyword() {
    for name in ["justify-self", "align-self"] {
        assert_valid(name, "anchor-center", "anchor-center");
        assert_eq!(
            computed("", &format!("{name}: anchor-center"), name),
            "anchor-center"
        );
    }
    assert_valid("place-self", "anchor-center", "anchor-center");
    assert_valid("place-self", "anchor-center center", "anchor-center center");
    assert_valid("place-self", "start anchor-center", "start anchor-center");
    assert_eq!(serialized("justify-self", "anchor-center"), "anchor-center");
    // Not on the `*-items` properties (the Editor's Draft dropped them), nor
    // on the content-distribution properties.
    for name in [
        "justify-items",
        "align-items",
        "justify-content",
        "align-content",
    ] {
        assert_rejects(name, "anchor-center");
    }
    assert_rejects("place-items", "anchor-center");
    // No `safe` / `unsafe` prefix in the Lynx alignment grammar.
    assert_rejects("align-self", "unsafe anchor-center");
}

// ---------------------------------------------------------------------------
// §6.1–6.3 `position-try-fallbacks`, `position-try-order`, `position-try`
// (WPT parsing/position-try-*.html)

#[test]
fn position_try_fallbacks_parses_per_wpt() {
    let p = "position-try-fallbacks";
    for (value, expected) in [
        ("initial", "initial"),
        ("inherit", "inherit"),
        ("unset", "unset"),
        ("revert", "revert"),
        ("none", "none"),
        ("flip-block", "flip-block"),
        ("flip-block ", "flip-block"),
        ("flip-start, flip-block", "flip-start, flip-block"),
        (
            "flip-start flip-inline, flip-block",
            "flip-start flip-inline, flip-block",
        ),
        ("flip-start, flip-start", "flip-start, flip-start"),
        (
            "flip-start flip-inline flip-block",
            "flip-start flip-inline flip-block",
        ),
        ("flip-block, --foo", "flip-block, --foo"),
        (
            "--bar, flip-block flip-start",
            "--bar, flip-block flip-start",
        ),
        ("--foo, --bar, --baz", "--foo, --bar, --baz"),
        ("--bar flip-block", "--bar flip-block"),
        (
            "--bar flip-inline flip-block",
            "--bar flip-inline flip-block",
        ),
        ("flip-inline --foo", "--foo flip-inline"),
        (
            "flip-inline flip-start --foo",
            "--foo flip-inline flip-start",
        ),
        ("left top", "left top"),
        ("top left", "left top"),
        ("start start", "start"),
        ("left, right", "left, right"),
        ("--foo, left", "--foo, left"),
        ("--foo, left, --bar", "--foo, left, --bar"),
        ("--foo, flip-start, left", "--foo, flip-start, left"),
        ("--foo flip-start, left", "--foo flip-start, left"),
        ("left, --bar flip-start", "left, --bar flip-start"),
        ("flip-x", "flip-x"),
        ("flip-x ", "flip-x"),
        ("flip-start, flip-x", "flip-start, flip-x"),
        ("flip-start flip-y, flip-x", "flip-start flip-y, flip-x"),
        ("flip-start flip-y flip-x", "flip-start flip-y flip-x"),
        ("flip-x, --foo", "flip-x, --foo"),
        ("--bar, flip-x flip-start", "--bar, flip-x flip-start"),
        ("--bar flip-x", "--bar flip-x"),
        ("--bar flip-y flip-x", "--bar flip-y flip-x"),
        ("flip-y --foo", "--foo flip-y"),
        ("flip-y flip-start --foo", "--foo flip-y flip-start"),
        (
            "flip-start flip-inline flip-y flip-block flip-x",
            "flip-start flip-inline flip-y flip-block flip-x",
        ),
    ] {
        assert_valid(p, value, expected);
    }
    for value in [
        "none, flip-start",
        "flip-block flip-block",
        "flip-block flip-inline flip-inline",
        "flip-block, flip-inline flip-inline",
        "--bar flip-block --foo",
        "--foo --bar",
        "flip-inline --bar flip-block",
        "-foo",
        "foo",
        "flip-start 123",
        "--foo 123",
        "--foo left",
        "flip-start left",
        "left --foo ",
        "left flip-start",
        "--foo, none",
        "none, flip-x",
        "flip-y flip-y",
    ] {
        assert_rejects(p, value);
    }
    // The withdrawn spelling.
    assert!(!is_author_property("position-try-options"));
}

#[test]
fn position_try_fallbacks_computes_per_wpt() {
    let p = "position-try-fallbacks";
    for (value, expected) in [
        ("none", "none"),
        ("flip-block", "flip-block"),
        ("flip-inline", "flip-inline"),
        ("flip-start", "flip-start"),
        ("flip-block, flip-inline", "flip-block, flip-inline"),
        ("--foo, --bar", "--foo, --bar"),
        (
            "flip-start flip-inline flip-block",
            "flip-start flip-inline flip-block",
        ),
        ("flip-start --flop", "--flop flip-start"),
        ("--flop flip-start", "--flop flip-start"),
        ("left top", "left top"),
        ("top left", "left top"),
        ("start start", "start"),
        ("left, right", "left, right"),
        ("--foo, left", "--foo, left"),
        ("--foo, left, --bar", "--foo, left, --bar"),
        ("--foo, flip-start, left", "--foo, flip-start, left"),
        ("--foo flip-start, left", "--foo flip-start, left"),
        ("left, --bar flip-start", "left, --bar flip-start"),
        ("flip-y", "flip-y"),
        ("flip-x", "flip-x"),
        ("flip-y, flip-x", "flip-y, flip-x"),
        ("flip-start flip-x flip-y", "flip-start flip-x flip-y"),
    ] {
        assert_eq!(
            computed("", &format!("{p}: {value}"), p),
            expected,
            "{value}"
        );
    }
    let fallbacks: PositionTryFallbacks =
        style_for("", "position-try-fallbacks: --a, flip-y").clone_position_try_fallbacks();
    assert_eq!(fallbacks.value.0.len(), 2);
    assert_eq!(fallbacks.scope, CascadeLevel::same_tree_author_normal());
}

#[test]
fn position_try_order_parses_and_computes_per_wpt() {
    let p = "position-try-order";
    for value in [
        "initial",
        "inherit",
        "unset",
        "revert",
        "normal",
        "most-width",
        "most-height",
        "most-block-size",
        "most-inline-size",
    ] {
        assert_valid(p, value, value);
    }
    for value in [
        "normal most-inline-size",
        "most-block-size most-inline-size",
        "most-block-size, most-inline-size",
    ] {
        assert_rejects(p, value);
    }
    for value in [
        "normal",
        "most-width",
        "most-height",
        "most-block-size",
        "most-inline-size",
    ] {
        assert_eq!(computed("", &format!("{p}: {value}"), p), value);
    }
    assert_eq!(computed("", "", p), "normal");
}

#[test]
fn position_try_shorthand_per_wpt() {
    let p = "position-try";
    for (value, expected) in [
        ("flip-inline", "flip-inline"),
        ("flip-y", "flip-y"),
        ("most-height none", "most-height none"),
        ("--bar, --baz", "--bar, --baz"),
        (
            "most-inline-size --baz, flip-inline",
            "most-inline-size --baz, flip-inline",
        ),
        (
            "most-block-size flip-inline flip-block, --bar, --baz",
            "most-block-size flip-inline flip-block, --bar, --baz",
        ),
        (
            "most-width flip-y flip-x, --foo, --bar",
            "most-width flip-y flip-x, --foo, --bar",
        ),
        ("normal none", "none"),
        ("most-width none", "most-width none"),
        ("normal --foo", "--foo"),
    ] {
        assert_valid(p, value, expected);
    }
    for value in [
        "normal --foo, most-width --bar",
        "none normal",
        "flip-block most-height",
        "most-height, flip-start",
    ] {
        assert_rejects(p, value);
    }
    for (value, order, fallbacks) in [
        ("flip-inline", "normal", "flip-inline"),
        ("flip-y", "normal", "flip-y"),
        ("most-width none", "most-width", "none"),
        ("--foo, --bar", "normal", "--foo, --bar"),
        (
            "most-inline-size --foo, flip-inline",
            "most-inline-size",
            "--foo, flip-inline",
        ),
        (
            "most-inline-size flip-inline flip-block, --foo, --bar",
            "most-inline-size",
            "flip-inline flip-block, --foo, --bar",
        ),
        (
            "most-width flip-y flip-x, --foo, --bar",
            "most-width",
            "flip-y flip-x, --foo, --bar",
        ),
    ] {
        let block = format!("{p}: {value}");
        assert_eq!(longhand_of(&block, "position-try-order"), order, "{value}");
        assert_eq!(
            longhand_of(&block, "position-try-fallbacks"),
            fallbacks,
            "{value}"
        );
    }
    // WPT parsing/position-try-computed.html
    for (value, expected) in [
        ("none", "none"),
        ("normal none", "none"),
        ("flip-block", "flip-block"),
        ("flip-y", "flip-y"),
        ("most-width none", "most-width none"),
        (
            "most-height flip-block, flip-inline",
            "most-height flip-block, flip-inline",
        ),
        ("most-height flip-y, flip-x", "most-height flip-y, flip-x"),
        ("most-width --foo, --bar", "most-width --foo, --bar"),
        ("normal --foo", "--foo"),
    ] {
        let style = style_for("", &format!("{p}: {value}"));
        let order = value_of(&style, "position-try-order");
        let fallbacks = computed("", &format!("{p}: {value}"), "position-try-fallbacks");
        let shorthand = if order == "normal" {
            fallbacks
        } else {
            format!("{order} {fallbacks}")
        };
        assert_eq!(shorthand, expected, "{value}");
    }
}

// ---------------------------------------------------------------------------
// §6.6 `position-visibility` (WPT parsing/position-visibility-*.html, in the
// Editor's Draft spelling: `anchors-valid` / `anchors-visible` are legacy
// aliases of `anchor-valid` / `anchor-visible` and serialize as those).

#[test]
fn position_visibility_parses_per_the_editors_draft() {
    let p = "position-visibility";
    for value in ["initial", "inherit", "unset", "revert"] {
        assert_valid(p, value, value);
    }
    for (value, expected) in [
        ("always", "always"),
        ("anchor-valid", "anchor-valid"),
        ("anchor-visible", "anchor-visible"),
        ("no-overflow", "no-overflow"),
        ("anchor-valid anchor-visible", "anchor-valid anchor-visible"),
        ("anchor-valid no-overflow", "anchor-valid no-overflow"),
        ("anchor-visible anchor-valid", "anchor-valid anchor-visible"),
        ("anchor-visible no-overflow", "anchor-visible no-overflow"),
        ("no-overflow anchor-valid", "anchor-valid no-overflow"),
        ("no-overflow anchor-visible", "anchor-visible no-overflow"),
        (
            "anchor-valid anchor-visible no-overflow",
            "anchor-valid anchor-visible no-overflow",
        ),
        (
            "anchor-valid no-overflow anchor-visible",
            "anchor-valid anchor-visible no-overflow",
        ),
        (
            "anchor-visible anchor-valid no-overflow",
            "anchor-valid anchor-visible no-overflow",
        ),
        (
            "anchor-visible no-overflow anchor-valid",
            "anchor-valid anchor-visible no-overflow",
        ),
        (
            "no-overflow anchor-valid anchor-visible",
            "anchor-valid anchor-visible no-overflow",
        ),
        (
            "no-overflow anchor-visible anchor-valid",
            "anchor-valid anchor-visible no-overflow",
        ),
        // The legacy aliases.
        ("anchors-valid", "anchor-valid"),
        ("anchors-visible", "anchor-visible"),
        (
            "anchors-visible anchors-valid",
            "anchor-valid anchor-visible",
        ),
        ("no-overflow anchors-valid", "anchor-valid no-overflow"),
        ("ANCHORS-VISIBLE", "anchor-visible"),
    ] {
        assert_valid(p, value, expected);
        assert_eq!(
            computed("", &format!("{p}: {value}"), p),
            expected,
            "{value}"
        );
    }
    for value in [
        "foobar",
        "always foobar",
        "always anchor-valid",
        "always anchor-visible",
        "always no-overflow",
        "always anchor-valid no-overflow",
        "always anchor-valid anchor-visible no-overflow",
        "anchor-valid always",
        "no-overflow foobar",
        "anchor-valid no-overflow foobar",
        "anchor-valid no-overflow anchor-valid anchor-visible",
        // An alias and its current spelling are the same flag.
        "anchor-valid anchors-valid",
        "anchors-visible anchor-visible",
        "",
    ] {
        assert_rejects(p, value);
    }
}

#[test]
fn position_visibility_initial_is_anchor_visible() {
    assert_eq!(computed("", "", "position-visibility"), "anchor-visible");
    assert_eq!(
        computed("", "position-visibility: initial", "position-visibility"),
        "anchor-visible"
    );
    let initial = style_for("", "").clone_position_visibility();
    assert_eq!(initial, PositionVisibility::ANCHOR_VISIBLE);
    assert_eq!(PositionVisibility::default(), initial);
    assert_eq!(PositionVisibility::always(), PositionVisibility::ALWAYS);
    let flags =
        style_for("", "position-visibility: no-overflow anchor-valid").clone_position_visibility();
    assert!(flags.contains(PositionVisibility::ANCHOR_VALID));
    assert!(flags.contains(PositionVisibility::NO_OVERFLOW));
    assert!(!flags.contains(PositionVisibility::ANCHOR_VISIBLE));
    assert!(style_for("", "position-visibility: always")
        .clone_position_visibility()
        .is_empty());
}

// ---------------------------------------------------------------------------
// §6.4 `@position-try` (WPT at-position-try-parse.html,
// at-position-try-allowed-declarations.html)

/// `@position-try` rules of `css`, serialized (the first rule's text for
/// `rule_text`).
fn position_try_rules(css: &str) -> Vec<String> {
    let lock = SharedRwLock::new();
    let sheet = stylesheet(&lock, css);
    let guard = lock.read();
    sheet
        .contents(&guard)
        .rules(&guard)
        .iter()
        .filter_map(|rule| match rule {
            CssRule::PositionTry(rule) => {
                let mut out = String::new();
                rule.read_with(&guard).to_css(&guard, &mut out).unwrap();
                Some(out)
            },
            _ => None,
        })
        .collect()
}

#[test]
fn position_try_rule_parses_per_wpt() {
    assert_eq!(
        position_try_rules("@position-try --foo { }"),
        ["@position-try --foo { }"]
    );
    assert_eq!(
        position_try_rules("@position-try --foo { top: 1px; }"),
        ["@position-try --foo { top: 1px; }"]
    );
    for invalid in [
        "@position-try { }",
        "@position-try foo { }",
        "@position-try --foo --bar { }",
        "@position-try --foo, --bar { }",
    ] {
        assert!(position_try_rules(invalid).is_empty(), "{invalid}");
    }
    for ignored in [
        "@position-try --foo { backround-color: green; }",
        "@position-try --foo { @keyframes bar {} }",
        "@position-try --foo { @font-face {} }",
        "@position-try --foo { @media print {} }",
        "@position-try --foo { & {} }",
        "@position-try --foo { arbitrary garbage }",
    ] {
        assert_eq!(
            position_try_rules(ignored),
            ["@position-try --foo { }"],
            "{ignored}"
        );
    }
    // The withdrawn at-rules.
    assert!(position_try_rules("@position-fallback --foo { @try { top: 0 } }").is_empty());
}

fn position_try_text(declaration: &str) -> String {
    let rules = position_try_rules(&format!("@position-try --foo {{ {declaration}; }}"));
    assert_eq!(rules.len(), 1, "{declaration}");
    rules.into_iter().next().unwrap()
}

#[test]
fn position_try_accepts_exactly_the_listed_properties() {
    // WPT's allowed list, on the Lynx surface.
    for (property, value) in [
        ("top", "1px"),
        ("bottom", "1px"),
        ("left", "1px"),
        ("right", "1px"),
        ("inset-inline-start", "1px"),
        ("inset-inline-end", "1px"),
        ("inset-inline", "1px"),
        ("inset", "1px"),
        ("position-area", "span-all"),
        ("margin-top", "1px"),
        ("margin-bottom", "1px"),
        ("margin-left", "1px"),
        ("margin-right", "1px"),
        ("margin-inline-start", "1px"),
        ("margin-inline-end", "1px"),
        ("margin-inline", "1px"),
        ("margin", "1px"),
        ("width", "1px"),
        ("height", "1px"),
        ("min-width", "1px"),
        ("min-height", "1px"),
        ("max-width", "1px"),
        ("max-height", "1px"),
        ("justify-self", "center"),
        ("align-self", "center"),
        ("place-self", "center"),
        ("justify-self", "anchor-center"),
        ("position-anchor", "--anchor"),
        ("top", "anchor(--a bottom)"),
        ("width", "anchor-size(--a width)"),
        ("top", "revert"),
        ("top", "revert-layer"),
        ("inset", "revert"),
        ("inset", "revert-layer"),
    ] {
        assert_eq!(
            position_try_text(&format!("{property}: {value}")),
            format!("@position-try --foo {{ {property}: {value}; }}"),
            "{property}: {value}"
        );
    }
    // WPT also allows these, but they are not Lynx properties at all, so they
    // are dropped as unknown (as in a style rule).
    for property in [
        "inset-block-start",
        "inset-block-end",
        "inset-block",
        "margin-block-start",
        "margin-block-end",
        "margin-block",
        "block-size",
        "inline-size",
        "min-block-size",
        "min-inline-size",
        "max-block-size",
        "max-inline-size",
    ] {
        assert!(!is_author_property(property), "{property}");
        assert_eq!(
            position_try_text(&format!("{property}: 1px")),
            "@position-try --foo { }"
        );
    }
    // Disallowed: custom properties, everything else, and !important (which
    // drops only that declaration).
    for (property, value) in [
        ("--custom", "1px"),
        ("font-size", "1px"),
        ("border-width", "1px"),
        ("padding", "1px"),
        ("padding-top", "1px"),
        ("padding-inline", "1px"),
        ("display", "flex"),
        ("position", "absolute"),
        ("justify-content", "center"),
        ("align-content", "center"),
        ("align-items", "center"),
        ("position-try-fallbacks", "flip-block"),
        ("position-try", "flip-block"),
        ("position-visibility", "always"),
        ("anchor-name", "--a"),
        ("anchor-scope", "all"),
        ("opacity", "0.5"),
        ("top", "1px !important"),
        ("inset", "1px !important"),
    ] {
        assert_eq!(
            position_try_text(&format!("{property}: {value}")),
            "@position-try --foo { }",
            "{property}: {value}"
        );
    }
    assert_eq!(
        position_try_text("top: 1px !important; left: 2px"),
        "@position-try --foo { left: 2px; }"
    );
}

// ---------------------------------------------------------------------------
// `Stylist::resolve_position_try` and the §6.5.2 try tactics, on a detached
// element (support/test_element.rs).

#[test]
fn resolve_position_try_applies_the_named_rule_in_the_fallback_origin() {
    let sheet = "
        @position-try --below { top: anchor(--a bottom); bottom: auto; margin-top: 4px }
        @position-try --empty { }
    ";
    let base = "position: absolute; top: 1px; bottom: 2px; left: 3px; margin-top: 0px; \
                color: red; position-try-fallbacks: --below, --missing, --empty";
    let env = Env::new(sheet);
    let style = env.style(base);
    let option = env
        .resolve(&style, "--below")
        .expect("a known @position-try rule");
    assert_eq!(value_of(&option, "top"), "anchor(--a bottom)");
    assert_eq!(value_of(&option, "bottom"), "auto");
    assert_eq!(value_of(&option, "margin-top"), "4px");
    // Everything else comes from the base styles.
    assert_eq!(value_of(&option, "left"), "3px");
    assert_eq!(value_of(&option, "position"), "absolute");
    assert_eq!(
        value_of(&option, "position-try-fallbacks"),
        "--below, --missing, --empty"
    );
    // §6.1: an unknown name adds no option.
    assert!(env.resolve(&style, "--missing").is_none());
    // An empty rule is an option equal to the base.
    let empty = env.resolve(&style, "--empty").unwrap();
    for property in ["top", "bottom", "left", "margin-top"] {
        assert_eq!(value_of(&empty, property), value_of(&style, property));
    }
}

#[test]
fn position_try_rules_cascade_by_name_like_keyframes() {
    let env = Env::new(
        "
        @position-try --a { top: 1px; left: 1px }
        @position-try --a { top: 2px }
        ",
    );
    let style = env.style("position: absolute; left: 9px");
    let option = env.resolve(&style, "--a").unwrap();
    // The later rule replaces the earlier one as a whole (no merge).
    assert_eq!(value_of(&option, "top"), "2px");
    assert_eq!(value_of(&option, "left"), "9px");
}

#[test]
fn position_try_rules_follow_cascade_layers() {
    // WPT position-try-cascade-layer-reorder.html: the rule in the later
    // layer wins regardless of source order.
    let env = Env::new(
        "
        @layer a, b;
        @layer b { @position-try --x { top: 1px } }
        @layer a { @position-try --x { top: 2px } }
        ",
    );
    let style = env.style("position: absolute");
    assert_eq!(value_of(&env.resolve(&style, "--x").unwrap(), "top"), "1px");
}

#[test]
fn a_flush_reports_the_changed_position_try_names() {
    // The layout engine re-resolves the options of the elements whose
    // `position-try-fallbacks` names a changed rule (§6.5.1).
    let lock = SharedRwLock::new();
    let mut stylist = Stylist::new(device(), QuirksMode::NoQuirks);
    let first = DocumentStyleSheet(Arc::new(stylesheet(
        &lock,
        "@position-try --a { top: 1px } @position-try --b { top: 1px }",
    )));
    let second = DocumentStyleSheet(Arc::new(stylesheet(
        &lock,
        "@position-try --b { top: 2px }",
    )));
    let guard = lock.read();
    let guards = StylesheetGuards::same(&guard);
    stylist.append_stylesheet(first, &guard);
    let added = stylist.flush(&guards);
    let mut names: Vec<_> = added
        .cascade_data_difference
        .changed_position_try_names
        .iter()
        .map(|n| n.to_string())
        .collect();
    names.sort();
    assert_eq!(names, ["--a", "--b"]);
    stylist.append_stylesheet(second, &guard);
    let changed = stylist.flush(&guards);
    let names: Vec<_> = changed
        .cascade_data_difference
        .changed_position_try_names
        .iter()
        .map(|n| n.to_string())
        .collect();
    assert_eq!(names, ["--b"]);
}

#[test]
fn position_fallback_origin_sits_above_author_normal_and_reverts_to_user() {
    let env = Env::new(
        "
        @position-try --revert { top: revert; left: revert-layer }
        @position-try --win { top: 5px }
        ",
    );
    let style = env.style("position: absolute; top: 1px; left: 2px");
    assert_eq!(
        value_of(&env.resolve(&style, "--win").unwrap(), "top"),
        "5px"
    );
    // `revert` reverts to the user origin (as for the animation origin): the
    // author's `top: 1px` is skipped, leaving the initial `auto`.
    let reverted = env.resolve(&style, "--revert").unwrap();
    assert_eq!(value_of(&reverted, "top"), "auto");
    // `revert-layer` has no special behavior: it rolls back to the author.
    assert_eq!(value_of(&reverted, "left"), "2px");
}

#[test]
fn position_area_items_replace_only_position_area() {
    let env = Env::new("");
    let style = env.style("position: absolute; position-area: top left; top: 1px");
    let option = env.resolve(&style, "bottom span-x-end").unwrap();
    assert_eq!(value_of(&option, "position-area"), "span-x-end bottom");
    assert_eq!(value_of(&option, "top"), "1px");
}

/// The value of `property` in `--pf <tactic>` with `untransformed` in the
/// rule; the WPT try-tactic-* harness, at computed-value level.
fn flipped(tactic: &str, untransformed: &str, property: &str) -> String {
    let env = Env::new(&format!(
        "@position-try --pf {{ inset: auto; {untransformed} }}"
    ));
    let style = env.style("position: absolute");
    let option = env
        .resolve(&style, &format!("--pf {tactic}"))
        .expect("--pf exists");
    value_of(&option, property)
}

/// Asserts `untransformed` under `tactic` computes like `transformed`
/// without one, for the four insets.
fn assert_flip(tactic: &str, untransformed: &str, transformed: &str) {
    let env = Env::new(&format!(
        "@position-try --pf {{ inset: auto; {untransformed} }}
         @position-try --ref {{ inset: auto; {transformed} }}"
    ));
    let style = env.style("position: absolute");
    let target = env.resolve(&style, &format!("--pf {tactic}")).unwrap();
    let reference = env.resolve(&style, "--ref").unwrap();
    for property in ["top", "right", "bottom", "left"] {
        assert_eq!(
            value_of(&target, property),
            value_of(&reference, property),
            "{tactic}: `{untransformed}` must compute as `{transformed}` ({property})"
        );
    }
}

#[test]
fn try_tactics_swap_insets() {
    // WPT try-tactic-basic.html, at computed-value level.
    let base = "top: 20px; left: 10px; width: 30px; height: 40px";
    for (tactic, expected) in [
        ("flip-block", "bottom: 20px; left: 10px"),
        ("flip-y", "bottom: 20px; left: 10px"),
        ("flip-inline", "top: 20px; right: 10px"),
        ("flip-x", "top: 20px; right: 10px"),
        ("flip-block flip-inline", "bottom: 20px; right: 10px"),
        ("flip-y flip-x", "bottom: 20px; right: 10px"),
        (
            "flip-start",
            "left: 20px; top: 10px; width: 40px; height: 30px",
        ),
        (
            "flip-block flip-start flip-inline",
            "left: 20px; top: 10px; width: 40px; height: 30px",
        ),
        (
            "flip-start flip-block",
            "left: 20px; bottom: 10px; width: 40px; height: 30px",
        ),
        (
            "flip-inline flip-start",
            "left: 20px; bottom: 10px; width: 40px; height: 30px",
        ),
        (
            "flip-start flip-inline",
            "right: 20px; top: 10px; width: 40px; height: 30px",
        ),
        (
            "flip-block flip-start",
            "right: 20px; top: 10px; width: 40px; height: 30px",
        ),
        (
            "flip-x flip-y flip-start",
            "right: 20px; bottom: 10px; width: 40px; height: 30px",
        ),
    ] {
        assert_flip(tactic, base, expected);
        let expected_width = if expected.contains("width") {
            "40px"
        } else {
            "30px"
        };
        assert_eq!(flipped(tactic, base, "width"), expected_width, "{tactic}");
    }
}

#[test]
fn try_tactics_rewrite_anchor_sides_and_percentages() {
    // WPT try-tactic-percentage.html
    for (tactic, from, to) in [
        ("flip-inline", "left:anchor(10%)", "right:anchor(90%)"),
        (
            "flip-inline",
            "left:anchor(calc(10% + 20%))",
            "right:anchor(70%)",
        ),
        ("flip-block", "left:anchor(0%)", "left:anchor(0%)"),
        ("flip-block", "left:anchor(100%)", "left:anchor(100%)"),
        ("flip-block", "top:anchor(0%)", "bottom:anchor(100%)"),
        ("flip-block", "top:anchor(100%)", "bottom:anchor(0%)"),
        ("flip-inline", "left:anchor(0%)", "right:anchor(100%)"),
        ("flip-inline", "left:anchor(100%)", "right:anchor(0%)"),
        ("flip-inline", "top:anchor(0%)", "top:anchor(0%)"),
        ("flip-inline", "top:anchor(100%)", "top:anchor(100%)"),
        (
            "flip-block flip-inline",
            "left:anchor(0%)",
            "right:anchor(100%)",
        ),
        (
            "flip-block flip-inline",
            "top:anchor(100%)",
            "bottom:anchor(0%)",
        ),
        ("flip-start", "left:anchor(0%)", "top:anchor(0%)"),
        ("flip-start", "left:anchor(100%)", "top:anchor(100%)"),
        ("flip-start", "bottom:anchor(0%)", "right:anchor(0%)"),
        ("flip-start", "bottom:anchor(100%)", "right:anchor(100%)"),
        ("flip-block flip-start", "left:anchor(0%)", "top:anchor(0%)"),
        (
            "flip-block flip-start",
            "bottom:anchor(0%)",
            "left:anchor(100%)",
        ),
        (
            "flip-inline flip-start",
            "left:anchor(0%)",
            "bottom:anchor(100%)",
        ),
        (
            "flip-inline flip-start",
            "bottom:anchor(100%)",
            "right:anchor(100%)",
        ),
        (
            "flip-block flip-inline flip-start",
            "left:anchor(0%)",
            "bottom:anchor(100%)",
        ),
        (
            "flip-block flip-inline flip-start",
            "bottom:anchor(100%)",
            "left:anchor(0%)",
        ),
    ] {
        assert_flip(tactic, from, to);
    }
    // WPT try-tactic-anchor.html, physical sides.
    for (tactic, from, to) in [
        ("", "right:anchor(left)", "right:anchor(left)"),
        ("flip-block", "right:anchor(left)", "right:anchor(left)"),
        ("flip-inline", "bottom:anchor(top)", "bottom:anchor(top)"),
        ("flip-inline", "right:anchor(left)", "left:anchor(right)"),
        ("flip-inline", "left:anchor(right)", "right:anchor(left)"),
        ("flip-start", "right:anchor(left)", "bottom:anchor(top)"),
        ("flip-start", "bottom:anchor(top)", "right:anchor(left)"),
        (
            "flip-inline flip-start",
            "right:anchor(left)",
            "top:anchor(bottom)",
        ),
        (
            "flip-start flip-inline",
            "top:anchor(bottom)",
            "right:anchor(left)",
        ),
        (
            "flip-start flip-block",
            "left:anchor(right)",
            "bottom:anchor(top)",
        ),
        (
            "flip-block flip-start",
            "bottom:anchor(top)",
            "left:anchor(right)",
        ),
        ("flip-start", "left:anchor(right)", "top:anchor(bottom)"),
        ("flip-start", "top:anchor(bottom)", "left:anchor(right)"),
        ("flip-block", "bottom:anchor(top)", "top:anchor(bottom)"),
        ("flip-block", "top:anchor(bottom)", "bottom:anchor(top)"),
    ] {
        assert_flip(tactic, from, to);
    }
    // WPT's logical-side cases compare layouts (`right:anchor(start)` under
    // flip-inline lays out as `left:anchor(right)`). At computed-value level
    // a logical side keeps its keyword across a perpendicular swap (it
    // resolves against the new property's axis) and flips start/end across
    // an opposing one; in horizontal-tb ltr the layout engine then resolves
    // `start`/`end` to left/right or top/bottom.
    for (tactic, from, to) in [
        ("flip-inline", "right:anchor(start)", "left:anchor(end)"),
        ("flip-inline", "left:anchor(end)", "right:anchor(start)"),
        ("flip-start", "right:anchor(start)", "bottom:anchor(start)"),
        ("flip-start", "bottom:anchor(start)", "right:anchor(start)"),
        (
            "flip-inline flip-start",
            "right:anchor(start)",
            "top:anchor(end)",
        ),
        (
            "flip-start flip-inline",
            "top:anchor(end)",
            "right:anchor(start)",
        ),
        (
            "flip-block",
            "bottom:anchor(self-start)",
            "top:anchor(self-end)",
        ),
        (
            "flip-block",
            "top:anchor(self-end)",
            "bottom:anchor(self-start)",
        ),
        (
            "flip-start",
            "left:anchor(self-end)",
            "top:anchor(self-end)",
        ),
        ("flip-block", "top:anchor(inside)", "bottom:anchor(inside)"),
        ("flip-start", "top:anchor(outside)", "left:anchor(outside)"),
        ("flip-block", "top:anchor(center)", "bottom:anchor(center)"),
    ] {
        assert_flip(tactic, from, to);
    }
    // anchor() inside math functions and as a fallback.
    assert_flip(
        "flip-block",
        "top: calc(anchor(--a top) + 10px)",
        "bottom: calc(anchor(--a bottom) + 10px)",
    );
    assert_flip(
        "flip-start",
        "top: anchor(--a bottom, anchor(--b top))",
        "left: anchor(--a right, anchor(--b left))",
    );
}

#[test]
fn try_tactics_mirror_plain_percentages_without_rewriting_them() {
    // §6.5.2 rewrites only the anchor() side: `top: 20%` mirrors to
    // `bottom: 20%` (upstream stylo also turned it into `bottom: 80%`, and
    // a margin's `10%` into `90%`).
    assert_flip("flip-block", "top: 20%", "bottom: 20%");
    assert_flip(
        "flip-inline",
        "left: calc(10% + 5px)",
        "right: calc(10% + 5px)",
    );
    assert_flip(
        "flip-block",
        "top: anchor(--a 25%, 10%)",
        "bottom: anchor(--a 75%, 10%)",
    );
    assert_flip(
        "flip-block",
        "top: calc(anchor(--a 25%) + 10%)",
        "bottom: calc(anchor(--a 75%) + 10%)",
    );
    assert_eq!(
        flipped("flip-block", "margin-top: 10%", "margin-bottom"),
        "10%"
    );
    assert_eq!(
        flipped("flip-block", "margin-top: 10%", "margin-top"),
        "0px"
    );
    assert_eq!(flipped("flip-start", "width: 50%", "height"), "50%");
}

#[test]
fn try_tactics_swap_margins_and_sizes_and_anchor_size_axes() {
    // WPT try-tactic-margin.html / try-tactic-sizing.html, computed.
    assert_eq!(
        flipped("flip-block", "margin-top: 5px", "margin-bottom"),
        "5px"
    );
    assert_eq!(
        flipped("flip-inline", "margin-left: 5px", "margin-right"),
        "5px"
    );
    assert_eq!(
        flipped("flip-start", "margin-left: 5px", "margin-top"),
        "5px"
    );
    assert_eq!(
        flipped("flip-start", "margin-bottom: 5px", "margin-right"),
        "5px"
    );
    assert_eq!(flipped("flip-start", "min-width: 5px", "min-height"), "5px");
    assert_eq!(flipped("flip-start", "max-height: 5px", "max-width"), "5px");
    // WPT try-tactic-anchor.html's anchor-size() part: only the cross-axis
    // flips turn width into height.
    let rule = "width: calc(anchor-size(width) + 20px); height: anchor-size(height)";
    for (tactic, width, height) in [
        ("", "calc(anchor-size(width) + 20px)", "anchor-size(height)"),
        (
            "flip-inline",
            "calc(anchor-size(width) + 20px)",
            "anchor-size(height)",
        ),
        (
            "flip-block",
            "calc(anchor-size(width) + 20px)",
            "anchor-size(height)",
        ),
        (
            "flip-start",
            "anchor-size(width)",
            "calc(anchor-size(height) + 20px)",
        ),
        (
            "flip-inline flip-start",
            "anchor-size(width)",
            "calc(anchor-size(height) + 20px)",
        ),
        (
            "flip-start flip-block",
            "anchor-size(width)",
            "calc(anchor-size(height) + 20px)",
        ),
    ] {
        assert_eq!(flipped(tactic, rule, "width"), width, "{tactic}");
        assert_eq!(flipped(tactic, rule, "height"), height, "{tactic}");
    }
    assert_eq!(
        flipped(
            "flip-start",
            "margin-top: anchor-size(--a block)",
            "margin-left"
        ),
        "anchor-size(--a inline)"
    );
    assert_eq!(
        flipped("flip-start", "top: anchor-size(self-inline)", "left"),
        "anchor-size(self-block)"
    );
}

#[test]
fn try_tactics_flip_self_alignment() {
    // WPT try-tactic-alignment.html (computed part), on the Lynx alignment
    // grammar (no self-start/self-end/left/right).
    for (tactic, property, value, expected) in [
        ("", "justify-self", "start", "start"),
        ("", "align-self", "end", "end"),
        ("flip-inline", "justify-self", "start", "end"),
        ("flip-inline", "justify-self", "end", "start"),
        ("flip-x", "justify-self", "start", "end"),
        ("flip-x", "justify-self", "end", "start"),
        ("flip-block", "align-self", "start", "end"),
        ("flip-block", "align-self", "end", "start"),
        ("flip-block", "align-self", "flex-start", "flex-end"),
        ("flip-block", "align-self", "flex-end", "flex-start"),
        ("flip-y", "align-self", "start", "end"),
        ("flip-y", "align-self", "flex-end", "flex-start"),
        // Unchanged: no side to keep a relationship with.
        ("flip-block", "align-self", "center", "center"),
        ("flip-block", "align-self", "anchor-center", "anchor-center"),
        ("flip-block", "align-self", "stretch", "stretch"),
        ("flip-block", "align-self", "baseline", "baseline"),
        // An opposing flip in the other axis leaves it alone.
        ("flip-inline", "align-self", "start", "start"),
        ("flip-block", "justify-self", "start", "start"),
    ] {
        assert_eq!(
            flipped(tactic, &format!("{property}: {value}"), property),
            expected,
            "{tactic} {property}: {value}"
        );
    }
    // flip-start swaps the two properties.
    let rule = "justify-self: start; align-self: end";
    assert_eq!(flipped("flip-start", rule, "justify-self"), "end");
    assert_eq!(flipped("flip-start", rule, "align-self"), "start");
}

#[test]
fn try_tactics_remap_position_area() {
    // WPT try-tactic-position-area.html (horizontal-tb), comparing computed
    // position-area values.
    let check = |tactic: &str, value: &str, expected: &str| {
        let got = flipped(tactic, &format!("position-area: {value}"), "position-area");
        assert_eq!(
            got,
            computed_position_area(expected),
            "{tactic}: position-area {value} must become {expected}"
        );
    };
    for (tactic, value, expected) in [
        ("flip-inline", "left top", "right top"),
        ("flip-inline", "right bottom", "left bottom"),
        ("flip-block", "left top", "left bottom"),
        ("flip-block", "right bottom", "right top"),
        ("flip-block flip-inline", "left top", "right bottom"),
        ("flip-block flip-inline", "right top", "left bottom"),
        ("flip-start", "left top", "left top"),
        ("flip-start", "left bottom", "right top"),
        ("flip-start", "right top", "left bottom"),
        ("flip-block flip-start", "left top", "right top"),
        ("flip-block flip-start", "left bottom", "left top"),
        ("flip-inline flip-start", "left top", "left bottom"),
        ("flip-inline flip-start", "right top", "left top"),
        (
            "flip-block flip-inline flip-start",
            "left top",
            "right bottom",
        ),
        (
            "flip-block flip-inline flip-start",
            "left bottom",
            "left bottom",
        ),
        (
            "flip-block flip-inline",
            "span-left span-top",
            "span-right span-bottom",
        ),
        ("flip-inline", "x-start y-start", "x-end y-start"),
        ("flip-block", "x-start y-end", "x-start y-start"),
        ("flip-start", "x-start y-end", "x-end y-start"),
        (
            "flip-block flip-inline",
            "span-x-start span-y-start",
            "span-x-end span-y-end",
        ),
        (
            "flip-block flip-inline",
            "self-x-start self-y-start",
            "self-x-end self-y-end",
        ),
        ("flip-inline", "block-start inline-start", "start end"),
        ("flip-inline", "block-end inline-start", "end"),
        ("flip-block", "block-start inline-start", "end start"),
        ("flip-block flip-inline", "block-start inline-start", "end"),
        ("flip-start", "block-end inline-start", "start end"),
        ("flip-start", "block-start inline-end", "end start"),
        (
            "flip-block flip-inline",
            "span-block-start span-inline-start",
            "span-end",
        ),
        (
            "flip-block flip-inline",
            "self-block-start self-inline-start",
            "self-end",
        ),
        ("", "start end", "start end"),
        ("flip-block", "start end", "end"),
        ("flip-inline", "start end", "start"),
        ("flip-block flip-inline", "start end", "end start"),
        ("flip-start", "start", "start"),
        ("flip-start", "start end", "end start"),
        (
            "flip-block flip-inline flip-start",
            "start end",
            "start end",
        ),
        (
            "flip-block flip-inline",
            "span-start span-end",
            "span-end span-start",
        ),
        ("flip-block", "left center", "left center"),
        ("flip-block", "center top", "center bottom"),
        ("flip-block", "center", "center"),
        ("flip-block", "start center", "end center"),
        ("flip-block", "center start", "center start"),
        ("flip-inline", "center start", "center end"),
        ("flip-start", "center start", "start center"),
        ("flip-block", "left span-all", "left"),
        ("flip-block", "span-all top", "bottom"),
        ("flip-block", "span-all", "span-all"),
        ("flip-block", "start span-all", "block-end"),
        ("flip-block", "span-all start", "inline-start"),
        ("flip-inline", "span-all start", "inline-end"),
        ("flip-start", "span-all start", "block-start"),
        ("flip-block", "left span-top", "left span-bottom"),
        ("flip-inline", "left span-top", "right span-top"),
        (
            "flip-start",
            "span-block-start inline-end",
            "end span-start",
        ),
    ] {
        check(tactic, value, expected);
    }
}

// ---------------------------------------------------------------------------
// A computed-value context on an 800 × 600 screen, and a stylist with one
// author sheet cascading for the detached test element.

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

fn stylesheet(lock: &SharedRwLock, css: &str) -> Stylesheet {
    Stylesheet::from_str(
        css,
        url_data(),
        Origin::Author,
        Arc::new(lock.wrap(MediaList::empty())),
        lock.clone(),
        None,
        None,
        QuirksMode::NoQuirks,
        AllowImportRules::Yes,
    )
}

/// A stylist holding one author sheet.
struct Env {
    lock: SharedRwLock,
    stylist: Stylist,
}

impl Env {
    fn new(sheet: &str) -> Self {
        let lock = SharedRwLock::new();
        let mut stylist = Stylist::new(device(), QuirksMode::NoQuirks);
        let sheet = DocumentStyleSheet(Arc::new(stylesheet(&lock, sheet)));
        {
            let guard = lock.read();
            stylist.append_stylesheet(sheet, &guard);
            stylist.flush(&StylesheetGuards::same(&guard));
        }
        Self { lock, stylist }
    }

    /// The test element's style with `declarations` as its only author
    /// declarations (an inline-style-like block at author normal).
    fn style(&self, declarations: &str) -> Arc<ComputedValues> {
        let block = parse_style_attribute(
            declarations,
            &url_data(),
            None,
            QuirksMode::NoQuirks,
            CssRuleType::Style,
        );
        let block = Arc::new(self.lock.wrap(block));
        let guard = self.lock.read();
        let guards = StylesheetGuards::same(&guard);
        let rules = self
            .stylist
            .rule_tree()
            .insert_ordered_rules(std::iter::once((
                StyleSource::from_declarations(block),
                style::applicable_declarations::CascadePriority::new(
                    CascadeLevel::same_tree_author_normal(),
                    LayerOrder::root(),
                    RuleCascadeFlags::empty(),
                ),
            )));
        let inputs = CascadeInputs {
            rules: Some(rules),
            visited_rules: None,
            flags: Default::default(),
            included_cascade_flags: RuleCascadeFlags::empty(),
        };
        self.stylist.cascade_style_and_visited(
            Some(TestElement),
            None,
            &inputs,
            &guards,
            None,
            None,
            FirstLineReparenting::No,
            &Default::default(),
            None,
            &mut RuleCacheConditions::default(),
            &mut TreeCountingCaches::default(),
        )
    }

    /// `Stylist::resolve_position_try` for one `position-try-fallbacks`
    /// entry, at the scope the base style's own `position-try-fallbacks`
    /// came from (as the layout engine calls it).
    fn resolve(&self, base: &Arc<ComputedValues>, item: &str) -> Option<Arc<ComputedValues>> {
        let PropertyDeclaration::PositionTryFallbacks(fallbacks) =
            declaration("position-try-fallbacks", item)
        else {
            panic!("position-try-fallbacks: {item}")
        };
        let item: &PositionTryFallbacksItem = &fallbacks.value.0[0];
        let scope = base.clone_position_try_fallbacks().scope;
        let guard = self.lock.read();
        self.stylist.resolve_position_try(
            base,
            &StylesheetGuards::same(&guard),
            scope,
            TestElement,
            item,
        )
    }
}

fn value_of(style: &ComputedValues, property: &str) -> String {
    let id = PropertyId::parse_enabled_for_all_content(property)
        .unwrap_or_else(|_| panic!("{property}"))
        .longhand_id()
        .unwrap_or_else(|| panic!("{property} is a longhand"));
    let mut out = String::new();
    style
        .computed_or_resolved_value(id, None, &mut out)
        .unwrap();
    out
}

fn style_for(sheet: &str, declarations: &str) -> Arc<ComputedValues> {
    Env::new(sheet).style(declarations)
}

/// The computed value of `property` for the test element.
fn computed(sheet: &str, declarations: &str, property: &str) -> String {
    value_of(&style_for(sheet, declarations), property)
}

/// A longhand's specified value in an inline block.
fn longhand_of(declarations: &str, longhand: &str) -> String {
    let block = parse_style_attribute(
        declarations,
        &url_data(),
        None,
        QuirksMode::NoQuirks,
        CssRuleType::Style,
    );
    let mut out = String::new();
    block
        .property_value_to_css(
            &PropertyId::parse_enabled_for_all_content(longhand).unwrap(),
            &mut out,
        )
        .unwrap();
    out
}
