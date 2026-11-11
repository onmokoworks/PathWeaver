Tweeq by Baku Hashimoto

Source: https://github.com/baku89/tweeq
Commit: 957ecf70ab51153faa0026314061820a57becfb4
License: MIT (see LICENSE)

The src/ tree is vendored from this commit. Local integration patch: `src/stores/multiSelect.ts` removes a departing input from `selectedIds` before unregistering it, and filters stale IDs when deriving `selectedInputs`. Upstream otherwise retains a missing selected input during component remount, crashing the next control setup. Covered by `src/multiSelect.test.ts`. PathWeaver imports the actual Vue components, rather than reproducing their controls. Application adapters live outside this directory.

Second compatibility patch: `src/use/useDrag.ts` uses `pointerPosition.ts` to derive ordinary pointer movement from client coordinates until the target actually owns pointer lock. Only locked movement uses `movementX/Y`, in CSS pixels, with non-finite deltas ignored. `window.outerWidth / innerWidth` is not a valid coordinate scale in an embedded WKWebView and could suppress native drags. Covered by `src/pointerPosition.test.ts`.

Third patch (PathWeaver web UI): `src/common.styl` no longer imports Google Fonts in `reset-viewport`; the app is offline and uses system fonts.

Fourth patch (measurement units): `src/InputNumber/context.ts` adds an optional provide/inject context (`InputNumberContextKey`). When an ancestor provides it, `InputNumber.vue` rewrites typed text through `expression` before evaluating it, falls back to its `suffix`, uses its `precision` as the display precision, and uses its `speed` / `keyStep` for unranged drags and arrow keys; `InputTranslate.vue` scales its 2D drag by `speed`. Without a provider nothing changes. PathWeaver provides it from `src/panels/LengthField.vue` / `LengthPair.vue` (lengths in the document unit).

Fifth patch (PathWeaver paint opacity): `src/InputColor/InputColor.vue` forwards focus/blur/confirm from its alpha (%) field like the colour-code field does, so an opacity edit closes its undo step.
