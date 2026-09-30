/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! A detached, childless element with no attributes, no state and no style
//! data: the smallest `TElement` a test can hand to the `Stylist` entry points
//! that cascade on an element's behalf without matching or traversing
//! (`Stylist::cascade_style_and_visited`, `Stylist::resolve_position_try`).
//!
//! Every tree query answers "none", so the element is a root in the same
//! tree as the author stylesheets; anything a cascade should never reach
//! (style data, snapshots, traversal bookkeeping) panics.

use dom::ElementState;
use selectors::attr::{AttrSelectorOperation, CaseSensitivity, NamespaceConstraint};
use selectors::bloom::BloomFilter;
use selectors::matching::{ElementSelectorFlags, MatchingContext, VisitedHandlingMode};
use selectors::sink::Push;
use selectors::OpaqueElement;
use servo_arc::{Arc, ArcBorrow};
use std::sync::LazyLock;
use style::applicable_declarations::ApplicableDeclarationBlock;
use style::context::SharedStyleContext;
use style::data::{ElementDataMut, ElementDataRef};
use style::dom::{LayoutIterator, NodeInfo, OpaqueNode, TDocument, TElement, TNode, TShadowRoot};
use style::properties::PropertyDeclarationBlock;
use style::selector_parser::{AttrValue, Lang, PseudoElement, SelectorImpl};
use style::shared_lock::{Locked, SharedRwLock};
use style::stylist::CascadeData;
use style::values::computed::Display;
use style::values::AtomIdent;
use style::{Atom as WeakAtom, LocalName, Namespace};

type BorrowedLocalName = <SelectorImpl as selectors::parser::SelectorImpl>::BorrowedLocalName;
type BorrowedNamespace = <SelectorImpl as selectors::parser::SelectorImpl>::BorrowedNamespaceUrl;

static SHARED_LOCK: LazyLock<SharedRwLock> = LazyLock::new(SharedRwLock::new);
static LOCAL_NAME: LazyLock<BorrowedLocalName> = LazyLock::new(|| BorrowedLocalName::from("div"));
static NAMESPACE: LazyLock<BorrowedNamespace> =
    LazyLock::new(|| BorrowedNamespace::from("http://www.w3.org/1999/xhtml"));
static IDENTITY: u8 = 0;

/// The element.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct TestElement;

/// Its node.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TestNode;

/// Its (never reached) document.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TestDocument;

/// No shadow roots exist.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TestShadowRoot {}

impl NodeInfo for TestNode {
    fn is_element(&self) -> bool {
        true
    }

    fn is_text_node(&self) -> bool {
        false
    }
}

impl TNode for TestNode {
    type ConcreteElement = TestElement;
    type ConcreteDocument = TestDocument;
    type ConcreteShadowRoot = TestShadowRoot;

    fn parent_node(&self) -> Option<Self> {
        None
    }

    fn first_child(&self) -> Option<Self> {
        None
    }

    fn last_child(&self) -> Option<Self> {
        None
    }

    fn prev_sibling(&self) -> Option<Self> {
        None
    }

    fn next_sibling(&self) -> Option<Self> {
        None
    }

    fn owner_doc(&self) -> TestDocument {
        TestDocument
    }

    fn is_in_document(&self) -> bool {
        false
    }

    fn traversal_parent(&self) -> Option<TestElement> {
        None
    }

    fn opaque(&self) -> OpaqueNode {
        OpaqueNode(&IDENTITY as *const u8 as usize)
    }

    fn debug_id(self) -> usize {
        0
    }

    fn as_element(&self) -> Option<TestElement> {
        Some(TestElement)
    }

    fn as_document(&self) -> Option<TestDocument> {
        None
    }

    fn as_shadow_root(&self) -> Option<TestShadowRoot> {
        None
    }
}

impl TDocument for TestDocument {
    type ConcreteNode = TestNode;

    fn as_node(&self) -> TestNode {
        unreachable!("the test element has no document node")
    }

    fn is_html_document(&self) -> bool {
        true
    }

