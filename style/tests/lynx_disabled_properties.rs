// Verifies that properties omitted from the Lynx grammar are absent from the
// generated author name table. Every name below is a real upstream property.
#![cfg(feature = "lynx")]

use style::properties::PropertyId;

/// Whether `name` parses as a content-enabled property.
fn is_content_enabled(name: &str) -> bool {
    PropertyId::parse_enabled_for_all_content(name).is_ok()
}

/// Properties that exist in stylo but that Lynx does not expose, grouped by the
/// reason Lynx omits them. All must be disabled under the `lynx` feature.
const NON_LYNX_UPSTREAM_PROPERTIES: &[&str] = &[
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
    // Logical properties outside both the project seed and its shorthand
    // closure.
    "padding-block",
    // Effects / misc not in the Lynx property set.
    "backdrop-filter",
    "mix-blend-mode",
    "backface-visibility",
    "perspective-origin",
    "transform-style",
    "object-fit",
    "object-position",
    "isolation",
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
    // Background pieces outside the supported subset.
    "background-blend-mode",
    // SVG paint properties are now content-enabled upstream for Servo, but
    // remain outside Lynx's author property surface.
    "fill",
    "fill-opacity",
    "fill-rule",
    "stroke",
    "stroke-width",
    "stroke-linecap",
    "stroke-linejoin",
    "stroke-dasharray",
    "stroke-dashoffset",
    "stroke-miterlimit",
    "stroke-opacity",
];

#[test]
fn unsupported_properties_are_disabled() {
    let mut still_enabled = Vec::new();
    for &name in NON_LYNX_UPSTREAM_PROPERTIES {
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
