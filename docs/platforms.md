# Platform requirements

Physical input capture depends on operating-system permissions. The built-in simulator and WebSocket clients do not require input permissions.

## Windows

Keyboard suppression uses a low-level global input hook. Run the daemon in an interactive desktop session. Windows security software may ask for approval or block the hook.

Protocol 0.1 does not accept Windows gamepad mappings because its backend cannot guarantee suppression.

## macOS

Grant **Accessibility** permission to the terminal or packaged executable that runs `usahpd`:

1. Open System Settings.
2. Go to Privacy & Security → Accessibility.
3. Enable the terminal or executable that launches USAHP.
4. Restart the daemon after changing permission.

Protocol 0.1 does not accept macOS gamepad mappings because its backend cannot guarantee suppression.

## Linux

Keyboard grabbing requires access to input devices and a graphical desktop session. Distribution setup varies; membership of an `input` or `plugdev` group or a narrow udev rule is common. Prefer a device-specific permission over running the daemon as root.

Linux gamepad and dedicated switch mappings require:

- read access to the configured `/dev/input/event*` path;
- permission to grab that device exclusively;
- the numeric evdev key code for the desired control.

Device event numbers can change between boots. A stable udev-created path is safer for a persistent setup.

## Verify suppression

For each configured input, manually confirm that:

- the reference listener receives one press and one release;
- the foreground application does not receive the configured physical input;
- unmapped keyboard input still works;
- simultaneous inputs mapped to one switch produce only a first press and final release;
- stopping USAHP releases the input grab cleanly.

Hosted CI cannot validate real global hooks, accessibility permission, or physical devices, so hardware verification remains a release responsibility.
