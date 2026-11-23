//! W3C SVG 2, css-masking-1 and css-transforms-1 properties under the `lynx`
//! feature. They are outside Lynx's own property list; lynx-vello's document
//! tree carries SVG-namespace elements and stylo cascades these onto them.
//!
//! The engine-agnostic SVG longhands (`fill*`, `stroke*`, `cx`, `cy`, `r`,
//! `rx`, `ry`, `x`, `y`, `d`) are seeded in `lynx_properties.txt`. The
//! gecko-only `clip-rule`, `paint-order`, `stop-color`, `stop-opacity`,
//! `text-anchor` and `transform-box` are ported to servo behind
//! `layout.unimplemented` and seeded as well; the `lynx` feature force-enables
//! them (lynx_feature_off_parity pins the stock-servo side).
//!
//! The cascade runs on a detached test element (`support/test_element.rs`).
#![cfg(feature = "lynx")]

#[path = "support/test_element.rs"]
mod test_element;

use euclid::{Scale, Size2D};
use servo_arc::Arc;
use style::context::{CascadeInputs, QuirksMode, TreeCountingCaches};
use style::device::servo::FontMetricsProvider;
use style::device::Device;
use style::font_metrics::FontMetrics;
use style::media_queries::MediaType;
use style::properties::animated_properties::AnimationValue;
use style::properties::declaration_block::{
    parse_one_declaration_into, parse_style_attribute, PropertyDeclarationBlock,
};
use style::properties::{
    style_structs, ComputedValues, FirstLineReparenting, Importance, LonghandId,
    PropertyDeclarationId, PropertyId, SourcePropertyDeclaration,
};
use style::queries::values::PrefersColorScheme;
use style::rule_cache::RuleCacheConditions;
use style::rule_tree::{CascadeLevel, RuleCascadeFlags, StyleSource};
use style::servo::media_features::PointerCapabilities;
use style::shared_lock::{SharedRwLock, StylesheetGuards};
use style::stylesheets::layer_rule::LayerOrder;
use style::stylesheets::{CssRuleType, Origin, UrlExtraData};
use style::stylist::Stylist;
use style::url::ComputedUrl;
use style::values::animated::{Animate, Procedure};
use style::values::computed::basic_shape::ClipPath;
use style::values::computed::font::GenericFontFamily;
use style::values::computed::{CSSPixelLength, Image, SVGPaintKind, TextAnchor, TransformBox};
use style::values::generics::basic_shape::FillRule;
use style::values::generics::svg::SVGPaintFallback;
use style::values::specified::font::QueryFontMetricsFlags;
use style_traits::{CSSPixel, DevicePixel, ParsingMode};
use test_element::TestElement;

fn url_data() -> UrlExtraData {
    UrlExtraData::from(::url::Url::parse("https://example.com/").unwrap())
}

fn parse_with_mode(
    name: &str,
    value: &str,
    mode: ParsingMode,
) -> Result<SourcePropertyDeclaration, ()> {
    let id = PropertyId::parse_enabled_for_all_content(name).map_err(|_| ())?;
    let mut declarations = SourcePropertyDeclaration::default();
    parse_one_declaration_into(
        &mut declarations,
        id,
        value,
        Origin::Author,
        &url_data(),
        None,
        mode,
        QuirksMode::NoQuirks,
        CssRuleType::Style,
    )
    .map_err(|_| ())?;
    Ok(declarations)
}

/// `name: value` parsed in `mode` and serialized back from a declaration
/// block, or `None` when it does not parse.
fn serialized_with_mode(name: &str, value: &str, mode: ParsingMode) -> Option<String> {
    let mut declarations = parse_with_mode(name, value, mode).ok()?;
    let mut block = PropertyDeclarationBlock::new();
    block.extend(declarations.drain(), Importance::Normal);
    let mut out = String::new();
    block
        .property_value_to_css(
            &PropertyId::parse_enabled_for_all_content(name).unwrap(),
            &mut out,
        )
        .unwrap();
    Some(out)
}

