import ApplicationServices
import IOKit.hid
import SwiftUI

struct MenuView: View {
    @EnvironmentObject private var engine: Engine
    @State private var opensAtLogin = LoginItem.enabled

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            if let status = engine.status {
                Header(status: status)
                Notices(status: status)
                Section(title: "Input") { InputPicker(status: status) }
                Section(title: "Monitor") { PresetPicker(status: status) }
                VStack(alignment: .leading, spacing: 8) {
                    HStack {
                        Text("Pointer speed on Windows")
                        Slider(value: Binding(get: { status.pointerSpeed }, set: { engine.pointerSpeed($0) }), in: 0.1...2.0)
                            .controlSize(.small)
                    }
                    if let hdr = status.hdr {
                        Toggle("HDR on the monitor", isOn: Binding(get: { hdr }, set: { engine.hdr($0) }))
                    }
                    Toggle("Play Windows sound on this Mac", isOn: Binding(
                        get: { status.sound },
                        set: { engine.sound($0) }
                    ))
                    Toggle("Type Windows shortcuts with ⌘", isOn: Binding(
                        get: { status.swapCommandControl },
                        set: { engine.swapCommandControl($0) }
                    ))
                    Toggle("Open at login", isOn: Binding(
                        get: { opensAtLogin },
                        set: { LoginItem.set($0); opensAtLogin = LoginItem.enabled }
                    ))
                }
                .toggleStyle(.switch)
                .controlSize(.small)
                Footer(status: status)
            } else {
                ProgressView().frame(maxWidth: .infinity)
            }
        }
        .padding(16)
        .frame(width: 340)
    }
}

private struct Section<Content: View>: View {
    let title: String
    @ViewBuilder let content: Content

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(title.uppercased())
                .font(.caption2.weight(.semibold))
                .foregroundStyle(.secondary)
                .tracking(0.6)
            content
        }
    }
}

private struct Header: View {
    let status: Status

    var body: some View {
        HStack(spacing: 10) {
            ZStack {
                RoundedRectangle(cornerRadius: 8, style: .continuous).fill(tint.gradient)
                Image(systemName: "rectangle.split.2x1.fill").foregroundStyle(.white).font(.system(size: 15, weight: .semibold))
            }
            .frame(width: 34, height: 34)
            VStack(alignment: .leading, spacing: 2) {
                Text("strait").font(.headline)
                Text(detail).font(.caption).foregroundStyle(.secondary).lineLimit(1)
            }
            Spacer()
            if let rtt = status.rttMs, case .connected = status.link {
                Text(String(format: "%.1f ms", rtt))
                    .font(.caption.monospacedDigit().weight(.medium))
                    .padding(.horizontal, 8).padding(.vertical, 3)
                    .background(Capsule().fill(tint.opacity(0.15)))
                    .foregroundStyle(tint)
                    .help("Round trip to the PC")
            }
        }
    }

    private var tint: Color {
        switch status.link {
        case .connected: .teal
        case .refused: .orange
        case .searching, .connecting: .gray
        }
    }

    private var detail: String {
        switch status.link {
        case .searching: status.pairedPc == nil ? "Looking for a PC to pair…" : "Looking for your PC…"
        case .connecting(let host): "Connecting to \(host)…"
        case .connected(let host, _): "Connected to \(host)"
        case .refused(let fp): "Refused an unknown PC (\(fp))"
        }
    }
}

private struct Notices: View {
    let status: Status

    var body: some View {
        VStack(spacing: 6) {
            if !status.accessibility {
                Notice(symbol: "hand.raised.fill", text: "Allow strait in Accessibility to share your keyboard and mouse.",
                       action: "Allow") {
                    // Asking through the API adds strait to the list, with this build's signature.
                    let prompt = kAXTrustedCheckOptionPrompt.takeUnretainedValue() as String
                    _ = AXIsProcessTrustedWithOptions([prompt: true] as CFDictionary)
                    open("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
                }
            }
            if !status.inputMonitoring {
                Notice(symbol: "cursorarrow.motionlines", text: "Allow Input Monitoring for the mouse's raw, unaccelerated motion.",
                       action: "Allow") {
                    IOHIDRequestAccess(kIOHIDRequestTypeListenEvent)
                    open("x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent")
                }
            }
            if status.secureInput {
                Notice(symbol: "lock.fill", text: "A password field is open; macOS hides the keyboard from strait until it closes.",
                       action: nil, perform: {})
            }
            if let problem = status.problem {
                Notice(symbol: "exclamationmark.triangle.fill", text: problem, action: nil, perform: {})
            }
        }
    }

    private func open(_ url: String) {
        if let url = URL(string: url) { NSWorkspace.shared.open(url) }
    }
}

private struct Notice: View {
    let symbol: String
    let text: String
    let action: String?
    let perform: () -> Void

