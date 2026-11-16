// 見えない部品。いちばん近い外側のスクロール（Whisker の ScrollView）に、
// iOS 標準の「引っ張って更新」（UIRefreshControl）を付ける。
//
// Rust から受け取るもの：
// - refreshing … "true" の間は更新中。false になったらくるくるを止める
// Rust へ知らせるもの：
// - "refresh" {} … 最上部から引っ張って離された
//
// キーボードが出ている間は UIRefreshControl を外す。そのとき下へ引っ張っても、キーボードが閉じるだけで
// 更新は始まらない。キーボードが閉じて指を離し、スクロールが戻りきったら付け直す。

#if canImport(UIKit)
import UIKit
import WhiskerModule

@objc(FieldRefreshControlView)
public final class FieldRefreshControlView: WhiskerUI<UIView> {
    private let refresh = UIRefreshControl()
    private weak var scrollView: UIScrollView?
    private var bouncedBefore = false
    private var suspended = false
    private var observers: [NSObjectProtocol] = []

    @objc public override func createView() -> UIView {
        let view = UIView()
        view.isUserInteractionEnabled = false
        refresh.addTarget(self, action: #selector(didPullToRefresh), for: .valueChanged)
        let center = NotificationCenter.default
        observers.append(center.addObserver(
            forName: UIResponder.keyboardWillShowNotification, object: nil, queue: .main
        ) { [weak self] _ in self?.suspend() })
        observers.append(center.addObserver(
            forName: UIResponder.keyboardDidHideNotification, object: nil, queue: .main
        ) { [weak self] _ in self?.resumeWhenReleased() })
        return view
    }

    deinit {
        observers.forEach { NotificationCenter.default.removeObserver($0) }
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
        if KeyboardDismisser.keyboardVisible { suspend() }
    }

    private func detach() {
        guard let target = scrollView else { return }
        if refresh.isRefreshing { refresh.endRefreshing() }
        let owned = target.refreshControl === refresh || suspended
        if target.refreshControl === refresh { target.refreshControl = nil }
        if owned { target.alwaysBounceVertical = bouncedBefore }
        target.panGestureRecognizer.removeTarget(self, action: #selector(panChanged))
        suspended = false
        scrollView = nil
    }

    /// 更新中は外さない（くるくるが消えるうえ、更新中に引っ張っても次の更新は始まらない）。
    private func suspend() {
        guard let target = scrollView, target.refreshControl === refresh, !refresh.isRefreshing else { return }
        target.refreshControl = nil
        suspended = true
    }

    private func resumeWhenReleased() {
        guard suspended, let target = scrollView else { return }
        if target.isTracking {
            target.panGestureRecognizer.addTarget(self, action: #selector(panChanged))
        } else {
            resume()
        }
    }

    @objc private func panChanged(_ pan: UIPanGestureRecognizer) {
        guard pan.state == .ended || pan.state == .cancelled || pan.state == .failed else { return }
        pan.removeTarget(self, action: #selector(panChanged))
        resume()
    }

    private func resume() {
        guard suspended, let target = scrollView, !KeyboardDismisser.keyboardVisible else { return }
        // 引っ張った分が戻る前に付けると、離したあとでも更新が始まることがあるので、戻りきるまで待つ。
        if target.isTracking || target.contentOffset.y < -target.adjustedContentInset.top - 1 {
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.1) { [weak self] in self?.resume() }
            return
        }
        suspended = false
        if target.refreshControl == nil { target.refreshControl = refresh }
    }

    @objc private func didPullToRefresh() {
        WhiskerCustomEvent.dispatch(from: self, name: "refresh", params: [:])
    }
}
#endif
