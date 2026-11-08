# PathWeaver's Tweeq inspector

This is the real [Tweeq](https://github.com/baku89/tweeq) Vue component library running inside the existing Mac application's WKWebView inspectors. The Rust document, renderer, selection and undo history remain authoritative. No development server or network connection is required by the installed app.

`vendor/tweeq/src` is vendored from commit `957ecf70ab51153faa0026314061820a57becfb4`. Two documented compatibility patches remove stale selected input IDs on unmount and use client pointer coordinates until pointer lock is actually granted. Regression tests cover inspector replacement and embedded WebView drag coordinates. Its MIT license and upstream package manifest accompany the source. `src/App.vue` is the PathWeaver adapter; it imports Tweeq's number, color, text, checkbox, dropdown, button, and parameter grid components directly. Tweeq's color picker, scrubbing, validation, expression entry, and light/dark theme come from those components.

```sh
cd tweeq-ui
npm ci
npm test
npm run build
```

The production build is one self-contained `dist/index.html`, embedded by the native host. It includes the runtime and all required icons. The Content Security Policy forbids network connections. Icon SVG data comes from Material Design Icons (Apache-2.0) and Google Material Symbols (Apache-2.0), bundled through Iconify; see `vendor/icon-licenses/`.

## Native bridge

The host calls `window.pathweaverUpdate(snapshot)`. See `src/protocol.ts` for the schema. UI messages use `window.webkit.messageHandlers.pathweaver.postMessage(JSON.stringify(message))`. A `ready` message tells the host the callback is installed. Field changes send `begin`, `change` previews, and `commit`/`cancel`; all carry the snapshot's stable revision and field ID. Rust validates the captured target and owns the single undo entry for a gesture. A selection/document revision change discards stale field state. Actions use `type: "action"` and the action ID provided by Rust.

`window.pathweaverCommand(command)` routes native edit menu commands to the focused web input, or to Rust document history when no text input is active. Snapshot errors are displayed below the fields.

The adapter limits popover width to the native inspector viewport. Native WKWebView pointer-lock and clipboard behavior still require device-specific verification; these are browser capability boundaries, not reimplemented Tweeq controls.

## Numeric metadata

Tweeq's `InputNumber.step` quantizes the actual model; it is not a scrub sensitivity hint. The adapter passes `step: 1` only for integer Seed fields. Coordinates, widths, rotation, scale, and other continuous effect values keep their fractional precision. Numeric model emissions are accepted only for the active or focused user field, so validator reconciliation of a native snapshot cannot edit unrelated document fields or split a transform gesture's undo transaction.
