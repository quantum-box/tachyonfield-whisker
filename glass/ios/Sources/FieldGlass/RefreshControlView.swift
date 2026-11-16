// 見えない部品。いちばん近い外側のスクロール（Whisker の ScrollView）に、
// iOS 標準の「引っ張って更新」（UIRefreshControl）を付ける。
//
// Rust から受け取るもの：
// - refreshing … "true" の間は更新中。false になったらくるくるを止める
// Rust へ知らせるもの：
// - "refresh" {} … 最上部から引っ張って離された

#if canImport(UIKit)
import UIKit
import WhiskerModule

@objc(FieldRefreshControlView)
public final class FieldRefreshControlView: WhiskerUI<UIView> {
    private let refresh = UIRefreshControl()
    private weak var scrollView: UIScrollView?
    private var bouncedBefore = false

    @objc public override func createView() -> UIView {
        let view = UIView()
        view.isUserInteractionEnabled = false
        refresh.addTarget(self, action: #selector(didPullToRefresh), for: .valueChanged)
        return view
    }

    func setRefreshing(_ on: Bool) {
        if !on, refresh.isRefreshing { refresh.endRefreshing() }
    }

    public override func didMoveToWindow() {
        super.didMoveToWindow()
        if window == nil { detach() } else { attach() }
    }

    private func attach() {
        var ancestor = superview
        while let current = ancestor, !(current is UIScrollView) { ancestor = current.superview }
        guard let target = ancestor as? UIScrollView, target !== scrollView else { return }
        detach()
        guard target.refreshControl == nil else { return }
        scrollView = target
        // 中身が画面より短くても引っ張れるようにする。
        bouncedBefore = target.alwaysBounceVertical
        target.alwaysBounceVertical = true
        target.refreshControl = refresh
    }

    private func detach() {
        guard let target = scrollView else { return }
        if refresh.isRefreshing { refresh.endRefreshing() }
        if target.refreshControl === refresh {
            target.refreshControl = nil
            target.alwaysBounceVertical = bouncedBefore
        }
        scrollView = nil
    }

    @objc private func didPullToRefresh() {
        WhiskerCustomEvent.dispatch(from: self, name: "refresh", params: [:])
    }
}
#endif
