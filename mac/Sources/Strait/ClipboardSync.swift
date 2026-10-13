import AppKit
import StraitCore

/// Sends the Mac's clipboard to Windows when it changes, and applies the
/// clipboard Windows sends. macOS has no change notification, so it polls
/// the change count.
@MainActor
final class ClipboardSync {
    private let pasteboard = NSPasteboard.general
    private var seen = NSPasteboard.general.changeCount
    private var timer: Timer?
    /// Larger images are not worth stalling the session for.
    private let maxImageBytes = 32 << 20

    func start() {
        timer = Timer.scheduledTimer(withTimeInterval: 0.25, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated { self?.poll() }
        }
    }

    private func poll() {
        guard pasteboard.changeCount != seen else { return }
        seen = pasteboard.changeCount
        if let text = pasteboard.string(forType: .string) {
            let bytes = Array(text.utf8)
            strait_clipboard(UInt32(STRAIT_CLIP_TEXT), bytes, bytes.count)
        } else if let png = pngData(), png.count <= maxImageBytes {
            png.withUnsafeBytes { raw in
                strait_clipboard(UInt32(STRAIT_CLIP_PNG), raw.bindMemory(to: UInt8.self).baseAddress, png.count)
            }
        }
    }

    private func pngData() -> Data? {
        if let png = pasteboard.data(forType: .png) { return png }
        guard let tiff = pasteboard.data(forType: .tiff), let image = NSBitmapImageRep(data: tiff) else { return nil }
        return image.representation(using: .png, properties: [:])
    }

    func apply(kind: UInt32, data: Data) {
        pasteboard.clearContents()
        switch Int(kind) {
        case Int(STRAIT_CLIP_TEXT):
            pasteboard.setString(String(decoding: data, as: UTF8.self), forType: .string)
        case Int(STRAIT_CLIP_PNG):
            pasteboard.setData(data, forType: .png)
        default:
            return
        }
        // The change just made came from Windows; do not send it back.
        seen = pasteboard.changeCount
    }
}
