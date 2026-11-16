// 証憑の原本（写真・PDF）を画面の中に出す（Web 版の StoredDocumentPreview と同じ）。
// 写真は枠の中に全体が収まるように縮めて出す（拡大して見るときはピンチ）。
//
// Rust から受け取るもの：
// - "url" … 原本の URL（https のときだけ読む。短い時間だけ使える署名付きのリンク）
//
// 一覧を読み直すたびに同じファイルの新しいリンクが来るが、出しているものが読めているうちは差し替えない
// （差し替えると読み直しになり、PDF が 1 ページ目に戻る）。読めなかったときだけ新しいリンクで読み直す。
// 保存先によってはファイルを「ダウンロード」として返すので、中身を先に取ってから出す。

#if canImport(UIKit)
import UIKit
import WebKit
import WhiskerModule

public final class FieldDocumentPreviewView: WhiskerUI<UIView> {
    private var shown = ""
    private var failed = false
    private var task: URLSessionDataTask?

    private lazy var webView: WKWebView = {
        let view = WKWebView(frame: .zero, configuration: WKWebViewConfiguration())
        view.backgroundColor = .secondarySystemBackground
        view.isOpaque = false
        view.scrollView.backgroundColor = .secondarySystemBackground
        view.layer.cornerRadius = 12
        view.clipsToBounds = true
        return view
    }()

    @objc public override func createView() -> UIView {
        webView
    }

    func setUrl(_ value: String) {
        if value.isEmpty {
            shown = ""
            showMessage("写真はありません。")
            return
        }
        guard shown.isEmpty || (failed && value != shown) else { return }
        guard let url = URL(string: value), url.scheme == "https" else {
            showMessage("写真はありません。")
            return
        }
        shown = value
        failed = false
        task?.cancel()
        showMessage("写真を読み込んでいます…")
        task = URLSession.shared.dataTask(with: url) { [weak self] data, response, error in
            DispatchQueue.main.async {
                guard let self, self.shown == value else { return }
                let status = (response as? HTTPURLResponse)?.statusCode ?? 0
                guard error == nil, let data, (200..<300).contains(status) else {
                    self.failed = true
                    self.showMessage("写真を表示できませんでした。再読み込みしてからもう一度開いてください。")
                    return
                }
                self.show(data: data, mime: response?.mimeType)
            }
        }
        task?.resume()
    }

    private func show(data: Data, mime: String?) {
        let isPdf = mime == "application/pdf" || data.starts(with: Array("%PDF".utf8))
        if isPdf {
            webView.load(data, mimeType: "application/pdf", characterEncodingName: "", baseURL: URL(string: "about:blank")!)
            return
        }
        let type = (mime?.hasPrefix("image/") ?? false) ? mime! : "image/jpeg"
        let html = """
        <html><head><meta name="viewport" content="width=device-width,initial-scale=1">
        <style>html,body{margin:0;height:100%;background:transparent;}img{display:block;width:100%;height:100%;object-fit:contain;}</style></head>
        <body><img src="data:\(type);base64,\(data.base64EncodedString())"></body></html>
        """
        webView.loadHTMLString(html, baseURL: nil)
    }

    private func showMessage(_ text: String) {
        let html = """
        <html><head><meta name="viewport" content="width=device-width,initial-scale=1">
        <style>body{margin:0;height:100vh;display:flex;align-items:center;justify-content:center;
        font:15px -apple-system;color:#8A8A8E;text-align:center;padding:0 24px;background:transparent;}</style></head>
        <body>\(text)</body></html>
        """
        webView.loadHTMLString(html, baseURL: nil)
    }
}
#endif
