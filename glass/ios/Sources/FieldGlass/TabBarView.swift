// iOS 標準のタブバー（UITabBar）。iOS 26 以降は本物の Liquid Glass で描かれ、
// 選んだ印がガラスの粒のように滑って移動する。
//
// Rust からは「|」区切りの文字で受け取る：
// - items    … タブの名前（例：取引先|受注|請求書）
// - icons    … SF Symbols の名前（例：person.2|cart|doc.text）
// - selected … 選んでいるタブの番号。-1 ならどれも選ばない
// タブが押されたら "select" を { index } 付きで Rust に知らせる。

#if canImport(UIKit)
import UIKit
import WhiskerModule

/// タブバーを入れる箱。画面に載ったときと大きさが変わったときに、
/// タブバーの大きさと中身を付け直す。
private final class TabBarHost: UIView {
    let bar = UITabBar()
    var onAttach: (() -> Void)?

    override init(frame: CGRect) {
        super.init(frame: frame)
        addSubview(bar)
    }

    required init?(coder: NSCoder) { nil }

    override func layoutSubviews() {
        super.layoutSubviews()
        bar.frame = bounds
    }

    override func didMoveToWindow() {
        super.didMoveToWindow()
        if window != nil { onAttach?() }
    }
}

@objc(FieldTabBarView)
public final class FieldTabBarView: WhiskerUI<UIView>, UITabBarDelegate {
    private var labels: [String] = []
    private var icons: [String] = []
    private var selectedIndex: Int = 0

    private lazy var host: TabBarHost = {
        let host = TabBarHost()
        host.bar.delegate = self
        host.onAttach = { [weak self] in self?.rebuild() }
        return host
    }()

    @objc public override func createView() -> UIView {
        host
    }

    func setItems(_ text: String) {
        labels = split(text)
        rebuild()
    }

    func setIcons(_ text: String) {
        icons = split(text)
        rebuild()
    }

    func setSelected(_ index: Int) {
        selectedIndex = index
        applySelection()
    }

    public func tabBar(_ tabBar: UITabBar, didSelect item: UITabBarItem) {
        WhiskerCustomEvent.dispatch(from: self, name: "select", params: ["index": item.tag])
    }

    private func split(_ text: String) -> [String] {
        text.isEmpty ? [] : text.components(separatedBy: "|")
    }

    private func rebuild() {
        let items = labels.enumerated().map { index, label -> UITabBarItem in
            let symbol = index < icons.count ? icons[index] : ""
            return UITabBarItem(title: label, image: UIImage(systemName: symbol), tag: index)
        }
        host.bar.setItems(items, animated: false)
        applySelection()
        host.setNeedsLayout()
    }

    private func applySelection() {
        let items = host.bar.items ?? []
        host.bar.selectedItem = items.indices.contains(selectedIndex) ? items[selectedIndex] : nil
    }
}
#endif
