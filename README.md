# ThreadShape

このチェックアウトには、新しい可変幅パス編集ツール **PathWeaver** の初期実装を追加しています。
芯のベジェ曲線に任意位置の幅コントローラーを配置し、左右の幅と端の切り口を編集してSVGへ書き出せます。

```powershell
cd threadtype-egui
cargo run --locked --bin pathweaver
```

Windowsではルートの `run-pathweaver.ps1` からも起動できます。
操作方法・実装範囲は [PathWeaverの説明](threadtype-egui/docs/pathweaver.md) を参照してください。

元のThreadShapeは `cargo run --locked --bin threadtype` で起動できます。以下は元アプリの説明です。

糸を円形ペグへ巻き付けながらグリフを設計する、Rust + egui製のフォント制作ツールです。

React版は開発を終了しました。現在の実装と開発対象は [`threadtype-egui`](threadtype-egui/) のみです。

## 起動

```bash
cd threadtype-egui
./run.sh
```

詳細は [`threadtype-egui/README.md`](threadtype-egui/README.md) を参照してください。
