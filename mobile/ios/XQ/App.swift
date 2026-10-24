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
    }

    func startFrames() {
        guard displayLink == nil, let w = gpuiWindow else { return }
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
        if !gpui_ios_request_frame(w) { displayLink?.isPaused = true }
    }

    override var preferredStatusBarStyle: UIStatusBarStyle { .lightContent }
}
