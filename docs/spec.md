# Draft: Universal Switch Access Handoff Protocol (USAHP)

**Status:** Request for comments · **Category:** Proposed accessibility standard and IPC design

::: danger Aspirational draft
This document describes a possible future standard. It is not the implemented USAHP contract and must not be used as evidence of current safety, operating-system integration, or platform support. See [protocol 0.2](/protocol-v0) for what the broker implements today.
:::

## **Abstract**

This document proposes a protocol for negotiating and handing accessibility switch hardware between an operating system and a foreground client application. It explores a hardware-agnostic input profile and an IPC state machine intended to support recoverable control transfer. Those guarantees require platform integration that the current broker does not provide.

## **1\. Introduction and Motivation**

Historically, switch integration has relied on input devices like keyboard emulation or remapping software. This creates a critical user experience failure when users transition between OS-level navigation (e.g., Apple Switch Control, Windows Native Access) and internal application scanning (e.g., Grid 3, custom web apps). Without a unified API, the OS and the client application frequently conflict over input streams.  
The proposed Universal Switch Access Handoff Protocol (USAHP) would address this with a standardized handshake that lets a foreground application request raw switch input while an OS-level service monitors recovery triggers.

## **2\. Terminology**

The key words "MUST", "MUST NOT", "REQUIRED", "SHALL", "SHALL NOT", "SHOULD", "SHOULD NOT", "RECOMMENDED", "MAY", and "OPTIONAL" in this document are to be interpreted as described in standard RFC guidelines.

> * **OS-SD:** Operating System Switch Daemon. The authoritative system-level service routing switch inputs.  
> * **Client App:** The switch-aware application currently in the foreground.

## **3\. The Two-Part Standard**

### **3.1 Part 1: The HID Profile (Hardware)**

An implementation of this future standard would normalize physical and virtual switch inputs into a profile containing:

