# Third-party notices

`src/input.rs` is adapted from `gpui` 0.2.2's `examples/input.rs`.
Copyright Zed Industries. Licensed under Apache License 2.0.

Source: https://github.com/zed-industries/zed/tree/main/crates/gpui
License: [Apache License 2.0](licenses/GPUI-APACHE-2.0.txt)

Changes include the PathWeaver field presentation, contextual keyboard bindings,
Japanese placeholder, public field initialization and focus helpers, and a UTF-16
composition selection-offset correction. The example's application/demo was removed.


## Tweeq Vue components

The Mac inspector bundles actual Tweeq Vue components in an embedded WKWebView. Sources are pinned to `957ecf70ab51153faa0026314061820a57becfb4` in `../tweeq-ui/vendor/tweeq/`, with selected-input lifecycle and embedded pointer-coordinate fixes documented in `../tweeq-ui/README.md`. The older `src/controls.rs` implementation remains for compatibility. Icon licenses are in `../tweeq-ui/vendor/icon-licenses/`.

Source: https://github.com/baku89/tweeq

MIT License

Copyright (c) 2025 Baku Hashimoto

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
