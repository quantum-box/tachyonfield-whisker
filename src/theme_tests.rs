use super::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use whisker::runtime::reactive::{__reset_for_tests, flush, Owner};
use whisker::runtime::view::{BindType, DynRenderer, Element, install_renderer, uninstall_renderer};
use whisker_engine::whisker_style::SpecifiedStyle;

#[derive(Default)]
struct Recorder {
    next: Cell<u32>,
    created: Rc<Cell<usize>>,
    removed: Rc<Cell<usize>>,
    styles: Rc<RefCell<Vec<(u32, SpecifiedStyle)>>>,
}
impl DynRenderer for Recorder {
    fn create_element(&self, _: ElementTag) -> Element {
        let id = self.next.get(); self.next.set(id + 1);
        self.created.set(self.created.get() + 1);
        Element::from_raw(id)
    }
    fn create_element_by_name(&self, _: &str) -> Element { self.create_element(ElementTag::View) }
    fn release_element(&self, _: Element) {}
    fn set_attribute(&self, _: Element, _: &str, _: &str) {}
    fn set_specified_style(&self, h: Element, style: &SpecifiedStyle) -> bool {
        self.styles.borrow_mut().push((h.id(), style.clone())); true
    }
    fn append_child(&self, _: Element, _: Element) {}
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
