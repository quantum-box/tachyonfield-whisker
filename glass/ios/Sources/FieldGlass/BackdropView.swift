// ログイン画面の背景の絵。淡い色の大きな光の玉が、ゆっくり漂う（iOS の壁紙のような見た目）。
//
// 画像ファイルは使わず、その場で描く。上にガラスの板（`GlassView.swift`）を重ねると、
// 色がにじんで透けて見える。

#if canImport(UIKit)
import UIKit
import WhiskerModule

/// 光の玉 1 つ分：色・濃さ・大きさ（画面の幅に対する割合）・置き場所と漂う先（画面に対する割合）。
private struct Orb {
    let color: UIColor
    let alpha: CGFloat
    let size: CGFloat
    let from: CGPoint
    let to: CGPoint
    let seconds: CFTimeInterval
}

private let orbs: [Orb] = [
    Orb(color: UIColor(red: 0.04, green: 0.52, blue: 1.00, alpha: 1), alpha: 0.55, size: 1.25,
        from: CGPoint(x: 0.05, y: 0.12), to: CGPoint(x: 0.30, y: 0.22), seconds: 14),
    Orb(color: UIColor(red: 0.56, green: 0.36, blue: 0.97, alpha: 1), alpha: 0.45, size: 1.10,
        from: CGPoint(x: 0.95, y: 0.30), to: CGPoint(x: 0.70, y: 0.12), seconds: 17),
    Orb(color: UIColor(red: 0.19, green: 0.84, blue: 0.78, alpha: 1), alpha: 0.40, size: 1.20,
        from: CGPoint(x: 0.15, y: 0.88), to: CGPoint(x: 0.40, y: 0.70), seconds: 19),
    Orb(color: UIColor(red: 1.00, green: 0.44, blue: 0.71, alpha: 1), alpha: 0.30, size: 0.95,
        from: CGPoint(x: 0.95, y: 0.85), to: CGPoint(x: 0.75, y: 0.62), seconds: 16),
]

private final class BackdropHost: UIView {
    private var layers: [CAGradientLayer] = []
    private var laidOutSize: CGSize = .zero

    override init(frame: CGRect) {
        super.init(frame: frame)
        backgroundColor = UIColor(red: 0.96, green: 0.97, blue: 0.99, alpha: 1)
        clipsToBounds = true
        isUserInteractionEnabled = false
        for orb in orbs {
            let layer = CAGradientLayer()
            layer.type = .radial
            layer.colors = [
                orb.color.withAlphaComponent(orb.alpha).cgColor,
                orb.color.withAlphaComponent(orb.alpha * 0.45).cgColor,
                orb.color.withAlphaComponent(0).cgColor,
            ]
            layer.locations = [0, 0.4, 1]
            layer.startPoint = CGPoint(x: 0.5, y: 0.5)
            layer.endPoint = CGPoint(x: 1, y: 1)
            self.layer.addSublayer(layer)
            layers.append(layer)
        }
    }

    required init?(coder: NSCoder) { nil }

    override func layoutSubviews() {
        super.layoutSubviews()
        guard bounds.size != laidOutSize, bounds.width > 0 else { return }
        laidOutSize = bounds.size
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        for (layer, orb) in zip(layers, orbs) {
            let side = bounds.width * orb.size
            layer.bounds = CGRect(x: 0, y: 0, width: side, height: side)
            layer.position = point(orb.from)
            layer.removeAllAnimations()
            let drift = CABasicAnimation(keyPath: "position")
            drift.fromValue = NSValue(cgPoint: point(orb.from))
            drift.toValue = NSValue(cgPoint: point(orb.to))
            drift.duration = orb.seconds
            drift.autoreverses = true
            drift.repeatCount = .infinity
            drift.timingFunction = CAMediaTimingFunction(name: .easeInEaseOut)
            // 画面を離れて戻っても止まらないように。
            drift.isRemovedOnCompletion = false
            layer.add(drift, forKey: "drift")
        }
        CATransaction.commit()
    }

    private func point(_ p: CGPoint) -> CGPoint {
        CGPoint(x: bounds.width * p.x, y: bounds.height * p.y)
    }
}

@objc(FieldBackdropView)
public final class FieldBackdropView: WhiskerUI<UIView> {
    @objc public override func createView() -> UIView {
        BackdropHost()
    }
}
#endif