    var body: some View {
        HStack(alignment: .top, spacing: 8) {
            Image(systemName: symbol).foregroundStyle(.orange)
            Text(text).font(.caption).fixedSize(horizontal: false, vertical: true)
            Spacer(minLength: 0)
            if let action {
                Button(action, action: perform).controlSize(.small)
            }
        }
        .padding(10)
        .background(RoundedRectangle(cornerRadius: 8, style: .continuous).fill(Color.orange.opacity(0.1)))
    }
}

private struct InputPicker: View {
    @EnvironmentObject private var engine: Engine
    let status: Status

    var body: some View {
        let connected: Bool = { if case .connected = status.link { true } else { false } }()
        HStack(spacing: 8) {
            Tile(title: "Mac", symbol: "laptopcomputer", selected: status.focus == .mac) { engine.focus(windows: false) }
            Tile(title: status.focus == .windowsLocked ? "Windows · locked" : "Windows", symbol: "pc",
                 selected: status.focus != .mac) { engine.focus(windows: true) }
                .disabled(!connected)
        }
        Text(status.pbp
             ? "⌃⌥⌘ Space moves the keyboard and mouse. Choosing Windows here keeps them there, for games."
             : "⌃⌥⌘ Space switches the keyboard, mouse and screen, like a KVM. Choosing Windows here keeps them there, for games.")
            .font(.caption2).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
    }
}

private struct PresetPicker: View {
    @EnvironmentObject private var engine: Engine
    let status: Status

    var body: some View {
        HStack(spacing: 8) {
            ForEach(Status.Preset.allCases, id: \.self) { preset in
                Tile(title: title(preset), drawing: AnyView(MonitorDrawing(preset: preset)), selected: status.preset == preset) {
                    engine.choose(preset)
                }
                // PBP, which only the monitor's menu switches, decides between
                // side by side and Windows full screen.
                .disabled(preset == (status.pbp ? .windows : .split))
            }
        }
        Text(status.pbp
             ? "PBP is on. For Windows full screen, turn PBP off in the monitor's menu."
             : "For side by side, turn on PBP in the monitor's menu.")
            .font(.caption2).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
    }

    private func title(_ p: Status.Preset) -> String {
        switch p {
        case .mac: "Mac"
        case .split: "Side by side"
        case .windows: "Windows"
        }
    }
}

/// A miniature of the monitor showing which machine is on each half.
private struct MonitorDrawing: View {
    let preset: Status.Preset

    var body: some View {
        HStack(spacing: 2) {
            switch preset {
            case .mac: half(.accentColor)
            case .split: half(.accentColor); half(.teal)
            case .windows: half(.teal)
            }
        }
        .padding(3)
        .frame(width: 56, height: 18)
        .background(RoundedRectangle(cornerRadius: 4, style: .continuous).strokeBorder(.secondary.opacity(0.5), lineWidth: 1))
    }

    private func half(_ color: Color) -> some View {
        RoundedRectangle(cornerRadius: 2, style: .continuous).fill(color.opacity(0.8))
    }
}

private struct Tile: View {
    let title: String
    var symbol: String? = nil
    var drawing: AnyView? = nil
    let selected: Bool
    let action: () -> Void
    @State private var hovering = false

    var body: some View {
        Button(action: action) {
            VStack(spacing: 6) {
                if let drawing { drawing.frame(height: 22) }
                if let symbol { Image(systemName: symbol).font(.system(size: 18)).frame(height: 22) }
                Text(title).font(.caption.weight(.medium)).lineLimit(1)
            }
            .frame(maxWidth: .infinity)
            .padding(.vertical, 10)
            .background(
                RoundedRectangle(cornerRadius: 10, style: .continuous)
                    .fill(selected ? Color.accentColor.opacity(0.18) : Color.primary.opacity(hovering ? 0.08 : 0.04))
            )
            .overlay(
                RoundedRectangle(cornerRadius: 10, style: .continuous)
                    .strokeBorder(selected ? Color.accentColor : .clear, lineWidth: 1.5)
            )
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .onHover { hovering = $0 }
    }
}

private struct Footer: View {
    @EnvironmentObject private var engine: Engine
    let status: Status

    var body: some View {
        HStack {
            VStack(alignment: .leading, spacing: 1) {
                Text("This Mac \(status.thisMac)")
                Text(status.pairedPc.map { "Paired PC \($0)" } ?? "No PC paired yet")
            }
            .font(.caption2.monospaced())
            .foregroundStyle(.secondary)
            Spacer()
            Menu {
                Button("Forget paired PC") { engine.forgetPC() }.disabled(status.pairedPc == nil)
                Divider()
                Button("Quit strait") { NSApp.terminate(nil) }
            } label: {
                Image(systemName: "ellipsis.circle")
            }
            .menuStyle(.borderlessButton)
            .menuIndicator(.hidden)
            .fixedSize()
        }
    }
}
