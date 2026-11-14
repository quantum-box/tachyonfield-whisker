// 画面の上の帯。iOS 標準の UINavigationBar をそのまま使う（設定アプリなどと同じ帯）。
// 題名・戻るボタン・左右のボタンは iOS が描くので、ダークモードや Liquid Glass のボタンも iOS 任せ。
//
// Rust から受け取るもの（どれも文字）：
// - title    … 真ん中の題名（空なら出さない）
// - back     … 戻る先の名前。空でなければ左に iOS の「‹ 名前」の戻るボタンを出す
// - menu     … "true" なら左に「☰」（裏のメニューを開く）。戻るボタンがあるときは出さない
// - action   … 右のボタンの文字（空なら出さない）
// Rust へ知らせるもの：
// - "press" { which } … "back"（戻る）/ "menu"（☰）/ "action"（右のボタン）が押された

#if canImport(UIKit)
import UIKit
import WhiskerModule

/// 帯を入れる箱。帯は箱の大きさいっぱいに広げる。
private final class NavBarHost: UIView, UINavigationBarDelegate {
    let bar = UINavigationBar()
    let current = UINavigationItem()
    /// 戻るボタンを出すための「一つ前の画面」の項目（題名が戻るボタンの文字になる）。
    let previous = UINavigationItem()
    var onPress: ((String) -> Void)?

    override init(frame: CGRect) {
        super.init(frame: frame)
        // 地の色は透かして、Rust 側の画面の色（Agent の背景の絵など）を見せる。
        let appearance = UINavigationBarAppearance()
        appearance.configureWithTransparentBackground()
        bar.standardAppearance = appearance
        bar.scrollEdgeAppearance = appearance
        bar.compactAppearance = appearance
        bar.delegate = self
        addSubview(bar)
    }

    required init?(coder: NSCoder) { nil }

    override func layoutSubviews() {
        super.layoutSubviews()
        let height = bar.sizeThatFits(bounds.size).height
        bar.frame = CGRect(x: 0, y: max((bounds.height - height) / 2, 0), width: bounds.width, height: height)
    }

    func position(for bar: UIBarPositioning) -> UIBarPosition { .top }

    /// 戻るボタンが押されたとき。帯の中身は自分で戻さず、Rust に知らせて画面ごと切り替えてもらう。
    func navigationBar(_ navigationBar: UINavigationBar, shouldPop item: UINavigationItem) -> Bool {
        onPress?("back")
        return false
    }
}

@objc(FieldNavBarView)
public final class FieldNavBarView: WhiskerUI<UIView> {
    private var title = ""
    private var back = ""
    private var menu = false
    private var action = ""

    private lazy var host: NavBarHost = {
        let host = NavBarHost()
        host.onPress = { [weak self] which in self?.press(which) }
        return host
    }()

    @objc public override func createView() -> UIView {
        apply()
        return host
    }

    func setTitle(_ text: String) { title = text; apply() }
    func setBack(_ text: String) { back = text; apply() }
    func setMenu(_ on: Bool) { menu = on; apply() }
    func setAction(_ text: String) { action = text; apply() }

    private func press(_ which: String) {
        WhiskerCustomEvent.dispatch(from: self, name: "press", params: ["which": which])
    }

    private func apply() {
        let item = host.current
        item.title = title.isEmpty ? nil : title
        if back.isEmpty {
            host.previous.title = nil
            item.leftBarButtonItem = menu
                ? UIBarButtonItem(
                    image: UIImage(systemName: "line.3.horizontal"),
                    primaryAction: UIAction { [weak self] _ in self?.press("menu") })
                : nil
            host.bar.setItems([item], animated: false)
        } else {
            host.previous.title = back
            item.leftBarButtonItem = nil
            host.bar.setItems([host.previous, item], animated: false)
        }
        item.rightBarButtonItem = action.isEmpty
            ? nil
            : UIBarButtonItem(
                title: action, primaryAction: UIAction { [weak self] _ in self?.press("action") })
    }
}
#endif
