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
        rune.onBackground = { [weak self] color in self?.view.backgroundColor = color }
        rune.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(rune)
        let guide = view.safeAreaLayoutGuide
        NSLayoutConstraint.activate([
            rune.leadingAnchor.constraint(equalTo: guide.leadingAnchor),
            rune.trailingAnchor.constraint(equalTo: guide.trailingAnchor),
            rune.topAnchor.constraint(equalTo: guide.topAnchor),
            rune.bottomAnchor.constraint(equalTo: view.keyboardLayoutGuide.topAnchor),
        ])
        rune.onTypingDone = { [weak self] in self?.becomeFirstResponder() }
    }

    // First responder whenever no field has focus, so a hardware keyboard's shortcuts reach the
    // app without bringing up the software keyboard, which only the view asks for.
    override var canBecomeFirstResponder: Bool { true }

    override func viewDidAppear(_ animated: Bool) {
        super.viewDidAppear(animated)
        becomeFirstResponder()
    }

    override func pressesBegan(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
        if !rune.handle(presses, typing: false) { super.pressesBegan(presses, with: event) }
    }
}

/// A view whose layer Rune presents to, fed by a display link and UIKit's touches and keyboard.
final class RuneView: UIView, UIKeyInput {
    override class var layerClass: AnyClass { CAMetalLayer.self }

    private var host: OpaquePointer?
    private var link: CADisplayLink?
    private var wantsKeyboard = false
    private var background: UInt32?

