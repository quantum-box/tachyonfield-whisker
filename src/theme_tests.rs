use super::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use whisker::runtime::reactive::{__reset_for_tests, flush, Owner};
use whisker::runtime::view::{BindType, DynRenderer, Element, install_renderer, uninstall_renderer};
use whisker::runtime::{RuntimeContext, RuntimeWakeHandle};
use whisker_engine::whisker_style::{SpecifiedStyle, StyleProperty, StyleValue, TextAlignValue};

#[derive(Default)]
struct Recorder {
    next: Cell<u32>,
    created: Rc<Cell<usize>>,
    removed: Rc<Cell<usize>>,
    styles: Rc<RefCell<Vec<(u32, SpecifiedStyle)>>>,
    text_nodes: Rc<RefCell<Vec<(u32, String)>>>,
    children: Rc<RefCell<Vec<(u32, u32)>>>,
}
impl DynRenderer for Recorder {
    fn create_element(&self, _: ElementTag) -> Element {
        let id = self.next.get(); self.next.set(id + 1);
        self.created.set(self.created.get() + 1);
        Element::from_raw(id)
    }
    fn create_element_by_name(&self, _: &str) -> Element { self.create_element(ElementTag::View) }
    fn release_element(&self, _: Element) {}
    fn set_attribute(&self, element: Element, key: &str, value: &str) {
        if key == "text" {
            self.text_nodes
                .borrow_mut()
                .push((element.id(), value.to_owned()));
        }
    }
    fn set_specified_style(&self, h: Element, style: &SpecifiedStyle) -> bool {
        self.styles.borrow_mut().push((h.id(), style.clone())); true
    }
    fn append_child(&self, parent: Element, child: Element) {
        self.children.borrow_mut().push((parent.id(), child.id()));
    }
    fn remove_child(&self, _: Element, _: Element) { self.removed.set(self.removed.get() + 1); }
    fn set_event_listener(&self, _: Element, _: &str, _: BindType, _: Box<dyn Fn(whisker::WhiskerValue) + 'static>) {}
    fn set_root(&self, _: Element) {}
    fn flush(&self) {}
}

type StateProbe = Rc<RefCell<Option<(RwSignal<String>, RwSignal<bool>)>>>;

#[component]
fn stateful_fixture(probe: StateProbe) -> Element {
    let draft = signal(String::from("unsaved"));
    let modal = signal(true);
    *probe.borrow_mut() = Some((draft, modal));
    render! {
        View(style: computed(move || group_style())) {
            Text(value: computed(move || draft.get()))
            Show(when: move || modal.get()) { Text(value: "modal") }
        }
    }
}

#[test]
fn appearance_updates_style_without_remounting_state() {
    __reset_for_tests();
    let recorder = Recorder::default();
    let created = recorder.created.clone();
    let removed = recorder.removed.clone();
    let styles = recorder.styles.clone();
    let previous = install_renderer(Box::new(recorder));
    let owner = Owner::new(None);
    owner.with(|| {
        let dark = signal(None::<bool>);
        let wide = signal(None::<bool>);
        provide_context(Appearance(dark));
        let probe: StateProbe = Rc::new(RefCell::new(None));
        let fixture_probe = probe.clone();
        let _tree = render! {
            AppearanceContent(dark: dark, wide: wide) {
                StatefulFixture(probe: fixture_probe.clone())
            }
        };
        dark.set(Some(false)); flush();
        assert!(probe.borrow().is_none(), "wait for layout as well as appearance");
        wide.set(Some(false)); flush();
        let (draft, modal) = probe.borrow().expect("child mounted");
        draft.set("edited after mount".to_owned());
        flush();
        let count = created.get();
        let light = styles.borrow().last().unwrap().clone();
        dark.set(Some(true)); flush();
        let dark_style = styles.borrow().last().unwrap().clone();
        assert_eq!(light.0, dark_style.0, "style must update the same element");
        assert_ne!(light.1, dark_style.1, "theme must change the style");
        assert_eq!(created.get(), count, "theme change must not recreate the tree");
        assert_eq!(removed.get(), 0, "theme change must not remove the tree");
        assert_eq!(draft.get(), "edited after mount");
        assert!(modal.get());
        wide.set(Some(true)); flush();
        assert_eq!(created.get(), count, "layout changes must preserve child state");
        assert_eq!(draft.get(), "edited after mount");
        dark.set(Some(false)); flush();
        assert_eq!(styles.borrow().last().unwrap(), &light);
        assert_eq!(created.get(), count);
        assert_eq!(removed.get(), 0);
    });
    owner.dispose();
    uninstall_renderer(previous);
}

#[test]
fn layout_can_arrive_before_appearance() {
    __reset_for_tests();
    let recorder = Recorder::default();
    let created = recorder.created.clone();
    let previous = install_renderer(Box::new(recorder));
    let owner = Owner::new(None);
    owner.with(|| {
        let dark = signal(None::<bool>);
        let wide = signal(None::<bool>);
        provide_context(Appearance(dark));
        let probe: StateProbe = Rc::new(RefCell::new(None));
        let fixture_probe = probe.clone();
        let _tree = render! {
            AppearanceContent(dark: dark, wide: wide) {
                StatefulFixture(probe: fixture_probe.clone())
            }
        };
        wide.set(Some(true)); flush();
        assert!(probe.borrow().is_none());
        dark.set(Some(true)); flush();
        let (draft, _) = probe.borrow().expect("child mounted after both values");
        let count = created.get();
        draft.set("still editing".to_owned());
        wide.set(Some(false));
        dark.set(Some(false));
        flush();
        assert_eq!(draft.get(), "still editing");
        assert_eq!(created.get(), count);
    });
    owner.dispose();
    uninstall_renderer(previous);
}

#[test]
fn sign_in_title_centers_wrapped_text() {
    let runtime = RuntimeContext::new(RuntimeWakeHandle::new(|| {}));
    runtime.enter(|| {
        let recorder = Recorder::default();
        let text_nodes = recorder.text_nodes.clone();
        let children = recorder.children.clone();
        let styles = recorder.styles.clone();
        let previous = install_renderer(Box::new(recorder));
        let owner = Owner::new(None);
        owner.with(|| {
            let token = signal(None::<String>);
            let _tree = render! { Login(token: token) };
            flush();
        });

        let title_text = text_nodes
            .borrow()
            .iter()
            .find_map(|(id, text)| (text == "TACHYON Field").then_some(*id))
            .expect("sign-in title text");
        let title = children
            .borrow()
            .iter()
            .find_map(|(parent, child)| (*child == title_text).then_some(*parent))
            .expect("sign-in title element");
        assert!(styles.borrow().iter().any(|(id, style)| {
            *id == title
                && style.declarations().any(|declaration| {
                    declaration.property() == StyleProperty::TextAlign
                        && declaration.value() == &StyleValue::TextAlign(TextAlignValue::Center)
                })
        }));
        owner.dispose();
        uninstall_renderer(previous);
    });
    runtime.shutdown();
}
