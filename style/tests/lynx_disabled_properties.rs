// Verifies that the CSS properties LynxJS does NOT support stop parsing from
// content under the `lynx` feature. Every name below is a real stylo property
// (so it parses in the default servo build) that is absent from Lynx's property
// set, and must therefore be rejected here. Gated behind `lynx`.
#![cfg(feature = "lynx")]

use style::properties::PropertyId;

/// Whether `name` parses as a content-enabled property.
fn is_content_enabled(name: &str) -> bool {
    PropertyId::parse_enabled_for_all_content(name).is_ok()
}

/// Properties that exist in stylo but that Lynx does not expose, grouped by the
/// reason Lynx omits them. All must be disabled under the `lynx` feature.
const NON_LYNX_PROPERTIES: &[&str] = &[
    // Floats / CSS tables (Lynx has neither).
    "float",
    "clear",
    "table-layout",
    "border-collapse",
    "border-spacing",
    "caption-side",
    "empty-cells",
    // List markers.
    "list-style",
    "list-style-type",
    "list-style-position",
    // Generated content / counters / quotes.
    "content",
    "quotes",
    "counter-increment",
    "counter-reset",
    // Outline.
    "outline",
    "outline-width",
    "outline-color",
    "outline-style",
    // Multicol.
    "columns",
    "column-count",
    "column-width",
    // Writing modes / bidi (Lynx is horizontal-only, uses `direction`).
    "writing-mode",
    "unicode-bidi",
    "text-orientation",
    // Block-logical box properties (Lynx exposes only the inline-logical ones).
    "inset-block-start",
    "margin-block-start",
    "padding-block-end",
    "border-block-start-color",
    "block-size",
    "inline-size",
    // Logical / physical shorthands Lynx does not list.
    "inset",
    "margin-inline",
    "padding-block",
    "border-inline",
    "border-image",
    // Effects / misc not in the Lynx property set.
    "backdrop-filter",
    "mix-blend-mode",
    "backface-visibility",
    "perspective-origin",
    "transform-style",
    "object-fit",
    "object-position",
    "isolation",
    "will-change",
    "contain",
    "appearance",
    "user-select",
    "zoom",
    "tab-size",
    "scrollbar-width",
    // Individual transform properties (Lynx only has the `transform` shorthand).
    "rotate",
    "scale",
    "translate",
    // Text bits Lynx does not expose.
    "text-transform",
    "text-justify",
    "word-spacing",
    "overflow-wrap",
    "caret-color",
    // Font longhands / shorthands outside the Lynx font set.
    "font",
    "font-variant",
    "font-stretch",
    "font-kerning",
    // Grid pieces Lynx does not list.
    "grid",
    "grid-area",
    "grid-template",
    "grid-template-areas",
    // Background/mask longhands outside the supported subset.
    "background-attachment",
    "background-blend-mode",
    "mask-repeat",
    "mask-position",
    // place-* shorthands.
    "place-content",
    "place-items",
    "place-self",
];

#[test]
fn non_lynx_properties_are_disabled() {
    let mut still_enabled = Vec::new();
    for &name in NON_LYNX_PROPERTIES {
        if is_content_enabled(name) {
            still_enabled.push(name);
        }
    }
    assert!(
        still_enabled.is_empty(),
        "these properties are not part of Lynx and must be disabled under the `lynx` feature, \
         but still parse: {still_enabled:?}",
    );
}