    fn quirks_mode(&self) -> style::context::QuirksMode {
        style::context::QuirksMode::NoQuirks
    }

    fn shared_lock(&self) -> &SharedRwLock {
        &SHARED_LOCK
    }
}

impl TShadowRoot for TestShadowRoot {
    type ConcreteNode = TestNode;

    fn as_node(&self) -> TestNode {
        match *self {}
    }

    fn host(&self) -> TestElement {
        match *self {}
    }

    fn style_data<'a>(&self) -> Option<&'a CascadeData>
    where
        Self: 'a,
    {
        match *self {}
    }
}

impl selectors::Element for TestElement {
    type Impl = SelectorImpl;

    fn opaque(&self) -> OpaqueElement {
        OpaqueElement::new(&IDENTITY)
    }

    fn parent_element(&self) -> Option<Self> {
        None
    }

    fn parent_node_is_shadow_root(&self) -> bool {
        false
    }

    fn containing_shadow_host(&self) -> Option<Self> {
        None
    }

    fn is_pseudo_element(&self) -> bool {
        false
    }

    fn prev_sibling_element(&self) -> Option<Self> {
        None
    }

    fn next_sibling_element(&self) -> Option<Self> {
        None
    }

    fn first_element_child(&self) -> Option<Self> {
        None
    }

    fn is_html_element_in_html_document(&self) -> bool {
        true
    }

    fn has_local_name(&self, local_name: &BorrowedLocalName) -> bool {
        *local_name == *LOCAL_NAME
    }

    fn has_namespace(&self, ns: &BorrowedNamespace) -> bool {
        *ns == *NAMESPACE
    }

    fn is_same_type(&self, _: &Self) -> bool {
        true
    }

    fn attr_matches(
        &self,
        _: &NamespaceConstraint<&<SelectorImpl as selectors::parser::SelectorImpl>::NamespaceUrl>,
        _: &<SelectorImpl as selectors::parser::SelectorImpl>::LocalName,
        _: &AttrSelectorOperation<&<SelectorImpl as selectors::parser::SelectorImpl>::AttrValue>,
    ) -> bool {
        false
    }

    fn match_non_ts_pseudo_class(
        &self,
        _: &<SelectorImpl as selectors::parser::SelectorImpl>::NonTSPseudoClass,
        _: &mut MatchingContext<SelectorImpl>,
    ) -> bool {
        false
    }

    fn match_pseudo_element(
        &self,
        _: &PseudoElement,
        _: &mut MatchingContext<SelectorImpl>,
    ) -> bool {
        false
    }

    fn apply_selector_flags(&self, _: ElementSelectorFlags) {}

    fn is_link(&self) -> bool {
        false
    }

    fn is_html_slot_element(&self) -> bool {
        false
    }

    fn has_id(&self, _: &AtomIdent, _: CaseSensitivity) -> bool {
        false
    }

    fn has_class(&self, _: &AtomIdent, _: CaseSensitivity) -> bool {
        false
    }

    fn has_custom_state(&self, _: &AtomIdent) -> bool {
        false
    }

    fn imported_part(&self, _: &AtomIdent) -> Option<AtomIdent> {
        None
    }

    fn is_part(&self, _: &AtomIdent) -> bool {
        false
    }

    fn is_empty(&self) -> bool {
        true
    }

    fn is_root(&self) -> bool {
        false
    }

    fn add_element_unique_hashes(&self, _: &mut BloomFilter) -> bool {
        false
    }
}

impl TElement for TestElement {
    type ConcreteNode = TestNode;
    type TraversalChildrenIterator = std::iter::Empty<TestNode>;

    fn as_node(&self) -> TestNode {
        TestNode
    }

    fn traversal_children(&self) -> LayoutIterator<Self::TraversalChildrenIterator> {
        LayoutIterator(std::iter::empty())
    }

    fn is_html_element(&self) -> bool {
        true
    }

    fn is_mathml_element(&self) -> bool {
        false
    }

    fn is_svg_element(&self) -> bool {
        false
    }

