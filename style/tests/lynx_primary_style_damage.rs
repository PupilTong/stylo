//! The embedder's primary-style damage hook under the `lynx` feature.
//!
//! `MatchMethods::compute_style_difference` — the diff the traversal runs
//! when it replaces an element's styles — consults
//! `TElement::compute_primary_style_damage` for every primary style it
//! replaces with a different one, whatever the longhand classification
//! answered, and never for an eager pseudo-element's style. The answer may
//! use only `ServoRestyleDamage::EMBEDDER`, which neither the classification
//! nor `ServoRestyleDamage::reconstruct` sets. The test element
//! (`support/test_element.rs`) answers one fixed embedder bit.
//!
//! `ElementDataWrapper::get` is the unguarded shared read an embedder hands
//! out `&ComputedValues` through without keeping a second owner of the
//! style; it is covered here too.
#![cfg(feature = "lynx")]

#[path = "support/test_element.rs"]
mod test_element;

use euclid::{Scale, Size2D};
use servo_arc::Arc;
use style::context::{CascadeInputs, QuirksMode, TreeCountingCaches};
use style::data::ElementDataWrapper;
use style::device::Device;
use style::device::servo::FontMetricsProvider;
use style::font_metrics::FontMetrics;
use style::matching::{MatchMethods, StyleChange};
use style::media_queries::{MediaList, MediaType};
use style::properties::{
    ComputedValues, FirstLineReparenting, PropertyDeclarationBlock, style_structs,
};
use style::queries::values::PrefersColorScheme;
use style::rule_cache::RuleCacheConditions;
use style::rule_tree::{CascadeLevel, CascadeOrigin, RuleCascadeFlags, StyleSource};
use style::selector_parser::PseudoElement;
use style::servo::media_features::PointerCapabilities;
use style::servo::restyle_damage::ServoRestyleDamage;
use style::shared_lock::{Locked, SharedRwLock, StylesheetGuards};
use style::stylesheets::layer_rule::LayerOrder;
use style::stylesheets::{
    AllowImportRules, CssRule, Origin, Stylesheet, StylesheetInDocument, UrlExtraData,
};
use style::stylist::Stylist;
use style::values::computed::CSSPixelLength;
use style::values::computed::font::GenericFontFamily;
use style::values::specified::font::QueryFontMetricsFlags;
use style_traits::{CSSPixel, DevicePixel};
use test_element::TestElement;

/// What the test element's hook answers.
const HOOK: ServoRestyleDamage = ServoRestyleDamage::from_bits_retain(1 << 8);

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

fn rule_block(lock: &SharedRwLock, css: &str) -> Arc<Locked<PropertyDeclarationBlock>> {
    let sheet = Stylesheet::from_str(
        css,
        url_data(),
        Origin::Author,
        Arc::new(lock.wrap(MediaList::empty())),
        lock.clone(),
        None,
        None,
        QuirksMode::NoQuirks,
        AllowImportRules::Yes,
    );
    let guard = lock.read();
    let rules = sheet.contents(&guard).rules(&guard);
    match rules[0] {
        CssRule::Style(ref rule) => rule.read_with(&guard).block.clone(),
        ref other => panic!("expected a style rule in `{css}`, got {other:?}"),
    }
}

/// The detached test element's style with `declarations` as its only rule.
fn style(declarations: &str) -> Arc<ComputedValues> {
    let lock = SharedRwLock::new();
    let stylist = Stylist::new(device(), QuirksMode::NoQuirks);
    let block = rule_block(&lock, &format!("div {{ {declarations} }}"));
    let guard = lock.read();
    let guards = StylesheetGuards::same(&guard);
    let rules = stylist.rule_tree().insert_ordered_rules(std::iter::once((
        StyleSource::from_declarations(block),
        style::applicable_declarations::CascadePriority::new(
            CascadeLevel::new(CascadeOrigin::Author),
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

fn primary_damage(old: &ComputedValues, new: &ComputedValues) -> ServoRestyleDamage {
    TestElement::div()
        .compute_style_difference(old, new, None)
        .damage
}

#[test]
fn a_custom_property_change_consults_the_hook() {
    let old = style("--limit: 1");
    let new = style("--limit: 2");
    let damage = primary_damage(&old, &new);
    assert!(damage.contains(HOOK), "{damage:?}");
    assert!(damage.contains(ServoRestyleDamage::REPAINT), "{damage:?}");
    assert!(!damage.contains(ServoRestyleDamage::RELAYOUT), "{damage:?}");
}

#[test]
fn a_relayout_change_consults_the_hook() {
    let damage = primary_damage(&style("width: 10px"), &style("width: 20px"));
    assert!(
        damage.contains(HOOK | ServoRestyleDamage::RELAYOUT),
        "{damage:?}"
    );
}

#[test]
fn an_equal_style_in_a_new_arc_still_consults_the_hook() {
    let old = style("width: 10px");
    let new = style("width: 10px");
    assert!(!std::ptr::eq(&*old, &*new));
    let difference = TestElement::div().compute_style_difference(&old, &new, None);
    assert_eq!(difference.damage, HOOK);
    assert!(matches!(difference.change, StyleChange::Unchanged));
}

#[test]
fn the_same_style_never_consults_the_hook() {
    let style = style("width: 10px");
    assert!(primary_damage(&style, &style).is_empty());
}

#[test]
fn a_pseudo_element_style_never_consults_the_hook() {
    let damage = TestElement::div()
        .compute_style_difference(
            &style("width: 10px"),
            &style("width: 20px"),
            Some(&PseudoElement::Before),
        )
        .damage;
    assert!(damage.contains(ServoRestyleDamage::RELAYOUT), "{damage:?}");
    assert!(
        !damage.intersects(ServoRestyleDamage::EMBEDDER),
        "{damage:?}"
    );
}

#[test]
fn only_the_hook_sets_the_embedder_bits() {
    assert!(!ServoRestyleDamage::reconstruct().intersects(ServoRestyleDamage::EMBEDDER));
    assert!(ServoRestyleDamage::reconstruct().contains(ServoRestyleDamage::RELAYOUT));
    for (old, new) in [
        ("width: 10px", "width: 20px"),
        ("display: none", "display: flex"),
        ("color: red", "color: blue"),
        ("--limit: 1", "--limit: 2"),
    ] {
        let damage = ServoRestyleDamage::compute_base_damage(&style(old), &style(new));
        assert!(
            !damage.intersects(ServoRestyleDamage::EMBEDDER),
            "{old} -> {new}: {damage:?}"
        );
    }
}

#[test]
fn get_reads_what_a_guard_wrote() {
    let wrapper = ElementDataWrapper::default();
    // SAFETY: no guard is alive across either read.
    assert!(unsafe { wrapper.get() }.styles.primary.is_none());
    let primary = style("width: 10px");
    wrapper.borrow_mut().styles.primary = Some(primary.clone());
    // SAFETY: as above; the guard died with the statement.
    let read = unsafe { wrapper.get() }.styles.primary.as_ref();
    assert!(read.is_some_and(|read| Arc::ptr_eq(read, &primary)));
}

#[cfg(debug_assertions)]
#[test]
#[should_panic]
fn get_under_a_live_mutable_guard_panics_in_debug() {
    let wrapper = ElementDataWrapper::default();
    let _guard = wrapper.borrow_mut();
    // SAFETY: deliberately violated; debug builds catch it before the read.
    let _ = unsafe { wrapper.get() };
}
