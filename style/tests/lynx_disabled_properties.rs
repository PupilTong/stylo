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
    // Background/mask pieces outside the supported subset. NOTE:
    // `background-attachment` and `mask-repeat` are NOT here — they are
    // sub-longhands of the supported `background`/`mask` shorthands, which
    // data.py keeps enabled so those shorthands can serialize and their
    // standalone wire ids can ingest (see LYNX_SUPPORTED). `mask-position`
    // stays disabled: it is itself a shorthand (over mask-position-x/y) and
    // not in the Lynx list.
    "background-blend-mode",
    "mask-position",
    // place-* shorthands.
    "place-content",
    "place-items",
    "place-self",
];

/// Sub-longhands of supported shorthands stay enabled even when Lynx does
/// not document them individually: shorthand parsing writes them, CSSOM-style
/// serialization iterates only enabled ones, and real `.web.bundle`s carry
/// some of their standalone ids — see the propagation loop in data.py.
const SHORTHAND_CARRIED_LONGHANDS: &[&str] = &[
    "background-attachment",   // via `background`
    "background-position-x",   // via `background-position`
    "background-position-y",   // via `background-position`
    "text-decoration-color",   // via `text-decoration` (wire id 148)
    "text-decoration-line",    // via `text-decoration`
    "text-decoration-style",   // via `text-decoration`
    "mask-repeat",             // via `mask`
    "border-image-source",     // via `border`
];

#[test]
fn shorthand_carried_longhands_stay_enabled() {
    let mut disabled = Vec::new();
    for &name in SHORTHAND_CARRIED_LONGHANDS {
        if !is_content_enabled(name) {
            disabled.push(name);
        }
    }
    assert!(
        disabled.is_empty(),
        "sub-longhands of supported shorthands must stay content-enabled \
         (data.py propagation), but these are disabled: {disabled:?}",
    );
}

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
