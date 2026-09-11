// Verifies the project seed and shorthand/longhand closure generated for Lynx.
#![cfg(feature = "lynx")]

use style::properties::PropertyId;

/// Whether `name` parses as a content-enabled property (the gate every author
/// stylesheet / CSSOM `setProperty` funnels through).
fn is_content_enabled(name: &str) -> bool {
    PropertyId::parse_enabled_for_all_content(name).is_ok()
}

const LYNX_PROPERTY_SEEDS: &str = include_str!("../properties/lynx_properties.txt");
const OMITTED_PROPERTIES: &[&str] = &[
    "-x-auto-font-size",
    "-x-auto-font-size-line-ranges",
    "-x-auto-font-size-preset-sizes",
    "-x-caret-gradient",
    "-x-caret-height",
    "-x-caret-radius",
    "-x-caret-width",
    "-x-handle-color",
    "-x-handle-size",
    "linear-cross-gravity",
    "linear-gravity",
    "linear-layout-gravity",
    // Lynx has no outline-offset (outlines are flush rings; see
    // lynx/core/style/outline_data.h) and it is not part of the `outline`
    // shorthand's closure — pin it out so a codegen change cannot expose it.
    "outline-offset",
];

fn lynx_property_seeds() -> impl Iterator<Item = &'static str> {
    LYNX_PROPERTY_SEEDS.lines().filter_map(|raw| {
        let name = raw.split('#').next().unwrap().trim();
        (!name.is_empty()).then_some(name)
    })
}

#[test]
fn lynx_supported_properties_are_content_enabled() {
    for name in lynx_property_seeds() {
        assert!(
            is_content_enabled(name),
            "`{name}` is Lynx-supported and must stay content-enabled under the `lynx` feature",
        );
    }
}

#[test]
fn deliberately_omitted_properties_are_absent() {
    for name in OMITTED_PROPERTIES {
        assert!(
            !is_content_enabled(name),
            "`{name}` is deliberately omitted from the Lynx property source"
        );
    }
}

#[test]
fn shorthand_longhand_closure_is_authorable() {
    // Each representative starts outside the official seed list and is pulled
    // in by a supported shorthand or longhand relation.
    for name in [
        "animation-range-end",
        "animation-range-start",
        "animation-timeline",
        "background-attachment",
        "border-image",
        "border-image-source",
        // The `contain-intrinsic-size` shorthand is seeded; its physical
        // longhands are pulled in by the closure (the logical block/inline pair
        // stays disabled — see lynx_disabled_properties / lynx_containment).
        "contain-intrinsic-height",
        "contain-intrinsic-width",
        "font",
        "font-kerning",
        "font-stretch",
        "font-variant",
        "grid",
        "grid-area",
        "grid-template",
        "grid-template-areas",
        "inset",
        "mask-position",
        "place-content",
        "place-items",
        "place-self",
        "text-decoration-color",
        "text-decoration-line",
        "text-decoration-style",
        "transition-behavior",
    ] {
        assert!(
            is_content_enabled(name),
            "`{name}` belongs to the shorthand closure"
        );
    }
}

#[test]
fn internal_storage_longhands_are_not_authorable() {
    // Internal storage exists only to satisfy Stylo invariants. Anything
    // reached through the supported shorthand/longhand closure belongs in
    // `shorthand_longhand_closure_is_authorable` instead.
    for name in [
        "-moz-default-appearance",
        "-servo-top-layer",
        "animation-composition",
        "color-scheme",
    ] {
        assert!(
            !is_content_enabled(name),
            "internal storage property `{name}` must stay out of the Lynx name table"
        );
    }
}

#[test]
fn undocumented_canonical_spellings_do_not_leak_through_aliases() {
    // These declarations are compiled because the documented unprefixed alias
    // maps to them, but only the Lynx spelling belongs in the name table.
    for name in [
        "-webkit-text-stroke",
        "-webkit-text-stroke-color",
        "-webkit-text-stroke-width",
        "all",
    ] {
        assert!(!is_content_enabled(name), "`{name}` is not a Lynx spelling");
    }
}

#[test]
fn custom_properties_are_still_supported() {
    // Custom properties go through a different id space and must be unaffected.
    assert!(is_content_enabled("--lynx-custom"));
}

#[test]
fn containment_hints_are_content_enabled() {
    for name in ["contain", "will-change"] {
        assert!(
            is_content_enabled(name),
            "the standard `{name}` property must be content-enabled"
        );
    }
}
