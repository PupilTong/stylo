//! The top layer under the `lynx` feature: the internal `-servo-top-layer`
//! longhand is reachable from user-agent stylesheets only, and the
//! css-color-4 system colors its HTML defaults spell (`Canvas`,
//! `CanvasText`) parse and compute.
//!
//! The outer engine's UA sheet puts `dialog:modal` and its `::backdrop` in
//! the top layer with `-servo-top-layer: auto`; `StyleAdjuster` then computes
//! `position` to `absolute` (unless already `absolute`/`fixed`) and
//! blockifies `display: contents` (css-position-4's top-layer rules). The
//! longhand is `enabled_in = "ua"`, so author and user sheets still cannot
//! spell it.
#![cfg(feature = "lynx")]

#[path = "support/test_element.rs"]
mod test_element;

use euclid::{Scale, Size2D};
use servo_arc::Arc;
use style::context::{CascadeInputs, QuirksMode, TreeCountingCaches};
use style::device::servo::FontMetricsProvider;
use style::device::Device;
use style::font_metrics::FontMetrics;
use style::media_queries::{MediaList, MediaType};
use style::properties::{
    style_structs, ComputedValues, FirstLineReparenting, PropertyDeclarationBlock, PropertyId,
};
use style::queries::values::PrefersColorScheme;
use style::rule_cache::RuleCacheConditions;
use style::rule_tree::{CascadeLevel, CascadeOrigin, RuleCascadeFlags, StyleSource};
use style::servo::media_features::PointerCapabilities;
use style::shared_lock::{Locked, SharedRwLock, StylesheetGuards};
use style::stylesheets::layer_rule::LayerOrder;
use style::stylesheets::{
    AllowImportRules, CssRule, Origin, Stylesheet, StylesheetInDocument, UrlExtraData,
};
use style::stylist::Stylist;
use style::values::computed::font::GenericFontFamily;
use style::values::computed::{CSSPixelLength, TopLayer};
use style::values::specified::font::QueryFontMetricsFlags;
use style_traits::{CSSPixel, DevicePixel};
use test_element::TestElement;

fn url_data() -> UrlExtraData {
    UrlExtraData::from(::url::Url::parse("https://example.com/").unwrap())
}

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

fn device(scheme: PrefersColorScheme) -> Device {
    Device::new(
        MediaType::screen(),
        QuirksMode::NoQuirks,
        Size2D::<f32, CSSPixel>::new(800.0, 600.0),
        Size2D::<f32, DevicePixel>::new(800.0, 600.0),
        Scale::<f32, CSSPixel, DevicePixel>::new(1.0),
        Box::new(TestFontMetricsProvider),
        ComputedValues::initial_values_with_font_override(style_structs::Font::initial_values()),
        scheme,
        PointerCapabilities::default(),
        PointerCapabilities::default(),
    )
}

/// The declaration block of the single style rule in `css`, parsed as a
/// stylesheet of `origin`.
fn rule_block(
    lock: &SharedRwLock,
    origin: Origin,
    css: &str,
) -> Arc<Locked<PropertyDeclarationBlock>> {
    let sheet = Stylesheet::from_str(
        css,
        url_data(),
        origin,
        Arc::new(lock.wrap(MediaList::empty())),
        lock.clone(),
        None,
        None,
        QuirksMode::NoQuirks,
        AllowImportRules::Yes,
    );
    let guard = lock.read();
    let rules = sheet.contents(&guard).rules(&guard);
    assert_eq!(rules.len(), 1, "{css}");
    match rules[0] {
        CssRule::Style(ref rule) => rule.read_with(&guard).block.clone(),
        ref other => panic!("expected a style rule in `{css}`, got {other:?}"),
    }
}

/// The property names a `origin` stylesheet keeps from `declarations`.
fn kept(origin: Origin, declarations: &str) -> Vec<String> {
    let lock = SharedRwLock::new();
    let block = rule_block(&lock, origin, &format!("div {{ {declarations} }}"));
    let guard = lock.read();
    let block = block.read_with(&guard);
    block
        .declarations()
        .iter()
        .map(|declaration| declaration.id().name().to_string())
        .collect()
}

