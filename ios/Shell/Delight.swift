// The parts of Stahl that only an iPhone or iPad has: haptics, and the vault that closes over the
// app in the app switcher and, with the lock on, until Face ID opens it.

import UIKit

/// The haptics `stahl_ios_tick` asks for.
enum Haptics {
    private static let selection = UISelectionFeedbackGenerator()
    private static let notice = UINotificationFeedbackGenerator()
    private static let knock = UIImpactFeedbackGenerator(style: .rigid)
    private static let tick = UIImpactFeedbackGenerator(style: .light)

    /// The vault's door, turning.
    static func turn() { knock.impactOccurred(intensity: 0.9) }

    static func play(_ flags: UInt32) {
        if flags & UInt32(STAHL_TICK_HAPTIC_TURN) != 0 {
            knock.impactOccurred(intensity: 0.9)
        } else if flags & UInt32(STAHL_TICK_HAPTIC_WARNING) != 0 {
            notice.notificationOccurred(.warning)
        } else if flags & UInt32(STAHL_TICK_HAPTIC_SUCCESS) != 0 {
            notice.notificationOccurred(.success)
        } else if flags & UInt32(STAHL_TICK_HAPTIC_SELECT) != 0 {
            selection.selectionChanged()
        } else if flags & UInt32(STAHL_TICK_HAPTIC_TICK) != 0 {
            tick.impactOccurred(intensity: 0.5)
        }
    }
}

/// The steel wall and its door, over the app while it isn't active, so the app switcher's
/// snapshot shows no balances. Coming back, the door turns a quarter and the wall fades: the
/// vault opening. Locked, it stays until Face ID opens it, and a tap on it asks again.
final class VaultCover: UIView {
    private let door = UIImageView(image: UIImage(named: "door"))
    private let hint = UILabel()
    /// A tap on the closed vault: ask Face ID again.
    var onTap: (() -> Void)?

    override init(frame: CGRect) {
        super.init(frame: frame)
        backgroundColor = UIColor(red: 10 / 255, green: 15 / 255, blue: 19 / 255, alpha: 1)
        door.translatesAutoresizingMaskIntoConstraints = false
        addSubview(door)
        NSLayoutConstraint.activate([
            door.centerXAnchor.constraint(equalTo: centerXAnchor),
            door.centerYAnchor.constraint(equalTo: centerYAnchor),
            door.widthAnchor.constraint(equalToConstant: 128),
            door.heightAnchor.constraint(equalToConstant: 128),
        ])
        hint.text = "Tap to open the vault"
        hint.font = .systemFont(ofSize: 15, weight: .medium)
        hint.textColor = UIColor(white: 0.62, alpha: 1)
        hint.alpha = 0
        hint.translatesAutoresizingMaskIntoConstraints = false
        addSubview(hint)
        NSLayoutConstraint.activate([
            hint.centerXAnchor.constraint(equalTo: centerXAnchor),
            hint.topAnchor.constraint(equalTo: door.bottomAnchor, constant: 28),
        ])
        addGestureRecognizer(UITapGestureRecognizer(target: self, action: #selector(tapped)))
    }

    @objc private func tapped() { onTap?() }

    /// Face ID didn't open it: say how to try again.
    func showHint() {
        UIView.animate(withDuration: 0.3) { self.hint.alpha = 1 }
    }

    required init?(coder: NSCoder) { fatalError("not from a storyboard") }

    func close() {
        layer.removeAllAnimations()
        door.transform = .identity
        hint.alpha = 0
        alpha = 1
        isHidden = false
        superview?.bringSubviewToFront(self)
    }

    func open() {
        guard !isHidden else { return }
        if UIAccessibility.isReduceMotionEnabled {
            isHidden = true
            return
        }
        UIView.animate(
            withDuration: 0.45, delay: 0, usingSpringWithDamping: 0.8, initialSpringVelocity: 0,
            options: [.curveEaseOut]
        ) {
            self.door.transform = CGAffineTransform(rotationAngle: .pi / 2).scaledBy(x: 1.15, y: 1.15)
            self.alpha = 0
        } completion: { finished in
            if finished { self.isHidden = true }
        }
    }
}

/// Time from iOS to finish something after the app leaves the screen, ended once, by whichever
/// comes first: the work, or iOS running out of patience.
final class BackgroundTask {
    private var id = UIBackgroundTaskIdentifier.invalid

    init() {
        id = UIApplication.shared.beginBackgroundTask { [weak self] in self?.end() }
    }

    func end() {
        guard id != .invalid else { return }
        UIApplication.shared.endBackgroundTask(id)
        id = .invalid
    }
}
