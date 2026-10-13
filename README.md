# strait

strait shares a Mac's keyboard, mouse and clipboard with a Windows PC, with
gaming-grade latency, and switches a Samsung Odyssey G9 between the two.

- The pointer crosses into Windows where the monitor shows it, and comes back
  the same way.
- **⌃⌥⌘ Space** gives Windows the keyboard and mouse until you press it again.
  Use it for games that hold the pointer.
- Text and images copied on one machine paste on the other.
- The menu bar picks what the monitor shows: the Mac, Windows, or both side by
  side.

## How it works

The Mac captures input with an event tap, plus raw HID motion so games get
the mouse's own counts, before any acceleration. It sends input to Windows as
UDP datagrams, and control messages over TCP. Both channels are encrypted and
authenticated by one Noise XX session. The first Mac and PC that meet pair;
neither accepts another machine afterwards. Windows injects input with
`SendInput`, using relative motion and scan codes. The Mac switches the
monitor's input over DDC.

| Directory | What it is |
|---|---|
| `crates/strait` | Wire format, session, key maps, layout and routing, shared by both sides |
| `crates/strait-mac` | The Mac engine: capture, link, DDC, behind a C API |
| `mac` | The SwiftUI menu bar app around the engine |
| `crates/strait-win` | The Windows tray app |
| `scripts` | Build, install and deploy scripts |

## Install

On the Mac, with Rust and Xcode installed:

```sh
scripts/mac.sh install
```

Then allow strait in System Settings → Privacy & Security under:

- **Accessibility**, for keys, buttons and scroll.
- **Input Monitoring**, for raw mouse motion.
- **Local Network**, to find and reach the PC.

On Windows, with Rust and the MSVC build tools installed, build
`strait-win.exe`. Then run it once with `--install` from an administrator
prompt. This registers it to start at logon with full rights, so it can also
type into administrator windows. `scripts/win.sh install` does both from the
Mac over SSH. Windows Firewall must allow `strait-win.exe` on TCP and UDP port
47800 for private networks.

The two machines find each other through Bonjour (`_strait._tcp`) and pair on
first contact. "Forget paired PC" (Mac) and "Forget paired Mac" (Windows)
undo the pairing.

## Monitor

The Mac sits on the G9's HDMI input and Windows on DisplayPort. DDC works only
over HDMI, so the Mac drives every switch.

- **Mac** and **Windows** select the monitor's input over DDC.
- **Side by side** is the monitor's PBP mode, which DDC cannot turn on: enable
  it in the monitor's menu. strait then places Windows to the right of the
  Mac.

## Development

```sh
cargo test                # shared logic, on any OS
scripts/win.sh run        # build on the PC over SSH and restart strait there
scripts/win.sh log        # read the PC's log
```