/// The detached test element's style with `declarations` as its only rule,
/// cascaded at `origin`'s normal level on a device preferring `scheme`.
fn style(origin: Origin, scheme: PrefersColorScheme, declarations: &str) -> Arc<ComputedValues> {
    let lock = SharedRwLock::new();
    let stylist = Stylist::new(device(scheme), QuirksMode::NoQuirks);
    let block = rule_block(&lock, origin, &format!("div {{ {declarations} }}"));
    let level = CascadeLevel::new(match origin {
        Origin::UserAgent => CascadeOrigin::UA,
        Origin::User => CascadeOrigin::User,
        Origin::Author => CascadeOrigin::Author,
    });
    let guard = lock.read();
    let guards = StylesheetGuards::same(&guard);
    let rules = stylist.rule_tree().insert_ordered_rules(std::iter::once((
        StyleSource::from_declarations(block),
        style::applicable_declarations::CascadePriority::new(
            level,
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

/// `in_top_layer` is a `StyleBuilder` method; read the computed longhand.
fn in_top_layer(style: &ComputedValues) -> bool {
    style.get_box().slow_clone__servo_top_layer() == TopLayer::Auto
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

// ---------------------------------------------------------------------------
// -servo-top-layer

#[test]
fn top_layer_is_in_the_name_table_but_not_content_enabled() {
    assert!(
        PropertyId::parse_unchecked("-servo-top-layer", None).is_ok(),
        "the UA sheet needs the name in the Lynx property-name table"
    );
    assert!(
        PropertyId::parse_enabled_for_all_content("-servo-top-layer").is_err(),
        "`-servo-top-layer` is internal and must never be content-enabled"
    );
}

#[test]
fn only_ua_sheets_can_declare_the_top_layer() {
    let declarations = "-servo-top-layer: auto; position: absolute";
    assert_eq!(
        kept(Origin::UserAgent, declarations),
        ["-servo-top-layer", "position"]
    );
    assert_eq!(kept(Origin::User, declarations), ["position"]);
    assert_eq!(kept(Origin::Author, declarations), ["position"]);
    assert_eq!(
        kept(Origin::UserAgent, "-servo-top-layer: none"),
        ["-servo-top-layer"]
    );
    assert!(kept(Origin::UserAgent, "-servo-top-layer: always").is_empty());
}

#[test]
fn an_author_top_layer_declaration_does_not_reach_the_cascade() {
    let style = style(
        Origin::Author,
        PrefersColorScheme::Light,
        "-servo-top-layer: auto; position: relative",
    );
    assert!(!in_top_layer(&style));
    assert_eq!(value_of(&style, "position"), "relative");
}

#[test]
fn the_ua_top_layer_absolutizes_position() {
    let light = PrefersColorScheme::Light;

    let normal = style(Origin::UserAgent, light, "position: relative");
    assert!(!in_top_layer(&normal));
    assert_eq!(value_of(&normal, "position"), "relative");

    for (specified, computed) in [
        ("static", "absolute"),
        ("relative", "absolute"),
        ("sticky", "absolute"),
        ("absolute", "absolute"),
        ("fixed", "fixed"),
    ] {
        let top = style(
            Origin::UserAgent,
            light,
            &format!("-servo-top-layer: auto; position: {specified}"),
        );
        assert!(in_top_layer(&top), "{specified}");
        assert_eq!(value_of(&top, "position"), computed, "{specified}");
    }
}

#[test]
fn the_ua_top_layer_blockifies_display_contents() {
    let light = PrefersColorScheme::Light;

    let normal = style(Origin::UserAgent, light, "display: contents");
    assert_eq!(value_of(&normal, "display"), "contents");

    let top = style(
        Origin::UserAgent,
        light,
        "-servo-top-layer: auto; display: contents",
    );
    assert!(in_top_layer(&top));
    assert_eq!(value_of(&top, "display"), "block");

    // A Lynx display value is already a box; the top layer keeps it.
    let linear = style(
        Origin::UserAgent,
        light,
        "-servo-top-layer: auto; display: linear",
    );
    assert_eq!(value_of(&linear, "display"), "linear");
}

// ---------------------------------------------------------------------------
// System colors

#[test]
fn system_colors_parse_in_every_origin() {
    for origin in [Origin::UserAgent, Origin::User, Origin::Author] {
        assert_eq!(
            kept(origin, "background-color: Canvas; color: CanvasText"),
            ["background-color", "color"],
            "{origin:?}"
        );
    }
}

#[test]
fn canvas_and_canvas_text_compute_through_the_device_scheme() {
    let declarations = "background-color: Canvas; color: CanvasText";

    let light = style(Origin::UserAgent, PrefersColorScheme::Light, declarations);
    assert_eq!(value_of(&light, "background-color"), "rgb(255, 255, 255)");
    assert_eq!(value_of(&light, "color"), "rgb(0, 0, 0)");

    let dark = style(Origin::UserAgent, PrefersColorScheme::Dark, declarations);
    assert_eq!(value_of(&dark, "background-color"), "rgb(30, 30, 30)");
    assert_eq!(value_of(&dark, "color"), "rgb(232, 232, 232)");
}