> * **Switch ID:** A normalized string (e.g., switch\_1, switch\_2).  
> * **Action Event:** A discrete payload representing a specific state change in the event lifecycle: pressed (leading edge), released (trailing edge), or held (sustained input). The OS-SD MUST NOT group these into a single click; they MUST be emitted as separate events (See Section 6.3).  
> * **Confidence Score:** A provisional analog value. The final units, range, absence semantics, sampling model, and snapshot representation remain open in [issue #6](https://github.com/usahp/usahp-core/issues/6). Physical binary switches currently emit interim values of 100.0 and 0.0.

### **3.2 Part 2: The Handoff Protocol (Software)**

Communication between the OS-SD and Client App SHOULD occur via gRPC over Unix Domain Sockets (UDS) for low-latency, kernel-level execution. For v0 prototyping, local WebSockets MAY be used.

### **3.3 Versioning**

The target standard proposes semantic versioning (`MAJOR.MINOR.PATCH`, e.g. `1.0.0`) for negotiated `protocol_version`. The implemented prototype currently uses `0.2.0`; that version is experimental and is not a commitment to the draft's eventual compatibility rules.

## **4\. Protocol State Machine**

The OS-SD MUST strictly manage switch routing through the following four states:

> 1. **STATE\_OS\_ROUTING (Default):** The OS-SD translates all hardware inputs into system-level UI scanning. The Client App receives zero raw switch events.  
> 2. **STATE\_HANDSHAKE:** Triggered when a switch-aware Client App gains focus. The App requests control via a manifest. The OS-SD validates capabilities and grants or rejects control.  
> 3. **STATE\_APP\_CONTROL:** The OS-SD pauses system-level scanning and pipes raw switch events (including confidence scores) directly to the Client App. The OS-SD simultaneously monitors for safety triggers.  
> 4. **STATE\_ORPHANED\_RECOVERY:** If the Client App crashes or fails to respond, the OS-SD instantly revokes access and forcefully reverts to STATE\_OS\_ROUTING.

## **5\. Safety Mechanisms (The Triggers)**

The target design requires recovery mechanisms so a user is not left without switch access. These mechanisms are not implemented or clinically validated in the current broker.

### **5.1 Trigger A: The Escape Hatch (User-Initiated)**

The OS-SD MUST intercept a universal hardware input pattern *before* passing events to the App. If triggered (e.g., a dedicated system switch, or a sustained 100% confidence signal for \>4000ms), the OS-SD forcefully revokes App control and displays a system-level navigation overlay.

### **5.2 Trigger B: The Heartbeat (Software-Initiated)**

While in STATE\_APP\_CONTROL, the Client App MUST send an acknowledgment ping (ack) to the OS-SD at a negotiated interval (default 500ms). If the OS-SD misses three consecutive heartbeats, it assumes the App is locked and triggers STATE\_ORPHANED\_RECOVERY.

## **6\. Payload Specifications**

### **6.1 The Handshake Manifest**

When gaining focus, the Client App would submit a SwitchControlManifest to request input routing:

```json
{
  "app_id": "com.smartbox.grid3",
  "protocol_version": "1.0.0",
  "requested_mode": "exclusive_foreground",
  "requested_inputs": [
    {
      "switch_id": "switch_1",
      "intended_action": "step"
    }
  ],
  "capabilities": {
    "supports_confidence_score": true,
    "handles_hold_events": false
  },
  "failsafe_contract": {
    "heartbeat_interval_ms": 500,
    "missed_heartbeat_limit": 3
  }
}
```

### **6.2 The OS Response**

The OS-SD evaluates the manifest and returns a SwitchControlResponse.

#### **6.2.1 Successful Acceptance**

```json
{
  "session_id": "sess_9f83a710-4b2e-43a1",
  "status": "ACCEPTED",
  "negotiated_config": {
    "heartbeat_interval_ms": 500,
    "confidence_mode_active": true,
    "assigned_switches": ["switch_1"],
    "system_escape_trigger": "sustained_hold_4000ms"
  },
  "endpoints": {
    "heartbeat": "ipc:///tmp/switch_daemon_hb.sock",
    "event_stream": "ipc:///tmp/switch_daemon_events.sock"
  }
}
```

#### **6.2.2 Rejection Handling**

If denied, the OS-SD returns a REJECTED status. The Client App MUST gracefully fall back to passive standard inputs.

| Error Code | Meaning | Required Action |
| :---- | :---- | :---- |
| PERMISSION\_DENIED | User has not authorized app access. | Drop to standard OS inputs. |
| HARDWARE\_INSUFFICIENT | System lacks requested hardware. | OS retains control. |
| SYSTEM\_LOCKED | System modal/alert is active. | App MAY poll on refocus. |
| PROTOCOL\_MISMATCH | OS cannot support App's version. | App MUST request legacy fallback. |

### **6.3 Event Lifecycle and Edge Detection**

Switch accessibility software often relies on distinguishing between the moment a switch is initially depressed (Leading Edge) and the moment it is let go (Trailing Edge). This is critical for users who may have tremors or spasticity and require "activation on release" to prevent accidental selections.  
To support this, the OS-SD MUST NOT send a single consolidated "click" event. Instead, it MUST emit discrete SwitchEvent payloads for every state change:

> * **Leading Edge ("action": "pressed"):** Emitted the exact millisecond the input crosses the activation threshold.  
> * **Trailing Edge ("action": "released"):** Emitted when the input drops below the activation threshold.  
> * **Sustained Input ("action": "held"):** (Optional) Emitted if the input remains in a pressed state beyond a system-defined threshold (e.g., 1000ms), assuming the App's manifest declared "handles\_hold\_events": false.

**Client Responsibility:** The Client App is entirely responsible for interpreting these edges. If an app is configured for "Trailing Edge Activation", it simply ignores the pressed events and executes its scanning step only upon receiving the released event.

### **6.4 Analog Inputs and Confidence Scores**

Traditional physical switches are binary. However, modern AAC relies heavily on Machine Learning (ML) systems, facial gesture recognition (e.g., Apple ARKit face tracking), and Brain-Computer Interfaces (BCI). These inputs are not binary; they operate on probability.  
The draft reserves space for confidence so physical and virtual switches do not require unrelated APIs. The final representation is deliberately unresolved; [issue #6](https://github.com/usahp/usahp-core/issues/6) owns that design.

#### **6.4.1 Physical Switches (Binary)**

For standard hardware (e.g., a Crick USB box or Bluetooth gamepad), the OS-SD MUST hardcode the confidence score:

> * "action": "pressed" is always accompanied by "confidence": 100.0.  
> * "action": "released" is always accompanied by "confidence": 0.0.

#### **6.4.2 ML / Virtual Switches (Analog)**

For continuous inputs (e.g., a webcam tracking a smile), the OS-SD streams the raw probability. As the user begins to smile, the OS-SD might emit a stream of events with confidence scores of 45.2, 68.5, and 92.1.

#### **6.4.3 Thresholding Negotiation**

The responsibility for deciding "what counts as a click" is determined during the Manifest Handshake:

> * **App-Side Thresholding ("supports\_confidence\_score": true):** The OS-SD passes the raw, fluctuating confidence floats directly to the App. The App's internal engine determines the activation threshold (e.g., only stepping the scanner when confidence \> 85.0%). This allows advanced apps to use predictive character models based on fuzzy inputs.  
> * **OS-Side Thresholding ("supports\_confidence\_score": false):** If the Client App is a legacy or simple application, it cannot handle fuzzy floats. The OS-SD MUST intercept the analog data, apply a system-level threshold, and only pass a binary 100.0 or 0.0 to the App, effectively turning a webcam smile into a standard physical button press.

### **6.5 Contention Resolution and System Arbitration**

In environments where multiple switch-aware applications are running simultaneously (e.g., a background AAC proxy app controlling a foreground web browser), the OS-SD MUST arbitrate input routing to prevent collisions. Apps MUST NOT self-declare priority; instead, they MUST declare their functional intent via the requested\_mode field in the SwitchControlManifest.

#### **6.5.1 Subscription Tiers**

To support both standard foreground applications and background "Computer Control" paradigms, the OS-SD MUST support the following three subscription tiers:

> * **exclusive\_foreground:** The Client App receives switch events ONLY when it holds active OS window focus. If focus is lost, the OS-SD pauses the event stream to this app until focus returns.  
> * **primary\_controller (Focus-Agnostic):** The Client App receives exclusive global access to the switches, regardless of whether it is in the foreground or background (e.g., Grid 3 or VoiceGarden driving the OS). This tier supersedes foreground applications.  
> * **passive\_observer:** The Client App receives a read-only copy of switch events globally, regardless of focus. The app CANNOT consume, block, or alter the event routing. Multiple apps MAY hold this status simultaneously.

> **Current implementation note:** Protocol 0.2 implements one local session named `exclusive_foreground`, but does not detect actual focus, provide system arbitration, perform OS takeover, implement an escape hatch, or guarantee recovery.

#### **6.5.2 The System Arbitration UI (Conflict Resolution)**

To prevent malicious or accidental lockouts—especially regarding the powerful primary\_controller tier—the OS-SD MUST enforce a strict user-arbitration flow when competing access requests occur.  
If App A currently holds a primary\_controller session, and App B requests either primary\_controller or exclusive\_foreground status:

> * **Pause:** The OS-SD MUST temporarily pause routing events to App A.  
> * **System UI Overlay:** The OS-SD MUST transition to STATE\_OS\_ROUTING and display a modal, high-contrast System UI Overlay (e.g., *"App B is requesting switch control. Transfer control from App A?"*).  
> * **User Resolution:** The user utilizes standard OS-level scanning to select "Transfer" or "Deny".  
> * **Resolution:**  
  * *If Transfer:* App A receives a SESSION\_REVOKED payload, and App B receives ACCEPTED.  
  * *If Deny:* App B receives a REJECTED (USER\_DENIED) payload, and App A resumes receiving events.  
> * **Timeout Failsafe:** If the user cannot or does not respond to the System UI within a defined timeout (e.g., 15 seconds), the OS-SD MUST default to "Deny" to prevent the user from being stranded in the arbitration screen, automatically returning control to App A.

#### **6.5.3 Event Routing Hierarchy**

When a hardware switch event is generated, the OS-SD MUST route the event sequentially from top to bottom. Once a tier consumes the event, routing stops.

> 1. **Failsafe Verification:** If the event matches the System Escape Hatch (Trigger A), the OS-SD forcefully reclaims control and halts routing.  
> 2. **System UI Override:** If the OS-SD is currently displaying a System UI Overlay (like the Arbitration UI or a low battery warning), the OS-SD consumes the event to navigate the overlay.  
> 3. **Primary Controller:** The event is passed to the active primary\_controller session (focus-agnostic).  
> 4. **Foreground Handoff:** If no primary\_controller exists, the event is passed to the active exclusive\_foreground session (must hold window focus).  
> 5. **OS Fallback (STATE\_OS\_ROUTING):** If no Client App session exists or holds focus, the OS-SD translates the event into standard system UI navigation.

*(Note: Active passive\_observer sessions receive a duplicate of the event asynchronously, independent of this hierarchy, and do not consume the event).*

#### **6.5.4 Mutual Exclusion (Mutex)**

To prevent catastrophic UI conflicts, the OS-SD MUST enforce strict Mutex rules based on the tiers:

> * **Foreground:** Mutex is naturally handled by the OS window manager. Only one app can hold window focus at a time.  
> * **Primary Controller:** Mutex is handled explicitly by the System Arbitration UI (Section 6.5.2). If a new app requests control, the user MUST approve the handoff, ensuring two apps never hold primary\_controller status simultaneously.

## **7\. Addendum: Android OS Implementation**

### **7.1 The Android Sandboxing Constraint**

Unlike desktop environments (Windows, macOS, Linux) where a system daemon can intercept global hardware interrupts via low-level hooks, Android's strict process isolation prevents this architecture. Consequently, the OS-SD on Android CANNOT be implemented via a standalone binary using Unix Domain Sockets (UDS) or Named Pipes.

### **7.2 The OS-SD as an AccessibilityService**

To comply with Android's security model, the OS-SD MUST be implemented as a registered Android AccessibilityService requiring the BIND\_ACCESSIBILITY\_SERVICE permission.

> * **Hardware Interception:** The service intercepts raw switch inputs globally by overriding the onKeyEvent() method.  
> * **System Routing (STATE\_OS\_ROUTING):** When in default mode, the service translates switch inputs into standard Android accessibility actions (e.g., iterating through AccessibilityNodeInfo elements or using performAction).

### **7.3 Android-Native IPC (AIDL)**

Because Android apps cannot securely bind to local UDS pipes, the IPC transport layer MUST utilize Android's native Bound Services via AIDL (Android Interface Definition Language).

> * The OS-SD exposes an AIDL interface that mirrors the USAHP SwitchControlManifest and SwitchEvent JSON schemas.  
> * The Client App uses bindService() to connect to the OS-SD and execute the handshake.

### **7.4 Failsafe Enhancements via Lifecycle Binding**

Android's native service binding model provides a robust, built-in mechanism for STATE\_ORPHANED\_RECOVERY:

> * When the Client App loses foreground focus (onPause) or the process is killed (crash), the Android OS automatically severs the bindService connection.  
> * The OS-SD MUST listen for this onUnbind event. Upon detection, the OS-SD instantly revokes STATE\_APP\_CONTROL and reverts to STATE\_OS\_ROUTING.  
> * While the Software Heartbeat (Trigger B) SHOULD still be implemented to detect main-thread freezes within the active app, Android's automatic unbinding acts as an immediate, system-enforced failsafe for focus loss and crashes.

## **8\. Addendum: iOS / iPadOS Implementation & Prototyping**

### **8.1 The iOS Sandboxing Constraint**

Apple’s iOS and iPadOS strictly prohibit third-party background daemons and custom accessibility services. All system-level switch routing is exclusively controlled by Apple's native "Switch Control" via standard HID (Human Interface Device) profiles (e.g., Bluetooth keyboards, MFi switch interfaces). Consequently, a software-only OS-SD cannot be deployed on a non-jailbroken iOS device.

### **8.2 The Native Integration Path (Long-Term)**

A true, native implementation of USAHP on iOS requires Apple to adopt the handshake standard within the system frameworks.

> * This would likely manifest as an extension to the UIAccessibility framework (e.g., a hypothetical UIAccessibilitySwitchHandoffSession API).  
> * The OS would recognize Apple’s proprietary BCI/HID confidence-factor profiles and pass those streams directly to the application upon a successful API handshake.

### **8.3 The Hardware Proxy Implementation (Prototyping & Demo)**

To prototype and deploy USAHP on iOS immediately without Apple's OS-level adoption, the OS-SD logic MUST be offloaded to a dedicated hardware bridge (e.g., an nRF52840 or ESP32 microcontroller acting as a BLE peripheral).  
In this architecture, the hardware acts as the OS-SD:

> 1. **System Routing (STATE\_OS\_ROUTING):** By default, the microcontroller acts as a standard BLE HID Keyboard. When a physical switch is pressed, it sends standard Space/Enter keystrokes to the iPad, which Apple's native Switch Control intercepts for OS navigation.  
> 2. **The IPC Layer (CoreBluetooth):** Instead of UDS or AIDL, the IPC transport layer uses BLE GATT characteristics. The Client App connects to the microcontroller via iOS CoreBluetooth.  
> 3. **The Handshake:** The Client App writes the SwitchControlManifest JSON to a specific GATT characteristic.  
> 4. **App Control (STATE\_APP\_CONTROL):** Upon accepting the manifest, the microcontroller stops sending HID Keyboard keystrokes to the OS. Instead, it begins streaming USAHP SwitchEvent payloads over a BLE notification characteristic directly to the Client App.  
> 5. **Failsafe (BLE Disconnect):** If the iOS Client App crashes, freezes, or is backgrounded, the CoreBluetooth connection is dropped. The microcontroller detects the BLE disconnect, instantly reverts to STATE\_OS\_ROUTING, and resumes sending standard HID keystrokes, safely returning control to Apple's native Switch Control.

