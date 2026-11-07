// The iOS shell: a window holding one view that Rune draws Stahl into.

import QuartzCore
import UIKit

@main
final class AppDelegate: UIResponder, UIApplicationDelegate {
    func application(
        _ application: UIApplication,
        configurationForConnecting session: UISceneSession,
        options: UIScene.ConnectionOptions
    ) -> UISceneConfiguration {
        let config = UISceneConfiguration(name: nil, sessionRole: session.role)
        config.delegateClass = SceneDelegate.self
        return config
    }
}

final class SceneDelegate: UIResponder, UIWindowSceneDelegate {
    var window: UIWindow?

    func scene(
        _ scene: UIScene, willConnectTo session: UISceneSession,
        options: UIScene.ConnectionOptions
    ) {
        guard let scene = scene as? UIWindowScene else { return }
        let window = UIWindow(windowScene: scene)
        window.rootViewController = RuneViewController()
        window.makeKeyAndVisible()
        self.window = window
    }

    func sceneDidEnterBackground(_ scene: UIScene) { runeView(scene)?.paused = true }
    func sceneWillEnterForeground(_ scene: UIScene) { runeView(scene)?.paused = false }

    private func runeView(_ scene: UIScene) -> RuneView? {
        (window?.rootViewController as? RuneViewController)?.rune
    }
}

final class RuneViewController: UIViewController {
    let rune = RuneView()

    override func viewDidLoad() {
        super.viewDidLoad()
        // Until Rune reads safe-area insets, keep the app inside them.
        view.backgroundColor = .black
        rune.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(rune)
        let guide = view.safeAreaLayoutGuide
        NSLayoutConstraint.activate([
            rune.leadingAnchor.constraint(equalTo: guide.leadingAnchor),
            rune.trailingAnchor.constraint(equalTo: guide.trailingAnchor),
            rune.topAnchor.constraint(equalTo: guide.topAnchor),
            rune.bottomAnchor.constraint(equalTo: view.keyboardLayoutGuide.topAnchor),
        ])
    }
}

/// A view whose layer Rune presents to, fed by a display link and UIKit's touches and keyboard.
final class RuneView: UIView, UIKeyInput {
    override class var layerClass: AnyClass { CAMetalLayer.self }

    private var host: OpaquePointer?
    private var link: CADisplayLink?
    private var wantsKeyboard = false

    var paused = false {
        didSet { link?.isPaused = paused }
    }

    override init(frame: CGRect) {
        super.init(frame: frame)
        isMultipleTouchEnabled = true
    }

    required init?(coder: NSCoder) { fatalError("not from a storyboard") }

    deinit {
        link?.invalidate()
        if let host { stahl_ios_free(host) }
    }

    private var scale: CGFloat { window?.screen.scale ?? traitCollection.displayScale }

    override func didMoveToWindow() {
        super.didMoveToWindow()
        guard window != nil, host == nil else { return }
        layer.contentsScale = scale
        let documents = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
        let dark = traitCollection.userInterfaceStyle == .dark
        host = documents.path.withCString { path in
            stahl_ios_new(
                Unmanaged.passUnretained(layer).toOpaque(), path,
                Float(bounds.width), Float(bounds.height), Float(scale), dark)
        }
        guard host != nil else { return }
        let link = CADisplayLink(target: self, selector: #selector(tick))
        link.preferredFrameRateRange = CAFrameRateRange(minimum: 60, maximum: 120, preferred: 120)
        link.add(to: .main, forMode: .common)
        self.link = link
        if let script = ProcessInfo.processInfo.environment["STAHL_IOS_SCRIPT"] {
            play(script)
        }
    }

    /// Plays input for checking the app in a simulator, which `simctl` can't tap:
    /// `wait 1; tap 30 200; drag 200 600 200 200; type Apple; delete`, in view points.
    private func play(_ script: String) {
        var at = 1.0
        var id: UInt64 = 1_000
        for step in script.split(separator: ";") {
            let words = step.split(separator: " ").map(String.init)
            guard let verb = words.first else { continue }
            let numbers = words.dropFirst().compactMap { Float($0) }
            let rest = words.dropFirst().joined(separator: " ")
            id += 1
            let touch = id
            func later(_ delay: Double, _ action: @escaping (OpaquePointer) -> Void) {
                DispatchQueue.main.asyncAfter(deadline: .now() + delay) { [weak self] in
                    if let host = self?.host { action(host) }
                }
            }
            switch verb {
            case "wait":
                at += Double(numbers.first ?? 1)
            case "tap" where numbers.count == 2:
                let (x, y) = (numbers[0], numbers[1])
                later(at) { stahl_ios_touch($0, touch, 0, x, y, CACurrentMediaTime()) }
                later(at + 0.08) { stahl_ios_touch($0, touch, 2, x, y, CACurrentMediaTime()) }
                at += 0.6
            case "drag" where numbers.count == 4:
                let steps = 20
                for i in 0...steps {
                    let f = Float(i) / Float(steps)
                    let x = numbers[0] + (numbers[2] - numbers[0]) * f
                    let y = numbers[1] + (numbers[3] - numbers[1]) * f
                    let phase: UInt32 = i == 0 ? 0 : (i == steps ? 2 : 1)
                    later(at + Double(i) * 0.016) {
                        stahl_ios_touch($0, touch, phase, x, y, CACurrentMediaTime())
                    }
                }
                at += 0.8
            case "type":
                later(at) { host in rest.withCString { stahl_ios_insert_text(host, $0) } }
                at += 0.3
            case "delete":
                later(at) { stahl_ios_delete_backward($0) }
                at += 0.2
            default:
                print("stahl: unknown script step: \(step)")
            }
        }
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        guard let host else { return }
        layer.contentsScale = scale
        stahl_ios_resize(host, Float(bounds.width), Float(bounds.height), Float(scale))
    }

    @objc private func tick() {
        guard let host else { return }
        let flags = stahl_ios_tick(host)
        let wants = flags & UInt32(STAHL_TICK_WANTS_KEYBOARD) != 0
        if wants != wantsKeyboard {
            wantsKeyboard = wants
            if wants { becomeFirstResponder() } else { resignFirstResponder() }
        }
    }

    // MARK: Touch

    private func send(_ touches: Set<UITouch>, _ phase: UInt32) {
        guard let host else { return }
        for touch in touches {
            let point = touch.location(in: self)
            let id = UInt64(UInt(bitPattern: ObjectIdentifier(touch).hashValue))
            stahl_ios_touch(host, id, phase, Float(point.x), Float(point.y), touch.timestamp)
        }
    }

    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent?) { send(touches, 0) }
    override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent?) { send(touches, 1) }
    override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent?) { send(touches, 2) }
    override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent?) {
        send(touches, 3)
    }

    // MARK: Keyboard

    override var canBecomeFirstResponder: Bool { wantsKeyboard }
    var hasText: Bool { true }
    var autocorrectionType: UITextAutocorrectionType = .no
    var autocapitalizationType: UITextAutocapitalizationType = .none

    func insertText(_ text: String) {
        guard let host else { return }
        text.withCString { stahl_ios_insert_text(host, $0) }
    }

    func deleteBackward() {
        guard let host else { return }
        stahl_ios_delete_backward(host)
    }
}