    fn style_attribute(&self) -> Option<ArcBorrow<'_, Locked<PropertyDeclarationBlock>>> {
        None
    }

    fn animation_rule(
        &self,
        _: &SharedStyleContext,
    ) -> Option<Arc<Locked<PropertyDeclarationBlock>>> {
        None
    }

    fn transition_rule(
        &self,
        _: &SharedStyleContext,
    ) -> Option<Arc<Locked<PropertyDeclarationBlock>>> {
        None
    }

    fn state(&self) -> ElementState {
        ElementState::empty()
    }

    fn has_part_attr(&self) -> bool {
        false
    }

    fn exports_any_part(&self) -> bool {
        false
    }

    fn id(&self) -> Option<&WeakAtom> {
        None
    }

    fn each_class<F>(&self, _: F)
    where
        F: FnMut(&AtomIdent),
    {
    }

    fn each_custom_state<F>(&self, _: F)
    where
        F: FnMut(&AtomIdent),
    {
    }

    fn each_attr_name<F>(&self, _: F)
    where
        F: FnMut(&LocalName),
    {
    }

    fn has_dirty_descendants(&self) -> bool {
        false
    }

    fn has_snapshot(&self) -> bool {
        false
    }

    fn handled_snapshot(&self) -> bool {
        false
    }

    unsafe fn set_handled_snapshot(&self) {
        unreachable!("the test element is never traversed")
    }

    unsafe fn set_dirty_descendants(&self) {
        unreachable!("the test element is never traversed")
    }

    unsafe fn unset_dirty_descendants(&self) {
        unreachable!("the test element is never traversed")
    }

    fn store_children_to_process(&self, _: isize) {
        unreachable!("the test element is never traversed")
    }

    fn did_process_child(&self) -> isize {
        unreachable!("the test element is never traversed")
    }

    unsafe fn ensure_data(&self) -> ElementDataMut<'_> {
        unreachable!("the test element keeps no style data")
    }

    unsafe fn clear_data(&self) {}

    fn has_data(&self) -> bool {
        false
    }

    fn borrow_data(&self) -> Option<ElementDataRef<'_>> {
        None
    }

    fn mutate_data(&self) -> Option<ElementDataMut<'_>> {
        None
    }

    fn skip_item_display_fixup(&self) -> bool {
        false
    }

    fn may_have_animations(&self) -> bool {
        false
    }

    fn has_animations(&self, _: &SharedStyleContext) -> bool {
        false
    }

    fn has_css_animations(&self, _: &SharedStyleContext, _: Option<PseudoElement>) -> bool {
        false
    }

    fn has_css_transitions(&self, _: &SharedStyleContext, _: Option<PseudoElement>) -> bool {
        false
    }

    fn shadow_root(&self) -> Option<TestShadowRoot> {
        None
    }

    fn containing_shadow(&self) -> Option<TestShadowRoot> {
        None
    }

    fn lang_attr(&self) -> Option<AttrValue> {
        None
    }

    fn match_element_lang(&self, _: Option<Option<AttrValue>>, _: &Lang) -> bool {
        false
    }

    fn is_html_document_body_element(&self) -> bool {
        false
    }

    fn synthesize_presentational_hints_for_legacy_attributes<V>(
        &self,
        _: VisitedHandlingMode,
        _: &mut V,
    ) where
        V: Push<ApplicableDeclarationBlock>,
    {
    }

    fn local_name(&self) -> &BorrowedLocalName {
        &LOCAL_NAME
    }

    fn namespace(&self) -> &BorrowedNamespace {
        &NAMESPACE
    }

    fn query_container_size(&self, _: &Display) -> euclid::default::Size2D<Option<app_units::Au>> {
        euclid::default::Size2D::new(None, None)
    }

    fn has_selector_flags(&self, _: ElementSelectorFlags) -> bool {
        false
    }

    fn relative_selector_search_direction(&self) -> ElementSelectorFlags {
        ElementSelectorFlags::empty()
    }

    fn get_attr(&self, _: &LocalName, _: &Namespace) -> Option<String> {
        None
    }
}
