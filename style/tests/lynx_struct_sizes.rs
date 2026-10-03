//! Style-struct sizes under the `lynx` feature.
//!
//! The layout engine reads `Box` and `Position` for every box in its hot
//! loops, and their field offsets show up in its benchmarks: growing either
//! struct moved the `relative_*` fields and cost 7–9% on the relative-layout
//! benches. css-anchor-position-1's element-level properties therefore live
//! in their own lynx-only reset struct, `Anchor`; only `position-area` stays
//! in `Position`, where the style adjuster and the try tactics set it.
//! Raising a ceiling here needs a measured reason.
#![cfg(feature = "lynx")]

use std::mem::size_of;
use style::properties::style_structs;

#[test]
fn hot_structs_stay_within_their_ceilings() {
    let position = size_of::<style_structs::Position>();
    let box_ = size_of::<style_structs::Box>();
    let anchor = size_of::<style_structs::Anchor>();
    println!("Position = {position} B, Box = {box_} B, Anchor = {anchor} B");
    assert!(
        position <= 472,
        "style_structs::Position is {position} bytes"
    );
    assert!(box_ <= 312, "style_structs::Box is {box_} bytes");
}

#[test]
fn anchor_properties_keep_their_accessor_names() {
    use style::properties::ComputedValues;
    use style::values::specified::position::{PositionAnchorKeyword, PositionVisibility};
    let style =
        ComputedValues::initial_values_with_font_override(style_structs::Font::initial_values());
    // The `ComputedValues` accessors the layout engine calls are unchanged.
    assert!(style.clone_anchor_name().value.0.is_empty());
    assert!(style.clone_anchor_scope().is_none());
    assert!(matches!(
        style.clone_position_anchor().value,
        PositionAnchorKeyword::Normal
    ));
    assert!(style.clone_position_try_fallbacks().value.is_none());
    assert!(style.clone_position_try_order().is_normal());
    assert_eq!(
        style.clone_position_visibility(),
        PositionVisibility::ANCHOR_VISIBLE
    );
    // Struct-level access goes through `get_anchor()` / `mutate_anchor()`;
    // `position-area` stays on `get_position()`.
    let anchor = style.get_anchor();
    assert!(anchor.clone_position_try_fallbacks().value.is_none());
    assert!(style.get_position().clone_position_area().is_none());
}