    /// The app's background changed, so what's behind the safe area can match it.
    var onBackground: ((UIColor) -> Void)?
    /// No field has focus any more: the keyboard's shortcuts go back to the view controller.
    var onTypingDone: (() -> Void)?
    private var lastScroll = CGPoint.zero
    private let picker = Picker()

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
        // Before Stahl starts, so it can reopen the portfolio it last opened.
        Access.restore()
        layer.contentsScale = scale
        let documents = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
        let dark = traitCollection.userInterfaceStyle == .dark
        host = documents.path.withCString { path in
            stahl_ios_new(
                Unmanaged.passUnretained(layer).toOpaque(), path,
                Float(bounds.width), Float(bounds.height), Float(scale), dark)
        }
        guard host != nil else { return }
        updateCompact()
        registerForTraitChanges([UITraitHorizontalSizeClass.self]) { (view: RuneView, _) in
            view.updateCompact()
        }
        // A trackpad or mouse on an iPad: hover, and two-finger scrolling. The pan takes scroll
        // events alone, never fingers, which arrive as touches.
        addGestureRecognizer(UIHoverGestureRecognizer(target: self, action: #selector(hover)))
        let scroll = UIPanGestureRecognizer(target: self, action: #selector(scroll))
        scroll.allowedScrollTypesMask = .all
        scroll.allowedTouchTypes = []
        addGestureRecognizer(scroll)
        let link = CADisplayLink(target: self, selector: #selector(tick))
        link.preferredFrameRateRange = CAFrameRateRange(minimum: 60, maximum: 120, preferred: 120)
        link.add(to: .main, forMode: .common)
        self.link = link
        if let script = ProcessInfo.processInfo.environment["STAHL_IOS_SCRIPT"] {
            play(script)
        }
    }

    /// Plays input for checking the app in a simulator, which `simctl` can't tap: `wait 1;
    /// tap 30 200; drag 200 600 200 200; type Apple; delete; key cmd k; key escape;
    /// hover 300 400; scroll 300 400 -200`, in view points.
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
            case "key" where words.count >= 2:
                // `key cmd k`, `key cmd shift o`, `key escape`, `key down`.
                let named: [String: Int] = [
                    "escape": STAHL_KEY_ESCAPE, "tab": STAHL_KEY_TAB, "enter": STAHL_KEY_ENTER,
                    "up": STAHL_KEY_UP, "down": STAHL_KEY_DOWN, "left": STAHL_KEY_LEFT,
                    "right": STAHL_KEY_RIGHT,
                ]
                var mods: UInt32 = 0
                if words.contains("shift") { mods |= 1 }
                if words.contains("cmd") { mods |= 2 }
                let last = words[words.count - 1]
                let key = words.contains("cmd") ? STAHL_KEY_CHORD : named[last]
                let character = last.unicodeScalars.first?.value ?? 0
                if let key {
                    later(at) { stahl_ios_key($0, UInt32(key), character, mods) }
                }
                at += 0.4
            case "pick":
                // Answers the document picker as if `rest` was picked, or cancels with no path.
                later(at) { [weak self] host in
                    self?.window?.rootViewController?.dismiss(animated: false)
                    if rest.isEmpty {
                        stahl_ios_picked(host, nil)
                    } else {
                        rest.withCString { stahl_ios_picked(host, $0) }
                    }
                }
                at += 1
            case "hover" where numbers.count == 2:
                let (x, y) = (numbers[0], numbers[1])
                later(at) { stahl_ios_hover($0, x, y, true) }
                at += 0.5
            case "scroll" where numbers.count == 3:
                let (x, y, dy) = (numbers[0], numbers[1], numbers[2])
                for i in 0...10 {
                    let phase: UInt32 = i == 0 ? 0 : (i == 10 ? 2 : 1)
                    later(at + Double(i) * 0.016) {
                        stahl_ios_scroll($0, x, y, 0, phase == 1 ? dy / 9 : 0, phase)
                    }
                }
                at += 0.5
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

    /// The phone layout wherever UIKit lays out for a narrow width: an iPhone, or an iPad in a
    /// narrow split.
    private func updateCompact() {
        guard let host else { return }
        stahl_ios_set_compact(host, traitCollection.horizontalSizeClass == .compact)
    }

    @objc private func tick() {
        guard let host else { return }
        let flags = stahl_ios_tick(host)
        if flags & UInt32(STAHL_TICK_DREW) != 0 {
            let rgb = stahl_ios_background(host)
            if rgb != background {
                background = rgb
                onBackground?(
                    UIColor(
                        red: CGFloat((rgb >> 16) & 0xFF) / 255,
                        green: CGFloat((rgb >> 8) & 0xFF) / 255,
                        blue: CGFloat(rgb & 0xFF) / 255, alpha: 1))
            }
        }
        if flags & UInt32(STAHL_TICK_PICK_FILE | STAHL_TICK_PICK_FOLDER) != 0 {
            pick(folder: flags & UInt32(STAHL_TICK_PICK_FOLDER) != 0)
        }
        let wants = flags & UInt32(STAHL_TICK_WANTS_KEYBOARD) != 0
        if wants != wantsKeyboard {
            wantsKeyboard = wants
            if wants {
                becomeFirstResponder()
            } else {
                resignFirstResponder()
                onTypingDone?()
            }
        }
    }

    /// The app asked for a file or a folder: UIKit's document picker, over whatever shows.
    private func pick(folder: Bool) {
        var controller = window?.rootViewController
        while let presented = controller?.presentedViewController { controller = presented }
        guard let host, let controller else {
            if let host { stahl_ios_picked(host, nil) }
            return
        }
        picker.present(folder: folder, from: controller) { [weak self] path in
            guard let self, let host = self.host else { return }
            if let path {
                path.withCString { stahl_ios_picked(host, $0) }
            } else {
                stahl_ios_picked(host, nil)
            }
            // A sheet going away doesn't make the controller first responder again.
            self.onTypingDone?()
        }
    }

    // MARK: Trackpad, mouse and hardware keyboard

    @objc private func hover(_ gesture: UIHoverGestureRecognizer) {
        guard let host else { return }
        let point = gesture.location(in: self)
        let inside = gesture.state == .began || gesture.state == .changed
        stahl_ios_hover(host, Float(point.x), Float(point.y), inside)
    }

    @objc private func scroll(_ gesture: UIPanGestureRecognizer) {
        guard let host else { return }
        let point = gesture.location(in: self)
        let moved = gesture.translation(in: self)
        let phase: UInt32
        switch gesture.state {
        case .began:
            phase = 0
            lastScroll = .zero
        case .changed: phase = 1
        default: phase = 2
        }
        let dx = moved.x - lastScroll.x
        let dy = moved.y - lastScroll.y
        lastScroll = moved
        stahl_ios_scroll(host, Float(point.x), Float(point.y), Float(dx), Float(dy), phase)
    }

    override func pressesBegan(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
        if !handle(presses, typing: true) { super.pressesBegan(presses, with: event) }
    }

    /// Passes on what a hardware keyboard sends that isn't text: chords with ⌘, arrows, Escape,
    /// Tab. Returns whether it took any. While `typing`, Return and Delete stay with UIKeyInput,
    /// which types them.
    func handle(_ presses: Set<UIPress>, typing: Bool) -> Bool {
        guard let host else { return false }
        var taken = false
        for press in presses {
            guard let key = press.key else { continue }
            let flags = key.modifierFlags
            var mods: UInt32 = 0
            if flags.contains(.shift) { mods |= 1 }
            if flags.contains(.command) { mods |= 2 }
            if flags.contains(.alternate) { mods |= 4 }
            if flags.contains(.control) { mods |= 8 }
            if flags.contains(.command),
                let character = key.charactersIgnoringModifiers.lowercased().unicodeScalars.first
            {
                stahl_ios_key(host, UInt32(STAHL_KEY_CHORD), character.value, mods)
                taken = true
                continue
            }
            let code: Int?
            switch key.keyCode {
            case .keyboardEscape: code = STAHL_KEY_ESCAPE
            case .keyboardTab: code = STAHL_KEY_TAB
            case .keyboardReturnOrEnter, .keypadEnter: code = typing ? nil : STAHL_KEY_ENTER
            case .keyboardDeleteOrBackspace: code = typing ? nil : STAHL_KEY_BACKSPACE
            case .keyboardDeleteForward: code = STAHL_KEY_DELETE
            case .keyboardLeftArrow: code = STAHL_KEY_LEFT
            case .keyboardRightArrow: code = STAHL_KEY_RIGHT
            case .keyboardUpArrow: code = STAHL_KEY_UP
            case .keyboardDownArrow: code = STAHL_KEY_DOWN
            case .keyboardHome: code = STAHL_KEY_HOME
            case .keyboardEnd: code = STAHL_KEY_END
            case .keyboardPageUp: code = STAHL_KEY_PAGE_UP
            case .keyboardPageDown: code = STAHL_KEY_PAGE_DOWN
            default: code = nil
            }
            if let code {
                stahl_ios_key(host, UInt32(code), 0, mods)
                taken = true
            }
        }
        return taken
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
