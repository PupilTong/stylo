//! Lynx's text tags under the `lynx` feature: `text`, `inline-text` and
//! `inline-truncation` compute any `display` other than `none` to
//! `-lynx-text`, in `StyleAdjuster`.
//!
//! The outer engine's UA sheet gives these tags `display: -lynx-text` as a
//! plain declaration, so an author `display: none` hides a text, and an
//! author `display: flex` — which native Lynx ignores, and under which no
//! layout mode here could lay text out — leaves it a text.
#![cfg(feature = "lynx")]

#[path = "support/test_element.rs"]
mod test_element;

use euclid::{Scale, Size2D};
use style::context::QuirksMode;
use style::device::servo::FontMetricsProvider;
use style::device::Device;
use style::font_metrics::FontMetrics;
use style::media_queries::MediaType;
use style::properties::{style_structs, ComputedValues, LonghandIdSet, StyleBuilder};
use style::queries::values::PrefersColorScheme;
use style::servo::media_features::PointerCapabilities;
use style::style_adjuster::StyleAdjuster;
use style::values::computed::font::GenericFontFamily;
use style::values::computed::position::PositionTryFallbacksTryTactic;
use style::values::computed::{CSSPixelLength, Display};
use style::values::specified::font::QueryFontMetricsFlags;
use style_traits::{CSSPixel, DevicePixel};
use test_element::TestElement;

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

/// `display` after the style adjuster runs on `element`, whose cascaded
/// `display` is `specified`, under an initial (`flex`) parent.
fn adjusted_display(element: TestElement, specified: Display) -> Display {
    let device = device();
    let parent = device.default_computed_values();
    let mut builder = StyleBuilder::new(&device, None, None, None, None, false);
    builder.mutate_box().set_display(specified);
    StyleAdjuster::new(&mut builder).adjust(
        parent,
        Some(element),
        &PositionTryFallbacksTryTactic::default(),
        &LonghandIdSet::default(),
    );
    *builder.get_box().get_display()
}

const TEXT_TAGS: [&str; 3] = ["text", "inline-text", "inline-truncation"];

/// Every Lynx `display` value other than `none` and `-lynx-text`.
const OTHER_DISPLAYS: [Display; 6] = [
    Display::Flex,
    Display::Grid,
    Display::GridLanes,
    Display::Linear,
    Display::LynxRelative,
    Display::Contents,
];

#[test]
fn a_text_tag_keeps_none_and_lynx_text() {
    for tag in TEXT_TAGS {
        for display in [Display::None, Display::LynxText] {
            assert_eq!(
                adjusted_display(TestElement::named(tag), display),
                display,
                "{tag}: {display:?}"
            );
        }
    }
}

#[test]
fn a_text_tag_computes_every_other_display_to_lynx_text() {
    for tag in TEXT_TAGS {
        for display in OTHER_DISPLAYS {
            assert_eq!(
                adjusted_display(TestElement::named(tag), display),
                Display::LynxText,
                "{tag}: {display:?}"
            );
        }
    }
}

#[test]
fn other_tags_are_untouched() {
    for tag in ["div", "view", "texts", "raw-text"] {
        for display in OTHER_DISPLAYS.into_iter().chain([Display::LynxText]) {
            let expected = adjusted_display(TestElement::div(), display);
            assert_eq!(
                adjusted_display(TestElement::named(tag), display),
                expected,
                "{tag}: {display:?}"
            );
        }
    }
}
