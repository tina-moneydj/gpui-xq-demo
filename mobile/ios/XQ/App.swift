import UIKit

// XQ（GPUI）iOS 外殼：UIScene 生命週期（Xcode 27／iOS 27 SDK 起必須），GPUI 以「嵌入」模式放進我們的畫面。
// 畫面更新由 CADisplayLink 驅動，只看「有沒有東西要畫」，不看輸入焦點：
// 控制中心、通知、來電橫幅蓋住時照畫（Android 版的凍結問題不會在這裡發生）；只有進背景才停。

@main
final class AppDelegate: UIResponder, UIApplicationDelegate {
    func application(_ application: UIApplication,
                     configurationForConnecting connectingSceneSession: UISceneSession,
                     options: UIScene.ConnectionOptions) -> UISceneConfiguration {
        let config = UISceneConfiguration(name: "Default", sessionRole: connectingSceneSession.role)
        config.delegateClass = SceneDelegate.self
        return config
    }
}

final class SceneDelegate: UIResponder, UIWindowSceneDelegate {
    var window: UIWindow?

    func scene(_ scene: UIScene, willConnectTo session: UISceneSession, options: UIScene.ConnectionOptions) {
        guard let windowScene = scene as? UIWindowScene else { return }
        let window = UIWindow(windowScene: windowScene)
        window.rootViewController = XQViewController()
        window.makeKeyAndVisible()
        self.window = window
    }

    func sceneWillEnterForeground(_ scene: UIScene) {
        gpui_ios_will_enter_foreground(nil)
        (window?.rootViewController as? XQViewController)?.startFrames()
    }
    func sceneDidBecomeActive(_ scene: UIScene) { gpui_ios_did_become_active(nil) }
    func sceneWillResignActive(_ scene: UIScene) { gpui_ios_will_resign_active(nil) }
    func sceneDidEnterBackground(_ scene: UIScene) {
        gpui_ios_did_enter_background(nil)
        (window?.rootViewController as? XQViewController)?.stopFrames()
    }
}

final class XQViewController: UIViewController {
    private var gpuiWindow: UnsafeMutableRawPointer?
    private var content: UIViewController?
    private var displayLink: CADisplayLink?

    override func viewDidLoad() {
        super.viewDidLoad()
        view.backgroundColor = UIColor(red: 0.08, green: 0.09, blue: 0.11, alpha: 1)
        gpui_ios_set_embedded()
        xq_ios_register_app()
        gpui_ios_run_demo()
        guard let w = gpui_ios_get_window(), let c = gpui_ios_view_controller(w) else {
            NSLog("[xq] GPUI 視窗建立失敗")
            showFatal(fatalMessage() ?? "GPUI 視窗建立失敗")
            return
        }
        gpuiWindow = w
        let controller = Unmanaged<UIViewController>.fromOpaque(c).takeUnretainedValue()
        content = controller
        addChild(controller)
        view.addSubview(controller.view)
        controller.didMove(toParent: self)
        startFrames()
    }

    override func viewDidLayoutSubviews() {
        super.viewDidLayoutSubviews()
        guard let w = gpuiWindow, let v = content?.view else { return }
        // 只用安全區域（避開瀏海／動態島／底部橫條）
        let frame = view.bounds.inset(by: view.safeAreaInsets)
        guard frame.width > 0, frame.height > 0, v.frame != frame else { return }
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        v.frame = frame
        gpui_ios_layout_view(w)
        _ = gpui_ios_request_frame(w)
        CATransaction.commit()
        checkFatal()
    }

    func startFrames() {
        guard displayLink == nil, fatalView == nil, let w = gpuiWindow else { return }
        let link = CADisplayLink(target: self, selector: #selector(tick))
        link.add(to: .main, forMode: .common)
        displayLink = link
        // GPUI 說「暫時不用畫」時暫停，有新行情／動畫時由 waker 叫醒（閒置不耗電）
        gpui_ios_set_frame_waker(w, { ctx in
            guard let ctx = ctx else { return }
            let me = Unmanaged<XQViewController>.fromOpaque(ctx).takeUnretainedValue()
            me.displayLink?.isPaused = false
        }, Unmanaged.passUnretained(self).toOpaque())
    }

    func stopFrames() {
        displayLink?.invalidate()
        displayLink = nil
    }

    @objc private func tick() {
        guard let w = gpuiWindow else { return }
        if !gpui_ios_request_frame(w) {
            displayLink?.isPaused = true
            checkFatal()
        }
    }

    // ── GPUI 內部出錯（Rust panic）時：不閃退，停止畫面更新，把錯誤訊息顯示出來方便截圖回報 ──
    private var fatalView: UITextView?

    private func fatalMessage() -> String? {
        guard let p = gpui_ios_fatal_error() else { return nil }
        return String(cString: p)
    }

    private func checkFatal() {
        guard fatalView == nil, let msg = fatalMessage() else { return }
        stopFrames()
        showFatal(msg)
    }

    private func showFatal(_ msg: String) {
        guard fatalView == nil else { return }
        NSLog("[xq] GPUI 錯誤：%@", msg)
        let tv = UITextView()
        tv.translatesAutoresizingMaskIntoConstraints = false
        tv.isEditable = false
        tv.isSelectable = true
        tv.backgroundColor = UIColor(red: 0.20, green: 0.05, blue: 0.05, alpha: 0.95)
        tv.textColor = .white
        tv.font = .systemFont(ofSize: 15)
        tv.layer.cornerRadius = 10
        tv.text = "GPUI 版出錯了（app 沒有閃退，錯誤已被攔下）\n請把這個畫面截圖傳回，或長按文字全選→拷貝。\n\n"
            + "iOS \(UIDevice.current.systemVersion) · \(UIDevice.current.model)\n\n" + msg
        view.addSubview(tv)
        let g = view.safeAreaLayoutGuide
        NSLayoutConstraint.activate([
            tv.leadingAnchor.constraint(equalTo: g.leadingAnchor, constant: 12),
            tv.trailingAnchor.constraint(equalTo: g.trailingAnchor, constant: -12),
            tv.topAnchor.constraint(equalTo: g.topAnchor, constant: 12),
            tv.bottomAnchor.constraint(equalTo: g.bottomAnchor, constant: -12),
        ])
        fatalView = tv
    }

    override var preferredStatusBarStyle: UIStatusBarStyle { .lightContent }
}