fn serialized(name: &str, value: &str) -> Option<String> {
    serialized_with_mode(name, value, ParsingMode::DEFAULT)
}

fn assert_valid(name: &str, value: &str, expected: &str) {
    assert_eq!(
        serialized(name, value).as_deref(),
        Some(expected),
        "`{name}: {value}`"
    );
}

fn assert_round_trips(name: &str, values: &[&str]) {
    for value in values {
        assert_valid(name, value, value);
    }
}

fn assert_rejects(name: &str, values: &[&str]) {
    for value in values {
        assert!(
            parse_with_mode(name, value, ParsingMode::DEFAULT).is_err(),
            "`{name}: {value}` must not parse"
        );
    }
}

// ---------------------------------------------------------------------------
// Cascade harness.

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

/// The test element's style with `declarations` as its only author
/// declarations (an inline-style-like block at author normal), no sheets.
fn style(declarations: &str) -> Arc<ComputedValues> {
    let lock = SharedRwLock::new();
    let stylist = Stylist::new(device(), QuirksMode::NoQuirks);
    let block = parse_style_attribute(
        declarations,
        &url_data(),
        None,
        QuirksMode::NoQuirks,
        CssRuleType::Style,
    );
    let block = Arc::new(lock.wrap(block));
    let guard = lock.read();
    let guards = StylesheetGuards::same(&guard);
    let rules = stylist
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
    stylist.cascade_style_and_visited(
        Some(TestElement::div()),
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

fn value_of(style: &ComputedValues, property: &str) -> String {
    let id = longhand(property);
    let mut out = String::new();
    style.computed_or_resolved_value(id, None, &mut out).unwrap();
    out
}

fn longhand(property: &str) -> LonghandId {
    PropertyId::parse_enabled_for_all_content(property)
        .unwrap_or_else(|_| panic!("`{property}` must be an author property"))
        .longhand_id()
        .unwrap_or_else(|| panic!("`{property}` is a longhand"))
}

fn computed(declarations: &str, property: &str) -> String {
    value_of(&style(declarations), property)
}

// ---------------------------------------------------------------------------
// Item 1: the engine-agnostic SVG longhands.

const ENGINE_AGNOSTIC: &[&str] = &[
    "fill",
    "fill-opacity",
    "fill-rule",
    "stroke",
    "stroke-width",
    "stroke-opacity",
    "stroke-linecap",
    "stroke-linejoin",
    "stroke-miterlimit",
    "stroke-dasharray",
    "stroke-dashoffset",
    "cx",
    "cy",
    "r",
    "rx",
    "ry",
    "x",
    "y",
    "d",
];

/// Item 2: gecko-only upstream, ported to servo.
const PORTED: &[&str] = &[
    "clip-rule",
    "paint-order",
    "stop-color",
    "stop-opacity",
    "text-anchor",
    "transform-box",
];

#[test]
fn svg_properties_are_author_properties() {
    for name in ENGINE_AGNOSTIC.iter().chain(PORTED) {
        assert!(
            PropertyId::parse_enabled_for_all_content(name).is_ok(),
            "`{name}` must be content-enabled under the `lynx` feature"
        );
    }
}

#[test]
fn out_of_scope_svg_properties_stay_absent() {
    for name in [
        "vector-effect",
        "marker",
        "marker-start",
        "marker-mid",
        "marker-end",
        "dominant-baseline",
        "color-interpolation",
        "color-interpolation-filters",
        "shape-rendering",
        "flood-color",
        "flood-opacity",
        "lighting-color",
        "mask-type",
    ] {
        assert!(
            PropertyId::parse_enabled_for_all_content(name).is_err(),
            "`{name}` is out of scope and must stay absent"
        );
    }
}

#[test]
fn paint_properties_round_trip() {
    for name in ["fill", "stroke"] {
        assert_round_trips(
            name,
            &[
                "none",
                "red",
                "url(\"#g\")",
                "url(\"#g\") none",
                "url(\"#g\") red",
            ],
        );
        assert_valid(name, "url(#g)", "url(\"#g\")");
        // SVG 2's `context-fill` / `context-stroke` are upstream grammar.
        assert_round_trips(name, &["context-fill", "context-stroke"]);
        assert_rejects(name, &["red url(\"#g\")"]);
        // The shared `lynx` `<color>` grammar has no `currentcolor`
        // (lynx_value_gating, lynx_value_grammar); `<paint>` accepts it, ASCII
        // case-insensitively, as the color and as the fallback after `url()`.
        assert_round_trips(name, &["currentcolor", "url(\"#g\") currentcolor"]);
        assert_valid(name, "currentColor", "currentcolor");
        assert_valid(name, "url(#g) CURRENTCOLOR", "url(\"#g\") currentcolor");
        assert_valid(name, "context-fill currentcolor", "context-fill currentcolor");
        assert_rejects(name, &["currentcolor red", "currentcolor url(\"#g\")"]);
    }
    for name in ["fill-opacity", "stroke-opacity"] {
        assert_round_trips(name, &["0.5", "1", "0"]);
        assert_valid(name, "50%", "0.5");
    }
    for name in ["fill-rule", "clip-rule"] {
        assert_round_trips(name, &["nonzero", "evenodd"]);
        assert_rejects(name, &["auto"]);
    }
}

#[test]
fn stroke_geometry_round_trips() {
    assert_round_trips("stroke-width", &["1px", "2.5px", "10%", "0px"]);
    assert_rejects("stroke-width", &["-1px", "auto"]);
    assert_round_trips("stroke-linecap", &["butt", "round", "square"]);
    assert_round_trips("stroke-linejoin", &["miter", "round", "bevel"]);
    assert_rejects("stroke-linejoin", &["arcs", "miter-clip"]);
    assert_round_trips("stroke-miterlimit", &["4", "1.5"]);
    assert_rejects("stroke-miterlimit", &["-1"]);
    assert_round_trips("stroke-dasharray", &["none", "4px, 2px", "10%, 1px"]);
    assert_round_trips("stroke-dashoffset", &["3px", "-3px", "10%"]);
}

#[test]
fn geometry_properties_round_trip() {
    for name in ["cx", "cy", "x", "y"] {
        assert_round_trips(name, &["10px", "-10px", "50%", "calc(50% + 2px)"]);
    }
    assert_round_trips("r", &["5px", "50%"]);
    assert_rejects("r", &["-5px", "auto"]);
    for name in ["rx", "ry"] {
        assert_round_trips(name, &["auto", "5px", "10%"]);
        assert_rejects(name, &["-5px"]);
    }
    assert_round_trips("d", &["none", "path(\"M 0 0 L 10 10 Z\")"]);
    assert_rejects("d", &["path(\"Q\")", "M 0 0"]);
}

#[test]
fn ported_properties_round_trip() {
    assert_round_trips(
        "paint-order",
        &["normal", "stroke", "markers", "fill markers", "stroke markers"],
    );
    // Trailing components implied by the order are dropped.
    assert_valid("paint-order", "fill stroke markers", "fill");
    assert_valid("paint-order", "stroke fill", "stroke");
    assert_rejects("paint-order", &["fill fill"]);
    assert_round_trips("stop-color", &["red", "currentcolor", "rgba(0, 0, 255, 0.5)"]);
    assert_valid("stop-color", "CurrentColor", "currentcolor");
    assert_round_trips("stop-opacity", &["0.5", "1"]);
    assert_round_trips("text-anchor", &["start", "middle", "end"]);
    assert_rejects("text-anchor", &["center"]);
    assert_round_trips("transform-box", &["border-box", "fill-box", "view-box"]);
    // Upstream servo parses `content-box` / `stroke-box` only where
    // `all_transform_boxes_are_enabled` (gecko); the port keeps that.
    assert_rejects("transform-box", &["content-box", "stroke-box", "padding-box"]);
}

// ---------------------------------------------------------------------------
// Computed values and the computed-struct fields the host reads.

#[test]
fn initial_values() {
    let s = style("");
    for (property, expected) in [
        ("fill", "rgb(0, 0, 0)"),
        ("stroke", "none"),
        ("fill-opacity", "1"),
        ("fill-rule", "nonzero"),
        ("clip-rule", "nonzero"),
        ("stroke-width", "1px"),
        ("stroke-linecap", "butt"),
        ("stroke-linejoin", "miter"),
        ("stroke-miterlimit", "4"),
        ("stroke-dasharray", "none"),
        ("stroke-dashoffset", "0px"),
        ("paint-order", "normal"),
        ("stop-color", "rgb(0, 0, 0)"),
        ("stop-opacity", "1"),
        ("text-anchor", "start"),
        ("transform-box", "view-box"),
        ("cx", "0px"),
        ("r", "0px"),
        ("rx", "auto"),
        ("x", "0px"),
        ("d", "none"),
    ] {
        assert_eq!(value_of(&s, property), expected, "initial `{property}`");
    }
}

#[test]
fn computed_values_serialize() {
    // A computed URL is resolved against the sheet's base URL.
    assert_eq!(
        computed("fill: url(#g) red", "fill"),
        "url(\"https://example.com/#g\") rgb(255, 0, 0)"
    );
    assert_eq!(computed("stroke-width: 2em", "stroke-width"), "32px");
    assert_eq!(computed("cx: calc(10px + 1em)", "cx"), "26px");
    assert_eq!(computed("paint-order: markers", "paint-order"), "markers");
    assert_eq!(computed("stop-color: red", "stop-color"), "rgb(255, 0, 0)");
    assert_eq!(computed("stop-opacity: 2", "stop-opacity"), "1");
    assert_eq!(computed("text-anchor: middle", "text-anchor"), "middle");
    assert_eq!(computed("transform-box: fill-box", "transform-box"), "fill-box");
}

#[test]
fn ported_struct_fields_exist() {
    let s = style(
        "clip-rule: evenodd; paint-order: stroke; stop-color: red; stop-opacity: 0.25; \
         text-anchor: end; transform-box: fill-box",
    );
    assert_eq!(s.get_inherited_svg().clip_rule, FillRule::Evenodd);
    assert_eq!(*s.get_inherited_svg().get_clip_rule(), FillRule::Evenodd);
    assert_eq!(s.slow_clone_clip_rule(), FillRule::Evenodd);
    assert_ne!(s.slow_clone_paint_order().0, 0, "`stroke` is not `normal`");
    assert_eq!(*s.get_box().get_transform_box(), TransformBox::FillBox);
    assert!(!s.get_svg().stop_color.is_currentcolor());
    assert_eq!(s.get_svg().stop_opacity, 0.25);
    assert_eq!(s.get_inherited_svg().text_anchor, TextAnchor::End);
    assert_eq!(s.get_box().transform_box, TransformBox::FillBox);
}

#[test]
fn inherited_svg_properties_inherit() {
    // The test element is a root, so inheritance is checked through the
    // longhand metadata rather than a parent cascade.
    for name in [
        "fill",
        "fill-opacity",
        "fill-rule",
        "stroke",
        "stroke-width",
        "stroke-opacity",
        "stroke-linecap",
        "stroke-linejoin",
        "stroke-miterlimit",
        "stroke-dasharray",
        "stroke-dashoffset",
        "clip-rule",
        "paint-order",
        "text-anchor",
    ] {
        assert!(longhand(name).inherited(), "`{name}` is inherited");
    }
    for name in [
        "cx",
        "cy",
        "r",
        "rx",
        "ry",
        "x",
        "y",
        "d",
        "stop-color",
        "stop-opacity",
        "transform-box",
    ] {
        assert!(!longhand(name).inherited(), "`{name}` is not inherited");
    }
}

// ---------------------------------------------------------------------------
// Item 3: paint-server and reference URLs compute to their URL variants.

fn assert_fragment_url(url: &ComputedUrl, fragment: &str) {
    let ComputedUrl::Valid(url) = url else {
        panic!("`url(#{fragment})` resolves against the sheet's base URL");
    };
    assert_eq!(url.fragment(), Some(fragment));
}

#[test]
fn fill_and_stroke_compute_to_their_paint_kinds() {
    let s = style("fill: url(#g) red; stroke: url(#h)");
    let fill = &s.get_inherited_svg().fill;
    let SVGPaintKind::PaintServer(ref url) = fill.kind else {
        panic!("`fill: url(#g) red` computes to a paint server");
    };
    assert_fragment_url(url, "g");
    let SVGPaintFallback::Color(ref fallback) = fill.fallback else {
        panic!("`fill: url(#g) red` keeps its color fallback");
    };
    assert!(!fallback.is_currentcolor());
    let stroke = &s.get_inherited_svg().stroke;
    let SVGPaintKind::PaintServer(ref url) = stroke.kind else {
        panic!("`stroke: url(#h)` computes to a paint server");
    };
    assert_fragment_url(url, "h");
    assert!(matches!(stroke.fallback, SVGPaintFallback::Unset));

    let s = style("fill: none; stroke: red");
    assert!(matches!(s.get_inherited_svg().fill.kind, SVGPaintKind::None));
    let SVGPaintKind::Color(ref color) = s.get_inherited_svg().stroke.kind else {
        panic!("`stroke: red` computes to a color");
    };
    assert!(!color.is_currentcolor());
}

#[test]
fn currentcolor_paint_computes_to_currentcolor() {
    let s = style("fill: currentColor; stroke: url(#g) currentcolor; stop-color: currentcolor");
    let SVGPaintKind::Color(ref color) = s.get_inherited_svg().fill.kind else {
        panic!("`fill: currentColor` computes to a color");
    };
    assert!(color.is_currentcolor());
    let stroke = &s.get_inherited_svg().stroke;
    assert!(matches!(stroke.kind, SVGPaintKind::PaintServer(..)));
    let SVGPaintFallback::Color(ref fallback) = stroke.fallback else {
        panic!("`stroke: url(#g) currentcolor` keeps its fallback");
    };
    assert!(fallback.is_currentcolor());
    assert!(s.get_svg().stop_color.is_currentcolor());
}

#[test]
fn shared_color_grammar_still_rejects_currentcolor() {
    for name in ["color", "outline-color", "background-color", "border-top-color"] {
        assert_rejects(name, &["currentcolor"]);
    }
}

#[test]
fn mask_image_computes_to_a_url() {
    let s = style("mask-image: url(#m)");
    let Image::Url(ref url) = s.get_svg().mask_image.0[0] else {
        panic!("`mask-image: url(#m)` computes to a URL image");
    };
    assert_fragment_url(url, "m");
}

#[test]
fn clip_path_url_computes_to_a_url() {
    // css-masking-1 `url()` references an SVG `<clipPath>`; the `lynx` shape
    // set stays inset/circle/ellipse/path (lynx_value_grammar).
    assert_valid("clip-path", "url(#c)", "url(\"#c\")");
    assert_rejects("clip-path", &["url(#c) border-box", "polygon(0 0, 1px 1px)"]);
    let s = style("clip-path: url(#c)");
    let ClipPath::Url(ref url) = s.get_svg().clip_path else {
        panic!("`clip-path: url(#c)` computes to a URL");
    };
    assert_fragment_url(url, "c");
}

// ---------------------------------------------------------------------------
// Animation: interpolable SVG longhands interpolate, keyword ones are
// discrete, as upstream declares.

fn midpoint(property: &str, from: &str, to: &str) -> (AnimationValue, AnimationValue, AnimationValue) {
    let id = PropertyDeclarationId::Longhand(longhand(property));
    let a = AnimationValue::from_computed_values(id, &style(&format!("{property}: {from}")))
        .unwrap_or_else(|| panic!("`{property}` has an animation value"));
    let b = AnimationValue::from_computed_values(id, &style(&format!("{property}: {to}")))
        .unwrap();
    let mid = a
        .animate(&b, Procedure::Interpolate { progress: 0.5 })
        .unwrap_or_else(|_| panic!("`{property}` must interpolate"));
    (a, b, mid)
}

#[test]
fn smooth_svg_longhands_interpolate() {
    for (property, from, to) in [
        ("fill", "red", "blue"),
        ("stroke", "red", "blue"),
        ("fill-opacity", "0", "1"),
        ("stroke-opacity", "0", "1"),
        ("stroke-width", "1px", "3px"),
        ("stroke-miterlimit", "1", "5"),
        ("stroke-dasharray", "2px, 4px", "4px, 8px"),
        ("stroke-dashoffset", "0px", "4px"),
        ("cx", "0px", "10px"),
        ("cy", "0px", "10px"),
        ("r", "0px", "10px"),
        ("rx", "0px", "10px"),
        ("ry", "0px", "10px"),
        ("x", "0px", "10px"),
        ("y", "0px", "10px"),
        ("d", "path(\"M 0 0 L 10 10\")", "path(\"M 0 0 L 20 20\")"),
        ("stop-color", "red", "blue"),
        ("stop-opacity", "0", "1"),
    ] {
        assert!(longhand(property).is_animatable(), "`{property}`");
        assert!(!longhand(property).is_discrete_animatable(), "`{property}`");
        let (a, b, mid) = midpoint(property, from, to);
        assert!(mid != a && mid != b, "`{property}` midpoint is between");
    }
}

#[test]
fn keyword_svg_longhands_are_discrete() {
    for property in [
        "fill-rule",
        "clip-rule",
        "stroke-linecap",
        "stroke-linejoin",
        "paint-order",
        "text-anchor",
        "transform-box",
    ] {
        assert!(longhand(property).is_animatable(), "`{property}`");
        assert!(longhand(property).is_discrete_animatable(), "`{property}`");
    }
}

// ---------------------------------------------------------------------------
// Item 4: SVG presentation attributes parse with
// `ParsingMode::ALLOW_UNITLESS_LENGTH` (a number is user units, i.e. px).

#[test]
fn unitless_lengths_parse_with_the_svg_parsing_mode() {
    for (name, value, expected) in [
        ("stroke-width", "2", "2px"),
        ("x", "10", "10px"),
        ("r", "5", "5px"),
        ("font-size", "12", "12px"),
        ("stroke-dasharray", "4 2", "4px, 2px"),
        ("stroke-dashoffset", "3", "3px"),
        ("width", "24", "24px"),
    ] {
        assert_eq!(
            serialized_with_mode(name, value, ParsingMode::ALLOW_UNITLESS_LENGTH).as_deref(),
            Some(expected),
            "`{name}: {value}` with ALLOW_UNITLESS_LENGTH"
        );
    }
}

#[test]
fn unitless_lengths_in_author_css() {
    // SVG 2's stroke lengths take `<number>` in CSS too (stylo parses them
    // with `AllowQuirks::Always`); the geometry properties and the CSS box
    // properties do not.
    for (name, value, expected) in [
        ("stroke-width", "2", "2px"),
        ("stroke-dasharray", "4 2", "4px, 2px"),
        ("stroke-dashoffset", "3", "3px"),
    ] {
        assert_eq!(
            serialized(name, value).as_deref(),
            Some(expected),
            "`{name}: {value}`"
        );
    }
    assert_rejects("x", &["10"]);
    assert_rejects("r", &["5"]);
    assert_rejects("font-size", &["12"]);
    assert_rejects("width", &["24"]);
}
