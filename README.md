# PathWeaver

Rust + egui製の、レタリング・ロゴ制作向け可変幅SVGパスエディターです。
ベジェ曲線に幅点を配置し、左右の太さ・切り口・ライブコーナーを編集できます。
編集用の`.pathweaver`形式で保存し、塗り輪郭のSVGとして書き出します。

## Macで開発する

1. Xcode Command Line Toolsを用意します：`xcode-select --install`（Xcode導入済みなら不要）。
2. [rustup](https://rustup.rs/)でRust stableを導入し、ターミナルを開き直します。
3. リポジトリ直下で実行します。

```bash
./run-pathweaver.sh           # 開発用ビルドで起動
./run-pathweaver.sh --test    # PathWeaverのテスト
./run-pathweaver.sh --check   # Clippy（警告をエラーとして検査）
./run-pathweaver.sh --release # 最適化ビルドで起動
```

初回は依存ライブラリの取得とビルドに時間がかかります。
日本語表示にはmacOS標準のヒラギノ角ゴシックを使用します。

### Finderから起動する

```bash
./scripts/build-macos-app.sh
open dist/PathWeaver.app
```

`dist/PathWeaver.app`を生成します。`--debug`を付けると開発用ビルドを使います。
ビルドしたMacのCPU向けのローカル用アプリです。署名はアドホック署名で、配布用のDeveloper ID署名・公証は行いません。
プロジェクトはアプリ内の「ファイル → 開く」で読み込みます。

### Macの操作

- 2本指スクロール：キャンバスをパン。ピンチ：ポインター位置を中心にズーム。
- Cmd＋スクロール、通常のマウスホイール：ズーム。
- Space＋ドラッグ／中ボタンドラッグ：パン。
- Cmd+0：全体を表示、Cmd+1：100%表示、Cmd+Plus／Minus：拡大・縮小。
- Cmd+N／O／S：新規／開く／保存。Cmd+Shift+S：別名保存。
- Cmd+Shift+E：SVG書き出し。Cmd+Z／Cmd+Shift+Z：Undo／Redo。
- Cmd+W／Cmd+Q：終了。未保存の変更がある場合は確認します。

Macのポイント単位スクロール（Magic Mouseを含む）はパンとして扱います。
ズームしたいときはCmdを併用してください。操作中のドラッグや未保存確認中はジェスチャーを受け付けません。

## 範囲選択とノード効果

Vで空白からドラッグして複数パスを選択し、まとめて移動・変形・削除できます。Shiftで追加・解除します。
パスを選択してプロパティの「ノードで外観を編集…」を開くと、ラフ・角丸・オフセット・縁取り・影・光彩などを接続できます。
値・計算・計測を含む25種類のノードに対応し、元の芯を保ったまま保存・SVG出力できます。
[ノードの操作・一覧と実装範囲](threadtype-egui/docs/pathweaver-appearance.md)を参照してください。

## Windows・Linux

Windowsでは`run-pathweaver.ps1`、Linuxでは`./run-pathweaver.sh`を使用します。
または`threadtype-egui`内で`cargo run --locked --bin pathweaver`を実行してください。
Windows・Linuxのホイール操作は従来どおりズームです。

[操作方法・実装範囲](threadtype-egui/docs/pathweaver.md)を参照してください。

## 元のThreadShape

元の円形ペグを使うフォント制作ツールも同梱しています。
`threadtype-egui`内で`cargo run --locked --bin threadtype`、または`./run.sh`で起動します。
[ThreadShapeの説明](threadtype-egui/README.md)を参照してください。
