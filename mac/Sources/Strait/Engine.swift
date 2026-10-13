import AppKit
import Foundation
import StraitCore

/// The engine's status, decoded from the JSON it publishes.
struct Status: Decodable, Equatable {
    enum Link: Equatable {
        case searching
        case connecting(host: String)
        case connected(host: String, fingerprint: String)
        case refused(fingerprint: String)
    }

    enum Focus: String, Decodable {
        case mac, windows, windowsLocked
    }

    enum Preset: String, Decodable, CaseIterable {
        case mac, split, windows
    }

    var link: Link
    var focus: Focus
    var preset: Preset
    var pbp: Bool
    var monitor: Bool
    var rttMs: Double?
    var accessibility: Bool
    var inputMonitoring: Bool
    var secureInput: Bool
    var swapCommandControl: Bool
    var pointerSpeed: Double
    var thisMac: String
    var pairedPc: String?
    var problem: String?
}

extension Status.Link: Decodable {
    private struct Host: Decodable { var host: String }
    private struct Connected: Decodable { var host: String; var fingerprint: String }
    private struct Refused: Decodable { var fingerprint: String }
    private enum Keys: String, CodingKey { case connecting, connected, refused }

    init(from decoder: Decoder) throws {
        if let unit = try? decoder.singleValueContainer().decode(String.self), unit == "searching" {
            self = .searching
            return
        }
        let c = try decoder.container(keyedBy: Keys.self)
        if let v = try c.decodeIfPresent(Host.self, forKey: .connecting) {
            self = .connecting(host: v.host)
        } else if let v = try c.decodeIfPresent(Connected.self, forKey: .connected) {
            self = .connected(host: v.host, fingerprint: v.fingerprint)
        } else if let v = try c.decodeIfPresent(Refused.self, forKey: .refused) {
            self = .refused(fingerprint: v.fingerprint)
        } else {
            throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "unknown link state"))
        }
    }
}

/// Owns the Rust engine and mirrors its status for SwiftUI.
@MainActor
final class Engine: ObservableObject {
    @Published private(set) var status: Status?
    private let clipboard = ClipboardSync()

    init() {
        let dir = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("strait", isDirectory: true)
        let context = Unmanaged.passRetained(self).toOpaque()
        let started = strait_start(dir.path, { json, context in
            guard let json, let context else { return }
            let data = Data(bytes: json, count: strlen(json))
            let engine = Unmanaged<Engine>.fromOpaque(context).takeUnretainedValue()
            guard let status = try? JSONDecoder().decode(Status.self, from: data) else { return }
            DispatchQueue.main.async { engine.status = status }
        }, { kind, bytes, len, context in
            guard let bytes, let context else { return }
            let data = Data(bytes: bytes, count: len)
            let engine = Unmanaged<Engine>.fromOpaque(context).takeUnretainedValue()
            DispatchQueue.main.async { engine.clipboard.apply(kind: kind, data: data) }
        }, context)
        if !started {
            NSLog("strait: the engine did not start")
        }
        clipboard.start()
        send(["refresh": true])
    }

    func send(_ command: [String: Any]) {
        guard let data = try? JSONSerialization.data(withJSONObject: command),
              let json = String(data: data, encoding: .utf8) else { return }
        strait_command(json)
    }

    func choose(_ preset: Status.Preset) { send(["preset": preset.rawValue]) }
    func focus(windows: Bool) { send(["focus": windows ? "windows" : "mac"]) }
    func swapCommandControl(_ on: Bool) { send(["swapCommandControl": on]) }
    func pointerSpeed(_ speed: Double) { send(["pointerSpeed": speed]) }
    func forgetPC() { send(["forget": true]) }
}
