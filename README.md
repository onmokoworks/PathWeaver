# PathWeaver

Rust製の、レタリング・ロゴ制作向け可変幅SVGパスエディターです。
Macの新しいフロントエンドは **GPUI**。キャンバス、パス一覧、設定欄、下部ノードエディターをGPUIで描画します。
ベジェ曲線・可変幅・25種類のノード効果・保存形式は、UIに依存しない `pathweaver-core` で共通化しています。

## Macで開発する

Xcode（Metalツールチェーンを含む）と [Rust stable](https://rustup.rs/) を用意し、リポジトリ直下で実行します。

```bash
./run-pathweaver.sh           # GPUI版を起動
./run-pathweaver.sh --release # 最適化ビルド
./run-pathweaver.sh --test    # 共通コア＋GPUI版のテスト
./run-pathweaver.sh --check   # 共通コア＋GPUI版のClippy
./run-pathweaver.sh --legacy  # 互換確認用のegui版
```

初回は依存ライブラリの取得とビルドに時間がかかります。GPUIは `0.2.2` に固定し、Macのテキスト表示に必要な `font-kit` 機能を有効にしています。日本語UIはmacOSのヒラギノを使用します。

### Finderから起動する

```bash
./scripts/build-macos-app.sh --debug
open dist/PathWeaver-GPUI.app
```

`--debug` を省くと最適化版、`--legacy` を追加するとegui版を作ります。
ビルドしたMacのCPU向けのローカルアプリで、署名はアドホック署名です。
GPUI版とegui版は別のbundle IDなので、従来版の作業を保ったままGPUI版を試せます。

## GPUI版の操作

- **P**：ペン。クリックで点、ドラッグでベジェハンドル。Enterで1本を終了。始点をクリックすると閉じます。
- **V**：パス選択。空白から範囲選択、Shiftで追加・解除。選択パスをまとめて移動・拡大縮小・回転・複製・削除できます。
- **A**：制御点とベジェハンドルを編集。Optionで片側ハンドルを操作します。
- **W**：幅ハンドル。パスをダブルクリックして幅点を追加。Optionで左右を別々に変更します。
- 2本指スクロール／中ボタンドラッグ：パン。Cmd＋スクロール／マウスホイール：ポインター位置でズーム。HomeまたはF：全体表示。
- Cmd+O／S／Shift+S：開く／保存／別名保存。Cmd+E：SVG書き出し。
- Cmd+Z／Shift+Z：Undo／Redo。Cmd+D：パス複製。Cmd+Q／W：終了（未保存なら確認）。

### 下部ノードエディター

「ノードを編集」で選択パスに効果を追加できます。境界を上下にドラッグして高さを変更できます。
種類ごとの見出し色、個別のノード名、形状・数値ソケットを備えています。

| 操作 | 動作 |
| --- | --- |
| Shift+A | ノード追加欄。日本語・英語の種類名で検索 |
| ドラッグ / Shift+クリック / 空白からドラッグ | 移動 / 複数選択 / 範囲選択 |
| G / Shift+D / M / X | 移動開始 / 複製 / 有効・無効 / 削除 |
| Home / N | 全ノードを表示 / 設定欄の開閉 |
| ダブルクリック / F2 | ノード名を変更 |
| ソケットからドラッグ | 接続。入力側からは既存接続をつなぎ直す |
| Option+右ドラッグ | Node Wrangler風の自動接続 |
| CtrlまたはCmd+Shift+クリック | 選択ノードを出力へ接続して確認（Undo可） |
| CtrlまたはCmd+右ドラッグ | 線を横切って接続を切る |
| 右クリック（入力ソケット） | 接続解除 |
| Escape | 移動・接続操作を取り消す |

ショートカットはポインターがある編集領域へ作用します。文字入力中は作品の編集キーを無効にします。

「セット」で名前と種類を付けて文書へ登録でき、別パスにも適用できます。
`.pathweaver-nodes` ファイルで書き出し／読み込みも可能です。適用時は独立したコピーになるため、別のパスや保存済みセットは連動して変わりません。
セットを修正して配布する場合は、同じ名前・種類で登録し直して書き出します。

## 保存形式と移行範囲

既存の `.pathweaver` を読み込み、同じ形式で保存します。元の芯、幅点、効果グラフ、名前付きセットを保持します。SVGは共通エンジンの色付き輪郭を出力します。

GPUI版はパス描画・選択・制御点・幅・基本変形・ノード編集の実用経路を移植した初版です。
従来UIのプリミティブ作成ツール、ライブコーナー専用ハンドル、切り口角度の専用UI、選択パス同士のブーリアン操作ボタンは引き続きegui版で利用できます。これらの既存データはGPUI版でも描画・保持します。
ネイティブのピンチ操作はGPUI版では未対応です。Cmd＋スクロールを使用してください。
名前付きセットは再利用できるグラフのコピーで、入れ子のグループや共有インスタンスではありません。
[効果一覧と描画仕様](threadtype-egui/docs/pathweaver-appearance.md)も参照してください。

## 構成

- `pathweaver-core`：文書・ベジェ・可変幅・輪郭・SVG・外観グラフ。GUIライブラリに依存しません。
- `pathweaver-gpui`：Mac用GPUIフロントエンド。入力、セッション履歴、キャンバス、ドック式ノードUI。
- `threadtype-egui`：従来フロントエンドとThreadShape。共通コアを使用し、互換性のテストも維持しています。

Windowsでは `run-pathweaver.ps1`、Linuxでは `./run-pathweaver.sh` が従来のegui版を起動します。GPUI版のWindows・Linux対応は未検証です。
元の円形ペグを使うフォント制作ツールは `./threadtype-egui/run.sh` または `threadtype-egui` 内で `cargo run --locked --bin threadtype` から起動できます。

## 参考とライセンス

操作の参考：[Blenderノード](https://docs.blender.org/manual/en/latest/interface/controls/nodes/index.html)、[Node Wrangler](https://docs.blender.org/manual/en/latest/addons/node_wrangler.html)。
文字入力の実装はGPUI公式サンプルを基にしています。詳細は [THIRD_PARTY_NOTICES.md](pathweaver-gpui/THIRD_PARTY_NOTICES.md) を参照してください。
