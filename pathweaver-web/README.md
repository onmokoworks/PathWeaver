# pathweaver-web

PathWeaver の画面すべて（Vue 3 + Tweeq）。`vendor/tweeq/src` は Tweeq の実ソース（固定コミットと3つの互換パッチは `vendor/tweeq/UPSTREAM.md`）。

- `src/core/` — wasm ローダー（`call(op, args)`）、型、エディター状態（選択・1操作1Undo の履歴・描画キャッシュ）
- `src/canvas/` — SVG キャンバスとツール（ペン／選択／ダイレクト／アンカー／線幅、定規・ガイド・吸着）
- `src/nodes/` — ノードエディター（ノード上での値編集、接続、リンクへの挿入、追加メニュー）
- `src/panels/` — パス一覧、プロパティ、グリッド・ガイド、外観セット
- `src/host.ts` — Mac ホストとのやり取り（ブラウザでは file input／ダウンロードで代替）

```sh
npm ci
npm run dev        # wasm とアイコンを用意してから vite（127.0.0.1:5174）
npm run build      # 1ファイルの dist/index.html
npx playwright test  # WebKit で E2E
```

`npm run prepare-assets` は `../pathweaver-wasm` を wasm32 でビルドして `src/core/pathweaver.wasm` に置き、使っているアイコンだけを `src/icons.generated.ts` に書き出します（実行時にアイコンを取りに行きません）。
