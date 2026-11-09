// Face ID, Touch ID or the passcode before the vault opens, when Settings asks for it.

import LocalAuthentication
import UIKit

enum Lock {
    private static let key = "lock-the-vault"

    /// Whether the person asked for the lock, in Stahl's Settings.
    static var enabled: Bool {
        get { UserDefaults.standard.bool(forKey: key) }
        set { UserDefaults.standard.set(newValue, forKey: key) }
    }

    /// Whether this device can lock anything: it has a passcode, at least.
    static var available: Bool {
        LAContext().canEvaluatePolicy(.deviceOwnerAuthentication, error: nil)
    }

    /// Whether the lock is on and can work: without a passcode, a lock would only lock the
    /// person out.
    static var active: Bool { enabled && available }

    /// What the device calls its lock.
    static var name: String {
        let context = LAContext()
        _ = context.canEvaluatePolicy(.deviceOwnerAuthenticationWithBiometrics, error: nil)
        switch context.biometryType {
        case .faceID: return "Face ID"
        case .touchID: return "Touch ID"
        case .opticID: return "Optic ID"
        default: return "Passcode"
        }
    }

    /// Ask, with the passcode as the way in when Face ID doesn't recognise the face.
    static func ask(then: @escaping (Bool) -> Void) {
        LAContext().evaluatePolicy(
            .deviceOwnerAuthentication, localizedReason: "Open the vault"
        ) { opened, _ in
            DispatchQueue.main.async { then(opened) }
        }
    }
}
