// 帳票の「印刷・PDF」ボタン。押すと Rust から受け取った HTML を A4 に組み、iOS の印刷画面を出す
// （プリンタへ送るほか、共有から PDF として保存・送信もできる）。
//
// Rust から受け取るもの：
// - "label"     … ボタンの文字
// - "html"      … 印刷する HTML（1 日 1 ページ。改ページは CSS の page-break）
// - "name"      … 印刷画面・PDF の名前（例: 出納帳20261115）
// - "landscape" … 横向きにするか
// Rust へ知らせるもの：
// - "done" { ok } … 印刷画面を閉じた

#if canImport(UIKit)
import UIKit
import WebKit
import WhiskerModule

/// HTML を読み終えたら印刷画面を出す。
private final class PrintLoader: NSObject, WKNavigationDelegate {
    var done: (() -> Void)?
    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
        done?()
        done = nil
    }
}

public final class FieldPrintButtonView: WhiskerUI<UIView> {
    private var html = ""
    private var jobName = "帳票"
    private var landscape = false
    private let loader = PrintLoader()
    /// 印刷用に HTML を組む見えない WebView（印刷画面を閉じるまで持っておく）。
    private var webView: WKWebView?

    private lazy var button: UIButton = {
        var config = UIButton.Configuration.plain()
        config.title = "印刷・PDF"
        config.image = UIImage(systemName: "printer")
        config.imagePadding = 6
        let button = UIButton(configuration: config)
        button.addTarget(self, action: #selector(tapped), for: .touchUpInside)
        return button
    }()

    @objc public override func createView() -> UIView {
        button
    }

    func setLabel(_ value: String) {
        button.configuration?.title = value.isEmpty ? "印刷・PDF" : value
    }
    func setHtml(_ value: String) {
        html = value
        button.isEnabled = !value.isEmpty
    }
    func setName(_ value: String) { jobName = value.isEmpty ? "帳票" : value }
    func setLandscape(_ value: Bool) { landscape = value }

    @objc private func tapped() {
        guard !html.isEmpty, webView == nil else { return }
        // WebView で HTML を組んでから印刷する（表・罫線・改ページを CSS のまま使える）。
        let web = WKWebView(frame: CGRect(x: 0, y: 0, width: 794, height: 1123))
        webView = web
        web.navigationDelegate = loader
        loader.done = { [weak self] in self?.present(web) }
        web.loadHTMLString(html, baseURL: nil)
    }

    private func present(_ web: WKWebView) {
        let info = UIPrintInfo(dictionary: nil)
        info.outputType = .general
        info.jobName = jobName
        info.orientation = landscape ? .landscape : .portrait
        let controller = UIPrintInteractionController.shared
        controller.printInfo = info
        controller.printFormatter = web.viewPrintFormatter()
        controller.present(animated: true) { [weak self] _, completed, _ in
            guard let self else { return }
            self.webView = nil
            WhiskerCustomEvent.dispatch(from: self, name: "done", params: ["ok": completed])
        }
    }
}
#endif
