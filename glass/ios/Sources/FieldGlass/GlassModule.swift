// `field-glass:Glass`・`TabBar`・`MenuList`・`RecordList` の登録。Rust 側の `NativeGlass` と名前・属性をそろえる。

import CoreGraphics
import WhiskerModule

@WhiskerModule
public final class GlassModule: Module {
    public override func definition() -> ModuleDefinition {
        ModuleDefinition {
            Name("Glass")
            View("field-glass:Glass", FieldGlassView.self) {
                Prop("corner-radius") { (view: FieldGlassView, value: WhiskerValue) in
                    // Rust からは文字列で届くことがあるので、数でも文字でも読む。
                    let radius = value.asDouble ?? Double(value.asString ?? "") ?? 0
                    view.setCornerRadius(CGFloat(radius))
                }
            }
            View("field-glass:TabBar", FieldTabBarView.self) {
                Prop("items") { (view: FieldTabBarView, value: WhiskerValue) in
                    view.setItems(value.asString ?? "")
                }
                Prop("icons") { (view: FieldTabBarView, value: WhiskerValue) in
                    view.setIcons(value.asString ?? "")
                }
                Prop("selected") { (view: FieldTabBarView, value: WhiskerValue) in
                    let index = value.asInt.map { Int($0) } ?? Int(value.asString ?? "") ?? 0
                    view.setSelected(index)
                }
                Events("select")
            }
            View("field-glass:MenuList", FieldMenuListView.self) {
                Prop("sections") { (view: FieldMenuListView, value: WhiskerValue) in
                    view.setSections(value.asString ?? "[]")
                }
                Events("select")
            }
            View("field-glass:RecordList", FieldRecordListView.self) {
                Prop("rows") { (view: FieldRecordListView, value: WhiskerValue) in
                    view.setRows(value.asString ?? "[]")
                }
                Prop("header") { (view: FieldRecordListView, value: WhiskerValue) in
                    view.setHeader(value.asString ?? "")
                }
                Prop("message") { (view: FieldRecordListView, value: WhiskerValue) in
                    view.setMessage(value.asString ?? "")
                }
                Prop("placeholder") { (view: FieldRecordListView, value: WhiskerValue) in
                    view.setPlaceholder(value.asString ?? "")
                }
                Prop("refreshing") { (view: FieldRecordListView, value: WhiskerValue) in
                    view.setRefreshing(value.asBool ?? (value.asString == "true"))
                }
                Prop("bottom-inset") { (view: FieldRecordListView, value: WhiskerValue) in
                    let inset = value.asDouble ?? Double(value.asString ?? "") ?? 0
                    view.setBottomInset(CGFloat(inset))
                }
                Events("select", "search", "refresh")
            }
            View("field-glass:Sheet", FieldSheetView.self) {
                Prop("open") { (view: FieldSheetView, value: WhiskerValue) in
                    view.setOpen(value.asBool ?? (value.asString == "true"))
                }
                Prop("title") { (view: FieldSheetView, value: WhiskerValue) in
                    view.setTitle(value.asString ?? "")
                }
                Prop("sections") { (view: FieldSheetView, value: WhiskerValue) in
                    view.setSections(value.asString ?? "[]")
                }
                Events("select", "dismiss")
            }
            View("field-glass:Backdrop", FieldBackdropView.self) {}
            View("field-glass:KeyboardObserver", FieldKeyboardObserverView.self) {
                Events("change")
            }
        }
    }
}
